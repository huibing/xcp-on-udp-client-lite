//! Message Router - Unified CTO/DAQ message handling
//!
//! This module provides a unified message receiver that:
//! 1. Receives normalized XCP payload packets from the transport
//! 2. Parses the PID to determine if it's a CTO response or DAQ packet
//! 3. Routes messages to the appropriate channel
//!
//! PID classification:
//! - PID >= 0xFC: CTO responses (ERR, EV, SERV, RES)
//! - PID < 0xFC: DAQ packets (DTO)

use crate::transport::Transport;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;

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
    pub payload: Vec<u8>,
}

impl XcpMessage {
    fn from_payload(payload: Vec<u8>) -> Option<Self> {
        if payload.is_empty() {
            return None;
        }

        let pid = payload[0];

        // Determine packet type based on PID
        // PID >= 0xFC: CTO (Command Transfer Object)
        // PID < 0xFC: DTO (Data Transfer Object / DAQ)
        let packet_type = if pid >= 0xFC {
            PacketType::Cto
        } else {
            PacketType::Dto
        };

        Some(XcpMessage {
            packet_type,
            pid,
            payload,
        })
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

        self.running
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let running = self.running.clone();
        let cto_tx = self.cto_tx.clone();
        let dto_tx = self.dto_tx.clone();

        let handle = tokio::spawn(async move {
            tracing::info!("Message router started");

            while running.load(std::sync::atomic::Ordering::SeqCst) {
                // Receive one normalized XCP payload packet from transport
                match transport.recv_packet().await {
                    Ok(payload) => {
                        tracing::trace!("Received XCP payload packet: {} bytes", payload.len());

                        let Some(msg) = XcpMessage::from_payload(payload) else {
                            tracing::warn!("Received empty XCP payload packet from transport");
                            continue;
                        };

                        tracing::trace!(
                            "Routing XCP message: type={:?}, pid=0x{:02X}, len={}",
                            msg.packet_type,
                            msg.pid,
                            msg.payload.len()
                        );

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

    /// Stop the router
    pub async fn stop(&self) {
        self.running
            .store(false, std::sync::atomic::Ordering::SeqCst);

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
    fn test_packet_type_detection_from_payload() {
        let cto = XcpMessage::from_payload(vec![0xFF, 0x00, 0x01, 0x02]).unwrap();
        assert_eq!(cto.packet_type, PacketType::Cto);
        assert_eq!(cto.pid, 0xFF);
        assert_eq!(cto.payload, vec![0xFF, 0x00, 0x01, 0x02]);

        let dto = XcpMessage::from_payload(vec![0xC0, 0x01, 0x02, 0x03, 0x04]).unwrap();
        assert_eq!(dto.packet_type, PacketType::Dto);
        assert_eq!(dto.pid, 0xC0);
        assert_eq!(dto.payload, vec![0xC0, 0x01, 0x02, 0x03, 0x04]);

        let boundary = XcpMessage::from_payload(vec![0xFB, 0x01, 0x02]).unwrap();
        assert_eq!(boundary.packet_type, PacketType::Dto);
        assert_eq!(boundary.pid, 0xFB);
    }

    #[test]
    fn test_empty_payload_is_rejected() {
        assert!(XcpMessage::from_payload(Vec::new()).is_none());
    }
}
