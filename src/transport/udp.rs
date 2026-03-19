//! UDP Transport for XCP on Ethernet
//!
//! XCP on Ethernet (UDP/TCP) uses a 4-byte transport layer header:
//! - LEN (2 bytes, little-endian): Length of XCP packet (command/response data)
//! - PACKET_COUNTER (2 bytes, little-endian): Counter for packet sequence (0-65535, wraps)

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tokio::time::timeout;

/// Default timeout for XCP commands (T1 = 1000ms)
const DEFAULT_TIMEOUT_MS: u64 = 1000;

/// Maximum DTO (Data Transfer Object) size
const MAX_DTO_SIZE: usize = 1496;

/// Counter tracking state for monitoring slave counter sequence
struct CounterTracker {
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

    /// Check and update counter, return true if warning should be logged
    fn check_and_update(&mut self, received: u16) -> Option<CounterWarning> {
        if !self.enabled {
            return None;
        }

        let warning = if let Some(expected) = self.expected {
            // Check if counter is sequential (with wrap-around handling)
            let expected_next = expected.wrapping_add(1);

            if received != expected_next {
                // Check for wrap-around case
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
            // First packet, no previous counter to compare
            None
        };

        // Update expected counter for next packet
        self.expected = Some(received);
        warning
    }

    /// Reset counter tracking (e.g., on reconnect)
    fn reset(&mut self) {
        self.expected = None;
    }
}

/// Warning information for counter discontinuity
#[derive(Debug, Clone)]
pub struct CounterWarning {
    pub expected: u16,
    pub received: u16,
    pub gap: u16,
}

/// UDP Transport for XCP on Ethernet communication
pub struct UdpTransport {
    socket: UdpSocket,
    remote_addr: SocketAddr,
    timeout_ms: u64,
    /// Packet counter for command packets (0-65535, wraps around)
    counter: AtomicU16,
    /// Counter tracking for slave responses/DAQ packets
    slave_counter: RwLock<CounterTracker>,
    /// Flag to enable/disable counter tracking
    counter_tracking_enabled: AtomicBool,
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
            slave_counter: RwLock::new(CounterTracker::new()),
            counter_tracking_enabled: AtomicBool::new(true),
        })
    }

    /// Set custom timeout
    pub fn set_timeout(&mut self, timeout_ms: u64) {
        self.timeout_ms = timeout_ms;
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

        // Check slave counter in response
        if buf.len() >= 4 && self.counter_tracking_enabled.load(Ordering::SeqCst) {
            let received_counter = buf[2] as u16 | ((buf[3] as u16) << 8);
            let mut tracker = self.slave_counter.write().await;
            if let Some(warning) = tracker.check_and_update(received_counter) {
                tracing::warn!(
                    "Slave counter discontinuity: expected {}, received {} (gap: {})",
                    warning.expected, warning.received, warning.gap
                );
            }
        }

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

        // Check slave counter in DAQ packet
        self.check_slave_counter(&buf).await;

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

                // Check slave counter in DAQ packet
                self.check_slave_counter(&buf).await;

                Ok(Some(buf))
            }
            Ok(Err(e)) => Err(TransportError::RecvFailed(e.to_string())),
            Err(_) => Ok(None), // Timeout
        }
    }

    /// Check slave counter in received packet and log warning if discontinuity detected
    async fn check_slave_counter(&self, data: &[u8]) {
        if data.len() >= 4 && self.counter_tracking_enabled.load(Ordering::SeqCst) {
            let received_counter = data[2] as u16 | ((data[3] as u16) << 8);
            let mut tracker = self.slave_counter.write().await;
            if let Some(warning) = tracker.check_and_update(received_counter) {
                tracing::warn!(
                    "Slave counter discontinuity: expected {}, received {} (gap: {})",
                    warning.expected, warning.received, warning.gap
                );
            }
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

    /// Enable or disable counter tracking
    pub fn set_counter_tracking(&self, enabled: bool) {
        self.counter_tracking_enabled.store(enabled, Ordering::SeqCst);
    }

    /// Check if counter tracking is enabled
    pub fn is_counter_tracking_enabled(&self) -> bool {
        self.counter_tracking_enabled.load(Ordering::SeqCst)
    }

    /// Reset slave counter tracking state (call on reconnect)
    pub async fn reset_slave_counter(&self) {
        let mut tracker = self.slave_counter.write().await;
        tracker.reset();
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
