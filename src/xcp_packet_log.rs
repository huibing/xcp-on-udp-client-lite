//! In-memory ring buffer for captured XCP packets.
//!
//! Mirrors the pattern of `ConsoleLogState`/`ConsoleLogLayer` in the Tauri
//! crate: a `std::sync::Mutex<VecDeque>` for entries + `AtomicU64` for IDs.
//! Pollers (Tauri commands) call `entries_since(last_id)` to drain new entries.
//!
//! The `enabled` `AtomicBool` is set to `false` by default. Producers must
//! check `is_enabled()` before doing any formatting work — this makes the
//! capture cost effectively zero when no UI panel is observing.

use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

pub const MAX_PACKET_ENTRIES: usize = 5000;
pub const DAQ_HEX_MAX_BYTES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PacketDirection {
    Outgoing,
    Incoming,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PacketClass {
    Cto,
    Daq,
}

#[derive(Debug, Clone, Serialize)]
pub struct XcpPacketEntry {
    pub id: u64,
    pub timestamp: String,
    pub source: String,
    pub direction: PacketDirection,
    pub hex: String,
    pub description: String,
    pub packet_class: PacketClass,
    pub daq_timestamp: Option<u64>,
}

pub struct XcpPacketLog {
    buffer: Mutex<VecDeque<XcpPacketEntry>>,
    next_id: AtomicU64,
    enabled: AtomicBool,
    dropped_count: AtomicU64,
}

impl XcpPacketLog {
    pub fn new() -> Self {
        Self {
            buffer: Mutex::new(VecDeque::with_capacity(MAX_PACKET_ENTRIES)),
            next_id: AtomicU64::new(1),
            enabled: AtomicBool::new(false),
            dropped_count: AtomicU64::new(0),
        }
    }

    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
    }

    pub fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    pub fn push(&self, entry: XcpPacketEntry) {
        let mut buf = self.buffer.lock().expect("packet log poisoned");
        if buf.len() >= MAX_PACKET_ENTRIES {
            buf.pop_front();
            self.dropped_count.fetch_add(1, Ordering::Relaxed);
        }
        buf.push_back(entry);
    }

    pub fn entries_since(&self, since_id: u64) -> Vec<XcpPacketEntry> {
        let buf = self.buffer.lock().expect("packet log poisoned");
        buf.iter().filter(|e| e.id > since_id).cloned().collect()
    }

    pub fn clear(&self) {
        let mut buf = self.buffer.lock().expect("packet log poisoned");
        buf.clear();
        self.dropped_count.store(0, Ordering::Relaxed);
    }

    pub fn dropped_count(&self) -> u64 {
        self.dropped_count.load(Ordering::Relaxed)
    }
}

impl Default for XcpPacketLog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(id: u64) -> XcpPacketEntry {
        XcpPacketEntry {
            id,
            timestamp: "00:00:00.000".into(),
            source: "test".into(),
            direction: PacketDirection::Outgoing,
            hex: "FF".into(),
            description: "CONNECT".into(),
            packet_class: PacketClass::Cto,
            daq_timestamp: None,
        }
    }

    #[test]
    fn test_push_and_entries_since() {
        let log = XcpPacketLog::new();
        log.push(make_entry(1));
        log.push(make_entry(2));
        log.push(make_entry(3));
        assert_eq!(log.entries_since(0).len(), 3);
        assert_eq!(log.entries_since(1).len(), 2);
        assert_eq!(log.entries_since(3).len(), 0);
    }

    #[test]
    fn test_ring_buffer_drops_oldest() {
        let log = XcpPacketLog::new();
        for i in 0..(MAX_PACKET_ENTRIES + 10) {
            log.push(make_entry(i as u64 + 1));
        }
        let all = log.entries_since(0);
        assert_eq!(all.len(), MAX_PACKET_ENTRIES);
        assert_eq!(log.dropped_count(), 10);
    }

    #[test]
    fn test_enabled_flag() {
        let log = XcpPacketLog::new();
        assert!(!log.is_enabled());
        log.set_enabled(true);
        assert!(log.is_enabled());
    }

    #[test]
    fn test_clear() {
        let log = XcpPacketLog::new();
        log.push(make_entry(1));
        log.clear();
        assert_eq!(log.entries_since(0).len(), 0);
    }
}
