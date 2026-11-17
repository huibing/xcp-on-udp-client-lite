//! The two halves of a transport link.
//!
//! Send and receive are **separate objects from the moment the link is created**
//! ([`TransportLink`]). The send half is shared (`&self`, `Sync`); the receive half is owned by
//! exactly one consumer (`&mut self`, no `Sync`). That ownership is what makes the
//! "single consumer" invariant a compile-time property instead of a lock.

use super::error::TransportError;
use super::TransportType;
use async_trait::async_trait;

/// One normalized XCP packet: payload plus an optional device reception timestamp.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReceivedPacket {
    /// XCP payload with all transport headers stripped.
    pub payload: Vec<u8>,
    /// Hardware/device reception timestamp in microseconds since the device clock epoch
    /// (e.g. a PCAN USB frame timestamp). `None` for transports without hardware timestamping.
    pub device_timestamp_us: Option<u64>,
}

impl ReceivedPacket {
    pub fn new(payload: Vec<u8>, device_timestamp_us: Option<u64>) -> Self {
        Self {
            payload,
            device_timestamp_us,
        }
    }
}

/// The send half of a link.
///
/// Sending has no single-consumer constraint, so `&self` + `Sync` is natural. Link lifecycle
/// (`close`) lives here because it manages connection/device resources, not the read stream.
#[async_trait]
pub trait TransportSender: Send + Sync + 'static {
    /// Send one normalized XCP payload.
    async fn send(&self, payload: &[u8]) -> Result<(), TransportError>;

    /// Close the link. The receive half ends as a consequence (channel closed / EOF).
    async fn close(&self) -> Result<(), TransportError>;

    /// Transport type.
    fn transport_type(&self) -> TransportType;

    /// Target address string (for logging/display).
    fn target_addr(&self) -> String;
}

/// The receive half of a link.
///
/// Owned by value by its **only** consumer (currently the router task), hence `&mut self`: the
/// single-consumer invariant is enforced by ownership, so no lock and no `Sync` is needed.
///
/// Contract:
/// - `recv` must be cancel-safe: the router polls it inside a `select!`.
/// - A closed link returns [`TransportError::ConnectionClosed`]; implementations do not retry.
/// - Malformed frames are dropped (with a warning) inside the implementation, never returned.
/// - There is no `close()`: dropping the receiver closes the read side.
#[async_trait]
pub trait TransportReceiver: Send + 'static {
    /// Wait for the next normalized XCP packet.
    async fn recv(&mut self) -> Result<ReceivedPacket, TransportError>;
}

/// A link whose two halves are already independent objects.
pub struct TransportLink {
    pub sender: Box<dyn TransportSender>,
    pub receiver: Box<dyn TransportReceiver>,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EchoSender;

    #[async_trait]
    impl TransportSender for EchoSender {
        async fn send(&self, _payload: &[u8]) -> Result<(), TransportError> {
            Ok(())
        }

        async fn close(&self) -> Result<(), TransportError> {
            Ok(())
        }

        fn transport_type(&self) -> TransportType {
            TransportType::Udp
        }

        fn target_addr(&self) -> String {
            "mock".to_string()
        }
    }

    struct EmptyReceiver;

    #[async_trait]
    impl TransportReceiver for EmptyReceiver {
        async fn recv(&mut self) -> Result<ReceivedPacket, TransportError> {
            Err(TransportError::closed("mock"))
        }
    }

    /// The two halves are independent values: the receiver can be moved into a task while the
    /// sender stays usable, without any shared state between them.
    #[tokio::test]
    async fn test_halves_are_independent() {
        let link = TransportLink {
            sender: Box::new(EchoSender),
            receiver: Box::new(EmptyReceiver),
        };
        let TransportLink { sender, mut receiver } = link;

        let handle = tokio::spawn(async move { receiver.recv().await });
        sender.send(&[0xFF]).await.unwrap();
        assert_eq!(sender.target_addr(), "mock");
        assert!(matches!(
            handle.await.unwrap(),
            Err(TransportError::ConnectionClosed { .. })
        ));
    }
}