//! TCP Transport for XCP on Ethernet
//!
//! XCP on TCP uses the same 4-byte transport layer header as XCP on UDP:
//! - LEN (2 bytes, little-endian): Length of XCP packet payload
//! - PACKET_COUNTER (2 bytes, little-endian): Packet sequence counter
//!
//! This file currently provides only the transport skeleton and framing helpers.
//! Socket I/O is intentionally left unimplemented.

use super::transport::{Transport, TransportError, TransportType};
use async_trait::async_trait;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};

/// Default timeout for XCP commands (T1 = 1000ms)
const DEFAULT_TIMEOUT_MS: u64 = 1000;

const XCP_TCP_HEADER_SIZE: usize = 4;

fn build_xcp_frame(payload: &[u8], counter: u16) -> Vec<u8> {
    let len = payload.len() as u16;
    let mut packet = Vec::with_capacity(XCP_TCP_HEADER_SIZE + payload.len());
    packet.push((len & 0xFF) as u8);
    packet.push((len >> 8) as u8);
    packet.push((counter & 0xFF) as u8);
    packet.push((counter >> 8) as u8);
    packet.extend_from_slice(payload);
    packet
}

#[cfg_attr(not(test), allow(dead_code))]
fn parse_xcp_frame(frame: &[u8]) -> Result<(u16, Vec<u8>), TransportError> {
    if frame.len() < XCP_TCP_HEADER_SIZE {
        return Err(TransportError::RecvFailed(
            "TCP frame too short for XCP transport header".to_string(),
        ));
    }

    let len = u16::from_le_bytes([frame[0], frame[1]]) as usize;
    let counter = u16::from_le_bytes([frame[2], frame[3]]);
    let expected_len = XCP_TCP_HEADER_SIZE + len;

    if frame.len() < expected_len {
        return Err(TransportError::RecvFailed(format!(
            "Incomplete XCP-on-TCP frame: expected {} bytes, got {}",
            expected_len,
            frame.len()
        )));
    }

    Ok((counter, frame[XCP_TCP_HEADER_SIZE..expected_len].to_vec()))
}

/// TCP Transport for XCP on Ethernet communication.
///
/// This is currently a placeholder transport that exposes the expected
/// framing/API surface without implementing socket I/O yet.
pub struct TcpTransport {
    remote_addr: SocketAddr,
    timeout_ms: u64,
    counter: AtomicU16,
}

impl TcpTransport {
    fn new(remote_addr: SocketAddr) -> Self {
        Self {
            remote_addr,
            timeout_ms: DEFAULT_TIMEOUT_MS,
            counter: AtomicU16::new(0),
        }
    }

    /// Create a new TCP transport skeleton.
    ///
    /// The address is validated and stored, but no socket is opened yet.
    pub async fn connect(remote_addr: &str) -> Result<Self, TransportError> {
        let remote: SocketAddr = remote_addr
            .parse()
            .map_err(|e: std::net::AddrParseError| TransportError::InvalidAddress(e.to_string()))?;

        tracing::warn!(
            "TCP transport skeleton created for {}, but socket I/O is not implemented yet",
            remote
        );

        Ok(Self::new(remote))
    }

    /// Set custom timeout for future TCP command I/O.
    pub fn set_timeout(&mut self, timeout_ms: u64) {
        self.timeout_ms = timeout_ms;
    }

    /// Get the configured remote address.
    pub fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }

    /// Get the next outgoing TCP packet counter value.
    pub fn current_counter(&self) -> u16 {
        self.counter.load(Ordering::SeqCst)
    }

    fn next_frame(&self, payload: &[u8]) -> Vec<u8> {
        let counter = self.counter.fetch_add(1, Ordering::SeqCst);
        build_xcp_frame(payload, counter)
    }
}

#[async_trait]
impl Transport for TcpTransport {
    async fn send_command_only(&self, command: &[u8]) -> Result<(), TransportError> {
        let _frame = self.next_frame(command);
        Err(TransportError::NotImplemented(
            "TCP transport send is not yet implemented".to_string(),
        ))
    }

    async fn send_command(&self, command: &[u8]) -> Result<Vec<u8>, TransportError> {
        let _frame = self.next_frame(command);
        Err(TransportError::NotImplemented(
            "TCP transport send/receive is not yet implemented".to_string(),
        ))
    }

    async fn recv_packet(&self) -> Result<Vec<u8>, TransportError> {
        Err(TransportError::NotImplemented(
            "TCP transport receive is not yet implemented".to_string(),
        ))
    }

    async fn try_recv_packet(&self, _timeout_ms: u64) -> Result<Option<Vec<u8>>, TransportError> {
        Err(TransportError::NotImplemented(
            "TCP transport receive is not yet implemented".to_string(),
        ))
    }

    async fn close(&self) -> Result<(), TransportError> {
        tracing::trace!(
            "Closing TCP transport skeleton to {} (timeout={}ms)",
            self.remote_addr,
            self.timeout_ms
        );
        Ok(())
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Tcp
    }

    fn target_addr(&self) -> String {
        self.remote_addr.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_xcp_frame_adds_tcp_header() {
        let payload = vec![0xFF, 0x01, 0x02];
        let frame = build_xcp_frame(&payload, 0x1234);
        assert_eq!(frame, vec![0x03, 0x00, 0x34, 0x12, 0xFF, 0x01, 0x02]);
    }

    #[test]
    fn test_parse_xcp_frame_extracts_counter_and_payload() {
        let frame = vec![0x03, 0x00, 0x34, 0x12, 0xFF, 0x01, 0x02];
        let (counter, payload) = parse_xcp_frame(&frame).unwrap();
        assert_eq!(counter, 0x1234);
        assert_eq!(payload, vec![0xFF, 0x01, 0x02]);
    }

    #[test]
    fn test_parse_xcp_frame_rejects_incomplete_payload() {
        let frame = vec![0x03, 0x00, 0x34, 0x12, 0xFF];
        let err = parse_xcp_frame(&frame).unwrap_err();
        assert!(matches!(err, TransportError::RecvFailed(_)));
    }

    #[tokio::test]
    async fn test_tcp_placeholder_api_methods() {
        let transport = TcpTransport::connect("127.0.0.1:5555").await.unwrap();

        assert_eq!(transport.transport_type(), TransportType::Tcp);
        assert_eq!(transport.target_addr(), "127.0.0.1:5555");

        let send_only = transport.send_command_only(&[0xFF]).await.unwrap_err();
        assert!(matches!(send_only, TransportError::NotImplemented(_)));

        let send_and_recv = transport.send_command(&[0xFF]).await.unwrap_err();
        assert!(matches!(send_and_recv, TransportError::NotImplemented(_)));

        let recv = transport.recv_packet().await.unwrap_err();
        assert!(matches!(recv, TransportError::NotImplemented(_)));

        let try_recv = transport.try_recv_packet(10).await.unwrap_err();
        assert!(matches!(try_recv, TransportError::NotImplemented(_)));

        transport.close().await.unwrap();
        assert_eq!(transport.current_counter(), 2);
    }
}
