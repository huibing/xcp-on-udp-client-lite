//! Event handling for TUI

use crossterm::event::{self, Event as CrosstermEvent, KeyEvent, KeyEventKind};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Terminal events
#[derive(Debug)]
pub enum Event {
    Key(KeyEvent),
    Tick,
    Resize,
    Error(String),
}

/// Event handler with tick rate
pub struct EventHandler {
    receiver: mpsc::UnboundedReceiver<Event>,
    cancel_token: CancellationToken,
}

impl EventHandler {
    /// Create new event handler with specified tick rate
    pub fn new(tick_rate: Duration) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let cancel_token = CancellationToken::new();

        // Clone sender before moving into spawn
        let event_sender = sender.clone();
        let tick_sender = sender.clone();

        // Spawn event loop
        let token = cancel_token.clone();
        tokio::spawn(async move {
            loop {
                let event = if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                    match event::read() {
                        Ok(CrosstermEvent::Key(key)) => {
                            // Only process Press events, ignore Repeat and Release
                            if key.kind == KeyEventKind::Press {
                                Some(Event::Key(key))
                            } else {
                                None
                            }
                        }
                        Ok(CrosstermEvent::Resize(_, _)) => Some(Event::Resize),
                        Ok(_) => None,
                        Err(e) => Some(Event::Error(e.to_string())),
                    }
                } else {
                    None
                };

                if let Some(evt) = event {
                    if event_sender.send(evt).is_err() {
                        break;
                    }
                }

                // Check cancellation
                if token.is_cancelled() {
                    break;
                }
            }
        });

        // Spawn tick generator
        let tick_token = cancel_token.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tick_rate);
            loop {
                interval.tick().await;
                if tick_sender.send(Event::Tick).is_err() {
                    break;
                }
                if tick_token.is_cancelled() {
                    break;
                }
            }
        });

        Self {
            receiver,
            cancel_token,
        }
    }

    /// Get next event
    pub async fn next(&mut self) -> Result<Event, Box<dyn std::error::Error>> {
        self.receiver
            .recv()
            .await
            .ok_or_else(|| "Channel closed".into())
    }
}

impl Drop for EventHandler {
    fn drop(&mut self) {
        self.cancel_token.cancel();
    }
}
