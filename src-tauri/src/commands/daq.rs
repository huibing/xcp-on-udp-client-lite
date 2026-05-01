/// DAQ Tauri commands.
///
/// Ports web/routes.rs daq_* handlers.
/// Live data is pushed to the frontend via tauri events ("daq-data") instead
/// of HTTP polling.

use serde_json::{json, Value};
use tauri::{AppHandle, State};

use crate::commands::daq_live::spawn_live_daq_event_pump;
use crate::state::SharedState;

#[tauri::command]
pub async fn daq_available(state: State<'_, SharedState>) -> Result<Value, String> {
    let app = state.read().await;
    let mut measurements = Vec::new();
    for dev in &app.devices {
        let mgr = dev.daq_manager.read().await;
        for (i, m) in mgr.all_measurements().iter().enumerate() {
            let selected = mgr.selected_indices().contains(&i);
            measurements.push(json!({
                "device_id": dev.id,
                "device_name": dev.display_name(),
                "idx": i,
                "name": m.name,
                "type": format!("{:?}", m.data_type),
                "address_hex": format!("0x{:08X}", m.address),
                "unit": m.unit,
                "selected": selected,
                "connected": dev.connected,
            }));
        }
    }
    Ok(json!({ "measurements": measurements }))
}

#[tauri::command]
pub async fn daq_start(
    app: AppHandle,
    state: State<'_, SharedState>,
    device_id: String,
) -> Result<Value, String> {
    let (slave_arc, daq_mgr_arc, daq_running_arc) = {
        let st = state.read().await;
        let dev = st
            .get_device(&device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id))?;
        if !dev.connected {
            return Ok(json!({ "success": false, "error": "Device not connected" }));
        }
        let sid = dev.slave_id.ok_or_else(|| "No slave ID".to_string())?;
        let slave_arc = st
            .client
            .get_slave(sid)
            .ok_or_else(|| "Slave not found".to_string())?;
        (slave_arc, dev.daq_manager.clone(), dev.daq_running.clone())
    };

    if *daq_running_arc.read().await {
        return Ok(json!({ "success": false, "error": "DAQ already running" }));
    }

    // Start DAQ
    {
        let slave_guard = slave_arc.lock().await;
        let mut daq_mgr = daq_mgr_arc.write().await;
        daq_mgr
            .start(&*slave_guard, 100)
            .await
            .map_err(|e| e.to_string())?;
    }
    *daq_running_arc.write().await = true;

    // Spawn background task to emit live data
    spawn_live_daq_event_pump(
        app,
        device_id,
        daq_running_arc.clone(),
        tokio::time::Duration::from_millis(100),
        move || {
            let daq_mgr_arc = daq_mgr_arc.clone();
            async move {
                let mgr = daq_mgr_arc.read().await;
                mgr.get_latest_sample().await
            }
        },
    );

    Ok(json!({ "success": true }))
}

#[tauri::command]
pub async fn daq_stop(
    state: State<'_, SharedState>,
    device_id: String,
) -> Result<Value, String> {
    let (slave_arc, daq_mgr_arc, daq_running_arc) = {
        let st = state.read().await;
        let dev = st
            .get_device(&device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id))?;
        if !dev.connected {
            return Ok(json!({ "success": false, "error": "Device not connected" }));
        }
        let sid = dev.slave_id.ok_or_else(|| "No slave ID".to_string())?;
        let slave_arc = st
            .client
            .get_slave(sid)
            .ok_or_else(|| "Slave not found".to_string())?;
        (slave_arc, dev.daq_manager.clone(), dev.daq_running.clone())
    };

    if !*daq_running_arc.read().await {
        return Ok(json!({ "success": false, "error": "DAQ not running" }));
    }

    {
        let slave_guard = slave_arc.lock().await;
        let daq_mgr = daq_mgr_arc.read().await;
        daq_mgr.stop(&*slave_guard).await.map_err(|e| e.to_string())?;
    }
    *daq_running_arc.write().await = false;

    Ok(json!({ "success": true }))
}

#[tauri::command]
pub async fn daq_status(state: State<'_, SharedState>) -> Result<Value, String> {
    let app = state.read().await;
    let mut statuses = Vec::new();
    for dev in &app.devices {
        let running = *dev.daq_running.read().await;
        statuses.push(json!({
            "device_id": dev.id,
            "device_name": dev.display_name(),
            "running": running,
            "connected": dev.connected,
        }));
    }
    Ok(json!({ "devices": statuses }))
}

#[tauri::command]
pub async fn daq_toggle(
    state: State<'_, SharedState>,
    device_id: String,
    idx: usize,
) -> Result<Value, String> {
    let app = state.read().await;
    let dev = app
        .get_device(&device_id)
        .ok_or_else(|| format!("Device '{}' not found", device_id))?;
    let mut mgr = dev.daq_manager.write().await;
    let selected = mgr.selected_indices().contains(&idx);
    if selected {
        mgr.deselect_measurement(idx);
    } else {
        mgr.select_measurement(idx);
    }
    Ok(json!({ "success": true, "selected": !selected }))
}

#[tauri::command]
pub async fn daq_set_selection(
    state: State<'_, SharedState>,
    device_id: String,
    indices: Vec<usize>,
) -> Result<Value, String> {
    let app = state.read().await;
    let dev = app
        .get_device(&device_id)
        .ok_or_else(|| format!("Device '{}' not found", device_id))?;
    let mut mgr = dev.daq_manager.write().await;
    mgr.deselect_all();
    for i in indices {
        mgr.select_measurement(i);
    }
    Ok(json!({ "success": true }))
}

#[tauri::command]
pub async fn daq_clear(
    state: State<'_, SharedState>,
    device_id: String,
) -> Result<Value, String> {
    let app = state.read().await;
    let dev = app
        .get_device(&device_id)
        .ok_or_else(|| format!("Device '{}' not found", device_id))?;
    let mut mgr = dev.daq_manager.write().await;
    mgr.deselect_all();
    Ok(json!({ "success": true }))
}

#[tauri::command]
pub async fn daq_data(
    state: State<'_, SharedState>,
    device_id: String,
) -> Result<Value, String> {
    let app = state.read().await;
    let dev = app
        .get_device(&device_id)
        .ok_or_else(|| format!("Device '{}' not found", device_id))?;
    let mgr = dev.daq_manager.read().await;
    let sample = mgr.get_latest_sample().await;
    if let Some(record) = sample {
        Ok(json!({
            "success": true,
            "timestamp_s": record.timestamp_s,
            "values": record.values,
        }))
    } else {
        Ok(json!({ "success": false, "error": "No data yet" }))
    }
}

#[tauri::command]
pub async fn daq_events(state: State<'_, SharedState>) -> Result<Value, String> {
    let app = state.read().await;
    let mut events_by_device = Vec::new();
    for dev in &app.devices {
        let mgr = dev.daq_manager.read().await;
        let events: Vec<Value> = mgr
            .get_available_events()
            .iter()
            .map(|e| {
                json!({
                    "channel": e.channel,
                    "name": e.name,
                    "cycle_time_ms": e.cycle_time_ms(),
                })
            })
            .collect();
        events_by_device.push(json!({ "device_id": dev.id, "events": events }));
    }
    Ok(json!({ "devices": events_by_device }))
}

#[tauri::command]
pub async fn daq_set_event(
    state: State<'_, SharedState>,
    device_id: String,
    measurement_idx: usize,
    event_channel: u16,
) -> Result<Value, String> {
    let app = state.read().await;
    let dev = app
        .get_device(&device_id)
        .ok_or_else(|| format!("Device '{}' not found", device_id))?;
    let mut mgr = dev.daq_manager.write().await;
    mgr.set_measurement_event(measurement_idx, event_channel);
    Ok(json!({ "success": true }))
}

#[tauri::command]
pub async fn daq_set_measurement_events(
    state: State<'_, SharedState>,
    device_id: String,
    assignments: Value,
) -> Result<Value, String> {
    let app = state.read().await;
    let dev = app
        .get_device(&device_id)
        .ok_or_else(|| format!("Device '{}' not found", device_id))?;
    let mut mgr = dev.daq_manager.write().await;

    if let Some(arr) = assignments.as_array() {
        for item in arr {
            if let (Some(idx), Some(ch)) = (item["idx"].as_u64(), item["channel"].as_u64()) {
                mgr.set_measurement_event(idx as usize, ch as u16);
            }
        }
    }
    Ok(json!({ "success": true }))
}

#[tauri::command]
pub async fn daq_start_recording(
    state: State<'_, SharedState>,
    device_id: String,
    config: Value,
) -> Result<Value, String> {
    let (slave_arc, daq_mgr_arc) = {
        let st = state.read().await;
        let dev = st
            .get_device(&device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id))?;
        if !dev.connected {
            return Ok(json!({ "success": false, "error": "Device not connected" }));
        }
        let sid = dev.slave_id.ok_or_else(|| "No slave ID".to_string())?;
        let slave_arc = st
            .client
            .get_slave(sid)
            .ok_or_else(|| "Slave not found".to_string())?;
        (slave_arc, dev.daq_manager.clone())
    };

    if let Some(filename) = config["filename"].as_str() {
        let mut mgr = daq_mgr_arc.write().await;
        let path = std::path::Path::new(filename);
        mgr.set_csv_path(path.with_extension("csv"));
        mgr.set_mdf4_path(path.with_extension("mf4"));
    }

    let slave_guard = slave_arc.lock().await;
    let mut mgr = daq_mgr_arc.write().await;
    mgr.start_recording(&*slave_guard)
        .await
        .map_err(|e| e.to_string())?;

    Ok(json!({ "success": true }))
}

#[tauri::command]
pub async fn daq_stop_recording(
    state: State<'_, SharedState>,
    device_id: String,
) -> Result<Value, String> {
    let daq_mgr_arc = {
        let st = state.read().await;
        let dev = st
            .get_device(&device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id))?;
        dev.daq_manager.clone()
    };

    let mgr = daq_mgr_arc.read().await;
    mgr.stop_recording().await.map_err(|e| e.to_string())?;

    Ok(json!({ "success": true }))
}

#[tauri::command]
pub async fn daq_download_csv(
    state: State<'_, SharedState>,
    device_id: String,
) -> Result<String, String> {
    let daq_mgr_arc = {
        let st = state.read().await;
        let dev = st
            .get_device(&device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id))?;
        dev.daq_manager.clone()
    };

    let mgr = daq_mgr_arc.read().await;
    let csv_path = mgr.get_csv_path().to_path_buf();
    drop(mgr);
    std::fs::read_to_string(&csv_path)
        .map_err(|e| format!("Failed to read CSV '{}': {}", csv_path.display(), e))
}
