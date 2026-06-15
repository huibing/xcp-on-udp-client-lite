/// A2L loading Tauri commands.
///
/// Uses a file path (from the Tauri dialog plugin) instead of multipart upload
/// and mirrors the web slave-management behavior.
use serde_json::{json, Value};
use tauri::State;
use xcp_client::a2l::{A2lReader, A2lTransportConfig};

use crate::commands::helpers::{convert_characteristic, convert_measurement};
use crate::state::SharedState;

fn display_device_address(config: &A2lTransportConfig) -> String {
    match config {
        A2lTransportConfig::Udp { addr } | A2lTransportConfig::Tcp { addr } => addr.clone(),
        A2lTransportConfig::Can { can_id } => format!("CAN 0x{:X}", can_id),
    }
}

#[tauri::command]
pub async fn load_a2l(
    state: State<'_, SharedState>,
    device_id: String,
    file_path: String,
) -> Result<Value, String> {
    let reader = A2lReader::load(&file_path).map_err(|e| format!("A2L parse error: {}", e))?;
    let transport_configs = reader.get_transport_configs();
    let device_addr = transport_configs
        .first()
        .map(display_device_address)
        .or_else(|| reader.get_slave_addresses().first().cloned())
        .unwrap_or_else(|| "127.0.0.1:5555".to_string());

    {
        let app = state.read().await;
        if app.get_device(&device_id).is_none() {
            return Ok(json!({
                "success": false,
                "error": "Device not found",
            }));
        }
        let duplicate = app
            .devices
            .iter()
            .filter(|device| device.id != device_id)
            .any(|device| !device_addr.is_empty() && device.address == device_addr);
        if duplicate {
            return Ok(json!({
                "success": false,
                "error": format!(
                    "Another device already uses address {}. Each device must have a unique address.",
                    device_addr
                ),
            }));
        }
    }

    let characteristics = reader.get_characteristics();
    let measurements = reader.get_measurements();
    let memory_segments = reader.get_memory_segments();
    let events = reader.get_events();
    let max_cto = reader.get_max_cto().unwrap_or(255);
    let cal_count = characteristics.len();
    let meas_count = measurements.len();
    let original_filename = std::path::Path::new(&file_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&file_path)
        .to_string();

    {
        let app = state.read().await;
        let dev = app
            .get_device(&device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id))?;

        let mut cal_mgr = dev.cal_manager.write().await;
        cal_mgr.clear_params();
        for characteristic in characteristics {
            if let Some(param) = convert_characteristic(characteristic, &reader) {
                cal_mgr.add_param(param);
            }
        }
        cal_mgr.set_memory_segments(memory_segments);
        drop(cal_mgr);

        let mut daq_mgr = dev.daq_manager.write().await;
        daq_mgr.clear_measurements();
        for measurement in measurements {
            if let Some(meas) = convert_measurement(measurement, &reader) {
                daq_mgr.add_measurement(meas);
            }
        }
        if !events.is_empty() {
            daq_mgr.set_available_events(events);
        }
    }

    let slave_id = {
        let mut app = state.write().await;
        let dev = app
            .get_device_mut(&device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id))?;
        dev.address = device_addr.clone();
        dev.a2l_path = file_path.clone();
        dev.original_filename = original_filename.clone();
        dev.max_cto = max_cto;
        dev.transport_configs = transport_configs;
        let slave_id = dev.slave_id;
        app.save_config()?;
        slave_id
    };

    if let Some(slave_id) = slave_id {
        let (slave_arc, cal_manager_arc) = {
            let app = state.read().await;
            let slave_arc = app
                .client
                .get_slave(slave_id)
                .ok_or_else(|| "Slave not found after A2L load".to_string())?;
            let dev = app
                .get_device(&device_id)
                .ok_or_else(|| "Device not found".to_string())?;
            (slave_arc, dev.cal_manager.clone())
        };

        let mut xcp_guard = slave_arc.lock().await;
        let segments = cal_manager_arc.read().await.get_memory_segments().to_vec();
        xcp_guard.set_memory_segments(segments, Some(std::path::PathBuf::from(&file_path)));
        if let Err(error) = xcp_guard.update_segment_page_state().await {
            tracing::warn!(
                "Page state query failed after A2L reload for {}: {}",
                device_id,
                error
            );
        }
    }

    Ok(json!({
        "success": true,
        "message": format!("Loaded A2L: {} characteristics, {} measurements", cal_count, meas_count),
        "cal_count": cal_count,
        "meas_count": meas_count,
        "address": device_addr,
    }))
}
