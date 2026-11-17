//! The router: the only consumer of a [`TransportReceiver`].
//!
//! It reads normalized XCP packets, classifies them by PID and forwards them to two independent
//! channels:
//! - PID >= 0xFC → CTO (command responses, plus asynchronous EV/SERV)
//! - PID <  0xFC → DTO (DAQ data)
//!
//! PID classification belongs to the router and only to the router; transports never classify.
//!
//! The router is a plain function plus two owned channels — not a shared service object. The
//! receive half is moved in by value, and each output channel is handed to its single consumer.

use super::link::TransportReceiver;
use super::TransportError;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// XCP packet types based on PID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketType {
    /// Command Transfer Object (CTO) — response (RES/ERR) or asynchronous event (EV/SERV).
    Cto,
    /// Data Transfer Object (DTO) — DAQ data.
    Dto,
}

/// One routed XCP packet.
#[derive(Debug, Clone)]
pub struct XcpMessage {
    pub packet_type: PacketType,
    pub pid: u8,
    pub payload: Vec<u8>,
    /// Hardware/device reception timestamp in microseconds since the device clock epoch, when the
    /// transport provides one (e.g. PCAN USB frames). `None` for transports without hardware
    /// timestamping.
    pub device_timestamp_us: Option<u64>,
}

impl XcpMessage {
    fn from_payload(payload: Vec<u8>, device_timestamp_us: Option<u64>) -> Option<Self> {
        let pid = *payload.first()?;
        let packet_type = if pid >= 0xFC {
            PacketType::Cto
        } else {
            PacketType::Dto
        };

        Some(XcpMessage {
            packet_type,
            pid,
            payload,
            device_timestamp_us,
        })
    }
}

/// Error returned while waiting on one of the router's output channels.
#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    #[error("no message within {0:?}")]
    Timeout(Duration),
    #[error("link to {addr} is gone")]
    Closed { addr: String },
}

/// CTO channel: its only consumer is `XcpSlave::send_command_and_wait`.
///
/// The `Mutex` exists solely because the slave API is `&self` (see the design doc §3.6). It is the
/// only lock left on the receive path.
pub struct CtoChannel {
    rx: Mutex<mpsc::Receiver<XcpMessage>>,
    addr: String,
}

impl CtoChannel {
    /// Wait for a command response (RES 0xFF / ERR 0xFE), skipping asynchronous EV (0xFD) /
    /// SERV (0xFC) packets.
    ///
    /// Skipping matters: consuming an EV/SERV as the response would leave the real RES in the
    /// channel to be misattributed to the *next* command.
    pub async fn wait_response(&self, timeout: Duration) -> Result<XcpMessage, ChannelError> {
        let deadline = tokio::time::Instant::now() + timeout;
        let mut rx = self.rx.lock().await;

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(ChannelError::Timeout(timeout));
            }

            match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Some(msg)) => {
                    if msg.pid == 0xFF || msg.pid == 0xFE {
                        return Ok(msg);
                    }
                    tracing::warn!(
                        "Skipping non-response CTO packet (pid=0x{:02X}) from {} while waiting \
                         for a command response",
                        msg.pid,
                        self.addr
                    );
                }
                // Router stopped: the link is gone.
                Ok(None) => return Err(ChannelError::Closed { addr: self.addr.clone() }),
                Err(_) => return Err(ChannelError::Timeout(timeout)),
            }
        }
    }
}

/// DTO channel: its only consumer is `DaqManager`, which owns it by value inside its task.
///
/// Because the task owns it exclusively, `recv` takes `&mut self` and needs no lock.
pub struct DtoChannel {
    rx: mpsc::Receiver<XcpMessage>,
    addr: String,
}

impl DtoChannel {
    /// Wait for the next DTO. The timeout lets the DAQ loop check its stop flag periodically.
    pub async fn recv(&mut self, timeout: Duration) -> Result<XcpMessage, ChannelError> {
        match tokio::time::timeout(timeout, self.rx.recv()).await {
            Ok(Some(msg)) => Ok(msg),
            Ok(None) => Err(ChannelError::Closed {
                addr: self.addr.clone(),
            }),
            Err(_) => Err(ChannelError::Timeout(timeout)),
        }
    }

    /// The address of the link this channel belongs to (for logging).
    pub fn addr(&self) -> &str {
        &self.addr
    }
}

/// Handle for stopping the router task.
pub struct RouterHandle {
    cancel: CancellationToken,
    handle: Option<JoinHandle<()>>,
}

impl RouterHandle {
    /// Cancel the router and wait for it to finish.
    ///
    /// The receive half is dropped with the task, which closes the read side of the link; the two
    /// output channels close as well, so pending waiters get [`ChannelError::Closed`].
    pub async fn shutdown(&mut self) {
        self.cancel.cancel();
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
    }

    /// Whether the router task is still running (e.g. the link has not closed).
    pub fn is_running(&self) -> bool {
        self.handle
            .as_ref()
            .map(|handle| !handle.is_finished())
            .unwrap_or(false)
    }
}

/// Start the router task.
///
/// `rx` is moved into the task (it is the only consumer), `addr` is used for logging only.
/// Returns the two output channels plus the handle that stops the task.
pub fn spawn_router(
    rx: Box<dyn TransportReceiver>,
    addr: String,
    cto_capacity: usize,
    dto_capacity: usize,
) -> (CtoChannel, DtoChannel, RouterHandle) {
    let (cto_tx, cto_rx) = mpsc::channel(cto_capacity);
    let (dto_tx, dto_rx) = mpsc::channel(dto_capacity);
    let cancel = CancellationToken::new();

    let handle = tokio::spawn(run_router(
        rx,
        addr.clone(),
        cto_tx,
        dto_tx,
        cancel.clone(),
    ));

    (
        CtoChannel {
            rx: Mutex::new(cto_rx),
            addr: addr.clone(),
        },
        DtoChannel { rx: dto_rx, addr },
        RouterHandle {
            cancel,
            handle: Some(handle),
        },
    )
}

async fn run_router(
    mut rx: Box<dyn TransportReceiver>,
    addr: String,
    cto_tx: mpsc::Sender<XcpMessage>,
    dto_tx: mpsc::Sender<XcpMessage>,
    cancel: CancellationToken,
) {
    tracing::info!("Router started for {}", addr);
    let mut backoff = Duration::from_millis(1);

    loop {
        let received = tokio::select! {
            _ = cancel.cancelled() => break,
            received = rx.recv() => received,
        };

        match received {
            Ok(packet) => {
                backoff = Duration::from_millis(1);

                let Some(msg) = XcpMessage::from_payload(packet.payload, packet.device_timestamp_us)
                else {
                    tracing::warn!("Received an empty XCP payload from {}", addr);
                    continue;
                };

                tracing::trace!(
                    "Routing {:?} pid=0x{:02X} len={} device_ts={:?}",
                    msg.packet_type,
                    msg.pid,
                    msg.payload.len(),
                    msg.device_timestamp_us
                );

                let (sender, label) = match msg.packet_type {
                    PacketType::Cto => (&cto_tx, "CTO"),
                    PacketType::Dto => (&dto_tx, "DTO"),
                };

                match sender.try_send(msg) {
                    Ok(()) => {}
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        tracing::warn!("{} channel full, dropping message", label);
                    }
                    // The consumer is gone; nobody can use the stream any more.
                    Err(mpsc::error::TrySendError::Closed(_)) => break,
                }
            }
            // The link ended normally: stop routing and close both channels.
            Err(TransportError::ConnectionClosed { .. }) => {
                tracing::info!("Link to {} closed, router stopping", addr);
                break;
            }
            // Transient I/O trouble (e.g. ECONNREFUSED on a UDP socket whose peer is not up yet):
            // retry with backoff instead of spinning or tearing the link down.
            Err(e) => {
                tracing::warn!("Receive error on {}: {}; retrying in {:?}", addr, e, backoff);
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(backoff) => {}
                }
                backoff = (backoff * 2).min(Duration::from_millis(100));
            }
        }
    }

    tracing::info!("Router for {} stopped", addr);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::link::ReceivedPacket;
    use async_trait::async_trait;
    use std::collections::VecDeque;

    /// A receiver driven by a script of results.
    struct ScriptedReceiver {
        script: VecDeque<Result<ReceivedPacket, TransportError>>,
    }

    impl ScriptedReceiver {
        fn new(script: Vec<Result<ReceivedPacket, TransportError>>) -> Self {
            Self {
                script: script.into(),
            }
        }
    }

    #[async_trait]
    impl TransportReceiver for ScriptedReceiver {
        async fn recv(&mut self) -> Result<ReceivedPacket, TransportError> {
            match self.script.pop_front() {
                Some(result) => result,
                // Stay pending forever once the script is exhausted.
                None => std::future::pending().await,
            }
        }
    }

    fn packet(payload: Vec<u8>, ts: Option<u64>) -> Result<ReceivedPacket, TransportError> {
        Ok(ReceivedPacket::new(payload, ts))
    }

    #[test]
    fn test_packet_type_detection_from_payload() {
        let cto = XcpMessage::from_payload(vec![0xFF, 0x00, 0x01, 0x02], None).unwrap();
        assert_eq!(cto.packet_type, PacketType::Cto);
        assert_eq!(cto.pid, 0xFF);
        assert_eq!(cto.device_timestamp_us, None);

        let dto = XcpMessage::from_payload(vec![0xC0, 0x01, 0x02], Some(12345)).unwrap();
        assert_eq!(dto.packet_type, PacketType::Dto);
        assert_eq!(dto.pid, 0xC0);
        assert_eq!(dto.device_timestamp_us, Some(12345));

        // Boundary: 0xFB is still DAQ data, 0xFC is a CTO.
        assert_eq!(
            XcpMessage::from_payload(vec![0xFB, 0x01], None)
                .unwrap()
                .packet_type,
            PacketType::Dto
        );
        assert_eq!(
            XcpMessage::from_payload(vec![0xFC, 0x01], None)
                .unwrap()
                .packet_type,
            PacketType::Cto
        );
    }

    #[test]
    fn test_empty_payload_is_rejected() {
        assert!(XcpMessage::from_payload(Vec::new(), None).is_none());
    }

    #[tokio::test]
    async fn test_router_classifies_packets_into_two_channels() {
        let rx = Box::new(ScriptedReceiver::new(vec![
            packet(vec![0xFF, 0x01], None),
            packet(vec![0xC0, 0xAA, 0xBB], Some(42)),
        ]));
        let (cto, mut dto, mut handle) = spawn_router(rx, "mock".to_string(), 8, 8);

        let response = cto
            .wait_response(Duration::from_secs(1))
            .await
            .expect("CTO response");
        assert_eq!(response.payload, vec![0xFF, 0x01]);

        let dto_msg = dto
            .recv(Duration::from_secs(1))
            .await
            .expect("DTO packet");
        assert_eq!(dto_msg.payload, vec![0xC0, 0xAA, 0xBB]);
        assert_eq!(dto_msg.device_timestamp_us, Some(42));

        handle.shutdown().await;
    }

    #[tokio::test]
    async fn test_cto_channel_skips_ev_and_serv() {
        let rx = Box::new(ScriptedReceiver::new(vec![
            packet(vec![0xFD, 0x00, 0x00, 0x00, 0xAA], None), // EV
            packet(vec![0xFC, 0x01], None),                   // SERV
            packet(vec![0xFF, 0x42], None),                   // RES (the response)
        ]));
        let (cto, _dto, mut handle) = spawn_router(rx, "mock".to_string(), 8, 8);

        let response = cto
            .wait_response(Duration::from_secs(1))
            .await
            .expect("RES after EV/SERV");
        assert_eq!(response.payload, vec![0xFF, 0x42]);

        handle.shutdown().await;
    }

    #[tokio::test]
    async fn test_wait_response_times_out_without_response() {
        let rx = Box::new(ScriptedReceiver::new(Vec::new()));
        let (cto, _dto, mut handle) = spawn_router(rx, "mock".to_string(), 8, 8);

        let err = cto
            .wait_response(Duration::from_millis(20))
            .await
            .unwrap_err();
        assert!(matches!(err, ChannelError::Timeout(_)));

        handle.shutdown().await;
    }

    #[tokio::test]
    async fn test_connection_closed_closes_both_channels() {
        let rx = Box::new(ScriptedReceiver::new(vec![Err(TransportError::closed(
            "mock",
        ))]));
        let (cto, mut dto, mut handle) = spawn_router(rx, "mock".to_string(), 8, 8);

        assert!(matches!(
            cto.wait_response(Duration::from_secs(1)).await.unwrap_err(),
            ChannelError::Closed { .. }
        ));
        assert!(matches!(
            dto.recv(Duration::from_secs(1)).await.unwrap_err(),
            ChannelError::Closed { .. }
        ));

        handle.shutdown().await;
    }

    #[tokio::test]
    async fn test_transient_error_does_not_stop_the_router() {
        let rx = Box::new(ScriptedReceiver::new(vec![
            Err(TransportError::recv("mock", "transient")),
            packet(vec![0xFF, 0x07], None),
        ]));
        let (cto, _dto, mut handle) = spawn_router(rx, "mock".to_string(), 8, 8);

        let response = cto
            .wait_response(Duration::from_secs(2))
            .await
            .expect("router retries after a transient error");
        assert_eq!(response.payload, vec![0xFF, 0x07]);

        handle.shutdown().await;
    }
}