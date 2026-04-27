/// A2L loading Tauri commands.
///
/// Uses a file path (from tauri-plugin-dialog file picker) instead of
/// multipart upload. Ports web/routes.rs a2l_load.

use serde_json::{json, Value};
use tauri::State;
use xcp_client::a2l::A2lReader;

use crate::commands::helpers::{convert_characteristic, convert_measurement};
use crate::state::SharedState;

#[tauri::command]
pub async fn load_a2l(
    state: State<'_, SharedState>,
    device_id: String,
    file_path: String,
) -> Result<Value, String> {
    // Parse A2L file
    let reader = A2lReader::load(&file_path).map_err(|e| format!("A2L parse error: {}", e))?;

    let characteristics = reader.get_characteristics();
    let measurements = reader.get_measurements();
    let memory_segments = reader.get_memory_segments();
    let events = reader.get_events();
    let max_cto = reader.get_max_cto();
    let transport_configs = reader.get_transport_configs();

    let char_count = characteristics.len();
    let meas_count = measurements.len();

    // Convert and load into device state
    {
        let app = state.read().await;
        let dev = app
            .get_device(&device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id))?;

        let mut cal_mgr = dev.cal_manager.write().await;
        cal_mgr.clear_params();
        for ch in characteristics {
            if let Some(param) = convert_characteristic(ch, &reader) {
                cal_mgr.add_param(param);
            }
        }
        cal_mgr.set_memory_segments(memory_segments);

        let mut daq_mgr = dev.daq_manager.write().await;
        for m in measurements {
            if let Some(meas) = convert_measurement(m, &reader) {
                daq_mgr.add_measurement(meas);
            }
        }
        if !events.is_empty() {
            daq_mgr.set_available_events(events);
        }
    }

    // Update device metadata (max_cto, transport_configs, a2l_path) under write lock
    {
        let mut app = state.write().await;
        if let Some(dev) = app.get_device_mut(&device_id) {
            dev.a2l_path = file_path.clone();
            dev.original_filename = std::path::Path::new(&file_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&file_path)
                .to_string();
            if let Some(cto) = max_cto {
                dev.max_cto = cto;
            }
            dev.transport_configs = transport_configs;
        }
    }

    Ok(json!({
        "success": true,
        "characteristics": char_count,
        "measurements": meas_count,
    }))
}
