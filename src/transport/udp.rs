//! UDP Transport for XCP on Ethernet
//!
//! XCP on Ethernet (UDP/TCP) uses a 4-byte transport layer header:
//! - LEN (2 bytes, little-endian): Length of XCP packet (command/response data)
//! - PACKET_COUNTER (2 bytes, little-endian): Counter for packet sequence (0-65535, wraps)

use super::transport::{Transport, TransportError, TransportType};
use async_trait::async_trait;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tokio::time::timeout;

/// Default timeout for XCP commands (T1 = 1000ms)
const DEFAULT_TIMEOUT_MS: u64 = 1000;

/// Maximum DTO (Data Transfer Object) size
const MAX_DTO_SIZE: usize = 1496;

const XCP_UDP_HEADER_SIZE: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
struct UdpXcpPacket {
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

fn build_xcp_frame(payload: &[u8], counter: u16) -> Vec<u8> {
    let len = payload.len() as u16;
    let mut packet = Vec::with_capacity(XCP_UDP_HEADER_SIZE + payload.len());
    packet.push((len & 0xFF) as u8);
    packet.push((len >> 8) as u8);
    packet.push((counter & 0xFF) as u8);
    packet.push((counter >> 8) as u8);
    packet.extend_from_slice(payload);
    packet
}

fn parse_xcp_frame(datagram: &[u8], offset: usize) -> Option<(UdpXcpPacket, usize)> {
    if offset + XCP_UDP_HEADER_SIZE > datagram.len() {
        return None;
    }

    let len = u16::from_le_bytes([datagram[offset], datagram[offset + 1]]) as usize;
    let counter = u16::from_le_bytes([datagram[offset + 2], datagram[offset + 3]]);
    let next_offset = offset + XCP_UDP_HEADER_SIZE + len;

    if next_offset > datagram.len() {
        tracing::warn!(
            "Incomplete XCP-on-UDP frame: expected {} bytes, got {}",
            XCP_UDP_HEADER_SIZE + len,
            datagram.len().saturating_sub(offset)
        );
        return None;
    }

    Some((
        UdpXcpPacket {
            counter,
            payload: datagram[offset + XCP_UDP_HEADER_SIZE..next_offset].to_vec(),
        },
        next_offset,
    ))
}

fn parse_xcp_datagram(datagram: &[u8]) -> Vec<UdpXcpPacket> {
    let mut packets = Vec::new();
    let mut offset = 0;

    while offset < datagram.len() {
        match parse_xcp_frame(datagram, offset) {
            Some((packet, next_offset)) => {
                packets.push(packet);
                offset = next_offset;
            }
            None => {
                if offset < datagram.len() {
                    tracing::warn!(
                        "Failed to parse XCP-on-UDP frame at offset {}, remaining {} bytes",
                        offset,
                        datagram.len() - offset
                    );
                }
                break;
            }
        }
    }

    packets
}

/// UDP Transport for XCP on Ethernet communication
pub struct UdpTransport {
    socket: UdpSocket,
    remote_addr: SocketAddr,
    timeout_ms: u64,
    /// Packet counter for command packets (0-65535, wraps around)
    counter: AtomicU16,
    /// Extra decoded payload packets from a previously received UDP datagram
    pending_packets: Mutex<VecDeque<Vec<u8>>>,
    /// Tracks incoming UDP counters without exposing them outside the transport
    received_counter_tracker: Mutex<ReceivedCounterTracker>,
}

impl UdpTransport {
    /// Create a new UDP transport
    /// Binds to a random local port and connects to the specified remote address
    pub async fn connect(remote_addr: &str) -> Result<Self, TransportError> {
        let remote: SocketAddr = remote_addr
            .parse()
            .map_err(|e: std::net::AddrParseError| TransportError::InvalidAddress(e.to_string()))?;

        // Bind to any available local port
        let local_addr = if remote.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        };

        let socket = UdpSocket::bind(local_addr)
            .await
            .map_err(|e: std::io::Error| TransportError::BindFailed(e.to_string()))?;

        // Connect to remote (allows using send/recv instead of send_to/recv_from)
        socket
            .connect(remote)
            .await
            .map_err(|e: std::io::Error| TransportError::ConnectFailed(e.to_string()))?;

        Ok(Self {
            socket,
            remote_addr: remote,
            timeout_ms: DEFAULT_TIMEOUT_MS,
            counter: AtomicU16::new(0),
            pending_packets: Mutex::new(VecDeque::new()),
            received_counter_tracker: Mutex::new(ReceivedCounterTracker::default()),
        })
    }

    /// Set custom timeout
    pub fn set_timeout(&mut self, timeout_ms: u64) {
        self.timeout_ms = timeout_ms;
    }

    /// Send a command only (without receiving response)
    /// Used in new architecture where MessageRouter handles all reception
    pub async fn send_command_only(&self, command: &[u8]) -> Result<(), TransportError> {
        let counter = self.counter.fetch_add(1, Ordering::SeqCst);
        let packet = build_xcp_frame(command, counter);

        tracing::trace!(
            "Sending XCP command: counter={}, cmd=0x{:02X}({}), len={}, packet=[{:02X?}]",
            counter,
            command.first().copied().unwrap_or(0xFF),
            command_name(command.first().copied().unwrap_or(0xFF)),
            command.len(),
            packet
        );

        self.socket
            .send(&packet)
            .await
            .map_err(|e: std::io::Error| TransportError::SendFailed(e.to_string()))?;

        tracing::trace!("XCP command sent successfully: counter={}", counter);

        Ok(())
    }

    async fn decode_datagram_payloads(
        &self,
        datagram: &[u8],
    ) -> Result<VecDeque<Vec<u8>>, TransportError> {
        let packets = parse_xcp_datagram(datagram);
        if packets.is_empty() {
            return Err(TransportError::RecvFailed(
                "Received datagram without a complete XCP frame".to_string(),
            ));
        }

        let mut tracker = self.received_counter_tracker.lock().await;
        let mut payloads = VecDeque::with_capacity(packets.len());

        for packet in packets {
            if let Some(warning) = tracker.observe(packet.counter) {
                tracing::warn!(
                    "Slave UDP counter discontinuity: expected {}, received {} (gap: {})",
                    warning.expected,
                    warning.received,
                    warning.gap
                );
            }
            payloads.push_back(packet.payload);
        }

        Ok(payloads)
    }

    async fn queue_extra_payloads(&self, payloads: VecDeque<Vec<u8>>) {
        if payloads.is_empty() {
            return;
        }

        let mut pending_packets = self.pending_packets.lock().await;
        pending_packets.extend(payloads);
    }

    async fn receive_payload_from_socket(
        &self,
        timeout_ms: Option<u64>,
    ) -> Result<Option<Vec<u8>>, TransportError> {
        let mut buf = vec![0u8; MAX_DTO_SIZE];
        let len = match timeout_ms {
            Some(timeout_ms) => match timeout(Duration::from_millis(timeout_ms), async {
                self.socket.recv(&mut buf).await
            })
            .await
            {
                Ok(Ok(len)) => len,
                Ok(Err(e)) => return Err(TransportError::RecvFailed(e.to_string())),
                Err(_) => return Ok(None),
            },
            None => self
                .socket
                .recv(&mut buf)
                .await
                .map_err(|e: std::io::Error| TransportError::RecvFailed(e.to_string()))?,
        };

        buf.truncate(len);

        let mut payloads = self.decode_datagram_payloads(&buf).await?;
        let first_payload = payloads
            .pop_front()
            .expect("decode_datagram_payloads guarantees at least one payload");

        if !payloads.is_empty() {
            self.queue_extra_payloads(payloads).await;
        }

        Ok(Some(first_payload))
    }

    async fn recv_payload_packet(
        &self,
        timeout_ms: Option<u64>,
    ) -> Result<Option<Vec<u8>>, TransportError> {
        {
            let mut pending_packets = self.pending_packets.lock().await;
            if let Some(payload) = pending_packets.pop_front() {
                return Ok(Some(payload));
            }
        }

        self.receive_payload_from_socket(timeout_ms).await
    }

    /// Send a command and receive the response payload.
    ///
    /// This method adds the XCP on Ethernet transport layer header:
    /// [LEN_L, LEN_H, COUNTER_L, COUNTER_H, command...]
    pub async fn send_command(&self, command: &[u8]) -> Result<Vec<u8>, TransportError> {
        let counter = self.counter.fetch_add(1, Ordering::SeqCst);
        let packet = build_xcp_frame(command, counter);

        self.socket
            .send(&packet)
            .await
            .map_err(|e: std::io::Error| TransportError::SendFailed(e.to_string()))?;

        match self
            .receive_payload_from_socket(Some(self.timeout_ms))
            .await?
        {
            Some(payload) => Ok(payload),
            None => Err(TransportError::Timeout),
        }
    }

    /// Get the remote address
    pub fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }

    /// Get the local address
    pub async fn local_addr(&self) -> Result<SocketAddr, TransportError> {
        self.socket
            .local_addr()
            .map_err(|e| TransportError::SocketError(e.to_string()))
    }

    /// Get the current counter value (next packet will use this counter)
    pub fn current_counter(&self) -> u16 {
        self.counter.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Transport for UdpTransport {
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
        // UDP is connectionless, just log the close operation
        tracing::trace!("Closing UDP transport to {}", self.remote_addr);
        Ok(())
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Udp
    }

    fn target_addr(&self) -> String {
        self.remote_addr.to_string()
    }
}

/// Get human-readable command name from command code
fn command_name(cmd_code: u8) -> &'static str {
    match cmd_code {
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
        0xD7 => "GET_DAQ_EVENT_INFO",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_xcp_frame_adds_udp_header() {
        let payload = vec![0xFF, 0x01, 0x02];
        let frame = build_xcp_frame(&payload, 0x1234);
        assert_eq!(frame, vec![0x03, 0x00, 0x34, 0x12, 0xFF, 0x01, 0x02]);
    }

    #[test]
    fn test_parse_xcp_datagram_strips_headers_and_preserves_counter() {
        let datagram = vec![
            0x04, 0x00, 0x01, 0x00, 0xFF, 0xAA, 0xBB, 0xCC, 0x03, 0x00, 0x02, 0x00, 0xC0, 0x10,
            0x11,
        ];

        let packets = parse_xcp_datagram(&datagram);
        assert_eq!(packets.len(), 2);

        assert_eq!(packets[0].counter, 1);
        assert_eq!(packets[0].payload, vec![0xFF, 0xAA, 0xBB, 0xCC]);

        assert_eq!(packets[1].counter, 2);
        assert_eq!(packets[1].payload, vec![0xC0, 0x10, 0x11]);
    }

    #[test]
    fn test_parse_xcp_datagram_stops_on_incomplete_frame() {
        let datagram = vec![
            0x01, 0x00, 0x01, 0x00, 0xFF, 0x04, 0x00, 0x02, 0x00, 0xC0, 0x10,
        ];

        let packets = parse_xcp_datagram(&datagram);
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].payload, vec![0xFF]);
    }
}
