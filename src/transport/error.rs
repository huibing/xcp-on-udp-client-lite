//! Transport layer errors.
//!
//! Variants are grouped by the half they can come from:
//! - [`TransportError::InvalidAddress`] / [`TransportError::OpenFailed`] only come from
//!   `open()` (configuration / link establishment).
//! - [`TransportError::SendFailed`] / [`TransportError::RecvFailed`] are runtime failures and
//!   carry the original error.
//! - [`TransportError::ConnectionClosed`] means the link ended normally; it is not a fault.
//!
//! Deliberately absent:
//! - `Timeout`: the receive half has no timeout method; callers express timeouts with
//!   `tokio::time::timeout`.
//! - `MalformedFrame`: a bad frame is a transport-internal concern. Receivers drop it and log a
//!   warning instead of surfacing it, so consumers only ever see a valid packet or a terminal error.

/// Underlying failure: a socket/IO error or a device driver error.
pub type ErrorSource = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("invalid address `{0}`")]
    InvalidAddress(String),

    #[error("cannot open link to {addr}: {source}")]
    OpenFailed {
        addr: String,
        #[source]
        source: ErrorSource,
    },

    #[error("send failed on {addr}: {source}")]
    SendFailed {
        addr: String,
        #[source]
        source: ErrorSource,
    },

    #[error("receive failed on {addr}: {source}")]
    RecvFailed {
        addr: String,
        #[source]
        source: ErrorSource,
    },

    #[error("link to {addr} closed")]
    ConnectionClosed { addr: String },
}

impl TransportError {
    pub(crate) fn open(addr: impl Into<String>, source: impl Into<ErrorSource>) -> Self {
        Self::OpenFailed {
            addr: addr.into(),
            source: source.into(),
        }
    }

    pub(crate) fn send(addr: impl Into<String>, source: impl Into<ErrorSource>) -> Self {
        Self::SendFailed {
            addr: addr.into(),
            source: source.into(),
        }
    }

    pub(crate) fn recv(addr: impl Into<String>, source: impl Into<ErrorSource>) -> Self {
        Self::RecvFailed {
            addr: addr.into(),
            source: source.into(),
        }
    }

    pub(crate) fn closed(addr: impl Into<String>) -> Self {
        Self::ConnectionClosed { addr: addr.into() }
    }

    /// The address of the link this error refers to, when the variant carries one.
    pub fn addr(&self) -> Option<&str> {
        match self {
            TransportError::InvalidAddress(_) => None,
            TransportError::OpenFailed { addr, .. }
            | TransportError::SendFailed { addr, .. }
            | TransportError::RecvFailed { addr, .. }
            | TransportError::ConnectionClosed { addr } => Some(addr),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display_includes_addr_and_source() {
        let err = TransportError::send("127.0.0.1:5555", "broken pipe");
        let text = err.to_string();
        assert!(text.contains("127.0.0.1:5555"), "{text}");
        assert!(text.contains("broken pipe"), "{text}");
        assert_eq!(err.addr(), Some("127.0.0.1:5555"));
    }

    #[test]
    fn test_error_keeps_io_source() {
        let io = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
        let err = TransportError::recv("can0", io);
        let source = std::error::Error::source(&err).expect("source kept");
        assert_eq!(
            source.downcast_ref::<std::io::Error>().map(|e| e.kind()),
            Some(std::io::ErrorKind::ConnectionRefused)
        );
    }

    #[test]
    fn test_invalid_address_has_no_addr() {
        let err = TransportError::InvalidAddress("nope".to_string());
        assert_eq!(err.addr(), None);
        assert!(err.to_string().contains("nope"));
    }
}