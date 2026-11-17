//! XCP on Ethernet over UDP.
//!
//! Each datagram may carry several XCP frames; [`EthFrameParser`] splits them and the extra
//! payloads are queued inside the receiver, which owns that state exclusively.

use super::codec::{build_frame, EthFrameParser};
use crate::transport::error::TransportError;
use crate::transport::link::{ReceivedPacket, TransportLink, TransportReceiver, TransportSender};
use crate::transport::TransportType;
use async_trait::async_trait;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use tokio::net::UdpSocket;

/// Maximum DTO payload we accept from one datagram.
const MAX_DTO_SIZE: usize = 1496;

/// Bind a local port and connect to `addr`, returning the two halves of the link.
pub async fn open(addr: &str) -> Result<TransportLink, TransportError> {
    let remote: SocketAddr = addr.parse().map_err(|e: std::net::AddrParseError| {
        TransportError::InvalidAddress(format!("{addr}: {e}"))
    })?;

    let local = if remote.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };

    let socket = UdpSocket::bind(local)
        .await
        .map_err(|e| TransportError::open(addr, e))?;
    socket
        .connect(remote)
        .await
        .map_err(|e| TransportError::open(addr, e))?;

    let socket = Arc::new(socket);
    let addr = remote.to_string();

    Ok(TransportLink {
        sender: Box::new(UdpSender {
            socket: socket.clone(),
            counter: AtomicU16::new(0),
            addr: addr.clone(),
        }),
        receiver: Box::new(UdpReceiver {
            socket,
            frames: EthFrameParser::new(),
            addr,
        }),
    })
}

/// Send half: writes framed commands on the shared socket.
pub struct UdpSender {
    socket: Arc<UdpSocket>,
    /// Packet counter for outgoing frames (wraps at 65535).
    counter: AtomicU16,
    addr: String,
}

#[async_trait]
impl TransportSender for UdpSender {
    async fn send(&self, payload: &[u8]) -> Result<(), TransportError> {
        let counter = self.counter.fetch_add(1, Ordering::SeqCst);
        let frame = build_frame(payload, counter);

        tracing::trace!(
            "Sending XCP command: counter={}, cmd=0x{:02X}({}), len={}, frame=[{:02X?}]",
            counter,
            payload.first().copied().unwrap_or(0xFF),
            crate::protocol::packet_display::command_name(payload.first().copied().unwrap_or(0xFF)),
            payload.len(),
            frame
        );

        self.socket
            .send(&frame)
            .await
            .map_err(|e| TransportError::send(&self.addr, e))?;

        Ok(())
    }

    async fn close(&self) -> Result<(), TransportError> {
        // UDP is connectionless: nothing to release. The read half ends when it is dropped.
        tracing::trace!("Closing UDP link to {}", self.addr);
        Ok(())
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Udp
    }

    fn target_addr(&self) -> String {
        self.addr.clone()
    }
}

/// Receive half: owns the read side, the pending queue and the counter tracker.
pub struct UdpReceiver {
    socket: Arc<UdpSocket>,
    frames: EthFrameParser,
    addr: String,
}

#[async_trait]
impl TransportReceiver for UdpReceiver {
    async fn recv(&mut self) -> Result<ReceivedPacket, TransportError> {
        loop {
            if let Some(payload) = self.frames.pop() {
                return Ok(ReceivedPacket::new(payload, None));
            }

            let mut buf = [0u8; MAX_DTO_SIZE];
            let len = self
                .socket
                .recv(&mut buf)
                .await
                .map_err(|e| TransportError::recv(&self.addr, e))?;

            self.frames.push_datagram(&buf[..len]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::Duration;

    /// Bind a server socket and a client link to it; returns the server and both halves.
    async fn connect_pair() -> (
        UdpSocket,
        Box<dyn TransportSender>,
        Box<dyn TransportReceiver>,
    ) {
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server_addr = server.local_addr().unwrap();
        let link = open(&server_addr.to_string()).await.unwrap();
        let TransportLink { sender, receiver } = link;
        (server, sender, receiver)
    }

    /// The client socket is connected, so the server learns its address from the first packet.
    async fn learn_client_addr(server: &UdpSocket, sender: &dyn TransportSender) -> SocketAddr {
        sender.send(&[0xFF]).await.unwrap();
        let mut buf = [0u8; 64];
        let (_, addr) = tokio::time::timeout(Duration::from_secs(1), server.recv_from(&mut buf))
            .await
            .unwrap()
            .unwrap();
        addr
    }

    #[tokio::test]
    async fn test_recv_splits_multiple_payloads_from_one_datagram() {
        let (server, sender, mut receiver) = connect_pair().await;
        let client_addr = learn_client_addr(&server, sender.as_ref()).await;

        let mut datagram = build_frame(&[0xFF, 0xAA], 0);
        datagram.extend_from_slice(&build_frame(&[0xC0, 0x10, 0x11], 1));
        server.send_to(&datagram, client_addr).await.unwrap();

        let first = receiver.recv().await.unwrap();
        let second = receiver.recv().await.unwrap();
        assert_eq!(first.payload, vec![0xFF, 0xAA]);
        assert_eq!(second.payload, vec![0xC0, 0x10, 0x11]);
        assert_eq!(first.device_timestamp_us, None);
    }

    #[tokio::test]
    async fn test_send_writes_framed_command_with_increasing_counter() {
        let (server, sender, _receiver) = connect_pair().await;

        sender.send(&[0xFF, 0x01]).await.unwrap();
        sender.send(&[0xFE]).await.unwrap();

        let mut buf = [0u8; 64];
        let len = tokio::time::timeout(Duration::from_secs(1), server.recv_from(&mut buf))
            .await
            .unwrap()
            .unwrap()
            .0;
        assert_eq!(&buf[..len], &build_frame(&[0xFF, 0x01], 0));

        let len = tokio::time::timeout(Duration::from_secs(1), server.recv_from(&mut buf))
            .await
            .unwrap()
            .unwrap()
            .0;
        assert_eq!(&buf[..len], &build_frame(&[0xFE], 1));
    }

    #[tokio::test]
    async fn test_incomplete_datagram_is_dropped_and_next_one_still_arrives() {
        let (server, sender, mut receiver) = connect_pair().await;
        let client_addr = learn_client_addr(&server, sender.as_ref()).await;

        // A frame declaring 4 payload bytes but carrying 2.
        server
            .send_to(&[0x04, 0x00, 0x01, 0x00, 0xC0, 0x10], client_addr)
            .await
            .unwrap();
        server
            .send_to(&build_frame(&[0xFF], 2), client_addr)
            .await
            .unwrap();

        let packet = receiver.recv().await.unwrap();
        assert_eq!(packet.payload, vec![0xFF]);
    }
}