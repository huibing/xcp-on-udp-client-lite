/// Experiment workflow Tauri commands.
///
/// Ports web/routes.rs experiment_* handlers.

use serde_json::{json, Value};
use tauri::{AppHandle, State};
use xcp_client::slave::PageMode;

use crate::commands::daq_live::spawn_live_daq_event_pump;
use crate::commands::helpers::bytes_to_calibration_value;
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

        let config_device_id = dev.id.clone();

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
                "config_device_id": config_device_id,
                "device_name": device_name,
                "idx": i,
                "name": m.name,
                "type": format!("{:?}", m.data_type),
                "address_hex": format!("{:08X}", m.address),
                "connected": connected,
                "unit": m.unit.as_deref().unwrap_or(""),
                "bytes": m.size,
                "is_enum": m.compu_method.is_enum(),
                "enum_options": enum_options,
            }));
        }

        let cal_mgr = dev.cal_manager.read().await;
        for (i, p) in cal_mgr.params().iter().enumerate() {
            let mut cal_json = p.to_json(&device_id, &device_name, i, connected);
            cal_json["config_device_id"] = Value::String(config_device_id.clone());
            calibrations.push(cal_json);
        }
    }

    Ok(json!({
        "success": true,
        "measurements": measurements,
        "calibrations": calibrations,
    }))
}

/// Return available DAQ events for each device (from the loaded A2L file).
///
/// Response shape:
/// `{ "success": true, "devices": [{ "device_id", "config_device_id", "device_name", "events": [{channel, name, short_name, cycle_time_ms}] }] }`
#[tauri::command]
pub async fn experiment_events(state: State<'_, SharedState>) -> Result<Value, String> {
    let app = state.read().await;
    let mut devices: Vec<Value> = Vec::new();

    for dev in &app.devices {
        let device_id = dev
            .slave_id
            .map(|s| s.to_string())
            .unwrap_or_else(|| dev.id.clone());
        let config_device_id = dev.id.clone();
        let device_name = dev.display_name();

        let daq_mgr = dev.daq_manager.read().await;
        let events: Vec<Value> = daq_mgr
            .get_available_events()
            .iter()
            .map(|e| json!({
                "channel": e.channel,
                "name": e.name,
                "short_name": e.short_name,
                "cycle_time_ms": e.cycle_time_ms(),
            }))
            .collect();

        devices.push(json!({
            "device_id": device_id,
            "config_device_id": config_device_id,
            "device_name": device_name,
            "events": events,
        }));
    }

    Ok(json!({ "success": true, "devices": devices }))
}

/// Return per-device DAQ identification type and timestamp metadata.
///
/// Used by the DAQ layout panel to display PID byte width and timestamp info.
/// Works without an active slave connection — shows defaults when not connected.
#[tauri::command]
pub async fn experiment_daq_meta(state: State<'_, SharedState>) -> Result<Value, String> {
    use xcp_client::daq::IdentificationFieldType;

    let app = state.read().await;
    let mut devices: Vec<Value> = Vec::new();

    for dev in &app.devices {
        let device_id = dev
            .slave_id
            .map(|s| s.to_string())
            .unwrap_or_else(|| dev.id.clone());
        let config_device_id = dev.id.clone();
        let device_name = dev.display_name();
        let connected = dev.connected;

        let daq_mgr = dev.daq_manager.read().await;

        let id_type = daq_mgr.get_identification_type();
        let (pid_bytes, id_type_str) = match id_type {
            IdentificationFieldType::AbosulteODT => (1u8, "AbsoluteODT"),
            IdentificationFieldType::RelatetiveODTBYTE => (2u8, "RelativeODT+Byte"),
            IdentificationFieldType::RelatetiveODTWORD => (3u8, "RelativeODT+Word"),
            IdentificationFieldType::RelatetiveODTWORDAlign => (4u8, "RelativeODT+WordAligned"),
        };

        let ts_size = daq_mgr.get_timestamp_size().unwrap_or(0);
        let ts_unit_str = match daq_mgr.get_timestamp_unit() {
            Some(xcp_client::daq::TimestampUnit::DAQTIMESTAMPUNIT1PS) => "1 ps",
            Some(xcp_client::daq::TimestampUnit::DAQTIMESTAMPUNIT10PS) => "10 ps",
            Some(xcp_client::daq::TimestampUnit::DAQTIMESTAMPUNIT100PS) => "100 ps",
            Some(xcp_client::daq::TimestampUnit::DAQTIMESTAMPUNIT1NS) => "1 ns",
            Some(xcp_client::daq::TimestampUnit::DAQTIMESTAMPUNIT10NS) => "10 ns",
            Some(xcp_client::daq::TimestampUnit::DAQTIMESTAMPUNIT100NS) => "100 ns",
            Some(xcp_client::daq::TimestampUnit::DAQTIMESTAMPUNIT1US) => "1 µs",
            Some(xcp_client::daq::TimestampUnit::DAQTIMESTAMPUNIT10US) => "10 µs",
            Some(xcp_client::daq::TimestampUnit::DAQTIMESTAMPUNIT100US) => "100 µs",
            Some(xcp_client::daq::TimestampUnit::DAQTIMESTAMPUNIT1MS) => "1 ms",
            Some(xcp_client::daq::TimestampUnit::DAQTIMESTAMPUNIT10MS) => "10 ms",
            Some(xcp_client::daq::TimestampUnit::DAQTIMESTAMPUNIT100MS) => "100 ms",
            Some(xcp_client::daq::TimestampUnit::DAQTIMESTAMPUNIT1S) => "1 s",
            None => "unknown",
        };

        let total_measurements = daq_mgr.all_measurements().len();

        devices.push(json!({
            "device_id": device_id,
            "config_device_id": config_device_id,
            "device_name": device_name,
            "connected": connected,
            "identification_type": id_type_str,
            "pid_bytes": pid_bytes,
            "has_timestamp": ts_size > 0,
            "timestamp_bytes": ts_size,
            "timestamp_unit": ts_unit_str,
            "total_measurements": total_measurements,
        }));
    }

    Ok(json!({ "success": true, "devices": devices }))
}

/// Set the DAQ event for a specific measurement on a device.
///
/// Only allowed when DAQ is stopped. Calling `experiment_start` will apply the
/// updated event assignment (it always calls `free_daq` + reconfigure).
#[tauri::command]
pub async fn experiment_set_event(
    state: State<'_, SharedState>,
    device_id: String,
    measurement_idx: usize,
    event_channel: u16,
) -> Result<Value, String> {
    let app = state.read().await;
    let dev = find_device(&app, &device_id)
        .ok_or_else(|| format!("Device '{}' not found", device_id))?;

    if *dev.daq_running.read().await {
        return Err("Cannot change event while DAQ is running — stop DAQ first".to_string());
    }

    let mut daq_mgr = dev.daq_manager.write().await;
    daq_mgr.set_measurement_event(measurement_idx, event_channel);

    Ok(json!({ "success": true }))
}

/// Start DAQ on the specified devices/measurement indices.
///
/// `config` shape: `{ "selections": { "<device_id>": [idx, ...], ... }, "events": { "<device_id>": { "<idx>": channel, ... }, ... } }`
#[tauri::command]
pub async fn experiment_start(
    app: AppHandle,
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

        // Set measurement selections and per-measurement event assignments
        {
            let mut daq_mgr = daq_mgr_arc.write().await;
            daq_mgr.deselect_all();
            for idx in &indices {
                daq_mgr.select_measurement(*idx);
            }
            // Apply per-measurement event channel assignments if provided
            if let Some(events_map) = config.get("events").and_then(|v| v.as_object()) {
                if let Some(dev_events) = events_map.get(device_id_str).and_then(|v| v.as_object()) {
                    for (idx_str, ch_val) in dev_events {
                        if let (Ok(idx), Some(ch)) = (
                            idx_str.parse::<usize>(),
                            ch_val.as_u64().map(|c| c as u16),
                        ) {
                            daq_mgr.set_measurement_event(idx, ch);
                        }
                    }
                }
            }
        }

        // Start DAQ (XCP I/O — no state lock held)
        let start_result = {
            let slave_guard = slave_arc.lock().await;
            let mut daq_mgr = daq_mgr_arc.write().await;
            daq_mgr.start(&*slave_guard, 1000).await
        };

        match start_result {
            Ok(_) => {
                *daq_running_arc.write().await = true;
                started_count += 1;
                let event_device_id = device_id_str.clone();
                spawn_live_daq_event_pump(
                    app.clone(),
                    event_device_id,
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
                tracing::info!("Experiment DAQ started on {}", dev_name);
            }
            Err(e) => {
                errors.push(format!("{}: {}", dev_name, e));
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
            "config_device_id": dev.id,
            "device_name": dev.display_name(),
            "connected": dev.connected,
            "daq_running": daq_running,
            "recording": is_recording,
            "page_mode": match page_mode {
                PageMode::Working => "working",
                PageMode::Reference => "reference",
            },
            "a2l_file": dev.original_filename,
            "a2l_path": dev.a2l_path,
            "address": dev.address,
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

/// DCM export: collect changed calibration values and write to a user-specified file.
///
/// `config` shape: `{ "path": "/abs/path/to/export.dcm", "title": "...", "device_ids": ["config_dev_id", ...] }`
/// If `device_ids` is omitted, exports all connected devices.
/// Only parameters with a cached value are exported.
#[tauri::command]
pub async fn experiment_dcm_export(
    state: State<'_, SharedState>,
    config: Value,
) -> Result<Value, String> {
    use xcp_client::a2l::CharacteristicType;
    use dcm_utils::{write_dcm_data, DcmData};
    use dcm_utils::block::Block;
    use dcm_utils::blocks::{FESTWERT, FESTWERTEBLOCK, GRUPPENKENNFELD, GRUPPENKENNLINIE};

    let path = config
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing 'path' field".to_string())?
        .to_string();

    let title = config
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("export")
        .to_string();

    let filter_ids: Option<Vec<String>> = config
        .get("device_ids")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(str::to_string)).collect());

    // Optional per-variable filter: [{device_id, name}]
    // If provided, only export those specific variables.
    let filter_vars: Option<Vec<(String, String)>> = config
        .get("variables")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|entry| {
                    let did = entry.get("device_id")?.as_str()?.to_string();
                    let name = entry.get("name")?.as_str()?.to_string();
                    Some((did, name))
                })
                .collect()
        });

    let app = state.read().await;

    let mut data = DcmData::from_blocks(Default::default());
    let mut exported = 0usize;

    for dev in &app.devices {
        if let Some(ref ids) = filter_ids {
            if !ids.contains(&dev.id) {
                continue;
            }
        }

        let cal_mgr = dev.cal_manager.read().await;
        let cal_cache = dev.cal_values.read().await;

        for (idx, param) in cal_mgr.params().iter().enumerate() {
            let _ = idx;
            // If per-variable filter is set, skip params not in the selection
            if let Some(ref fvars) = filter_vars {
                let did = dev.id.as_str();
                if !fvars.iter().any(|(d, n)| d == did && n == &param.name) {
                    continue;
                }
            }
            let value = match cal_cache.get(&param.name) {
                Some(v) => v.clone(),
                None => continue,
            };

            let name = param.name.clone();
            let desc = param.description.clone();
            let unit = param.unit.clone().unwrap_or_default();

            let get_axis = |axis_idx: usize, fallback_n: usize| -> Vec<f64> {
                param.axis_info
                    .get(axis_idx)
                    .and_then(|a| a.get_static_values())
                    .unwrap_or_else(|| (0..fallback_n).map(|i| i as f64).collect())
            };

            let block_opt: Option<Block> = match param.characteristic_type {
                CharacteristicType::Value => dcm_f64(&value).map(|v| {
                    Block::Constant(FESTWERT::from_f64(name.clone(), v, desc.clone(), unit.clone()))
                }),

                CharacteristicType::Curve => dcm_f64_array(&value).map(|values| {
                    let n = values.len();
                    let axis_info = param.axis_info.first();
                    let axis = get_axis(0, n);
                    let axis_name = axis_info.map(|a| a.pts_ref_name.as_deref().unwrap_or(a.name.as_str())).unwrap_or("").to_string();
                    let unit_x = axis_info.and_then(|a| a.unit.as_deref()).unwrap_or("").to_string();
                    Block::Table(GRUPPENKENNLINIE::from_f64(
                        &name, &values, &desc, &unit, &unit_x, &axis_name, &axis,
                    ))
                }),

                CharacteristicType::Map => dcm_f64_matrix(&value).map(|matrix| {
                    let x_count = matrix.len();
                    let y_count = matrix.first().map(|r| r.len()).unwrap_or(0);
                    let x_axis_info = param.axis_info.first();
                    let y_axis_info = param.axis_info.get(1);
                    let x_axis = get_axis(0, x_count);
                    let y_axis = get_axis(1, y_count);
                    let x_name = x_axis_info.map(|a| a.pts_ref_name.as_deref().unwrap_or(a.name.as_str())).unwrap_or("").to_string();
                    let y_name = y_axis_info.map(|a| a.pts_ref_name.as_deref().unwrap_or(a.name.as_str())).unwrap_or("").to_string();
                    let unit_x = x_axis_info.and_then(|a| a.unit.as_deref()).unwrap_or("").to_string();
                    let unit_y = y_axis_info.and_then(|a| a.unit.as_deref()).unwrap_or("").to_string();
                    let dcm_matrix: Vec<Vec<f64>> = (0..y_count)
                        .map(|y| (0..x_count).map(|x| matrix.get(x).and_then(|r| r.get(y)).copied().unwrap_or(0.0)).collect())
                        .collect();
                    Block::Map(GRUPPENKENNFELD::from_f64(
                        &name, dcm_matrix, x_axis, y_axis, &x_name, &y_name, &desc, &unit, &unit_x, &unit_y,
                    ))
                }),

                CharacteristicType::ValBlk => {
                    let flat = dcm_f64_array(&value).or_else(|| {
                        dcm_f64_matrix(&value).map(|m| m.into_iter().flatten().collect())
                    });
                    flat.map(|values| {
                        Block::ConstantBlock(FESTWERTEBLOCK::from_f64(name.clone(), values, desc.clone(), unit.clone()))
                    })
                }

                CharacteristicType::Ascii => None, // ASCII strings are not representable in DCM format
            };

            if let Some(block) = block_opt {
                data.insert_block(block);
                exported += 1;
            }
        }
    }

    drop(app);

    if exported == 0 {
        return Ok(json!({ "success": false, "error": "No cached calibration values to export. Read calibration values first." }));
    }

    #[cfg(debug_assertions)]
    let _ = std::fs::create_dir_all("./output");

    let path_buf = std::path::PathBuf::from(&path);
    write_dcm_data(&data, &path_buf);

    Ok(json!({
        "success": true,
        "message": format!("Exported {} parameter(s) to '{}'", exported, title),
        "path": path,
        "count": exported,
    }))
}

fn dcm_f64(value: &xcp_client::calibration::CalibrationValue) -> Option<f64> {
    use xcp_client::calibration::CalibrationValue;
    match value {
        CalibrationValue::U8(v) => Some(*v as f64),
        CalibrationValue::U16(v) => Some(*v as f64),
        CalibrationValue::U32(v) => Some(*v as f64),
        CalibrationValue::U64(v) => Some(*v as f64),
        CalibrationValue::I8(v) => Some(*v as f64),
        CalibrationValue::I16(v) => Some(*v as f64),
        CalibrationValue::I32(v) => Some(*v as f64),
        CalibrationValue::I64(v) => Some(*v as f64),
        CalibrationValue::F32(v) => Some(*v as f64),
        CalibrationValue::F64(v) => Some(*v),
        CalibrationValue::Bool(v) => Some(if *v { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn dcm_f64_array(value: &xcp_client::calibration::CalibrationValue) -> Option<Vec<f64>> {
    use xcp_client::calibration::CalibrationValue;
    match value {
        CalibrationValue::U8Array(a) => Some(a.iter().map(|v| *v as f64).collect()),
        CalibrationValue::U16Array(a) => Some(a.iter().map(|v| *v as f64).collect()),
        CalibrationValue::U32Array(a) => Some(a.iter().map(|v| *v as f64).collect()),
        CalibrationValue::U64Array(a) => Some(a.iter().map(|v| *v as f64).collect()),
        CalibrationValue::I8Array(a) => Some(a.iter().map(|v| *v as f64).collect()),
        CalibrationValue::I16Array(a) => Some(a.iter().map(|v| *v as f64).collect()),
        CalibrationValue::I32Array(a) => Some(a.iter().map(|v| *v as f64).collect()),
        CalibrationValue::I64Array(a) => Some(a.iter().map(|v| *v as f64).collect()),
        CalibrationValue::F32Array(a) => Some(a.iter().map(|v| *v as f64).collect()),
        CalibrationValue::F64Array(a) => Some(a.clone()),
        CalibrationValue::BoolArray(a) => Some(a.iter().map(|v| if *v { 1.0 } else { 0.0 }).collect()),
        _ => None,
    }
}

fn dcm_f64_matrix(value: &xcp_client::calibration::CalibrationValue) -> Option<Vec<Vec<f64>>> {
    use xcp_client::calibration::CalibrationValue;
    macro_rules! to_f64_matrix {
        ($m:expr) => { Some($m.iter().map(|r| r.iter().map(|v| *v as f64).collect()).collect()) };
    }
    match value {
        CalibrationValue::U8Matrix(m) => to_f64_matrix!(m),
        CalibrationValue::U16Matrix(m) => to_f64_matrix!(m),
        CalibrationValue::U32Matrix(m) => to_f64_matrix!(m),
        CalibrationValue::U64Matrix(m) => to_f64_matrix!(m),
        CalibrationValue::I8Matrix(m) => to_f64_matrix!(m),
        CalibrationValue::I16Matrix(m) => to_f64_matrix!(m),
        CalibrationValue::I32Matrix(m) => to_f64_matrix!(m),
        CalibrationValue::I64Matrix(m) => to_f64_matrix!(m),
        CalibrationValue::F32Matrix(m) => Some(m.iter().map(|r| r.iter().map(|v| *v as f64).collect()).collect()),
        CalibrationValue::F64Matrix(m) => Some(m.clone()),
        CalibrationValue::BoolMatrix(m) => Some(m.iter().map(|r| r.iter().map(|v| if *v { 1.0 } else { 0.0 }).collect()).collect()),
        _ => None,
    }
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
    // Return cached value if already stored
    let (cal_original_arc, hex_image_arc, param) = {
        let app = state.read().await;
        let dev = find_device(&app, &device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id))?;

        let originals = dev.cal_original_values.read().await;
        if let Some(val) = originals.get(&idx) {
            return Ok(json!({ "success": true, "value": val.clone() }));
        }
        drop(originals);

        let page_mode = *dev.page_mode.read().await;

        let param = dev
            .cal_manager
            .read()
            .await
            .params()
            .get(idx)
            .cloned()
            .ok_or_else(|| "Parameter not found".to_string())?;

        let cal_original_arc = dev.cal_original_values.clone();

        // Both pages read from hex: Working → working hex, Reference → reference hex
        let hex_image_arc = match page_mode {
            PageMode::Working => dev.working_hex_image.clone(),
            PageMode::Reference => dev.hex_image.clone(),
        };

        (cal_original_arc, hex_image_arc, param)
    };

    let hex_guard = hex_image_arc.read().await;
    let hex = hex_guard
        .as_ref()
        .ok_or_else(|| "No HEX file loaded for this device".to_string())?;
    let size = param.data_type.size() as u32;
    let bytes = hex.extract_range(param.address, size, 0xFF);
    let value = bytes_to_calibration_value(&bytes, &param.data_type)?;
    let formatted = value.format_scalar_str();
    drop(hex_guard);

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
    let mut saved_paths: Vec<String> = Vec::new();

    for dev in &app.devices {
        let daq_mgr = dev.daq_manager.read().await;
        if !daq_mgr.is_recording() {
            continue;
        }
        // Capture the path before stopping (path is set at record_start time)
        let mf4_path = daq_mgr.get_mdf4_path().to_path_buf();
        match daq_mgr.stop_recording().await {
            Ok(_) => {
                stopped_count += 1;
                tracing::info!("Recording stopped on {}", dev.display_name());
                saved_paths.push(mf4_path.to_string_lossy().to_string());
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
        "saved_paths": saved_paths,
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

// ─── File I/O helpers for experiment save/load ───────────────────────────────

/// Write a text string to a file at the given absolute path.
#[tauri::command]
pub async fn write_text_file(path: String, content: String) -> Result<Value, String> {
    std::fs::write(&path, content.as_bytes())
        .map_err(|e| format!("Failed to write '{}': {}", path, e))?;
    Ok(json!({ "success": true, "path": path }))
}

/// Read a text file from the given absolute path and return its contents.
#[tauri::command]
pub async fn read_text_file(path: String) -> Result<Value, String> {
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read '{}': {}", path, e))?;
    Ok(json!({ "success": true, "content": content }))
}

/// Open a folder path in the system file explorer (Explorer / Finder / Nautilus).
#[tauri::command]
pub async fn open_folder(path: String) -> Result<Value, String> {
    let folder = std::path::Path::new(&path);

    // If the path points to a file, open its parent directory
    let dir = if folder.is_file() {
        folder.parent().unwrap_or(folder)
    } else {
        folder
    };

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(dir)
            .spawn()
            .map_err(|e| format!("Failed to open explorer: {}", e))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(dir)
            .spawn()
            .map_err(|e| format!("Failed to open Finder: {}", e))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(dir)
            .spawn()
            .map_err(|e| format!("Failed to open file manager: {}", e))?;
    }

    Ok(json!({ "success": true, "path": dir.display().to_string() }))
}
