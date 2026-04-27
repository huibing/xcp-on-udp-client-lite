/// Device management Tauri commands.
///
/// Ports the route handlers from web/routes.rs:
/// connect_device, disconnect_device, list_devices, add_device_entry,
/// rename_device, remove_device, connect_all_devices.

use serde_json::{json, Value};
use tauri::State;

use crate::state::SharedState;

#[tauri::command]
pub async fn list_devices(state: State<'_, SharedState>) -> Result<Value, String> {
    let app = state.read().await;
    let devices: Vec<Value> = app
        .devices
        .iter()
        .map(|d| {
            json!({
                "id": d.id,
                "name": d.display_name(),
                "address": d.address,
                "connected": d.connected,
                "a2l_loaded": !d.a2l_path.is_empty(),
                "original_filename": d.original_filename,
            })
        })
        .collect();
    Ok(json!({ "devices": devices }))
}

#[tauri::command]
pub async fn add_device(
    state: State<'_, SharedState>,
    name: String,
    address: String,
) -> Result<Value, String> {
    let mut app = state.write().await;
    let id = format!("device_{}", uuid::Uuid::new_v4().simple());
    use crate::state::DeviceState;
    app.devices.push(DeviceState::new(id.clone(), name.clone(), address.clone()));
    Ok(json!({ "id": id, "name": name, "address": address }))
}

#[tauri::command]
pub async fn connect_device(
    state: State<'_, SharedState>,
    device_id: String,
) -> Result<Value, String> {
    // Gather device info under read lock
    let (dev_name, dev_address, dev_a2l_path, transport_configs, already_connected) = {
        let app = state.read().await;
        let dev = app
            .get_device(&device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id))?;
        (
            dev.name.clone(),
            dev.address.clone(),
            dev.a2l_path.clone(),
            dev.transport_configs.clone(),
            dev.connected,
        )
    };

    if already_connected {
        return Ok(json!({ "success": false, "error": "Device already connected" }));
    }

    // Resolve transport config
    use xcp_client::a2l::A2lTransportConfig;
    let transport_config = if let Some(cfg) = transport_configs.first() {
        cfg.to_transport_config()
    } else {
        A2lTransportConfig::Udp { addr: dev_address.clone() }.to_transport_config()
    };

    // Connect slave (requires write lock)
    let slave_id = {
        let mut app = state.write().await;
        app.client
            .connect_slave(&dev_name, transport_config)
            .await
            .map_err(|e| format!("Failed to connect: {}", e))?
    };

    // Get arcs needed for post-connect setup (brief read lock)
    let (slave_arc, cal_manager_arc, daq_manager_arc) = {
        let app = state.read().await;
        let slave_arc = app
            .client
            .get_slave(slave_id)
            .ok_or_else(|| "Slave not found after connect".to_string())?;
        let dev = app.get_device(&device_id).ok_or_else(|| "Device not found".to_string())?;
        (slave_arc, dev.cal_manager.clone(), dev.daq_manager.clone())
    };

    // Set memory segments and query page state (no state lock held)
    if !dev_a2l_path.is_empty() {
        let mut xcp_guard = slave_arc.lock().await;
        let segments = {
            let mgr = cal_manager_arc.read().await;
            mgr.get_memory_segments().to_vec()
        };
        xcp_guard.set_memory_segments(segments, Some(std::path::PathBuf::from(&dev_a2l_path)));
        if let Err(e) = xcp_guard.update_segment_page_state().await {
            tracing::warn!("Page state query failed for {}: {}", dev_name, e);
        }
    }

    // Query live DAQ events from slave
    {
        let slave_guard = slave_arc.lock().await;
        if let Ok(daq_info) = slave_guard.get_daq_processor_info().await {
            if daq_info.max_event_channel > 0 {
                if let Ok(live_events) =
                    slave_guard.get_daq_events(daq_info.max_event_channel).await
                {
                    if !live_events.is_empty() {
                        let mut daq_mgr = daq_manager_arc.write().await;
                        daq_mgr.set_available_events(live_events);
                    }
                }
            }
        }
    }

    // Update device connection status
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

    Ok(json!({ "success": true, "slave_id": slave_id.to_string() }))
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
            return Ok(json!({ "success": false, "error": "Device not connected" }));
        }
        let sid = dev
            .slave_id
            .ok_or_else(|| "No slave ID found".to_string())?;
        (sid, dev.name.clone())
    };

    // Stop DAQ if running before disconnecting
    {
        let app = state.read().await;
        if let Some(dev) = app.get_device(&device_id) {
            let daq_running = *dev.daq_running.read().await;
            if daq_running {
                if let Some(slave_arc) = app.client.get_slave(slave_id) {
                    let daq_mgr = dev.daq_manager.read().await;
                    let slave_guard = slave_arc.lock().await;
                    daq_mgr.stop(&*slave_guard).await.ok();
                    drop(slave_guard);
                    drop(daq_mgr);
                }
                *dev.daq_running.write().await = false;
            }
        }
    }

    // Disconnect slave
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

    Ok(json!({ "success": true, "message": format!("Disconnected from {}", dev_name) }))
}

#[tauri::command]
pub async fn remove_device(
    state: State<'_, SharedState>,
    device_id: String,
) -> Result<Value, String> {
    let mut app = state.write().await;
    app.devices.retain(|d| d.id != device_id);
    Ok(json!({ "ok": true }))
}

#[tauri::command]
pub async fn rename_device(
    state: State<'_, SharedState>,
    device_id: String,
    new_name: String,
) -> Result<Value, String> {
    let mut app = state.write().await;
    let dev = app
        .get_device_mut(&device_id)
        .ok_or_else(|| format!("device '{}' not found", device_id))?;
    let mut custom = dev.custom_name.try_write().map_err(|e| e.to_string())?;
    *custom = Some(new_name);
    Ok(json!({ "ok": true }))
}

#[tauri::command]
pub async fn connect_all_devices(state: State<'_, SharedState>) -> Result<Value, String> {
    // Collect IDs of devices not yet connected
    let device_ids: Vec<String> = {
        let app = state.read().await;
        app.devices
            .iter()
            .filter(|d| !d.connected)
            .map(|d| d.id.clone())
            .collect()
    };

    let mut results = Vec::new();
    for id in device_ids {
        match connect_device(state.clone(), id.clone()).await {
            Ok(v) => results.push(json!({ "id": id, "result": v })),
            Err(e) => results.push(json!({ "id": id, "error": e })),
        }
    }
    Ok(json!({ "results": results }))
}
