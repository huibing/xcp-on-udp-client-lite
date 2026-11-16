//! Shared CTRL+C handling for the non-interactive CLI subcommands.
//!
//! `daq-record` and `daq-print` react to CTRL+C by running a graceful shutdown
//! (stop recording/DAQ, `FREE_DAQ`, disconnect), which takes a moment per
//! slave. Listening with the one-shot `tokio::signal::ctrl_c()` future has a
//! Windows pitfall: the listener is unregistered as soon as the future
//! completes, so tokio's console control handler returns `FALSE` and the next
//! CTRL+C falls through to the default handler, terminating the process with
//! `STATUS_CONTROL_C_EXIT` (0xC000013A) in the middle of the shutdown.
//!
//! [`ctrl_c_channel`] therefore uses the platform *stream* APIs (which keep the
//! listener registered until the stream is dropped) and keeps it alive for as
//! long as the returned receiver lives. Extra CTRL+C presses during shutdown
//! are then simply consumed and the process exits normally.

use tokio::sync::watch;

/// Spawn a background task that forwards every CTRL+C press through a watch
/// channel.
///
/// Keep the returned receiver alive for the whole command, including the
/// shutdown phase: while it exists the CTRL+C listener stays registered.
pub fn ctrl_c_channel() -> watch::Receiver<bool> {
    let (tx, rx) = watch::channel(false);
    tokio::spawn(forward_ctrl_c(tx));
    rx
}

/// Forward CTRL+C events until the receiver is dropped.
#[cfg(windows)]
async fn forward_ctrl_c(tx: watch::Sender<bool>) {
    // The listener is registered when the stream is created and stays
    // registered until it is dropped, so the console handler keeps returning
    // TRUE between events (no re-arm window).
    let mut stream = match tokio::signal::windows::ctrl_c() {
        Ok(stream) => stream,
        Err(error) => {
            tracing::warn!("Failed to listen for CTRL+C: {}", error);
            // Keep the channel open so callers keep waiting instead of
            // treating a closed channel as a CTRL+C press.
            tx.closed().await;
            return;
        }
    };
    while stream.recv().await.is_some() {
        if tx.send(true).is_err() {
            break;
        }
    }
}

/// Forward CTRL+C (SIGINT) events until the receiver is dropped.
#[cfg(unix)]
async fn forward_ctrl_c(tx: watch::Sender<bool>) {
    use tokio::signal::unix::{signal, SignalKind};

    let mut stream = match signal(SignalKind::interrupt()) {
        Ok(stream) => stream,
        Err(error) => {
            tracing::warn!("Failed to listen for SIGINT: {}", error);
            tx.closed().await;
            return;
        }
    };
    while stream.recv().await.is_some() {
        if tx.send(true).is_err() {
            break;
        }
    }
}

/// Fallback for platforms without a persistent signal stream.
#[cfg(not(any(unix, windows)))]
async fn forward_ctrl_c(tx: watch::Sender<bool>) {
    loop {
        if tokio::signal::ctrl_c().await.is_err() {
            break;
        }
        if tx.send(true).is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ctrl_c_channel_semantics() {
        // The subcommands rely on: initial value false, `changed()` resolves on
        // every press (the listener keeps running, so a second press works too).
        let (tx, mut rx) = watch::channel(false);
        assert!(!*rx.borrow());

        tx.send(true).unwrap();
        assert!(rx.changed().await.is_ok());
        assert!(*rx.borrow_and_update());

        tx.send(true).unwrap();
        assert!(rx.changed().await.is_ok());
    }

    #[tokio::test]
    async fn test_ctrl_c_channel_starts_unset() {
        let rx = ctrl_c_channel();
        assert!(!*rx.borrow());
    }
}
