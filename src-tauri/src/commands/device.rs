/// Device management Tauri commands.
///
/// Mirrors the web slave management routes more closely than the initial stub:
/// list/add/rename/remove/connect devices, plus transport selection metadata.

use std::collections::HashMap;

use serde_json::{json, Value};
use tauri::State;
use xcp_client::transport::TransportType;

use crate::state::{DeviceState, SharedState};

fn parse_transport_type(value: Option<&str>) -> Result<Option<TransportType>, String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    if value.eq_ignore_ascii_case("auto") {
        return Ok(None);
    }

    if value.eq_ignore_ascii_case("udp") {
        return Ok(Some(TransportType::Udp));
    }

    if value.eq_ignore_ascii_case("can") {
        return Ok(Some(TransportType::Can));
    }

    if value.eq_ignore_ascii_case("tcp") {
        return Ok(Some(TransportType::Tcp));
    }

    Err(format!("Unsupported transport protocol '{}'", value))
}

fn transport_option_value(transport_type: TransportType) -> &'static str {
    match transport_type {
        TransportType::Udp => "udp",
        TransportType::Can => "can",
        TransportType::Tcp => "tcp",
    }
}

fn build_transport_options(device: &DeviceState) -> Vec<Value> {
    let mut options = vec![json!({
        "value": "",
        "label": "Auto (UDP -> CAN -> TCP)",
    })];

    for config in &device.transport_configs {
        options.push(json!({
            "value": transport_option_value(config.transport_type()),
            "label": config.display_label(),
        }));
    }

    options
}

#[tauri::command]
pub async fn list_devices(state: State<'_, SharedState>) -> Result<Value, String> {
    let app = state.read().await;
    let mut devices = Vec::with_capacity(app.devices.len());

    for device in &app.devices {
        let hex_filename = device.hex_filename.read().await.clone();
        let working_hex_name = if device.working_hex_path.is_empty() {
            None
        } else {
            Some(
                std::path::Path::new(&device.working_hex_path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| device.working_hex_path.clone()),
            )
        };
        let checksum_report = device.checksum_report.read().await;
        let checksum_status = if let Some(report) = checksum_report.as_ref() {
            if report.all_passed() {
                "passed"
            } else {
                "failed"
            }
        } else {
            "not_run"
        };
        drop(checksum_report);

        let cal_count = device.cal_manager.read().await.params().len();
        let meas_count = device.daq_manager.read().await.available_count();

        devices.push(json!({
            "id": device.id,
            "name": device.display_name(),
            "address": device.address,
            "a2l_loaded": !device.a2l_path.is_empty(),
            "a2l_name": device.original_filename,
            "hex_loaded": hex_filename.is_some(),
            "hex_name": hex_filename,
            "working_hex_name": working_hex_name,
            "connected": device.connected,
            "checksum_status": checksum_status,
            "cal_count": cal_count,
            "meas_count": meas_count,
            "transport_options": build_transport_options(device),
        }));
    }

    Ok(json!({
        "success": true,
        "devices": devices,
    }))
}

#[tauri::command]
pub async fn add_device(
    state: State<'_, SharedState>,
    name: String,
    address: Option<String>,
) -> Result<Value, String> {
    let trimmed_name = name.trim();
    if trimmed_name.is_empty() {
        return Ok(json!({
            "success": false,
            "error": "Name cannot be empty",
        }));
    }

    let mut app = state.write().await;
    let id = format!("device_{}", uuid::Uuid::new_v4().simple());
    let mut device = DeviceState::new_empty(id.clone(), trimmed_name.to_string());
    device.address = address.unwrap_or_default().trim().to_string();
    app.devices.push(device);
    app.save_config()?;

    Ok(json!({
        "success": true,
        "device_id": id,
        "message": format!("Added slave '{}'", trimmed_name),
    }))
}

/// Register devices by their saved IDs and names, idempotent.
///
/// For each entry, if a device with the same `id` or `name` already exists, it is skipped.
/// Otherwise a new placeholder `DeviceState` is created with the provided `id` — preserving
/// the original identity so that experiment variable references (which store device_id) remain
/// valid after the registration.
///
/// If `a2l_path` is provided and the file exists, the A2L is parsed and used to populate
/// calibration / measurement lists immediately (same behaviour as startup restore).
/// If `address` is provided, it is set on the device (takes lower priority than A2L-derived
/// address if A2L restore also sets one).
#[tauri::command]
pub async fn ensure_devices_registered(
    state: State<'_, SharedState>,
    devices: Vec<Value>,
) -> Result<Value, String> {
    let mut registered: Vec<String> = Vec::new();
    let mut already_existed: Vec<String> = Vec::new();

    for entry in &devices {
        let id = entry.get("id").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let a2l_path = entry.get("a2l_path").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let address = entry.get("address").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();

        if id.is_empty() && name.is_empty() {
            continue;
        }

        let exists = {
            let app = state.read().await;
            app.devices.iter().any(|d| {
                (!id.is_empty() && d.id == id)
                    || (!name.is_empty() && d.name.eq_ignore_ascii_case(&name))
            })
        };

        if exists {
            already_existed.push(name.clone());
            continue;
        }

        let device_id = if !id.is_empty() { id.clone() } else {
            format!("device_{}", uuid::Uuid::new_v4().simple())
        };
        let device_name = if !name.is_empty() { name.clone() } else { device_id.clone() };

        let mut device = crate::state::DeviceState::new_empty(device_id.clone(), device_name.clone());

        // Set address as initial value (may be overridden by A2L-derived address below)
        if !address.is_empty() {
            device.address = address.clone();
        }

        // Restore A2L if the file path is known and accessible
        if !a2l_path.is_empty() {
            if std::path::Path::new(&a2l_path).exists() {
                if let Err(e) = crate::state::TauriAppState::restore_a2l_pub(&mut device, &a2l_path) {
                    tracing::warn!("ensure_devices_registered: A2L restore failed for '{}': {}", device_name, e);
                    // Still set the path so it can be retried on next connect
                    device.a2l_path = a2l_path.clone();
                    device.original_filename = a2l_path.split(['/', '\\']).last().unwrap_or(&a2l_path).to_string();
                }
            } else {
                tracing::warn!("ensure_devices_registered: A2L file not found at '{}' for '{}'", a2l_path, device_name);
                device.a2l_path = a2l_path.clone();
                device.original_filename = a2l_path.split(['/', '\\']).last().unwrap_or(&a2l_path).to_string();
            }
        }

        let mut app = state.write().await;
        app.devices.push(device);
        app.save_config()?;
        registered.push(device_name);
    }

    Ok(json!({
        "success": true,
        "registered": registered,
        "already_existed": already_existed,
    }))
}

#[tauri::command]
pub async fn connect_device(
    state: State<'_, SharedState>,
    device_id: String,
    transport_protocol: Option<String>,
) -> Result<Value, String> {
    let requested_transport = match parse_transport_type(transport_protocol.as_deref()) {
        Ok(value) => value,
        Err(error) => {
            return Ok(json!({
                "success": false,
                "error": error,
            }));
        }
    };

    let (dev_name, dev_a2l_path, transport_config, already_connected) = {
        let app = state.read().await;
        let dev = app
            .get_device(&device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id))?;
        let transport = match dev.resolve_transport_config(requested_transport) {
            Ok(config) => config,
            Err(error) => {
                return Ok(json!({
                    "success": false,
                    "error": error,
                }));
            }
        };
        (
            dev.name.clone(),
            dev.a2l_path.clone(),
            transport,
            dev.connected,
        )
    };

    if already_connected {
        return Ok(json!({
            "success": false,
            "error": "Device already connected",
        }));
    }

    let slave_id = {
        let mut app = state.write().await;
        app.client
            .connect_slave(&dev_name, transport_config.clone().to_transport_config())
            .await
            .map_err(|e| format!("Failed to connect: {}", e))?
    };

    let (slave_arc, cal_manager_arc, daq_manager_arc) = {
        let app = state.read().await;
        let slave_arc = app
            .client
            .get_slave(slave_id)
            .ok_or_else(|| "Slave not found after connect".to_string())?;
        let dev = app
            .get_device(&device_id)
            .ok_or_else(|| "Device not found".to_string())?;
        (slave_arc, dev.cal_manager.clone(), dev.daq_manager.clone())
    };

    if !dev_a2l_path.is_empty() {
        let mut xcp_guard = slave_arc.lock().await;
        let segments = cal_manager_arc.read().await.get_memory_segments().to_vec();
        xcp_guard.set_memory_segments(segments, Some(std::path::PathBuf::from(&dev_a2l_path)));
        if let Err(error) = xcp_guard.update_segment_page_state().await {
            tracing::warn!("Page state query failed for {}: {}", dev_name, error);
        }
    }

    {
        let slave_guard = slave_arc.lock().await;
        if let Ok(daq_info) = slave_guard.get_daq_processor_info().await {
            if daq_info.max_event_channel > 0 {
                if let Ok(live_events) = slave_guard.get_daq_events(daq_info.max_event_channel).await {
                    if !live_events.is_empty() {
                        daq_manager_arc.write().await.set_available_events(live_events);
                    }
                }
            }
        }
    }

    {
        let mut app = state.write().await;
        if let Some(dev) = app.get_device_mut(&device_id) {
            dev.connected = true;
            dev.slave_id = Some(slave_id);
        }
        if app.current_device_id.is_none() {
            app.current_device_id = Some(device_id.clone());
        }
    }

    Ok(json!({
        "success": true,
        "slave_id": slave_id.to_string(),
        "message": format!(
            "Connected to {} via {}",
            dev_name,
            transport_config.display_label()
        ),
    }))
}

#[tauri::command]
pub async fn disconnect_device(
    state: State<'_, SharedState>,
    device_id: String,
) -> Result<Value, String> {
    let (slave_id, dev_name) = {
        let app = state.read().await;
        let dev = app
            .get_device(&device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id))?;
        if !dev.connected {
            return Ok(json!({
                "success": false,
                "error": "Device not connected",
            }));
        }
        let sid = dev
            .slave_id
            .ok_or_else(|| "No slave ID found".to_string())?;
        (sid, dev.name.clone())
    };

    {
        let app = state.read().await;
        if let Some(dev) = app.get_device(&device_id) {
            let daq_running = *dev.daq_running.read().await;
            if daq_running {
                if let Some(slave_arc) = app.client.get_slave(slave_id) {
                    let daq_mgr = dev.daq_manager.read().await;
                    let slave_guard = slave_arc.lock().await;
                    daq_mgr.stop(&*slave_guard).await.ok();
                }
                *dev.daq_running.write().await = false;
            }
        }
    }

    {
        let mut app = state.write().await;
        app.client
            .disconnect_slave(slave_id)
            .await
            .map_err(|e| format!("Failed to disconnect: {}", e))?;
        if let Some(dev) = app.get_device_mut(&device_id) {
            dev.connected = false;
            dev.slave_id = None;
        }
        if app.current_device_id.as_deref() == Some(&device_id) {
            let next = app.devices.iter().find(|d| d.connected).map(|d| d.id.clone());
            app.current_device_id = next;
        }
    }

    Ok(json!({
        "success": true,
        "message": format!("Disconnected from {}", dev_name),
    }))
}

#[tauri::command]
pub async fn remove_device(
    state: State<'_, SharedState>,
    device_id: String,
) -> Result<Value, String> {
    let connected = {
        let app = state.read().await;
        let dev = app
            .get_device(&device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id.clone()))?;
        dev.connected
    };

    if connected {
        let disconnect_result = disconnect_device(state.clone(), device_id.clone()).await?;
        if !disconnect_result["success"].as_bool().unwrap_or(false) {
            return Ok(disconnect_result);
        }
    }

    let mut app = state.write().await;
    app.devices.retain(|d| d.id != device_id);
    if app.current_device_id.as_deref() == Some(device_id.as_str()) {
        app.current_device_id = app.devices.first().map(|d| d.id.clone());
    }
    app.save_config()?;

    Ok(json!({
        "success": true,
        "message": "Device removed",
    }))
}

#[tauri::command]
pub async fn rename_device(
    state: State<'_, SharedState>,
    device_id: String,
    name: String,
) -> Result<Value, String> {
    let trimmed_name = name.trim();
    if trimmed_name.is_empty() {
        return Ok(json!({
            "success": false,
            "error": "Name cannot be empty",
        }));
    }

    let mut app = state.write().await;
    let dev = app
        .get_device_mut(&device_id)
        .ok_or_else(|| format!("Device '{}' not found", device_id))?;
    dev.name = trimmed_name.to_string();
    *dev.custom_name.write().await = None;
    app.save_config()?;

    Ok(json!({
        "success": true,
        "message": format!("Renamed to '{}'", trimmed_name),
    }))
}

#[tauri::command]
pub async fn connect_all_devices(
    state: State<'_, SharedState>,
    transport_protocols: Option<HashMap<String, String>>,
) -> Result<Value, String> {
    let device_ids: Vec<String> = {
        let app = state.read().await;
        app.devices
            .iter()
            .filter(|d| !d.connected)
            .map(|d| d.id.clone())
            .collect()
    };

    let transport_protocols = transport_protocols.unwrap_or_default();
    let mut connected_count = 0usize;
    let mut failed_devices = Vec::new();

    for id in device_ids {
        let protocol = transport_protocols.get(&id).cloned();
        match connect_device(state.clone(), id.clone(), protocol).await {
            Ok(result) if result["success"].as_bool().unwrap_or(false) => {
                connected_count += 1;
            }
            Ok(result) => {
                let error = result["error"].as_str().unwrap_or("Unknown error");
                failed_devices.push(format!("{}: {}", id, error));
            }
            Err(error) => failed_devices.push(format!("{}: {}", id, error)),
        }
    }

    let message = if connected_count > 0 {
        if failed_devices.is_empty() {
            format!("Connected to {} device(s)", connected_count)
        } else {
            format!(
                "Connected to {} device(s). Failed: {}",
                connected_count,
                failed_devices.join(", ")
            )
        }
    } else if failed_devices.is_empty() {
        "All devices already connected or no devices available".to_string()
    } else {
        format!("Failed to connect: {}", failed_devices.join(", "))
    };

    Ok(json!({
        "success": failed_devices.is_empty() || connected_count > 0,
        "message": message,
        "connected": connected_count,
    }))
}
