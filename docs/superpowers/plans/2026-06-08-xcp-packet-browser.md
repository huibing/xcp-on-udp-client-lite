# XCP Packet Browser Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an XCP packet browser to the Experiment page showing real-time hex-dump + simple parse of CTO commands/responses (not DAQ packets) flowing between client and slave.

**Architecture:** A shared ring buffer (`XcpPacketLog`, `std::sync::Mutex<VecDeque>` + `AtomicU64`) holds timestamped XCP packets. `XcpSlave::send_command_and_wait()` logs outgoing CTO commands + incoming CTO responses with sequential request/response correlation (the slave remembers the last outgoing command code so the next response can be labeled with the command name). The DAQ DTO receive loop logs raw DAQ packets separately (hex only, plus the slave timestamp when present in ODT 0). The log honors an `AtomicBool` `enabled` flag — when the UI panel is closed, the hot path short-circuits to zero allocations. A Tauri polling command (same pattern as `log_console`) feeds a new Vue component. The 4-byte transport header (len+counter) is already stripped before the router — `msg.payload` is pure XCP.

**Tech Stack:** Rust (protocol layer formatting, Tauri commands), Vue 3 (polling UI component), mirrors the existing `LogConsole.vue` + `log_console.rs` pattern.

**Key Constraints:**
- Only pure XCP payload bytes are displayed (no len/counter/timestamp transport framing).
- DAQ packets show raw hex + slave timestamp (when ODT 0) but are NOT parsed semantically.
- CTO commands show hex + human-readable name (e.g. `CONNECT`, `SHORT_UPLOAD`).
- CTO responses show hex + correlated description (e.g. `RES: CONNECT`, `ERR: Unknown command`).
- DAQ hex display is truncated to 64 bytes to bound IPC payload (`AB CD ... +N more`).
- Logging is short-circuited when the UI panel is closed (zero overhead by default).

---

## File Structure

### Files to Create
- **`src/protocol/packet_display.rs`** (~100 lines) — Pure formatting helpers:
  - `format_hex(bytes: &[u8]) -> String` — `FF 00 01`
  - `format_hex_truncated(bytes: &[u8], max: usize) -> String` — `FF 00 ... +5 more`
  - `command_name(code: u8) -> &'static str` — moved here from `udp.rs` (single source of truth)
  - `describe_cto_command(bytes: &[u8]) -> String` — e.g. `CONNECT`
  - `describe_cto_response(bytes: &[u8], last_cmd: Option<u8>) -> String` — e.g. `RES: CONNECT` or `ERR: Unknown command`
- **`src/xcp_packet_log.rs`** (~150 lines) — Ring buffer + enabled flag:
  - `XcpPacketEntry { id, timestamp, source, direction, hex, description, packet_class, daq_timestamp }`
  - `XcpPacketLog { buffer, next_id, enabled: AtomicBool, dropped_count: AtomicU64 }`
  - `MAX_PACKET_ENTRIES = 5000`
  - `DAQ_HEX_MAX_BYTES = 64`
- **`src-tauri/src/commands/xcp_packet.rs`** (~70 lines) — Three sync Tauri commands:
  - `xcp_packet_get(since_id: u64) -> Vec<XcpPacketEntry>`
  - `xcp_packet_clear()`
  - `xcp_packet_set_enabled(enabled: bool)` — toggled when panel opens/closes
- **`ui/src/components/XcpPacketBrowser.vue`** (~250 lines) — Vue component mirroring `LogConsole.vue` polling pattern.

### Files to Modify
- **`src/protocol.rs`** (module entry, NOT `protocol/mod.rs`) — Add `pub mod packet_display;` and re-export
- **`src/protocol/types.rs`** — Add `pub fn from_repr(value: u8) -> Option<Self>` to `CommandCode`
- **`src/transport/udp.rs`** — Replace local `command_name()` with re-export from `packet_display`
- **`src/slave.rs`** — Add `packet_log` field + `last_cmd_code` field, instrument `send_command_and_wait()`. Also accept optional `packet_log` in `XcpSlave::connect()` so CONNECT/DISCONNECT exchanges can be captured.
- **`src/daq.rs`** — Add `packet_log` + `slave_name` fields to `DaqManager`; clone them into the spawned DTO task and log each DTO packet
- **`src/lib.rs`** — `pub mod xcp_packet_log;`
- **`src-tauri/src/state.rs`** — Add `pub packet_log: Arc<XcpPacketLog>` to `TauriAppState`, initialize in `new()`
- **`src-tauri/src/commands/mod.rs`** — `pub mod xcp_packet;`
- **`src-tauri/src/lib.rs`** — `.manage(state.packet_log.clone())` (separate managed state so commands can take `tauri::State<'_, Arc<XcpPacketLog>>` directly, mirroring `ConsoleLogState`); register 3 new commands.
- **`src-tauri/src/commands/device.rs`** — In `connect_device`, pass `packet_log` into `XcpSlave::connect()` (so CONNECT is captured); after success, also wire the log into `DaqManager`. In `disconnect_device`, push a marker entry indicating the slave disconnected (don't clear — preserves history for inspection).
- **`ui/src/components/Experiment.vue`** — Import `XcpPacketBrowser`, add sidebar toggle (📡), mount panel, call `xcp_packet_set_enabled(true/false)` on toggle.

---

### Task 1: Create `src/protocol/packet_display.rs` with formatters and command-name lookup

**Files:**
- Create: `src/protocol/packet_display.rs`
- Modify: `src/protocol.rs` (add `pub mod packet_display;` and `pub use packet_display::*;`)
- Modify: `src/protocol/types.rs` (add `from_repr` to `CommandCode`)
- Modify: `src/transport/udp.rs` (remove local `command_name`, use `crate::protocol::packet_display::command_name`)

- [ ] **Step 1: Write `src/protocol/packet_display.rs`**

```rust
//! XCP packet display formatting — pure functions for hex dump + human-readable
//! descriptions used by the in-app packet browser. All inputs are pure XCP
//! payload bytes (first byte = PID/command code). Transport-layer framing
//! (len/counter) must already be stripped.

use super::responses::parse_response;
use super::types::{ErrorCode, Response};

/// Format bytes as space-separated uppercase hex.
pub fn format_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ")
}

/// Hex dump truncated to `max` bytes. Appends ` ... +N more` when truncated.
pub fn format_hex_truncated(bytes: &[u8], max: usize) -> String {
    if bytes.len() <= max {
        format_hex(bytes)
    } else {
        let head = format_hex(&bytes[..max]);
        let extra = bytes.len() - max;
        format!("{} ... +{} more", head, extra)
    }
}

/// Human-readable XCP command name from command-code byte.
/// (Moved from transport/udp.rs to make this the single source of truth.)
pub fn command_name(code: u8) -> &'static str {
    match code {
        0xFF => "CONNECT",
        0xFE => "DISCONNECT",
        0xFD => "GET_STATUS",
        0xFC => "SYNCH",
        0xFB => "GET_COMM_MODE_INFO",
        0xFA => "GET_ID",
        0xF9 => "SET_REQUEST",
        0xF8 => "GET_SEED",
        0xF7 => "UNLOCK",
        0xF6 => "SET_MTA",
        0xF5 => "UPLOAD",
        0xF4 => "SHORT_UPLOAD",
        0xF3 => "BUILD_CHECKSUM",
        0xF2 => "TRANSPORT_LAYER_CMD",
        0xF1 => "USER_CMD",
        0xF0 => "DOWNLOAD",
        0xEF => "DOWNLOAD_NEXT",
        0xEE => "DOWNLOAD_MAX",
        0xED => "SHORT_DOWNLOAD",
        0xEC => "MODIFY_BITS",
        0xEB => "SET_CAL_PAGE",
        0xEA => "GET_CAL_PAGE",
        0xE4 => "COPY_CAL_PAGE",
        0xE2 => "SET_DAQ_PTR",
        0xE1 => "WRITE_DAQ",
        0xE0 => "SET_DAQ_LIST_MODE",
        0xDF => "GET_DAQ_LIST_MODE",
        0xDE => "START_STOP_DAQ_LIST",
        0xDD => "START_STOP_SYNCH",
        0xDC => "GET_DAQ_CLOCK",
        0xDB => "READ_DAQ",
        0xDA => "GET_DAQ_PROCESSOR_INFO",
        0xD9 => "GET_DAQ_RESOLUTION_INFO",
        0xD8 => "GET_DAQ_LIST_INFO",
        0xD7 => "GET_DAQ_EVENT_INFO",
        0xD6 => "FREE_DAQ",
        0xD5 => "ALLOC_DAQ",
        0xD4 => "ALLOC_ODT",
        0xD3 => "ALLOC_ODT_ENTRY",
        0xD2 => "PROGRAM_START",
        0xD1 => "PROGRAM_CLEAR",
        0xD0 => "PROGRAM",
        0xCF => "PROGRAM_RESET",
        0xCE => "GET_PGM_PROCESSOR_INFO",
        0xC6 => "TIME_CORRELATION_PROPERTIES",
        0xC4 => "DAQ_MULTIPLE",
        _ => "UNKNOWN",
    }
}

/// Describe an outgoing CTO command packet. Returns e.g. `"CONNECT"` or
/// `"UNKNOWN(0x12)"`. Always returns a non-empty string.
pub fn describe_cto_command(bytes: &[u8]) -> String {
    match bytes.first() {
        Some(&code) => {
            let name = command_name(code);
            if name == "UNKNOWN" {
                format!("UNKNOWN(0x{:02X})", code)
            } else {
                name.to_string()
            }
        }
        None => "<empty>".to_string(),
    }
}

/// Describe an incoming CTO response packet, correlating with `last_cmd` for
/// positive responses. `last_cmd` is the command code byte of the most recent
/// outgoing command from the same slave (used purely for labeling).
pub fn describe_cto_response(bytes: &[u8], last_cmd: Option<u8>) -> String {
    match parse_response(bytes) {
        Ok(Response::Res(_)) => match last_cmd {
            Some(code) => format!("RES: {}", command_name(code)),
            None => "RES".to_string(),
        },
        Ok(Response::Err(code)) => format!("ERR: {}", code),
        Ok(Response::Daq { .. }) => "EV/SERV/DAQ".to_string(), // CTO channel only carries pid>=0xFC
        Err(e) => format!("<parse error: {}>", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_hex() {
        assert_eq!(format_hex(&[0xFF, 0x00, 0x01]), "FF 00 01");
        assert_eq!(format_hex(&[]), "");
    }

    #[test]
    fn test_format_hex_truncated() {
        let data = [0u8; 80];
        let s = format_hex_truncated(&data, 64);
        assert!(s.contains("+16 more"));
        assert_eq!(format_hex_truncated(&[0xAB], 64), "AB");
    }

    #[test]
    fn test_command_name() {
        assert_eq!(command_name(0xFF), "CONNECT");
        assert_eq!(command_name(0xFE), "DISCONNECT");
        assert_eq!(command_name(0x00), "UNKNOWN");
    }

    #[test]
    fn test_describe_cto_command() {
        assert_eq!(describe_cto_command(&[0xFF, 0x00]), "CONNECT");
        assert_eq!(describe_cto_command(&[0x12]), "UNKNOWN(0x12)");
        assert_eq!(describe_cto_command(&[]), "<empty>");
    }

    #[test]
    fn test_describe_cto_response_positive_with_correlation() {
        // RES packet: 0xFF + data; should be labeled with last_cmd's name
        let resp = vec![0xFF, 0x11, 0x00, 0x08];
        assert_eq!(describe_cto_response(&resp, Some(0xFF)), "RES: CONNECT");
        assert_eq!(describe_cto_response(&resp, None), "RES");
    }

    #[test]
    fn test_describe_cto_response_error() {
        // ERR packet: 0xFE + error_code
        let resp = vec![0xFE, 0x20];
        assert_eq!(describe_cto_response(&resp, Some(0xFF)), "ERR: Unknown command");
    }
}
```

- [ ] **Step 2: Add `from_repr` to `CommandCode` in `src/protocol/types.rs`**

While `packet_display` uses the `command_name` byte-lookup directly (no enum conversion needed), we still add `from_repr` for future ergonomics. Place it directly under the `CommandCode` enum:

```rust
impl CommandCode {
    /// Reverse lookup from byte value to enum variant.
    pub fn from_repr(value: u8) -> Option<Self> {
        match value {
            0xFF => Some(Self::Connect),
            0xFE => Some(Self::Disconnect),
            0xFD => Some(Self::GetStatus),
            0xFC => Some(Self::Synch),
            0xFB => Some(Self::GetCommModeInfo),
            0xFA => Some(Self::GetId),
            0xF9 => Some(Self::SetRequest),
            0xF8 => Some(Self::GetSeed),
            0xF7 => Some(Self::Unlock),
            0xF6 => Some(Self::SetMta),
            0xF5 => Some(Self::Upload),
            0xF4 => Some(Self::ShortUpload),
            0xF3 => Some(Self::BuildChecksum),
            0xF2 => Some(Self::TransportLayerCmd),
            0xF1 => Some(Self::UserCmd),
            0xF0 => Some(Self::Download),
            0xEF => Some(Self::DownloadNext),
            0xEE => Some(Self::DownloadMax),
            0xED => Some(Self::ShortDownload),
            0xEC => Some(Self::ModifyBits),
            0xEB => Some(Self::SetCalPage),
            0xEA => Some(Self::GetCalPage),
            0xE4 => Some(Self::CopyCalPage),
            0xD6 => Some(Self::FreeDaq),
            0xD5 => Some(Self::AllocDaq),
            0xD4 => Some(Self::AllocOdt),
            0xD3 => Some(Self::AllocOdtEntry),
            0xE2 => Some(Self::SetDaqPtr),
            0xE1 => Some(Self::WriteDaq),
            0xE0 => Some(Self::SetDaqListMode),
            0xDF => Some(Self::GetDaqListMode),
            0xDE => Some(Self::StartStopDaqList),
            0xDD => Some(Self::StartStopSynch),
            0xDC => Some(Self::GetDaqClock),
            0xDB => Some(Self::ReadDaq),
            0xDA => Some(Self::GetDaqProcessorInfo),
            0xD9 => Some(Self::GetDaqResolutionInfo),
            0xD8 => Some(Self::GetDaqListInfo),
            0xD7 => Some(Self::GetDaqEventInfo),
            0xD2 => Some(Self::ProgramStart),
            0xD1 => Some(Self::ProgramClear),
            0xD0 => Some(Self::Program),
            0xCF => Some(Self::ProgramReset),
            0xCE => Some(Self::GetPgmProcessorInfo),
            0xC6 => Some(Self::TimeCorrelationProperties),
            0xC4 => Some(Self::DaqMultiple),
            _ => None,
        }
    }
}
```

- [ ] **Step 3: Wire packet_display into `src/protocol.rs`**

Edit `src/protocol.rs`:
```rust
pub mod commands;
mod responses;
mod types;
pub mod packet_display;

pub use commands::*;
pub use responses::*;
pub use types::*;
pub use packet_display::{format_hex, format_hex_truncated, command_name,
                          describe_cto_command, describe_cto_response};
```

Note: `responses` and `types` stay private (existing pattern) — `packet_display` accesses them as sibling sub-modules via `super::responses::*` / `super::types::*`, which works because they share a parent module.

- [ ] **Step 4: Update `src/transport/udp.rs` to use the shared `command_name`**

Delete the local `fn command_name(cmd_code: u8) -> &'static str { ... }` function (lines 372–422). Update the single call site (line ~192 in trace log) to:
```rust
use crate::protocol::packet_display::command_name;
```
…or use the fully-qualified path: `crate::protocol::packet_display::command_name(...)`.

- [ ] **Step 5: Run tests**

Run: `cargo test --lib protocol::packet_display -- --nocapture`
Expected: 6 tests pass.

Run: `cargo build`
Expected: clean build (no unused warnings from removed `command_name` in udp.rs).

- [ ] **Step 6: Commit**

```bash
git add src/protocol.rs src/protocol/packet_display.rs src/protocol/types.rs src/transport/udp.rs
git commit -m "feat(protocol): add packet_display module with hex formatting and command names"
```

---

### Task 2: Create `src/xcp_packet_log.rs`

**Files:**
- Create: `src/xcp_packet_log.rs`
- Modify: `src/lib.rs` (add `pub mod xcp_packet_log;`)

- [ ] **Step 1: Write `src/xcp_packet_log.rs`**

```rust
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
    Outgoing, // client → slave
    Incoming, // slave → client
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
    pub timestamp: String,       // wall-clock e.g. "12:34:56.789"
    pub source: String,          // e.g. "ECU1" or "xcp-client" (for outgoing)
    pub direction: PacketDirection,
    pub hex: String,             // pure XCP bytes, possibly truncated for DAQ
    pub description: String,     // e.g. "CONNECT", "RES: CONNECT", "ERR: ...", "DAQ"
    pub packet_class: PacketClass,
    pub daq_timestamp: Option<u64>, // slave-side raw timestamp, ODT 0 only
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
```

- [ ] **Step 2: Register module in `src/lib.rs`**

Add (alphabetic order with other modules):
```rust
pub mod xcp_packet_log;
```

- [ ] **Step 3: Compile + test**

Run: `cargo test --lib xcp_packet_log -- --nocapture`
Expected: 4 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/xcp_packet_log.rs src/lib.rs
git commit -m "feat: add XcpPacketLog ring buffer with enable flag and drop counter"
```

---

### Task 3: Instrument `slave.rs` to log CTO packets

**Files:**
- Modify: `src/slave.rs`

- [ ] **Step 1: Add new fields and constructor support to `XcpSlave`**

Find the `XcpSlave` struct (around line 165–190) and add:
```rust
pub packet_log: Option<Arc<crate::xcp_packet_log::XcpPacketLog>>,
/// Last outgoing CTO command code, used for correlating the next response label.
pub last_cmd_code: std::sync::Mutex<Option<u8>>,
```

Note: `last_cmd_code` uses `std::sync::Mutex` (sync) because `send_command_and_wait` takes `&self`. Since the existing `Arc<Mutex<XcpSlave>>` wrapper already serializes calls per slave, contention is nil.

- [ ] **Step 2: Update `XcpSlave::connect()` signature to accept optional packet log**

Change signature (line 194):
```rust
pub async fn connect(
    name: impl Into<String>,
    transport_config: TransportConfig,
    packet_log: Option<Arc<crate::xcp_packet_log::XcpPacketLog>>,
) -> Result<(Self, SlaveId), SlaveError> {
```

Initialize the new fields when constructing `slave`:
```rust
let mut slave = Self {
    info: ...,
    transport,
    router,
    cto_rx,
    cal_manager: CalibrationManager::new(),
    daq_manager: Arc::new(Mutex::new(DaqManager::new())),
    a2l_path: None,
    segment_pages: Vec::new(),
    packet_log,                                  // NEW
    last_cmd_code: std::sync::Mutex::new(None),  // NEW
};
```

Update all callers (search for `XcpSlave::connect(`):
- `src/client.rs` — `XcpClient::connect_slave` must accept and forward `packet_log`. Add an optional parameter (defaulting to `None`) and forward it.
- Any tests that call `XcpSlave::connect` — pass `None`.

- [ ] **Step 3: Instrument `send_command_and_wait()` (line ~293)**

```rust
pub async fn send_command_and_wait(&self, command: &[u8]) -> Result<Vec<u8>, SlaveError> {
    // Send command
    self.transport
        .send_command_only(command)
        .await
        .map_err(SlaveError::Transport)?;

    // Capture outgoing CTO (cheap when disabled)
    let cmd_code = command.first().copied();
    if let Some(log) = &self.packet_log {
        if log.is_enabled() {
            let entry = crate::xcp_packet_log::XcpPacketEntry {
                id: log.next_id(),
                timestamp: chrono::Local::now().format("%H:%M:%S%.3f").to_string(),
                source: "xcp-client".to_string(),
                direction: crate::xcp_packet_log::PacketDirection::Outgoing,
                hex: crate::protocol::packet_display::format_hex(command),
                description: crate::protocol::packet_display::describe_cto_command(command),
                packet_class: crate::xcp_packet_log::PacketClass::Cto,
                daq_timestamp: None,
            };
            log.push(entry);
        }
    }
    if let Some(code) = cmd_code {
        if let Ok(mut last) = self.last_cmd_code.lock() {
            *last = Some(code);
        }
    }

    // Wait for response
    let mut rx = self.cto_rx.lock().await;

    match timeout(Duration::from_secs(1), rx.recv()).await {
        Ok(Some(msg)) => {
            // Capture incoming CTO
            if let Some(log) = &self.packet_log {
                if log.is_enabled() {
                    let last = self.last_cmd_code.lock().ok().and_then(|g| *g);
                    let entry = crate::xcp_packet_log::XcpPacketEntry {
                        id: log.next_id(),
                        timestamp: chrono::Local::now().format("%H:%M:%S%.3f").to_string(),
                        source: self.info.name.clone(),
                        direction: crate::xcp_packet_log::PacketDirection::Incoming,
                        hex: crate::protocol::packet_display::format_hex(&msg.payload),
                        description: crate::protocol::packet_display::describe_cto_response(
                            &msg.payload, last,
                        ),
                        packet_class: crate::xcp_packet_log::PacketClass::Cto,
                        daq_timestamp: None,
                    };
                    log.push(entry);
                }
            }

            // (existing logic) check for negative response, return
            if let Ok(Response::Err(err_code)) = parse_response(&msg.payload) {
                let cmd_code_v = command.first().copied().unwrap_or(0xFF);
                tracing::error!(
                    "XCP Negative Response from {}: cmd=0x{:02X}, err={:?}",
                    self.info.name, cmd_code_v, err_code
                );
            }
            Ok(msg.payload)
        }
        Ok(None) => Err(SlaveError::NotConnected),
        Err(_) => Err(SlaveError::ResponseTimeout),
    }
}
```

- [ ] **Step 4: Compile check**

Run: `cargo check --all-targets`
Expected: no errors. Update any test call sites that broke from the `connect()` signature change.

- [ ] **Step 5: Commit**

```bash
git add src/slave.rs src/client.rs
git commit -m "feat(slave): capture CTO commands and responses into XcpPacketLog"
```

---

### Task 4: Instrument `daq.rs` to log DAQ DTO packets

**Files:**
- Modify: `src/daq.rs`

- [ ] **Step 1: Add fields to `DaqManager`**

Find `DaqManager` struct and add:
```rust
pub packet_log: Option<Arc<crate::xcp_packet_log::XcpPacketLog>>,
/// Slave display name (used to label DAQ entries). Set when wiring up the manager.
pub slave_name: String,
```

Initialize defaults in `DaqManager::new()` to `None` / `String::new()`.

- [ ] **Step 2: Add setters**

```rust
pub fn set_packet_log(&mut self, log: Arc<crate::xcp_packet_log::XcpPacketLog>) {
    self.packet_log = Some(log);
}
pub fn set_slave_name(&mut self, name: String) {
    self.slave_name = name;
}
```

- [ ] **Step 3: Capture clones BEFORE `tokio::spawn` in the DTO loop**

In `DaqManager::start()` (around lines 1380–1413), alongside the other `let foo = self.foo.clone();` captures, add:
```rust
let packet_log = self.packet_log.clone();
let slave_name = self.slave_name.clone();
```

- [ ] **Step 4: Log inside the spawned task**

Inside the spawned task, right after the `Ok(Some(msg))` branch begins and the DTO packet is parsed (after `extract_odt0_timestamp` resolves the timestamp around line 1462), insert:

```rust
if let Some(log) = &packet_log {
    if log.is_enabled() {
        let hex = crate::protocol::packet_display::format_hex_truncated(
            &msg.payload,
            crate::xcp_packet_log::DAQ_HEX_MAX_BYTES,
        );
        let entry = crate::xcp_packet_log::XcpPacketEntry {
            id: log.next_id(),
            timestamp: chrono::Local::now().format("%H:%M:%S%.3f").to_string(),
            source: format!("{} DAQ {}/{}", slave_name, resolved_daq_list, resolved_odt),
            direction: crate::xcp_packet_log::PacketDirection::Incoming,
            hex,
            description: "DAQ".to_string(),
            packet_class: crate::xcp_packet_log::PacketClass::Daq,
            // data.timestamp is non-zero only for ODT 0 with timestamp enabled
            daq_timestamp: if resolved_odt == 0 && data.timestamp != 0 {
                Some(data.timestamp)
            } else {
                None
            },
        };
        log.push(entry);
    }
}
```

This is placed after the timestamp extraction so `data.timestamp` is already populated, and uses `format_hex_truncated` to bound the IPC payload.

- [ ] **Step 5: Compile check**

Run: `cargo check --all-targets`
Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add src/daq.rs
git commit -m "feat(daq): capture DAQ DTO packets into XcpPacketLog with hex truncation"
```

---

### Task 5: Tauri backend — managed state + commands

**Files:**
- Create: `src-tauri/src/commands/xcp_packet.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add `packet_log: Arc<XcpPacketLog>` to `TauriAppState`**

In `src-tauri/src/state.rs`:
```rust
use xcp_client::xcp_packet_log::XcpPacketLog;

pub struct TauriAppState {
    pub client: XcpClient,
    pub devices: Vec<DeviceState>,
    pub current_device_id: Option<String>,
    pub compare_sessions: Vec<CompareSession>,
    pub experiment_active: bool,
    pub packet_log: Arc<XcpPacketLog>,   // NEW
}
```

In `TauriAppState::new()`, after the existing init:
```rust
Self {
    client: XcpClient::new(),
    devices,
    current_device_id: None,
    compare_sessions: Vec::new(),
    experiment_active: false,
    packet_log: Arc::new(XcpPacketLog::new()),
}
```

- [ ] **Step 2: Create `src-tauri/src/commands/xcp_packet.rs`**

```rust
use std::sync::Arc;
use xcp_client::xcp_packet_log::{XcpPacketEntry, XcpPacketLog};

#[tauri::command]
pub fn xcp_packet_get(
    state: tauri::State<'_, Arc<XcpPacketLog>>,
    since_id: u64,
) -> Vec<XcpPacketEntry> {
    state.entries_since(since_id)
}

#[tauri::command]
pub fn xcp_packet_clear(state: tauri::State<'_, Arc<XcpPacketLog>>) {
    state.clear();
}

#[tauri::command]
pub fn xcp_packet_set_enabled(state: tauri::State<'_, Arc<XcpPacketLog>>, enabled: bool) {
    state.set_enabled(enabled);
}
```

All three are sync (no `async`), matching `console_logs_get` / `console_logs_clear`.

- [ ] **Step 3: Register in `src-tauri/src/commands/mod.rs`**

```rust
pub mod xcp_packet;
```

- [ ] **Step 4: Wire managed state and commands in `src-tauri/src/lib.rs`**

Two adjustments to `run()`:

1. Create the `TauriAppState` first, then `.manage()` a CLONE of its `packet_log` so the Tauri commands can take `tauri::State<'_, Arc<XcpPacketLog>>` directly (parallel to how `ConsoleLogState` is managed). The same `Arc` lives inside `TauriAppState` for backend wiring use.

```rust
let app_state = TauriAppState::new();
let packet_log = app_state.packet_log.clone();   // separate handle for .manage()
let app_state_arc = Arc::new(RwLock::new(app_state));

tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_shell::init())
    .manage(console_state)
    .manage(app_state_arc)
    .manage(packet_log)                          // NEW
    .manage(commands::mf4_viewer::Mf4FileCache::new())
    .invoke_handler(tauri::generate_handler![
        ...
        commands::xcp_packet::xcp_packet_get,
        commands::xcp_packet::xcp_packet_clear,
        commands::xcp_packet::xcp_packet_set_enabled,
        ...
    ])
    .run(tauri::generate_context!())
    .expect("error while running xcp-client-desktop");
```

- [ ] **Step 5: Compile check**

Run: `cargo check`
Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/state.rs src-tauri/src/commands/xcp_packet.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs
git commit -m "feat(tauri): expose XcpPacketLog as managed state with poll/clear/enable commands"
```

---

### Task 6: Wire packet_log into device connect flow (capture CONNECT/DISCONNECT)

**Files:**
- Modify: `src-tauri/src/commands/device.rs`

- [ ] **Step 1: Pass packet_log into `XcpSlave::connect()` (or the XcpClient wrapper)**

Locate `connect_device` in `src-tauri/src/commands/device.rs`. Before calling `client.connect_slave()`, fetch the packet log from `TauriAppState`:

```rust
let packet_log = {
    let app = state.read().await;
    app.packet_log.clone()
};
```

Update the call to `connect_slave` (or however the slave is created) to forward `Some(packet_log.clone())`. This ensures the CONNECT command + response are captured.

- [ ] **Step 2: After successful connect, wire log into DaqManager and set slave_name**

```rust
// After connect_slave returns Ok(slave_id):
if let Some(slave_arc) = client.get_slave(slave_id) {
    let slave_guard = slave_arc.lock().await;
    let daq_mgr_arc = slave_guard.daq_manager.clone();
    let display_name = slave_guard.info.name.clone();
    drop(slave_guard);

    let mut dm = daq_mgr_arc.lock().await;
    dm.set_packet_log(packet_log.clone());
    dm.set_slave_name(display_name);
}
```

(Adjust field/method names to match the actual `XcpClient` / `XcpSlave` API discovered while editing.)

- [ ] **Step 3: In `disconnect_device`, push a marker entry (don't clear)**

Right after disconnect succeeds:
```rust
let packet_log = state.read().await.packet_log.clone();
if packet_log.is_enabled() {
    packet_log.push(xcp_client::xcp_packet_log::XcpPacketEntry {
        id: packet_log.next_id(),
        timestamp: chrono::Local::now().format("%H:%M:%S%.3f").to_string(),
        source: device_name,
        direction: xcp_client::xcp_packet_log::PacketDirection::Incoming,
        hex: String::new(),
        description: "<disconnected>".to_string(),
        packet_class: xcp_client::xcp_packet_log::PacketClass::Cto,
        daq_timestamp: None,
    });
}
```

This preserves history (per reviewer recommendation) rather than clearing.

- [ ] **Step 4: Compile check**

Run: `cargo check`
Expected: no errors.

Run: `cargo build --release` (or `cargo tauri build`) to validate full compilation.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands/device.rs
git commit -m "feat(tauri): wire XcpPacketLog through connect/disconnect, capture CONNECT exchange"
```

---

### Task 7: Vue component — `XcpPacketBrowser.vue`

**Files:**
- Create: `ui/src/components/XcpPacketBrowser.vue`

- [ ] **Step 1: Write the component**

Mirror `LogConsole.vue`'s polling pattern. Skeleton:

```vue
<template>
  <div class="packet-panel" :style="{ height: panelHeight + 'px' }">
    <div class="packet-resize-handle" @mousedown.prevent="startResize"></div>
    <div class="packet-toolbar">
      <div class="filter-group">
        <button v-for="f in filters" :key="f.key"
          class="filter-btn" :class="{ active: activeFilter === f.key }"
          @click="activeFilter = f.key">{{ f.label }}</button>
      </div>
      <input class="packet-search" v-model="searchText" placeholder="Search hex/desc…" />
      <span class="packet-count">{{ filteredPackets.length }} / {{ logEntries.length }} packets</span>
      <span v-if="droppedCount > 0" class="packet-dropped" title="Oldest entries discarded (ring buffer overflow)">
        ⚠ {{ droppedCount }} dropped
      </span>
      <div class="packet-toolbar-spacer"></div>
      <button class="packet-btn" @click="clearPackets" title="Clear">🗑</button>
      <button class="packet-btn" @click="scrollToBottom" v-if="!atBottom">↓</button>
      <button class="packet-btn" @click="$emit('close')" title="Close">✕</button>
    </div>
    <div class="packet-body" ref="container" @scroll="onScroll">
      <div v-for="pkt in filteredPackets" :key="pkt.id" class="pkt-row"
           :class="['dir-' + pkt.direction, 'cls-' + pkt.packet_class]">
        <span class="pkt-time">{{ pkt.timestamp }}</span>
        <span class="pkt-source">[{{ pkt.source }}]</span>
        <span class="pkt-arrow">{{ pkt.direction === 'outgoing' ? '→' : '←' }}</span>
        <span class="pkt-hex">{{ pkt.hex }}</span>
        <span class="pkt-desc">({{ pkt.description }})</span>
        <span v-if="pkt.daq_timestamp != null" class="pkt-ts">ts={{ pkt.daq_timestamp }}</span>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'

const emit = defineEmits(['close'])

const filters = [
  { key: 'all', label: 'All' },
  { key: 'cto', label: 'CTO (non-DAQ)' },
  { key: 'daq', label: 'DAQ only' },
]
const activeFilter = ref('all')
const searchText = ref('')
const panelHeight = ref(280)
const logEntries = ref([])
const lastId = ref(0)
const atBottom = ref(true)
const container = ref(null)
const droppedCount = ref(0)

const filteredPackets = computed(() => {
  let r = logEntries.value
  if (activeFilter.value === 'cto') r = r.filter(e => e.packet_class === 'cto')
  else if (activeFilter.value === 'daq') r = r.filter(e => e.packet_class === 'daq')
  const t = searchText.value.trim().toLowerCase()
  if (t) r = r.filter(e => e.hex.toLowerCase().includes(t) || e.description.toLowerCase().includes(t))
  return r
})

async function poll() {
  try {
    const entries = await invoke('xcp_packet_get', { sinceId: lastId.value })
    if (entries.length > 0) {
      lastId.value = entries[entries.length - 1].id
      logEntries.value.push(...entries)
      // Trim local cache to last 10000 to avoid runaway frontend memory
      if (logEntries.value.length > 10000) logEntries.value.splice(0, logEntries.value.length - 10000)
      if (atBottom.value) requestAnimationFrame(() => {
        const el = container.value
        if (el) el.scrollTop = el.scrollHeight
      })
    }
  } catch (e) { /* shutdown */ }
}

async function clearPackets() {
  await invoke('xcp_packet_clear')
  logEntries.value = []
  lastId.value = 0
  droppedCount.value = 0
}

function onScroll() {
  const el = container.value
  if (!el) return
  atBottom.value = el.scrollHeight - el.scrollTop - el.clientHeight < 30
}
function scrollToBottom() {
  const el = container.value
  if (el) { el.scrollTop = el.scrollHeight; atBottom.value = true }
}

function startResize(e) {
  const startY = e.clientY, startH = panelHeight.value
  const onMove = ev => { panelHeight.value = Math.max(120, Math.min(800, startH + (startY - ev.clientY))) }
  const onUp = () => { document.removeEventListener('mousemove', onMove); document.removeEventListener('mouseup', onUp) }
  document.addEventListener('mousemove', onMove); document.addEventListener('mouseup', onUp)
}

let pollTimer = null
onMounted(async () => {
  await invoke('xcp_packet_set_enabled', { enabled: true })
  poll()
  pollTimer = setInterval(poll, 200)
})
onUnmounted(async () => {
  if (pollTimer) clearInterval(pollTimer)
  try { await invoke('xcp_packet_set_enabled', { enabled: false }) } catch (e) {}
})
</script>

<style scoped>
/* Mirror LogConsole.vue color tokens. Outgoing rows tinted brown,
   incoming green; DAQ rows muted. */
.packet-panel { background: var(--exp-bg2, #060606); border-top: 1px solid var(--exp-border, #003311);
  display: flex; flex-direction: column; flex-shrink: 0; position: relative; }
.packet-resize-handle { height: 4px; cursor: ns-resize; background: var(--exp-border3, #001a09); flex-shrink: 0; }
.packet-toolbar { display: flex; align-items: center; gap: 8px; padding: 4px 8px;
  background: var(--exp-bg3, #0d0d0d); border-bottom: 1px solid var(--exp-border2, #002211); flex-shrink: 0; }
.filter-group { display: flex; gap: 2px; }
.filter-btn { background: none; border: 1px solid var(--exp-border, #003311);
  color: var(--exp-fg-mid, #00aa33); padding: 2px 8px; cursor: pointer; font-size: 0.78rem; border-radius: 2px; }
.filter-btn.active { background: var(--exp-fg-mid, #00aa33); color: var(--exp-bg, #090909); }
.packet-search { background: var(--exp-bg, #090909); color: var(--exp-fg, #00cc44);
  border: 1px solid var(--exp-border, #003311); padding: 2px 6px; font-family: inherit; font-size: 0.8rem; width: 180px; }
.packet-count { color: var(--exp-fg-dim, #556644); font-size: 0.75rem; }
.packet-dropped { color: #ccaa00; font-size: 0.75rem; }
.packet-toolbar-spacer { flex: 1; }
.packet-btn { background: none; border: 1px solid var(--exp-border, #003311);
  color: var(--exp-fg-mid, #00aa33); padding: 2px 8px; cursor: pointer; font-size: 0.8rem; border-radius: 2px; }
.packet-btn:hover { background: var(--exp-hover, #003311); }
.packet-body { flex: 1; overflow-y: auto; padding: 2px 0; font-size: 0.78rem; line-height: 1.5; font-family: monospace; }
.pkt-row { display: flex; gap: 8px; padding: 1px 8px; white-space: nowrap; }
.pkt-row:hover { background: var(--exp-hover, #003311); }
.dir-outgoing .pkt-arrow { color: #cc8833; }
.dir-incoming .pkt-arrow { color: #33cc66; }
.cls-daq { opacity: 0.75; }
.pkt-time { color: var(--exp-fg-dim2, #558844); min-width: 90px; }
.pkt-source { color: var(--exp-fg-dim3, #446633); min-width: 120px; }
.pkt-arrow { font-weight: bold; }
.pkt-hex { color: var(--exp-fg, #00cc44); }
.pkt-desc { color: var(--exp-fg-dim, #556644); font-style: italic; }
.pkt-ts { color: #66aacc; }
</style>
```

- [ ] **Step 2: Commit**

```bash
git add ui/src/components/XcpPacketBrowser.vue
git commit -m "feat(ui): add XcpPacketBrowser Vue component with All/CTO/DAQ filter"
```

---

### Task 8: Integrate into `Experiment.vue`

**Files:**
- Modify: `ui/src/components/Experiment.vue`

- [ ] **Step 1: Import and add toggle**

Add to the script setup imports:
```js
import XcpPacketBrowser from './XcpPacketBrowser.vue'
```

Add state ref:
```js
const showXcpBrowser = ref(false)
```

- [ ] **Step 2: Add sidebar button**

In the sidebar (`<div class="exp-sidebar">`), place near the existing console toggle or in its own slot:
```vue
<button class="sb-btn" :class="{ 'sb-active': showXcpBrowser }"
  title="XCP Packet Browser — view raw XCP packets in real time"
  @click="showXcpBrowser = !showXcpBrowser">📡</button>
```

- [ ] **Step 3: Mount the panel**

At the bottom of the experiment content area (similar location to where `LogConsole` is mounted — find the existing `<LogConsole v-if=... />` and place this near it; both can coexist or stack vertically):
```vue
<XcpPacketBrowser v-if="showXcpBrowser" @close="showXcpBrowser = false" />
```

- [ ] **Step 4: Run dev server and verify visually**

Run: `cargo tauri dev`
Expected: app launches, Experiment page renders, 📡 button toggles a packet browser panel.

- [ ] **Step 5: Commit**

```bash
git add ui/src/components/Experiment.vue
git commit -m "feat(ui): mount XcpPacketBrowser panel in Experiment page with sidebar toggle"
```

---

### Task 9: Manual verification and final commit

- [ ] **Step 1: Full compilation + tests**

Run: `cargo test --workspace`
Expected: all existing tests pass + new tests (packet_display, xcp_packet_log) pass.

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
Expected: clean.

- [ ] **Step 2: Manual verification checklist (cargo tauri dev)**
  - [ ] Open Experiment page
  - [ ] Click 📡 to open packet browser (panel appears)
  - [ ] Connect a device
  - [ ] CTO commands appear, e.g. outgoing `FF 00 (CONNECT)`
  - [ ] CTO responses appear, e.g. incoming `FF 11 00 08 08 00 01 01 (RES: CONNECT)` (single leading FF — no transport header)
  - [ ] Trigger an error (e.g. unsupported command) — verify `FE 20 (ERR: Unknown command)` style entry
  - [ ] Start DAQ — DAQ packets appear with `source = "<slave> DAQ <list>/<odt>"`, hex truncated to 64 bytes with `... +N more` for long packets, description `DAQ`, `ts=<n>` shown for ODT 0 entries
  - [ ] Click "CTO" filter — DAQ rows disappear
  - [ ] Click "DAQ only" filter — only DAQ rows shown
  - [ ] Click "All" — both visible again
  - [ ] Verify hex contains NO `len_l len_h counter_l counter_h` prefix (compare against transport-layer trace logs in `logs/`)
  - [ ] Click 🗑 — packets clear; new packets continue arriving
  - [ ] Close panel — verify no more entries arrive (enabled flag is `false`)
  - [ ] Reopen panel — entries resume
  - [ ] Disconnect device — `<disconnected>` marker entry appears
  - [ ] Generate >5000 packets — `⚠ N dropped` counter appears

- [ ] **Step 3: Final commit (only if any fixes needed during verification)**

```bash
git add -A
git commit -m "fix: address packet browser issues found during manual verification"
```

If no fixes needed, skip this step.

---

## Notes on Design Trade-offs

- **Single global ring buffer (5000 entries)**: All slaves' packets are interleaved. Filter by source string in the UI if multi-slave debugging becomes a need. Acceptable for v1 since the experiment page is typically single-device focused.
- **Disabled-by-default capture**: Zero overhead until the user opens the panel. `is_enabled()` is an atomic read with `Relaxed` ordering — single instruction on x86, negligible cost on the hot path.
- **DAQ hex truncation (64 bytes)**: A 256-byte DAQ packet → ~768-char hex string × 100 Hz = 76 KB/s of UI payload uncapped. Truncation keeps it manageable while still showing PID/list/odt/timestamp prefix.
- **No request/response correlation across reorder**: XCP is strict request/response, one outstanding command per slave, so `last_cmd_code` is correct under the existing single-threaded-per-slave invariant.
- **Disconnect marker (not clear)**: Preserves troubleshooting history. User can manually clear with 🗑.
