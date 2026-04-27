/// Experiment workflow Tauri commands.
///
/// Ports web/routes.rs experiment_* handlers.

use serde_json::{json, Value};
use tauri::State;
use xcp_client::slave::PageMode;

use crate::commands::helpers::read_calibration_from_slave;
use crate::state::SharedState;

/// Return all measurements and calibration params across all devices.
#[tauri::command]
pub async fn experiment_variables(state: State<'_, SharedState>) -> Result<Value, String> {
    let app = state.read().await;
    let mut measurements: Vec<Value> = Vec::new();
    let mut calibrations: Vec<Value> = Vec::new();

    for dev in &app.devices {
        let device_id = dev
            .slave_id
            .map(|s| s.to_string())
            .unwrap_or_else(|| dev.id.clone());
        let device_name = dev.display_name();
        let connected = dev.connected;

        let daq_mgr = dev.daq_manager.read().await;
        for (i, m) in daq_mgr.all_measurements().iter().enumerate() {
            let enum_options: Vec<Value> = m
                .compu_method
                .enum_options()
                .map(|opts| {
                    opts.iter()
                        .map(|opt| json!({"raw": opt.raw_value, "text": opt.text_value}))
                        .collect()
                })
                .unwrap_or_default();
            measurements.push(json!({
                "device_id": device_id,
                "device_name": device_name,
                "idx": i,
                "name": m.name,
                "type": format!("{:?}", m.data_type),
                "address_hex": format!("{:08X}", m.address),
                "connected": connected,
                "unit": m.unit.as_deref().unwrap_or(""),
                "is_enum": m.compu_method.is_enum(),
                "enum_options": enum_options,
            }));
        }

        let cal_mgr = dev.cal_manager.read().await;
        for (i, p) in cal_mgr.params().iter().enumerate() {
            calibrations.push(p.to_json(&device_id, &device_name, i, connected));
        }
    }

    Ok(json!({
        "success": true,
        "measurements": measurements,
        "calibrations": calibrations,
    }))
}

/// Start DAQ on the specified devices/measurement indices.
///
/// `config` shape: `{ "selections": { "<device_id>": [idx, ...], ... } }`
#[tauri::command]
pub async fn experiment_start(
    state: State<'_, SharedState>,
    config: Value,
) -> Result<Value, String> {
    let selections = config
        .get("selections")
        .and_then(|v| v.as_object())
        .ok_or_else(|| "Missing 'selections' object".to_string())?
        .clone();

    let mut errors: Vec<String> = Vec::new();
    let mut started_count: usize = 0;

    for (device_id_str, indices_val) in &selections {
        let indices: Vec<usize> = indices_val
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_u64().map(|n| n as usize)).collect())
            .unwrap_or_default();
        if indices.is_empty() {
            continue;
        }

        // Brief read lock: get slave Arc, daq_manager Arc
        let (slave_arc, daq_mgr_arc, daq_running_arc, dev_name) = {
            let app = state.read().await;
            let dev = match find_device(&app, device_id_str) {
                Some(d) => d,
                None => {
                    errors.push(format!("Device {} not found", device_id_str));
                    continue;
                }
            };
            if !dev.connected {
                errors.push(format!("{}: not connected", dev.name));
                continue;
            }
            let sid = match dev.slave_id {
                Some(id) => id,
                None => {
                    errors.push(format!("{}: no slave ID", dev.name));
                    continue;
                }
            };
            let slave_arc = match app.client.get_slave(sid) {
                Some(s) => s,
                None => {
                    errors.push(format!("{}: slave not found", dev.name));
                    continue;
                }
            };
            (
                slave_arc,
                dev.daq_manager.clone(),
                dev.daq_running.clone(),
                dev.name.clone(),
            )
        };

        // Set measurement selections
        {
            let mut daq_mgr = daq_mgr_arc.write().await;
            daq_mgr.deselect_all();
            for idx in &indices {
                daq_mgr.select_measurement(*idx);
            }
        }

        // Start DAQ (XCP I/O — no state lock held)
        {
            let slave_guard = slave_arc.lock().await;
            let mut daq_mgr = daq_mgr_arc.write().await;
            match daq_mgr.start(&*slave_guard, 1000).await {
                Ok(_) => {
                    *daq_running_arc.write().await = true;
                    started_count += 1;
                    tracing::info!("Experiment DAQ started on {}", dev_name);
                }
                Err(e) => {
                    errors.push(format!("{}: {}", dev_name, e));
                }
            }
        }
    }

    if started_count > 0 {
        state.write().await.experiment_active = true;
    }

    if errors.is_empty() && started_count == 0 {
        return Ok(json!({ "success": false, "error": "No devices to start DAQ on" }));
    }

    Ok(json!({
        "success": started_count > 0,
        "started": started_count,
        "errors": errors,
    }))
}

/// Stop DAQ on all running devices.
#[tauri::command]
pub async fn experiment_stop(state: State<'_, SharedState>) -> Result<Value, String> {
    // Collect running devices under read lock
    let devices_info = {
        let app = state.read().await;
        let mut info = Vec::new();
        for dev in &app.devices {
            if !dev.connected || !*dev.daq_running.read().await {
                continue;
            }
            if let Some(sid) = dev.slave_id {
                if let Some(slave_arc) = app.client.get_slave(sid) {
                    info.push((
                        slave_arc,
                        dev.daq_manager.clone(),
                        dev.daq_running.clone(),
                        dev.name.clone(),
                    ));
                }
            }
        }
        info
    };

    let mut errors: Vec<String> = Vec::new();
    let mut stopped_count: usize = 0;

    for (slave_arc, daq_mgr_arc, daq_running_arc, dev_name) in devices_info {
        // Stop recording if active
        {
            let daq_mgr = daq_mgr_arc.read().await;
            if daq_mgr.is_recording() {
                if let Err(e) = daq_mgr.stop_recording().await {
                    tracing::warn!("Failed to stop recording on {}: {}", dev_name, e);
                }
            }
        }

        let slave_guard = slave_arc.lock().await;
        let daq_mgr = daq_mgr_arc.read().await;
        match daq_mgr.stop(&*slave_guard).await {
            Ok(_) => {
                *daq_running_arc.write().await = false;
                stopped_count += 1;
                tracing::info!("Experiment DAQ stopped on {}", dev_name);
            }
            Err(e) => {
                errors.push(format!("{}: {}", dev_name, e));
            }
        }
    }

    state.write().await.experiment_active = false;

    Ok(json!({
        "success": errors.is_empty(),
        "stopped": stopped_count,
        "errors": errors,
    }))
}

/// Return experiment status across all devices.
#[tauri::command]
pub async fn experiment_status(state: State<'_, SharedState>) -> Result<Value, String> {
    let app = state.read().await;
    let experiment_active = app.experiment_active;
    let mut daq_running_count = 0;
    let mut recording_count = 0;
    let mut device_statuses: Vec<Value> = Vec::new();

    for dev in &app.devices {
        let daq_running = *dev.daq_running.read().await;
        let daq_mgr = dev.daq_manager.read().await;
        let is_recording = daq_mgr.is_recording();
        let page_mode = *dev.page_mode.read().await;
        let device_id = dev
            .slave_id
            .map(|s| s.to_string())
            .unwrap_or_else(|| dev.id.clone());

        if daq_running {
            daq_running_count += 1;
        }
        if is_recording {
            recording_count += 1;
        }

        device_statuses.push(json!({
            "device_id": device_id,
            "device_name": dev.display_name(),
            "connected": dev.connected,
            "daq_running": daq_running,
            "recording": is_recording,
            "page_mode": match page_mode {
                PageMode::Working => "working",
                PageMode::Reference => "reference",
            },
            "a2l_file": dev.original_filename,
        }));
    }

    Ok(json!({
        "success": true,
        "experiment_active": experiment_active,
        "daq_running_count": daq_running_count,
        "recording_count": recording_count,
        "devices": device_statuses,
    }))
}

/// Switch working/reference page mode on all connected devices.
#[tauri::command]
pub async fn experiment_pagemode(
    state: State<'_, SharedState>,
    mode: String,
) -> Result<Value, String> {
    let target = match mode.as_str() {
        "working" => PageMode::Working,
        "reference" => PageMode::Reference,
        _ => {
            return Ok(json!({
                "success": false,
                "error": "Invalid mode. Use 'working' or 'reference'."
            }))
        }
    };

    // Collect connected devices
    let devices_info = {
        let app = state.read().await;
        let mut info = Vec::new();
        for dev in &app.devices {
            if !dev.connected {
                continue;
            }
            if let Some(sid) = dev.slave_id {
                if let Some(slave_arc) = app.client.get_slave(sid) {
                    info.push((
                        slave_arc,
                        dev.page_mode.clone(),
                        dev.id.clone(),
                        dev.name.clone(),
                    ));
                }
            }
        }
        info
    };

    let mut results: Vec<Value> = Vec::new();
    let mut success_count = 0;
    let mut fail_count = 0;

    for (slave_arc, page_mode_arc, dev_id, dev_name) in devices_info {
        let mut slave_guard = slave_arc.lock().await;
        match slave_guard.switch_all_pages(target).await {
            Ok(_) => {
                *page_mode_arc.write().await = target;
                success_count += 1;
                results.push(json!({
                    "device_id": dev_id,
                    "device_name": dev_name,
                    "success": true,
                    "page_mode": mode,
                }));
            }
            Err(e) => {
                fail_count += 1;
                results.push(json!({
                    "device_id": dev_id,
                    "device_name": dev_name,
                    "success": false,
                    "error": e.to_string(),
                }));
            }
        }
    }

    Ok(json!({
        "success": fail_count == 0,
        "switched": success_count,
        "failed": fail_count,
        "results": results,
        "mode": mode,
    }))
}

/// Rename a device (set custom display name).
#[tauri::command]
pub async fn experiment_rename_slave(
    state: State<'_, SharedState>,
    device_id: String,
    name: String,
) -> Result<Value, String> {
    let trimmed = name.trim().to_string();
    if trimmed.is_empty() {
        return Ok(json!({ "success": false, "error": "Name is required" }));
    }
    let app = state.read().await;
    let dev = find_device(&app, &device_id)
        .ok_or_else(|| format!("Device '{}' not found", device_id))?;
    *dev.custom_name.write().await = Some(trimmed.clone());
    Ok(json!({ "success": true, "name": trimmed }))
}

/// Run checksum verification on all connected devices that have a HEX file loaded.
#[tauri::command]
pub async fn experiment_checksum(
    state: State<'_, SharedState>,
    _config: Value,
) -> Result<Value, String> {
    // Gather connected devices with hex images under read lock
    let devices_data = {
        let app = state.read().await;
        let mut data = Vec::new();
        for dev in &app.devices {
            if !dev.connected {
                continue;
            }
            let hex_image = dev.hex_image.read().await.clone();
            let hex_image = match hex_image {
                Some(img) => img,
                None => continue,
            };
            let sid = match dev.slave_id {
                Some(id) => id,
                None => continue,
            };
            let slave_arc = match app.client.get_slave(sid) {
                Some(s) => s,
                None => continue,
            };
            let segments = dev.cal_manager.read().await.get_memory_segments().to_vec();
            let a2l_path = dev.a2l_path.clone();
            data.push((
                slave_arc,
                hex_image,
                segments,
                a2l_path,
                dev.checksum_report.clone(),
                dev.slave_id.map(|s| s.to_string()).unwrap_or_else(|| dev.id.clone()),
                dev.display_name(),
            ));
        }
        data
    };

    let mut results: Vec<Value> = Vec::new();

    for (slave_arc, hex_image, segments, a2l_path, report_arc, dev_id, dev_name) in devices_data {
        // Resolve segments if needed
        let segments = if segments.is_empty() && !a2l_path.is_empty() {
            let path = a2l_path.clone();
            match tokio::task::spawn_blocking(move || {
                xcp_client::A2lReader::load(&path).map(|r| r.get_memory_segments())
            })
            .await
            .map_err(|e| format!("Task error: {}", e))
            {
                Ok(Ok(segs)) => segs,
                Ok(Err(e)) => {
                    results.push(json!({
                        "device_id": dev_id,
                        "device_name": dev_name,
                        "success": false,
                        "error": format!("A2L load error: {:?}", e),
                    }));
                    continue;
                }
                Err(e) => {
                    results.push(json!({
                        "device_id": dev_id,
                        "device_name": dev_name,
                        "success": false,
                        "error": e,
                    }));
                    continue;
                }
            }
        } else {
            segments
        };

        if segments.is_empty() {
            results.push(json!({
                "device_id": dev_id,
                "device_name": dev_name,
                "success": false,
                "error": "No memory segments found",
            }));
            continue;
        }

        let mut slave_guard = slave_arc.lock().await;
        match xcp_client::checksum::verify_segments(&mut slave_guard, &hex_image, &segments, None).await {
            Ok(report) => {
                let passed = report.all_passed();
                *report_arc.write().await = Some(report.clone());
                results.push(json!({
                    "device_id": dev_id,
                    "device_name": dev_name,
                    "success": true,
                    "passed": passed,
                    "segments": report.results.iter().map(|s| json!({
                        "name": s.name,
                        "address": format!("0x{:08X}", s.address),
                        "size": s.size,
                        "algorithm": s.algorithm,
                        "master_checksum": format!("0x{:08X}", s.master_checksum),
                        "slave_checksum": format!("0x{:08X}", s.slave_checksum),
                        "match": s.passed,
                        "status": format!("{}", s.status),
                    })).collect::<Vec<_>>(),
                }));
            }
            Err(e) => {
                results.push(json!({
                    "device_id": dev_id,
                    "device_name": dev_name,
                    "success": false,
                    "error": format!("{}", e),
                }));
            }
        }
    }

    let all_passed = results.iter().all(|r| {
        r["success"].as_bool().unwrap_or(false) && r.get("passed").and_then(|v| v.as_bool()).unwrap_or(false)
    });

    Ok(json!({
        "success": true,
        "all_passed": all_passed,
        "results": results,
    }))
}

/// Return calibration parameters that differ from their original (reference) values.
#[tauri::command]
pub async fn experiment_cal_changed(state: State<'_, SharedState>) -> Result<Value, String> {
    let app = state.read().await;
    let mut device_results: Vec<Value> = Vec::new();

    for dev in &app.devices {
        if !dev.connected {
            continue;
        }

        let device_id = dev
            .slave_id
            .map(|s| s.to_string())
            .unwrap_or_else(|| dev.id.clone());
        let device_name = dev.display_name();

        let original_values = dev.cal_original_values.read().await;
        let cal_values = dev.cal_values.read().await;
        let cal_mgr = dev.cal_manager.read().await;
        let params = cal_mgr.params();

        let mut indices: Vec<usize> = original_values.keys().cloned().collect();
        indices.sort();

        let mut variables: Vec<Value> = Vec::new();

        for idx in indices {
            let original_value = match original_values.get(&idx) {
                Some(v) => v.clone(),
                None => continue,
            };
            let param = match params.get(idx) {
                Some(p) => p,
                None => continue,
            };
            let current_value = cal_values
                .get(&param.name)
                .map(|v| v.format_scalar_str())
                .unwrap_or_default();

            if !current_value.is_empty() && current_value == original_value {
                continue;
            }

            variables.push(json!({
                "idx": idx,
                "name": param.name,
                "description": param.description,
                "unit": param.unit.as_deref().unwrap_or(""),
                "type_name": param.data_type.type_name(),
                "characteristic_type": param.characteristic_type.as_str(),
                "original_value": original_value,
                "current_value": current_value,
            }));
        }

        device_results.push(json!({
            "device_id": device_id,
            "device_name": device_name,
            "variables": variables,
        }));
    }

    Ok(json!({ "success": true, "devices": device_results }))
}

/// DCM export is not supported in the desktop app (use file save dialog separately).
#[tauri::command]
pub async fn experiment_dcm_export(
    _state: State<'_, SharedState>,
    _config: Value,
) -> Result<Value, String> {
    Err("DCM export is not yet supported in the desktop app".to_string())
}

/// Write a calibration value during an experiment (also snapshots original if not stored).
#[tauri::command]
pub async fn experiment_cal_write(
    state: State<'_, SharedState>,
    device_id: String,
    idx: usize,
    value: Value,
) -> Result<Value, String> {
    use crate::commands::calibration;
    // Delegate to the standard cal_write command
    calibration::cal_write(state, device_id, idx, value).await
}

/// Get or capture the original (reference) calibration value for a parameter.
///
/// On first call for a given parameter, reads the current value from the XCP slave
/// and stores it as the "original" for future diff comparisons.
#[tauri::command]
pub async fn experiment_cal_original(
    state: State<'_, SharedState>,
    device_id: String,
    idx: usize,
) -> Result<Value, String> {
    // Check if already stored
    let (already_stored, cal_original_arc, slave_arc, param) = {
        let app = state.read().await;
        let dev = find_device(&app, &device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id))?;

        let originals = dev.cal_original_values.read().await;
        if let Some(val) = originals.get(&idx) {
            return Ok(json!({ "success": true, "value": val.clone() }));
        }
        drop(originals);

        if !dev.connected {
            return Ok(json!({ "success": false, "error": "Device not connected" }));
        }

        let sid = dev
            .slave_id
            .ok_or_else(|| "No slave ID".to_string())?;
        let slave_arc = app
            .client
            .get_slave(sid)
            .ok_or_else(|| "Slave not found".to_string())?;

        let param = dev
            .cal_manager
            .read()
            .await
            .params()
            .get(idx)
            .cloned()
            .ok_or_else(|| "Parameter not found".to_string())?;

        (false, dev.cal_original_values.clone(), slave_arc, param)
    };
    let _ = already_stored;

    // Read from XCP slave (no state lock held)
    let slave_guard = slave_arc.lock().await;
    let value = read_calibration_from_slave(&slave_guard, &param)
        .await
        .map_err(|e| format!("Read failed: {}", e))?;
    drop(slave_guard);

    let formatted = value.format_scalar_str();
    cal_original_arc.write().await.insert(idx, formatted.clone());

    Ok(json!({ "success": true, "value": formatted }))
}

/// Start MF4 recording on all DAQ-running devices.
#[tauri::command]
pub async fn experiment_record_start(
    state: State<'_, SharedState>,
    config: Value,
) -> Result<Value, String> {
    let title = config
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("experiment")
        .to_string();
    let dir = config
        .get("dir")
        .and_then(|v| v.as_str())
        .unwrap_or("records")
        .to_string();

    let now = chrono::Local::now();
    let timestamp = now.format("%Y%m%d_%H%M%S").to_string();
    let safe_title = sanitize_filename(&title);

    let dir_path = std::path::PathBuf::from(&dir);
    if !dir_path.exists() {
        std::fs::create_dir_all(&dir_path)
            .map_err(|e| format!("Failed to create output directory: {}", e))?;
    }

    // Collect running devices
    let devices_info = {
        let app = state.read().await;
        let connected_count = app.devices.iter().filter(|d| d.connected).count();
        let mut info = Vec::new();
        for dev in &app.devices {
            if !dev.connected || !*dev.daq_running.read().await {
                continue;
            }
            if let Some(sid) = dev.slave_id {
                if let Some(slave_arc) = app.client.get_slave(sid) {
                    info.push((
                        slave_arc,
                        dev.daq_manager.clone(),
                        dev.display_name(),
                        connected_count,
                    ));
                }
            }
        }
        info
    };

    let mut errors: Vec<String> = Vec::new();
    let mut started_count = 0;

    for (slave_arc, daq_mgr_arc, dev_name, total_connected) in devices_info {
        let safe_device = sanitize_filename(&dev_name);
        let mf4_filename = if total_connected > 1 {
            format!("{}_{}_{}.mf4", safe_title, safe_device, timestamp)
        } else {
            format!("{}_{}.mf4", safe_title, timestamp)
        };
        let mf4_path = dir_path.join(&mf4_filename);
        let csv_path = mf4_path.with_extension("csv");

        let slave_guard = slave_arc.lock().await;
        let mut daq_mgr = daq_mgr_arc.write().await;
        daq_mgr.set_mdf4_path(&mf4_path);
        daq_mgr.set_csv_path(&csv_path);

        match daq_mgr.start_recording(&*slave_guard).await {
            Ok(_) => {
                started_count += 1;
                tracing::info!("Recording started on {} → {}", dev_name, mf4_path.display());
            }
            Err(e) => {
                errors.push(format!("{}: {}", dev_name, e));
            }
        }
    }

    if started_count == 0 && errors.is_empty() {
        return Ok(json!({ "success": false, "error": "No running DAQ found to record" }));
    }

    Ok(json!({
        "success": started_count > 0,
        "started": started_count,
        "errors": errors,
    }))
}

/// Stop MF4 recording on all recording devices.
#[tauri::command]
pub async fn experiment_record_stop(state: State<'_, SharedState>) -> Result<Value, String> {
    let app = state.read().await;
    let mut errors: Vec<String> = Vec::new();
    let mut stopped_count = 0;

    for dev in &app.devices {
        let daq_mgr = dev.daq_manager.read().await;
        if !daq_mgr.is_recording() {
            continue;
        }
        match daq_mgr.stop_recording().await {
            Ok(_) => {
                stopped_count += 1;
                tracing::info!("Recording stopped on {}", dev.display_name());
            }
            Err(e) => {
                errors.push(format!("{}: {}", dev.display_name(), e));
            }
        }
    }

    Ok(json!({
        "success": errors.is_empty(),
        "stopped": stopped_count,
        "errors": errors,
    }))
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn find_device<'a>(
    app: &'a crate::state::TauriAppState,
    device_id: &str,
) -> Option<&'a crate::state::DeviceState> {
    // Try direct id match first
    if let Some(dev) = app.get_device(device_id) {
        return Some(dev);
    }
    // Try matching by slave_id (UUID string)
    if let Ok(uuid) = uuid::Uuid::parse_str(device_id) {
        let sid = xcp_client::slave::SlaveId(uuid);
        return app.find_device_by_slave_id(sid);
    }
    None
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}
