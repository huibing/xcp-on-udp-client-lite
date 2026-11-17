//! XCP on Ethernet over TCP.
//!
//! TCP delivers a byte stream, so the receive half keeps a read buffer and emits one normalized
//! payload at a time. `into_split()` gives the two halves directly, which is why TCP needs no
//! conversion device at all.

use super::codec::{build_frame, EthFrameParser};
use crate::transport::error::TransportError;
use crate::transport::link::{ReceivedPacket, TransportLink, TransportReceiver, TransportSender};
use crate::transport::TransportType;
use async_trait::async_trait;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

const TCP_READ_CHUNK_SIZE: usize = 4096;

/// Connect to `addr`, returning the two halves of the link.
pub async fn open(addr: &str) -> Result<TransportLink, TransportError> {
    let remote: SocketAddr = addr.parse().map_err(|e: std::net::AddrParseError| {
        TransportError::InvalidAddress(format!("{addr}: {e}"))
    })?;

    let stream = TcpStream::connect(remote)
        .await
        .map_err(|e| TransportError::open(addr, e))?;
    stream
        .set_nodelay(true)
        .map_err(|e| TransportError::open(addr, e))?;

    let (reader, writer) = stream.into_split();
    let addr = remote.to_string();

    Ok(TransportLink {
        sender: Box::new(TcpSender {
            writer: Mutex::new(writer),
            counter: AtomicU16::new(0),
            addr: addr.clone(),
        }),
        receiver: Box::new(TcpReceiver {
            reader,
            frames: EthFrameParser::new(),
            addr,
        }),
    })
}

/// Send half: serializes writes onto the write half.
pub struct TcpSender {
    /// This `Mutex` is genuinely required: writing needs `&mut OwnedWriteHalf` across `.await`.
    writer: Mutex<OwnedWriteHalf>,
    counter: AtomicU16,
    addr: String,
}

#[async_trait]
impl TransportSender for TcpSender {
    async fn send(&self, payload: &[u8]) -> Result<(), TransportError> {
        let counter = self.counter.fetch_add(1, Ordering::SeqCst);
        let frame = build_frame(payload, counter);

        let mut writer = self.writer.lock().await;
        writer
            .write_all(&frame)
            .await
            .map_err(|e| TransportError::send(&self.addr, e))?;
        writer
            .flush()
            .await
            .map_err(|e| TransportError::send(&self.addr, e))?;
        Ok(())
    }

    async fn close(&self) -> Result<(), TransportError> {
        let mut writer = self.writer.lock().await;
        writer
            .shutdown()
            .await
            .map_err(|e| TransportError::send(&self.addr, e))
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Tcp
    }

    fn target_addr(&self) -> String {
        self.addr.clone()
    }
}

/// Receive half: owns the read half and the stream reassembly buffer.
pub struct TcpReceiver {
    reader: OwnedReadHalf,
    frames: EthFrameParser,
    addr: String,
}

#[async_trait]
impl TransportReceiver for TcpReceiver {
    async fn recv(&mut self) -> Result<ReceivedPacket, TransportError> {
        loop {
            if let Some(payload) = self.frames.pop() {
                return Ok(ReceivedPacket::new(payload, None));
            }

            let mut chunk = [0u8; TCP_READ_CHUNK_SIZE];
            let bytes_read = self
                .reader
                .read(&mut chunk)
                .await
                .map_err(|e| TransportError::recv(&self.addr, e))?;

            if bytes_read == 0 {
                return Err(TransportError::closed(&self.addr));
            }

            self.frames.push_stream(&chunk[..bytes_read]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use tokio::time::{sleep, Duration};

    async fn serve_once<F, Fut>(handler: F) -> (String, tokio::task::JoinHandle<()>)
    where
        F: FnOnce(tokio::net::TcpStream) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            handler(socket).await;
        });
        (addr.to_string(), handle)
    }

    #[tokio::test]
    async fn test_send_writes_framed_command() {
        let (addr, server) = serve_once(|mut socket| async move {
            let mut buf = [0u8; 7];
            socket.read_exact(&mut buf).await.unwrap();
            assert_eq!(buf.to_vec(), build_frame(&[0xFF, 0x01, 0x02], 0));
        })
        .await;

        let TransportLink { sender, .. } = open(&addr).await.unwrap();
        sender.send(&[0xFF, 0x01, 0x02]).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_recv_buffers_multiple_frames_from_stream() {
        let (addr, server) = serve_once(|mut socket| async move {
            let mut combined = build_frame(&[0xFF], 1);
            combined.extend_from_slice(&build_frame(&[0xC0, 0x11], 2));
            // Split inside the first frame to exercise reassembly.
            socket.write_all(&combined[..5]).await.unwrap();
            socket.write_all(&combined[5..]).await.unwrap();
        })
        .await;

        let TransportLink { mut receiver, .. } = open(&addr).await.unwrap();
        assert_eq!(receiver.recv().await.unwrap().payload, vec![0xFF]);
        assert_eq!(receiver.recv().await.unwrap().payload, vec![0xC0, 0x11]);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_peer_close_returns_connection_closed() {
        let (addr, server) = serve_once(|_socket| async move {
            sleep(Duration::from_millis(10)).await;
        })
        .await;

        let TransportLink { mut receiver, .. } = open(&addr).await.unwrap();
        let err = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
            .await
            .expect("recv should not hang")
            .unwrap_err();
        assert!(matches!(err, TransportError::ConnectionClosed { .. }));
        server.await.unwrap();
    }
}