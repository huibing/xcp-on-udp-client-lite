//! TCP Transport for XCP on Ethernet
//!
//! XCP on TCP uses the same 4-byte transport layer header as XCP on UDP:
//! - LEN (2 bytes, little-endian): Length of XCP packet payload
//! - PACKET_COUNTER (2 bytes, little-endian): Packet sequence counter
//!
//! Unlike UDP datagrams, TCP delivers a byte stream, so this transport keeps an
//! internal read buffer and emits normalized XCP payload packets one at a time.

use super::transport::{Transport, TransportError, TransportType};
use async_trait::async_trait;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::timeout;

/// Default timeout for XCP commands (T1 = 1000ms)
const DEFAULT_TIMEOUT_MS: u64 = 1000;

const XCP_TCP_HEADER_SIZE: usize = 4;
const TCP_READ_CHUNK_SIZE: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
struct TcpXcpPacket {
    counter: u16,
    payload: Vec<u8>,
}

#[derive(Debug, Clone)]
struct CounterWarning {
    expected: u16,
    received: u16,
    gap: u16,
}

#[derive(Default)]
struct ReceivedCounterTracker {
    last_received: Option<u16>,
}

impl ReceivedCounterTracker {
    fn observe(&mut self, received: u16) -> Option<CounterWarning> {
        let warning = self.last_received.and_then(|previous| {
            let expected = previous.wrapping_add(1);
            let wrapped = previous == u16::MAX && received == 0;

            if received == expected || wrapped {
                None
            } else {
                Some(CounterWarning {
                    expected,
                    received,
                    gap: received.wrapping_sub(expected),
                })
            }
        });

        self.last_received = Some(received);
        warning
    }
}

struct TcpReadState {
    reader: OwnedReadHalf,
    buffer: Vec<u8>,
    pending_packets: VecDeque<Vec<u8>>,
    received_counter_tracker: ReceivedCounterTracker,
}

impl TcpReadState {
    fn new(reader: OwnedReadHalf) -> Self {
        Self {
            reader,
            buffer: Vec::new(),
            pending_packets: VecDeque::new(),
            received_counter_tracker: ReceivedCounterTracker::default(),
        }
    }
}

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

fn try_take_xcp_frame(buffer: &[u8]) -> Option<(TcpXcpPacket, usize)> {
    if buffer.len() < XCP_TCP_HEADER_SIZE {
        return None;
    }

    let len = u16::from_le_bytes([buffer[0], buffer[1]]) as usize;
    let counter = u16::from_le_bytes([buffer[2], buffer[3]]);
    let expected_len = XCP_TCP_HEADER_SIZE + len;

    if buffer.len() < expected_len {
        return None;
    }

    Some((
        TcpXcpPacket {
            counter,
            payload: buffer[XCP_TCP_HEADER_SIZE..expected_len].to_vec(),
        },
        expected_len,
    ))
}

/// TCP Transport for XCP on Ethernet communication.
pub struct TcpTransport {
    remote_addr: SocketAddr,
    timeout_ms: u64,
    counter: AtomicU16,
    writer: Mutex<OwnedWriteHalf>,
    reader_state: Mutex<TcpReadState>,
}

impl TcpTransport {
    fn new(remote_addr: SocketAddr, reader: OwnedReadHalf, writer: OwnedWriteHalf) -> Self {
        Self {
            remote_addr,
            timeout_ms: DEFAULT_TIMEOUT_MS,
            counter: AtomicU16::new(0),
            writer: Mutex::new(writer),
            reader_state: Mutex::new(TcpReadState::new(reader)),
        }
    }

    /// Extract as many complete XCP-on-TCP frames as possible from the buffered byte stream.
    fn extract_buffered_packets(read_state: &mut TcpReadState) {
        while let Some((packet, consumed)) = try_take_xcp_frame(&read_state.buffer) {
            if let Some(warning) = read_state.received_counter_tracker.observe(packet.counter) {
                tracing::warn!(
                    "Slave TCP counter discontinuity: expected {}, received {} (gap: {})",
                    warning.expected,
                    warning.received,
                    warning.gap
                );
            }

            read_state.pending_packets.push_back(packet.payload);
            read_state.buffer.drain(..consumed);
        }
    }

    /// Read from the TCP stream until either one normalized XCP payload is available,
    /// the timeout expires, or the socket closes.
    async fn recv_payload_packet(&self, timeout_ms: Option<u64>) -> Result<Option<Vec<u8>>, TransportError> {
        let mut read_state = self.reader_state.lock().await;

        loop {
            if let Some(payload) = read_state.pending_packets.pop_front() {
                return Ok(Some(payload));
            }

            let mut chunk = [0u8; TCP_READ_CHUNK_SIZE];
            let bytes_read = match timeout_ms {
                Some(timeout_ms) => match timeout(Duration::from_millis(timeout_ms), async {
                    read_state.reader.read(&mut chunk).await
                })
                .await
                {
                    Ok(Ok(len)) => len,
                    Ok(Err(e)) => return Err(TransportError::RecvFailed(e.to_string())),
                    Err(_) => return Ok(None),
                },
                None => read_state
                    .reader
                    .read(&mut chunk)
                    .await
                    .map_err(|e: std::io::Error| TransportError::RecvFailed(e.to_string()))?,
            };

            if bytes_read == 0 {
                return Err(TransportError::ConnectionClosed);
            }

            read_state.buffer.extend_from_slice(&chunk[..bytes_read]);
            Self::extract_buffered_packets(&mut read_state);
        }
    }

    pub async fn connect(remote_addr: &str) -> Result<Self, TransportError> {
        let remote: SocketAddr = remote_addr
            .parse()
            .map_err(|e: std::net::AddrParseError| TransportError::InvalidAddress(e.to_string()))?;

        let stream = TcpStream::connect(remote)
            .await
            .map_err(|e: std::io::Error| TransportError::ConnectFailed(e.to_string()))?;
        stream
            .set_nodelay(true)
            .map_err(|e| TransportError::SocketError(e.to_string()))?;
        let (reader, writer) = stream.into_split();

        Ok(Self::new(remote, reader, writer))
    }

    /// Set custom timeout.
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

    fn build_next_frame(&self, payload: &[u8]) -> Vec<u8> {
        let counter = self.counter.fetch_add(1, Ordering::SeqCst);
        build_xcp_frame(payload, counter)
    }

    /// Send a command only (without waiting for response).
    pub async fn send_command_only(&self, command: &[u8]) -> Result<(), TransportError> {
        let frame = self.build_next_frame(command);
        let mut writer = self.writer.lock().await;
        writer
            .write_all(&frame)
            .await
            .map_err(|e: std::io::Error| TransportError::SendFailed(e.to_string()))?;
        writer
            .flush()
            .await
            .map_err(|e: std::io::Error| TransportError::SendFailed(e.to_string()))?;
        Ok(())
    }

    /// Send a command and wait for one normalized XCP response payload.
    pub async fn send_command(&self, command: &[u8]) -> Result<Vec<u8>, TransportError> {
        self.send_command_only(command).await?;
        match self.recv_payload_packet(Some(self.timeout_ms)).await? {
            Some(payload) => Ok(payload),
            None => Err(TransportError::Timeout),
        }
    }
}

#[async_trait]
impl Transport for TcpTransport {
    async fn send_command_only(&self, command: &[u8]) -> Result<(), TransportError> {
        self.send_command_only(command).await
    }

    async fn send_command(&self, command: &[u8]) -> Result<Vec<u8>, TransportError> {
        self.send_command(command).await
    }

    async fn recv_packet(&self) -> Result<Vec<u8>, TransportError> {
        match self.recv_payload_packet(None).await? {
            Some(payload) => Ok(payload),
            None => Err(TransportError::ConnectionClosed),
        }
    }

    async fn try_recv_packet(&self, timeout_ms: u64) -> Result<Option<Vec<u8>>, TransportError> {
        self.recv_payload_packet(Some(timeout_ms)).await
    }

    async fn close(&self) -> Result<(), TransportError> {
        let mut writer = self.writer.lock().await;
        writer
            .shutdown()
            .await
            .map_err(|e: std::io::Error| TransportError::SocketError(e.to_string()))
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::time::{sleep, Duration};

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
    async fn test_send_command_only_writes_framed_command() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 7];
            socket.read_exact(&mut buf).await.unwrap();
            assert_eq!(buf.to_vec(), vec![0x03, 0x00, 0x00, 0x00, 0xFF, 0x01, 0x02]);
        });

        let transport = TcpTransport::connect(&addr.to_string()).await.unwrap();
        transport.send_command_only(&[0xFF, 0x01, 0x02]).await.unwrap();
        server.await.unwrap();
        assert_eq!(transport.current_counter(), 1);
    }

    #[tokio::test]
    async fn test_send_command_returns_payload_response() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();

            let mut request = [0u8; 5];
            socket.read_exact(&mut request).await.unwrap();
            assert_eq!(request.to_vec(), vec![0x01, 0x00, 0x00, 0x00, 0xF5]);

            let response = build_xcp_frame(&[0xFF, 0xAA], 7);
            socket.write_all(&response[..3]).await.unwrap();
            socket.write_all(&response[3..]).await.unwrap();
        });

        let transport = TcpTransport::connect(&addr.to_string()).await.unwrap();
        let payload = transport.send_command(&[0xF5]).await.unwrap();
        assert_eq!(payload, vec![0xFF, 0xAA]);
        assert_eq!(transport.current_counter(), 1);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_recv_packet_buffers_multiple_frames_from_stream() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();

            let first = build_xcp_frame(&[0xFF], 1);
            let second = build_xcp_frame(&[0xC0, 0x11], 2);
            let mut combined = first;
            combined.extend_from_slice(&second);

            socket.write_all(&combined[..5]).await.unwrap();
            socket.write_all(&combined[5..]).await.unwrap();
        });

        let transport = TcpTransport::connect(&addr.to_string()).await.unwrap();
        let first = transport.recv_packet().await.unwrap();
        let second = transport.recv_packet().await.unwrap();
        assert_eq!(first, vec![0xFF]);
        assert_eq!(second, vec![0xC0, 0x11]);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_try_recv_packet_times_out() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            sleep(Duration::from_millis(50)).await;
        });

        let transport = TcpTransport::connect(&addr.to_string()).await.unwrap();
        let result = transport.try_recv_packet(10).await.unwrap();
        assert_eq!(result, None);
        server.await.unwrap();
    }
}
