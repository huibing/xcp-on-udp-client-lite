//! Message Router - Unified CTO/DAQ message handling
//!
//! This module provides a unified message receiver that:
//! 1. Receives all packets from the slave
//! 2. Parses the PID to determine if it's a CTO response or DAQ packet
//! 3. Routes messages to the appropriate channel
//!
//! PID classification:
//! - PID >= 0xFC: CTO responses (ERR, EV, SERV, RES)
//! - PID < 0xFC: DAQ packets (DTO)

use crate::transport::Transport;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::task::JoinHandle;

/// Counter tracking state for monitoring slave counter sequence
pub struct CounterTracker {
    /// Expected next counter from slave (None means not initialized)
    expected: Option<u16>,
    /// Whether counter tracking is enabled
    enabled: bool,
}

impl CounterTracker {
    fn new() -> Self {
        Self {
            expected: None,
            enabled: true,
        }
    }

    /// Check and update counter, return warning if discontinuity detected
    fn check_and_update(&mut self, received: u16) -> Option<CounterWarning> {
        if !self.enabled {
            return None;
        }

        let warning = if let Some(expected) = self.expected {
            let expected_next = expected.wrapping_add(1);

            if received != expected_next {
                let is_wrap = expected == 65535 && received == 0;
                if !is_wrap {
                    Some(CounterWarning {
                        expected: expected_next,
                        received,
                        gap: received.wrapping_sub(expected_next),
                    })
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        self.expected = Some(received);
        warning
    }

    /// Reset counter tracking (e.g., on reconnect)
    fn reset(&mut self) {
        self.expected = None;
    }

    /// Enable or disable counter tracking
    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

/// Warning information for counter discontinuity
#[derive(Debug, Clone)]
pub struct CounterWarning {
    pub expected: u16,
    pub received: u16,
    pub gap: u16,
}

/// XCP Packet Types based on PID
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketType {
    /// Command Transfer Object (CTO) - Response
    Cto,
    /// Data Transfer Object (DTO) - DAQ data
    Dto,
}

/// XCP Message wrapper
#[derive(Debug, Clone)]
pub struct XcpMessage {
    pub packet_type: PacketType,
    pub pid: u8,
    pub data: Vec<u8>,
}

impl XcpMessage {
    /// Parse a single XCP packet (starting at offset) and determine type based on PID
    /// Returns (message, next_offset) if successful
    fn parse_one(packet: &[u8], offset: usize) -> Option<(Self, usize)> {
        if offset + 4 > packet.len() {
            return None;
        }

        // XCP header: [LEN_L, LEN_H, COUNTER_L, COUNTER_H]
        let len = u16::from_le_bytes([packet[offset], packet[offset + 1]]) as usize;
        let xcp_packet_len = 4 + len; // 4 bytes header + payload

        if offset + xcp_packet_len > packet.len() {
            tracing::warn!("Incomplete XCP packet: expected {} bytes, got {}", xcp_packet_len, packet.len() - offset);
            return None;
        }

        // Get the XCP packet data (including 4-byte header)
        let xcp_packet = &packet[offset..offset + xcp_packet_len];

        // PID is the first byte after the 4-byte transport header
        if xcp_packet_len < 5 {
            return None;
        }

        let pid = xcp_packet[4];

        // Determine packet type based on PID
        // PID >= 0xFC: CTO (Command Transfer Object)
        // PID < 0xFC: DTO (Data Transfer Object / DAQ)
        let packet_type = if pid >= 0xFC {
            PacketType::Cto
        } else {
            PacketType::Dto
        };

        Some((XcpMessage {
            packet_type,
            pid,
            data: xcp_packet.to_vec(), // Complete XCP packet including 4-byte header
        }, offset + xcp_packet_len))
    }

    /// Parse all XCP packets from a UDP packet
    /// A UDP packet may contain multiple XCP packets
    pub fn parse_packets(packet: Vec<u8>) -> Vec<Self> {
        let mut messages = Vec::new();
        let mut offset = 0;

        while offset < packet.len() {
            match Self::parse_one(&packet, offset) {
                Some((msg, next_offset)) => {
                    messages.push(msg);
                    offset = next_offset;
                }
                None => {
                    if offset < packet.len() {
                        tracing::warn!("Failed to parse XCP packet at offset {}, remaining {} bytes", offset, packet.len() - offset);
                    }
                    break;
                }
            }
        }

        messages
    }
}

/// Message Router - routes messages to appropriate channels
pub struct MessageRouter {
    /// Channel for CTO responses (command responses)
    cto_tx: mpsc::Sender<XcpMessage>,
    cto_rx: Arc<Mutex<mpsc::Receiver<XcpMessage>>>,

    /// Channel for DAQ data packets
    dto_tx: mpsc::Sender<XcpMessage>,
    dto_rx: Arc<Mutex<mpsc::Receiver<XcpMessage>>>,

    /// Background task handle
    router_task: Arc<Mutex<Option<JoinHandle<()>>>>,

    /// Running flag
    running: Arc<std::sync::atomic::AtomicBool>,

    /// Counter tracker for detecting discontinuities in slave packet sequence
    counter_tracker: Arc<RwLock<CounterTracker>>,
}

impl MessageRouter {
    /// Create new message router with specified channel capacity
    pub fn new(channel_capacity: usize) -> Self {
        let (cto_tx, cto_rx) = mpsc::channel(channel_capacity);
        let (dto_tx, dto_rx) = mpsc::channel(channel_capacity);

        Self {
            cto_tx,
            cto_rx: Arc::new(Mutex::new(cto_rx)),
            dto_tx,
            dto_rx: Arc::new(Mutex::new(dto_rx)),
            router_task: Arc::new(Mutex::new(None)),
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            counter_tracker: Arc::new(RwLock::new(CounterTracker::new())),
        }
    }

    /// Get sender for CTO responses (used by transport layer)
    pub fn get_cto_sender(&self) -> mpsc::Sender<XcpMessage> {
        self.cto_tx.clone()
    }

    /// Get sender for DAQ packets (used by transport layer)
    pub fn get_dto_sender(&self) -> mpsc::Sender<XcpMessage> {
        self.dto_tx.clone()
    }

    /// Get receiver for CTO responses (used by client commands)
    pub fn get_cto_receiver(&self) -> Arc<Mutex<mpsc::Receiver<XcpMessage>>> {
        self.cto_rx.clone()
    }

    /// Get receiver for DAQ packets (used by DAQ manager)
    pub fn get_dto_receiver(&self) -> Arc<Mutex<mpsc::Receiver<XcpMessage>>> {
        self.dto_rx.clone()
    }

    /// Start the router task that receives from transport and routes messages
    pub async fn start(&self, transport: Arc<dyn Transport>) {
        if self.running.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }

        self.running.store(true, std::sync::atomic::Ordering::SeqCst);

        let running = self.running.clone();
        let cto_tx = self.cto_tx.clone();
        let dto_tx = self.dto_tx.clone();
        let counter_tracker = self.counter_tracker.clone();

        let handle = tokio::spawn(async move {
            tracing::info!("Message router started");

            while running.load(std::sync::atomic::Ordering::SeqCst) {
                // Receive packet from transport
                match transport.recv_packet().await {
                    Ok(packet) => {
                        tracing::trace!("Received UDP packet: {} bytes", packet.len());
                        // Parse all XCP packets from the UDP packet
                        let messages = XcpMessage::parse_packets(packet);
                        tracing::trace!("Parsed {} XCP messages from UDP packet", messages.len());

                        for msg in messages {
                            // Check counter for each XCP packet (in transport header bytes 2-3)
                            if msg.data.len() >= 4 {
                                let counter = u16::from_le_bytes([msg.data[2], msg.data[3]]);
                                let pid = msg.data.get(4).copied().unwrap_or(0xFF);
                                tracing::trace!(
                                    "Routing XCP message: type={:?}, counter={}, pid=0x{:02X}, len={}",
                                    msg.packet_type, counter, pid, msg.data.len()
                                );
                                let mut tracker = counter_tracker.write().await;
                                if let Some(warning) = tracker.check_and_update(counter) {
                                    tracing::warn!(
                                        "Slave counter discontinuity: expected {}, received {} (gap: {})",
                                        warning.expected, warning.received, warning.gap
                                    );
                                }
                            }

                            match msg.packet_type {
                                PacketType::Cto => {
                                    tracing::trace!("Routing CTO message: pid=0x{:02X}", msg.pid);
                                    // Use try_send to avoid blocking router if channel is full
                                    match cto_tx.try_send(msg) {
                                        Ok(_) => {}
                                        Err(mpsc::error::TrySendError::Full(_)) => {
                                            tracing::warn!("CTO channel full, dropping message");
                                        }
                                        Err(e) => {
                                            tracing::error!("Failed to send CTO message: {}", e);
                                            break;
                                        }
                                    }
                                }
                                PacketType::Dto => {
                                    tracing::trace!("Routing DTO message: pid=0x{:02X}", msg.pid);
                                    // Use try_send to avoid blocking router if DAQ is not consuming
                                    match dto_tx.try_send(msg) {
                                        Ok(_) => {}
                                        Err(mpsc::error::TrySendError::Full(_)) => {
                                            tracing::trace!("DTO channel full, dropping DAQ data");
                                        }
                                        Err(e) => {
                                            tracing::error!("Failed to send DTO message: {}", e);
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::trace!("Transport receive error: {}", e);
                        // Small delay to avoid busy loop on error
                        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                    }
                }
            }

            tracing::info!("Message router stopped");
        });

        {
            let mut task_guard = self.router_task.lock().await;
            *task_guard = Some(handle);
        }
    }

    /// Reset counter tracking (call on reconnect)
    pub async fn reset_counter(&self) {
        let mut tracker = self.counter_tracker.write().await;
        tracker.reset();
    }

    /// Enable or disable counter tracking
    pub async fn set_counter_tracking(&self, enabled: bool) {
        let mut tracker = self.counter_tracker.write().await;
        tracker.set_enabled(enabled);
    }

    /// Stop the router
    pub async fn stop(&self) {
        self.running.store(false, std::sync::atomic::Ordering::SeqCst);

        let mut task_guard = self.router_task.lock().await;
        if let Some(handle) = task_guard.take() {
            handle.abort();
        }
    }
}

impl Default for MessageRouter {
    fn default() -> Self {
        Self::new(100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_packet_type_detection() {
        // Single CTO response (PID >= 0xFC)
        // XCP header: len=4, counter=1, then PID=0xFF (RES) + 3 bytes data
        let cto_packet = vec![0x04, 0x00, 0x01, 0x00, 0xFF, 0x00, 0x01, 0x02];
        let messages = XcpMessage::parse_packets(cto_packet);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].packet_type, PacketType::Cto);
        assert_eq!(messages[0].pid, 0xFF);
        // Data includes the full XCP packet: 4 bytes header + payload
        assert_eq!(messages[0].data.len(), 8); // 4 (header) + 4 (payload)

        // Single DAQ packet (PID < 0xFC)
        // XCP header: len=5, counter=1, then PID=0xC0 + 4 bytes data
        let dto_packet = vec![0x05, 0x00, 0x01, 0x00, 0xC0, 0x01, 0x02, 0x03, 0x04];
        let messages = XcpMessage::parse_packets(dto_packet);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].packet_type, PacketType::Dto);
        assert_eq!(messages[0].pid, 0xC0);
        assert_eq!(messages[0].data.len(), 9); // 4 (header) + 5 (payload)

        // Another DAQ packet (PID = 0xFB is still DTO)
        let dto_packet = vec![0x03, 0x00, 0x01, 0x00, 0xFB, 0x01, 0x02];
        let messages = XcpMessage::parse_packets(dto_packet);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].packet_type, PacketType::Dto);
        assert_eq!(messages[0].pid, 0xFB);
    }

    #[test]
    fn test_multiple_packets_in_one_udp() {
        // Multiple XCP packets in one UDP packet:
        // 1. CTO response: len=1, counter=1, PID=0xFF
        // 2. DAQ packet: len=2, counter=2, PID=0xC0 + 1 byte data
        let multi_packet = vec![
            0x01, 0x00, 0x01, 0x00, 0xFF,  // CTO: len=1, counter=1, PID=0xFF
            0x02, 0x00, 0x02, 0x00, 0xC0, 0xAB,  // DAQ: len=2, counter=2, PID=0xC0, data=0xAB
        ];
        let messages = XcpMessage::parse_packets(multi_packet);
        assert_eq!(messages.len(), 2);

        // First packet should be CTO
        assert_eq!(messages[0].packet_type, PacketType::Cto);
        assert_eq!(messages[0].pid, 0xFF);
        assert_eq!(messages[0].data.len(), 5); // 4 (header) + 1 (payload)

        // Second packet should be DTO
        assert_eq!(messages[1].packet_type, PacketType::Dto);
        assert_eq!(messages[1].pid, 0xC0);
        assert_eq!(messages[1].data.len(), 6); // 4 (header) + 2 (payload)
    }
}
