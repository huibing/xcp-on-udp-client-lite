//! Transport abstraction layer for XCP
//!
//! Provides a unified interface for different transport protocols (UDP, CAN, etc.)

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Transport type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TransportType {
    Udp,
    Can,
}

impl TransportType {
    pub fn as_str(&self) -> &'static str {
        match self {
            TransportType::Udp => "UDP",
            TransportType::Can => "CAN",
        }
    }
}

/// Transport configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransportConfig {
    Udp { addr: String },
    Can { channel: String, can_id: u32 },
}

impl TransportConfig {
    pub fn transport_type(&self) -> TransportType {
        match self {
            TransportConfig::Udp { .. } => TransportType::Udp,
            TransportConfig::Can { .. } => TransportType::Can,
        }
    }

    /// Get display address string
    pub fn addr_string(&self) -> String {
        match self {
            TransportConfig::Udp { addr } => addr.clone(),
            TransportConfig::Can { channel, can_id } => {
                format!("{}:{:03X}", channel, can_id)
            }
        }
    }
}

/// Transport errors
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("Invalid address: {0}")]
    InvalidAddress(String),

    #[error("Failed to bind socket: {0}")]
    BindFailed(String),

    #[error("Failed to connect: {0}")]
    ConnectFailed(String),

    #[error("Failed to send: {0}")]
    SendFailed(String),

    #[error("Failed to receive: {0}")]
    RecvFailed(String),

    #[error("Timeout")]
    Timeout,

    #[error("Socket error: {0}")]
    SocketError(String),

    #[error("Not implemented: {0}")]
    NotImplemented(String),

    #[error("Connection closed")]
    ConnectionClosed,
}

/// Transport trait - abstraction for different transport protocols
#[async_trait]
pub trait Transport: Send + Sync {
    /// Send a command packet (without waiting for response)
    async fn send_command_only(&self, command: &[u8]) -> Result<(), TransportError>;

    /// Send a command and wait for response
    async fn send_command(&self, command: &[u8]) -> Result<Vec<u8>, TransportError>;

    /// Receive a packet (blocking)
    async fn recv_packet(&self) -> Result<Vec<u8>, TransportError>;

    /// Try to receive a packet with timeout
    async fn try_recv_packet(&self, timeout_ms: u64) -> Result<Option<Vec<u8>>, TransportError>;

    /// Close the connection
    async fn close(&self) -> Result<(), TransportError>;

    /// Get transport type
    fn transport_type(&self) -> TransportType;

    /// Get target address string (for logging/display)
    fn target_addr(&self) -> String;

    /// Get current packet counter
    fn current_counter(&self) -> u16;
}

/// Transport factory for creating transport instances
pub struct TransportFactory;

impl TransportFactory {
    /// Create a new transport instance based on configuration
    pub async fn create(config: &TransportConfig) -> Result<Arc<dyn Transport>, TransportError> {
        match config {
            TransportConfig::Udp { addr } => {
                let transport = super::udp::UdpTransport::connect(addr).await?;
                Ok(Arc::new(transport))
            }
            TransportConfig::Can { .. } => Err(TransportError::NotImplemented(
                "CAN transport not yet implemented".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_type_as_str() {
        assert_eq!(TransportType::Udp.as_str(), "UDP");
        assert_eq!(TransportType::Can.as_str(), "CAN");
    }

    #[test]
    fn test_transport_config_udp() {
        let config = TransportConfig::Udp {
            addr: "127.0.0.1:5555".to_string(),
        };
        assert_eq!(config.transport_type(), TransportType::Udp);
        assert_eq!(config.addr_string(), "127.0.0.1:5555");
    }

    #[test]
    fn test_transport_config_can() {
        let config = TransportConfig::Can {
            channel: "can0".to_string(),
            can_id: 0x123,
        };
        assert_eq!(config.transport_type(), TransportType::Can);
        assert_eq!(config.addr_string(), "can0:123");
    }

    #[test]
    fn test_transport_error_display() {
        let err = TransportError::InvalidAddress("test".to_string());
        assert!(err.to_string().contains("Invalid address"));

        let err = TransportError::Timeout;
        assert_eq!(err.to_string(), "Timeout");

        let err = TransportError::NotImplemented("CAN".to_string());
        assert!(err.to_string().contains("Not implemented"));
        assert!(err.to_string().contains("CAN"));
    }

    #[test]
    fn test_transport_type_equality() {
        assert_eq!(TransportType::Udp, TransportType::Udp);
        assert_eq!(TransportType::Can, TransportType::Can);
        assert_ne!(TransportType::Udp, TransportType::Can);
    }

    #[test]
    fn test_transport_config_clone() {
        let config = TransportConfig::Udp {
            addr: "192.168.1.1:5555".to_string(),
        };
        let cloned = config.clone();
        assert_eq!(config.addr_string(), cloned.addr_string());
    }

    #[test]
    fn test_transport_type_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(TransportType::Udp);
        set.insert(TransportType::Can);
        assert_eq!(set.len(), 2);
    }
}
