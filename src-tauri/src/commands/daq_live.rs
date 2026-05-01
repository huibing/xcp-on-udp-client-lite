use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use xcp_client::daq::DaqRecord;

pub(crate) trait DaqEventEmitter: Clone + Send + Sync + 'static {
    fn emit_daq_data(&self, payload: &Value) -> Result<(), String>;
}

impl DaqEventEmitter for AppHandle {
    fn emit_daq_data(&self, payload: &Value) -> Result<(), String> {
        self.emit("daq-data", payload).map_err(|e| e.to_string())
    }
}

pub(crate) fn build_daq_event_payload(device_id: &str, record: &DaqRecord) -> Value {
    json!({
        "device_id": device_id,
        "timestamp_s": record.timestamp_s,
        "values": record.values,
    })
}

pub(crate) fn spawn_live_daq_event_pump<E, G, Fut>(
    emitter: E,
    device_id: String,
    daq_running: Arc<RwLock<bool>>,
    poll_interval: Duration,
    get_latest_sample: G,
) -> JoinHandle<()>
where
    E: DaqEventEmitter,
    G: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Option<DaqRecord>> + Send + 'static,
{
    tokio::spawn(async move {
        loop {
            if !*daq_running.read().await {
                break;
            }

            if let Some(record) = get_latest_sample().await {
                let payload = build_daq_event_payload(&device_id, &record);
                if let Err(error) = emitter.emit_daq_data(&payload) {
                    tracing::warn!("Failed to emit live DAQ data for {}: {}", device_id, error);
                }
            }

            tokio::time::sleep(poll_interval).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use serde_json::Value;
    use tokio::sync::RwLock;
    use xcp_client::daq::DaqRecord;

    use super::{build_daq_event_payload, spawn_live_daq_event_pump, DaqEventEmitter};

    #[derive(Clone, Default)]
    struct MockEmitter {
        payloads: Arc<Mutex<Vec<Value>>>,
    }

    impl MockEmitter {
        fn payloads(&self) -> Vec<Value> {
            self.payloads.lock().unwrap().clone()
        }
    }

    impl DaqEventEmitter for MockEmitter {
        fn emit_daq_data(&self, payload: &Value) -> Result<(), String> {
            self.payloads.lock().unwrap().push(payload.clone());
            Ok(())
        }
    }

    fn sample_record() -> DaqRecord {
        let mut record = DaqRecord::new(1.25, 42);
        record.add_value("rpm", 1234.0);
        record
    }

    #[test]
    fn build_payload_keeps_device_id_and_values() {
        let payload = build_daq_event_payload("slave-1", &sample_record());

        assert_eq!(payload["device_id"], "slave-1");
        assert_eq!(payload["timestamp_s"], 1.25);
        assert_eq!(payload["values"]["rpm"], 1234.0);
    }

    #[tokio::test]
    async fn emitter_pump_publishes_latest_sample_while_running() {
        let emitter = MockEmitter::default();
        let running = Arc::new(RwLock::new(true));
        let latest_sample = Arc::new(RwLock::new(Some(sample_record())));

        let task = spawn_live_daq_event_pump(
            emitter.clone(),
            "slave-1".to_string(),
            running.clone(),
            Duration::from_millis(5),
            {
                let latest_sample = latest_sample.clone();
                move || {
                    let latest_sample = latest_sample.clone();
                    async move { latest_sample.read().await.clone() }
                }
            },
        );

        tokio::time::sleep(Duration::from_millis(20)).await;
        *running.write().await = false;
        let _ = task.await;

        let payloads = emitter.payloads();
        assert!(!payloads.is_empty(), "expected at least one daq-data payload");
        assert_eq!(payloads[0]["device_id"], "slave-1");
        assert_eq!(payloads[0]["values"]["rpm"], 1234.0);
    }
}
