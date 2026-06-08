use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tracing::{field::Field, field::Visit, Event};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

const MAX_LOG_ENTRIES: usize = 5000;

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub id: u64,
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub message: String,
}

pub struct ConsoleLogState {
    buffer: Mutex<VecDeque<LogEntry>>,
    next_id: AtomicU64,
}

impl ConsoleLogState {
    pub fn new() -> Self {
        Self {
            buffer: Mutex::new(VecDeque::with_capacity(MAX_LOG_ENTRIES)),
            next_id: AtomicU64::new(1),
        }
    }

    fn push(&self, entry: LogEntry) {
        let mut buffer = self.buffer.lock().unwrap();
        buffer.push_back(entry);
        if buffer.len() > MAX_LOG_ENTRIES {
            buffer.pop_front();
        }
    }
}

pub struct ConsoleLogLayer {
    state: Arc<ConsoleLogState>,
}

impl ConsoleLogLayer {
    pub fn new(state: Arc<ConsoleLogState>) -> Self {
        Self { state }
    }
}

impl<S> Layer<S> for ConsoleLogLayer
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let id = self.state.next_id.fetch_add(1, Ordering::Relaxed);

        let mut visitor = LogVisitor(String::new());
        event.record(&mut visitor);

        let entry = LogEntry {
            id,
            timestamp: chrono::Local::now()
                .format("%H:%M:%S%.3f")
                .to_string(),
            level: event.metadata().level().to_string(),
            target: event.metadata().target().to_string(),
            message: visitor.0,
        };

        self.state.push(entry);
    }
}

struct LogVisitor(String);

impl Visit for LogVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.0 = value.to_string();
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{:?}", value);
        }
    }
}

// ── Tauri commands ─────────────────────────────────────────────────────────

#[tauri::command]
pub fn console_logs_get(
    state: tauri::State<'_, Arc<ConsoleLogState>>,
    since_id: u64,
) -> Vec<LogEntry> {
    let buffer = state.buffer.lock().unwrap();
    buffer.iter().filter(|e| e.id > since_id).cloned().collect()
}

#[tauri::command]
pub fn console_logs_clear(state: tauri::State<'_, Arc<ConsoleLogState>>) {
    state.buffer.lock().unwrap().clear();
}
