//! UDP Transport for XCP on Ethernet
//!
//! XCP on Ethernet (UDP/TCP) uses a 4-byte transport layer header:
//! - LEN (2 bytes, little-endian): Length of XCP packet (command/response data)
//! - PACKET_COUNTER (2 bytes, little-endian): Counter for packet sequence (0-65535, wraps)

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time::timeout;

/// Default timeout for XCP commands (T1 = 1000ms)
const DEFAULT_TIMEOUT_MS: u64 = 1000;

/// Maximum DTO (Data Transfer Object) size
const MAX_DTO_SIZE: usize = 1496;

/// UDP Transport for XCP on Ethernet communication
pub struct UdpTransport {
    socket: UdpSocket,
    remote_addr: SocketAddr,
    timeout_ms: u64,
    /// Packet counter for command packets (0-65535, wraps around)
    counter: AtomicU16,
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
        })
    }

    /// Set custom timeout
    pub fn set_timeout(&mut self, timeout_ms: u64) {
        self.timeout_ms = timeout_ms;
    }

    /// Send a command only (without receiving response)
    /// Used in new architecture where MessageRouter handles all reception
    pub async fn send_command_only(&self, command: &[u8]) -> Result<(), TransportError> {
        // Build XCP on Ethernet header
        let len = command.len() as u16;
        let counter = self.counter.fetch_add(1, Ordering::SeqCst);

        let mut packet = Vec::with_capacity(4 + command.len());
        packet.push((len & 0xFF) as u8);
        packet.push((len >> 8) as u8);
        packet.push((counter & 0xFF) as u8);
        packet.push((counter >> 8) as u8);
        packet.extend_from_slice(command);

        tracing::debug!(
            "Sending XCP command: counter={}, cmd=0x{:02X}({}), len={}, packet=[{:02X?}]",
            counter,
            command.first().copied().unwrap_or(0xFF),
            command_name(command.first().copied().unwrap_or(0xFF)),
            len,
            packet
        );

        self.socket
            .send(&packet)
            .await
            .map_err(|e: std::io::Error| TransportError::SendFailed(e.to_string()))?;

        tracing::debug!("XCP command sent successfully: counter={}", counter);

        Ok(())
    }

    /// Send a command and receive the response
    ///
    /// This method adds the XCP on Ethernet transport layer header:
    /// [LEN_L, LEN_H, COUNTER_L, COUNTER_H, command...]
    pub async fn send_command(&self, command: &[u8]) -> Result<Vec<u8>, TransportError> {
        // Build XCP on Ethernet header
        // Header: [LEN_L, LEN_H, COUNTER_L, COUNTER_H]
        let len = command.len() as u16;
        let counter = self.counter.fetch_add(1, Ordering::SeqCst);

        let mut packet = Vec::with_capacity(4 + command.len());
        packet.push((len & 0xFF) as u8);           // LEN low byte
        packet.push((len >> 8) as u8);             // LEN high byte
        packet.push((counter & 0xFF) as u8);       // COUNTER low byte
        packet.push((counter >> 8) as u8);         // COUNTER high byte
        packet.extend_from_slice(command);          // XCP command

        // Send command packet
        self.socket
            .send(&packet)
            .await
            .map_err(|e: std::io::Error| TransportError::SendFailed(e.to_string()))?;

        // Receive response with timeout
        let mut buf = vec![0u8; MAX_DTO_SIZE];
        let len = timeout(Duration::from_millis(self.timeout_ms), async {
            self.socket.recv(&mut buf).await
        })
        .await
        .map_err(|_| TransportError::Timeout)?
        .map_err(|e: std::io::Error| TransportError::RecvFailed(e.to_string()))?;

        buf.truncate(len);

        // Note: Counter checking moved to MessageRouter for per-XCP-packet checking
        // (needed when multiple XCP packets are aggregated in one UDP packet)

        Ok(buf)
    }

    /// Receive a DAQ packet (no timeout for continuous reception)
    pub async fn recv_daq_packet(&self) -> Result<Vec<u8>, TransportError> {
        let mut buf = vec![0u8; MAX_DTO_SIZE];
        let len = self
            .socket
            .recv(&mut buf)
            .await
            .map_err(|e: std::io::Error| TransportError::RecvFailed(e.to_string()))?;

        buf.truncate(len);

        Ok(buf)
    }

    /// Try to receive a DAQ packet with timeout
    pub async fn try_recv_daq_packet(&self, timeout_ms: u64) -> Result<Option<Vec<u8>>, TransportError> {
        let mut buf = vec![0u8; MAX_DTO_SIZE];

        match timeout(Duration::from_millis(timeout_ms), async {
            self.socket.recv(&mut buf).await
        })
        .await
        {
            Ok(Ok(len)) => {
                buf.truncate(len);

                Ok(Some(buf))
            }
            Ok(Err(e)) => Err(TransportError::RecvFailed(e.to_string())),
            Err(_) => Ok(None), // Timeout
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
        0xE9 => "COPY_CAL_PAGE",
        0xE4 => "GET_DAQ_EVENT_INFO",
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
}
