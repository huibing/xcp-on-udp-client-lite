//! Hardware scan command for the Devices page.
//!
//! Wraps [`xcp_client::hardware::scan`] (network adapters + PCAN/ES582 CAN
//! channels) as a Tauri command. The scan is blocking (vendor DLL loading) so
//! it runs on the blocking pool to keep the async runtime responsive.

use serde_json::{json, Value};

/// Scan the host PC for network adapters and CAN hardware.
#[tauri::command]
pub async fn scan_hardware() -> Result<Value, String> {
    let report = tauri::async_runtime::spawn_blocking(xcp_client::hardware::scan)
        .await
        .map_err(|error| format!("hardware scan failed: {error}"))?;

    let mut payload = serde_json::to_value(&report)
        .map_err(|error| format!("failed to serialize hardware scan: {error}"))?;

    if let Value::Object(map) = &mut payload {
        map.insert("success".to_string(), Value::Bool(true));
        map.insert("network_count".to_string(), json!(report.network_count()));
        map.insert("can_count".to_string(), json!(report.can_count()));
    }

    Ok(payload)
}
