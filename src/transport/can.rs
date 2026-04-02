//! CAN Transport for XCP on CAN
//!
//! XCP on CAN uses standard CAN frames with specific CAN IDs for communication.
//! This is a placeholder implementation for future CAN support.
//!
//! CAN Frame Format for XCP:
//! - CAN ID: Configured per slave (e.g., 0x100 for command, 0x101 for response)
//! - Data: XCP packet data (max 8 bytes per frame for classic CAN)
//!
//! For FD-CAN, the data length can be up to 64 bytes.

use super::transport::{Transport, TransportError, TransportType};
use async_trait::async_trait;

/// CAN Transport for XCP on CAN communication
///
/// Note: This is a placeholder implementation. Full CAN support requires
/// a CAN driver library (e.g., socketcan on Linux).
pub struct CanTransport {
    channel: String,
    can_id: u32,
    #[allow(dead_code)]
    connected: bool,
}

impl CanTransport {
    /// Create a new CAN transport
    ///
    /// # Arguments
    /// * `channel` - CAN channel name (e.g., "can0" on Linux)
    /// * `can_id` - CAN ID for XCP communication
    ///
    /// # Errors
    /// Returns `TransportError::NotImplemented` as CAN support is not yet implemented
    pub async fn connect(channel: &str, can_id: u32) -> Result<Self, TransportError> {
        tracing::warn!(
            "CAN transport requested for channel {} with ID 0x{:03X}, but not yet implemented",
            channel,
            can_id
        );

        Err(TransportError::NotImplemented(
            "CAN transport not yet implemented. Please use UDP transport instead.".to_string(),
        ))
    }

    /// Get the CAN channel name
    pub fn channel(&self) -> &str {
        &self.channel
    }

    /// Get the CAN ID
    pub fn can_id(&self) -> u32 {
        self.can_id
    }
}

#[async_trait]
impl Transport for CanTransport {
    async fn send_command_only(&self, _command: &[u8]) -> Result<(), TransportError> {
        Err(TransportError::NotImplemented(
            "CAN transport not yet implemented".to_string(),
        ))
    }

    async fn send_command(&self, _command: &[u8]) -> Result<Vec<u8>, TransportError> {
        Err(TransportError::NotImplemented(
            "CAN transport not yet implemented".to_string(),
        ))
    }

    async fn recv_packet(&self) -> Result<Vec<u8>, TransportError> {
        Err(TransportError::NotImplemented(
            "CAN transport not yet implemented".to_string(),
        ))
    }

    async fn try_recv_packet(&self, _timeout_ms: u64) -> Result<Option<Vec<u8>>, TransportError> {
        Err(TransportError::NotImplemented(
            "CAN transport not yet implemented".to_string(),
        ))
    }

    async fn close(&self) -> Result<(), TransportError> {
        tracing::trace!("Closing CAN transport on {} (ID: 0x{:03X})", self.channel, self.can_id);
        Ok(())
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Can
    }

    fn target_addr(&self) -> String {
        format!("{}:0x{:03X}", self.channel, self.can_id)
    }

    fn current_counter(&self) -> u16 {
        // CAN doesn't use packet counters like UDP
        0
    }
}

/// Create a CAN transport from configuration
#[allow(dead_code)]
pub async fn create_can_transport(
    channel: &str,
    can_id: u32,
) -> Result<CanTransport, TransportError> {
    CanTransport::connect(channel, can_id).await
}
