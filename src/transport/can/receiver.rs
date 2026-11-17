//! The receive half of an XCP-on-CAN link.

use super::device::CanFrame;
use super::shared::CanShared;
use crate::transport::error::TransportError;
use crate::transport::link::{ReceivedPacket, TransportReceiver};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedReceiver;

/// Receives accepted frames in arrival order.
///
/// It keeps an `Arc<CanShared>` alive so the reader thread and the device outlive the send half.
pub struct CanReceiver {
    _shared: Arc<CanShared>,
    frames: UnboundedReceiver<CanFrame>,
    addr: String,
}

impl CanReceiver {
    pub(super) fn new(
        shared: Arc<CanShared>,
        frames: UnboundedReceiver<CanFrame>,
        addr: String,
    ) -> Self {
        Self {
            _shared: shared,
            frames,
            addr,
        }
    }
}

#[async_trait]
impl TransportReceiver for CanReceiver {
    async fn recv(&mut self) -> Result<ReceivedPacket, TransportError> {
        match self.frames.recv().await {
            Some(frame) => Ok(ReceivedPacket::new(frame.data, frame.timestamp_us)),
            // The reader thread exited (link closed / device stopped).
            None => Err(TransportError::closed(&self.addr)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::can::device::{CanDevice, CanDeviceError, CanDeviceInfo};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;

    /// A device stub: never produces frames (the tests feed the channel directly).
    struct IdleDevice;

    impl CanDevice for IdleDevice {
        fn send_frame(&self, _frame: &CanFrame) -> Result<(), CanDeviceError> {
            Ok(())
        }

        fn recv_frame(
            &self,
            _timeout: Option<Duration>,
        ) -> Result<Option<CanFrame>, CanDeviceError> {
            Ok(None)
        }

        fn close(&mut self) -> Result<(), CanDeviceError> {
            Ok(())
        }

        fn info(&self) -> CanDeviceInfo {
            CanDeviceInfo {
                driver: "stub".to_string(),
                device_id: "0".to_string(),
                name: "stub".to_string(),
                device_type: "stub".to_string(),
                controller: 0,
                channel: "stub".to_string(),
                available: true,
                fd_capable: false,
            }
        }

        fn is_fd(&self) -> bool {
            false
        }
    }

    /// Build a receiver whose shared state owns an idle reader thread.
    fn receiver(frames: UnboundedReceiver<CanFrame>) -> CanReceiver {
        let shared = CanShared::new(
            Arc::new(Mutex::new(Box::new(IdleDevice) as Box<dyn CanDevice>)),
            CancellationToken::new(),
            std::thread::spawn(|| {}),
        );
        CanReceiver::new(shared, frames, "mock".to_string())
    }

    fn frame(pid: u8, ts: u64) -> CanFrame {
        CanFrame {
            id: 0x551,
            extended: false,
            data: vec![pid, 0x11],
            timestamp_us: Some(ts),
        }
    }

    #[tokio::test]
    async fn test_frames_are_delivered_in_arrival_order_with_timestamps() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        tx.send(frame(0x00, 1)).unwrap();
        tx.send(frame(0xFF, 2)).unwrap();

        let mut receiver = receiver(rx);

        let first = receiver.recv().await.unwrap();
        assert_eq!(first.payload, vec![0x00, 0x11]);
        assert_eq!(first.device_timestamp_us, Some(1));

        // FIFO: the CTO frame is not promoted, it simply comes next.
        let second = receiver.recv().await.unwrap();
        assert_eq!(second.payload, vec![0xFF, 0x11]);
        assert_eq!(second.device_timestamp_us, Some(2));
    }

    #[tokio::test]
    async fn test_closed_channel_reports_connection_closed() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        drop(tx); // reader thread exit

        let mut receiver = receiver(rx);
        assert!(matches!(
            receiver.recv().await,
            Err(TransportError::ConnectionClosed { .. })
        ));
    }
}