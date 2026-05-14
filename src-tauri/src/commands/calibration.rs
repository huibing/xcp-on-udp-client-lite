/// Calibration Tauri commands.
///
/// Ports web/routes.rs cal_* handlers.

use serde_json::{json, Value};
use tauri::State;
use xcp_client::calibration::CalibrationValue;
use xcp_client::slave::PageMode;
use xcp_client::CharacteristicType;

use crate::commands::helpers::{
    bytes_to_calibration_value, cal_value_display_string, calibration_value_to_bytes,
    calibration_value_to_display_json, calibration_value_to_json, read_axis_values_from_hex,
    read_axis_values_from_slave, write_calibration_to_slave,
};
use crate::state::SharedState;

/// List all calibration parameters across all devices.
#[tauri::command]
pub async fn cal_available(state: State<'_, SharedState>) -> Result<Value, String> {
    let app = state.read().await;
    let mut params = Vec::new();
    for dev in &app.devices {
        let mgr = dev.cal_manager.read().await;
        for (i, p) in mgr.params().iter().enumerate() {
            params.push(json!({
                "device_id": dev.id,
                "device_name": dev.display_name(),
                "idx": i,
                "name": p.name,
                "type": format!("{:?}", p.data_type),
                "address_hex": format!("0x{:08X}", p.address),
                "description": p.description,
                "unit": p.unit,
                "connected": dev.connected,
            }));
        }
    }
    Ok(json!({ "params": params }))
}

/// Read a calibration parameter from the XCP slave.
#[tauri::command]
pub async fn cal_read(
    state: State<'_, SharedState>,
    device_id: String,
    idx: usize,
) -> Result<Value, String> {
    // Extract handles from state without cloning heavy data.
    // Arc clones are just refcount bumps; HexImage is NOT cloned here.
    let (param, hex_image_arc, cal_values_arc, _dev_id, source) = {
        let app = state.read().await;
        let dev = app
            .get_device(&device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id))?;
        let mgr = dev.cal_manager.read().await;
        let param = mgr
            .params()
            .get(idx)
            .ok_or_else(|| format!("Parameter index {} out of range", idx))?
            .clone();
        let cal_values_arc = dev.cal_values.clone();
        let page_mode = *dev.page_mode.read().await;

        // Working page → read from working hex image; Reference page → reference hex image.
        let (hex_image_arc, source) = if matches!(page_mode, PageMode::Working) {
            (dev.working_hex_image.clone(), "hex_working")
        } else {
            (dev.hex_image.clone(), "hex_reference")
        };

        (param, hex_image_arc, cal_values_arc, dev.id.clone(), source)
    };

    // Read from the appropriate HEX image (no state lock held)
    {
        let hex_guard = hex_image_arc.read().await;
        if let Some(hex_image) = hex_guard.as_ref() {
            let size = param.data_type.size() as u32;
            if size == 0 {
                return Ok(json!({ "success": false, "error": "Unknown parameter size" }));
            }
            let bytes = hex_image.extract_range(param.address, size, 0xFF);
            if bytes.iter().all(|&b| b == 0xFF) && !hex_image.has_data_in_range(param.address, size) {
                return Ok(json!({ "success": false, "error": "Address not found in HEX file" }));
            }
            let value = bytes_to_calibration_value(&bytes, &param.data_type)
                .map_err(|e| format!("Decode error: {}", e))?;
            drop(hex_guard);

            cal_values_arc.write().await.insert(param.name.clone(), value.clone());

            let is_enum = param.is_enum();
            let raw_i64 = cal_value_to_raw_i64(&value);
            let display_value: Option<String> =
                raw_i64.map(|raw| param.format_value(raw));
            tracing::debug!(
                "cal_read [hex] '{}': data_type={:?} is_enum={} raw_i64={:?} value_format={:?} display_value={:?}",
                param.name,
                param.data_type,
                is_enum,
                raw_i64,
                value.format(),
                display_value,
            );
            let is_1d = param.characteristic_type == CharacteristicType::Curve;
            let display_value_arr = cal_value_display_string(&value, &param);
            let display_value_data = calibration_value_to_display_json(&value, is_1d, &param);
            let effective_display = display_value.or(display_value_arr);
            tracing::debug!(
                "cal_read [hex] '{}': compu_method={} effective_display={:?}",
                param.name,
                param.compu_method.variant_name(),
                effective_display,
            );
            return Ok(json!({
                "success": true,
                "name": param.name,
                "value": value.format(),
                "value_data": calibration_value_to_json(&value, is_1d),
                "is_enum": is_enum,
                "display_value": effective_display,
                "display_value_data": display_value_data,
                "source": source,
            }));
        }
    }

    Ok(json!({ "success": false, "error": "Device not connected and no HEX file loaded" }))
}

/// Batch-read all requested calibration parameters in a single IPC call.
///
/// Groups items by device so each slave mutex is acquired only once, then reads
/// all parameters for that device sequentially (XCP is request-response, so
/// pipelining is not possible). Returns one result entry per input item.
#[tauri::command]
pub async fn cal_read_all(
    state: State<'_, SharedState>,
    items: Vec<serde_json::Map<String, serde_json::Value>>,
) -> Result<Value, String> {
    use std::collections::HashMap;

    // Parse into (device_id, param_idx) keeping the original position
    let parsed: Vec<(String, usize)> = items
        .iter()
        .filter_map(|m| {
            let device_id = m.get("device_id")?.as_str()?.to_string();
            let idx = m.get("idx")?.as_u64()? as usize;
            Some((device_id, idx))
        })
        .collect();

    // Group item positions by device_id: device_id → [(item_pos, param_idx)]
    let mut by_device: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
    for (pos, (device_id, param_idx)) in parsed.iter().enumerate() {
        by_device.entry(device_id.clone()).or_default().push((pos, *param_idx));
    }

    let mut results: Vec<Value> = vec![json!({ "success": false, "error": "not processed" }); parsed.len()];

    for (device_id, positions) in &by_device {
        // Snapshot params and Arc handles. No HexImage data is cloned.
        let (params, hex_image_arc, cache_arc) = {
            let app = state.read().await;
            let dev = match app.get_device(device_id) {
                Some(d) => d,
                None => continue,
            };
            let mgr = dev.cal_manager.read().await;
            let params: Vec<Option<_>> = positions
                .iter()
                .map(|(_, pi)| mgr.params().get(*pi).cloned())
                .collect();
            let cache_arc = dev.cal_values.clone();
            let page_mode = *dev.page_mode.read().await;

            // Working page → working hex image; Reference page → reference hex image.
            let hex_image_arc = if matches!(page_mode, PageMode::Working) {
                dev.working_hex_image.clone()
            } else {
                dev.hex_image.clone()
            };

            (params, hex_image_arc, cache_arc)
        }; // state read lock released here

        // Acquire a single read lock on the HexImage for all reads — no clone of the map data
        let hex_guard = hex_image_arc.read().await;
        if let Some(hex_image) = hex_guard.as_ref() {
            for ((pos, _), param_opt) in positions.iter().zip(params.iter()) {
                let param = match param_opt {
                    Some(p) => p,
                    None => {
                        results[*pos] = json!({ "success": false, "error": "Parameter index out of range" });
                        continue;
                    }
                };
                let size = param.data_type.size() as u32;
                if size == 0 {
                    results[*pos] = json!({ "success": false, "name": param.name, "error": "Unknown parameter size" });
                    continue;
                }
                let bytes = hex_image.extract_range(param.address, size, 0xFF);
                if bytes.iter().all(|&b| b == 0xFF) && !hex_image.has_data_in_range(param.address, size) {
                    results[*pos] = json!({ "success": false, "name": param.name, "error": "Address not found in HEX file" });
                    continue;
                }
                match bytes_to_calibration_value(&bytes, &param.data_type) {
                    Ok(value) => {
                        cache_arc.write().await.insert(param.name.clone(), value.clone());
                        results[*pos] = json!({
                            "success": true,
                            "name": param.name,
                            "value": value.format(),
                        });
                    }
                    Err(e) => {
                        results[*pos] = json!({ "success": false, "name": param.name, "error": e });
                    }
                }
            }
        }
    }

    Ok(json!({ "results": results }))
}

/// Write a calibration parameter to the XCP slave.
/// `value` is expected as a flat array of strings matching the parameter type.
#[tauri::command]
pub async fn cal_write(
    state: State<'_, SharedState>,
    device_id: String,
    idx: usize,
    value: Value,
) -> Result<Value, String> {
    let (slave_arc, param, working_hex_arc, working_hex_path) = {
        let app = state.read().await;
        let dev = app
            .get_device(&device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id))?;
        if !dev.connected {
            return Ok(json!({ "success": false, "error": "Device not connected" }));
        }
        let sid = dev.slave_id.ok_or_else(|| "No slave ID".to_string())?;
        let slave_arc = app
            .client
            .get_slave(sid)
            .ok_or_else(|| "Slave not found".to_string())?;
        let mgr = dev.cal_manager.read().await;
        let param = mgr
            .params()
            .get(idx)
            .ok_or_else(|| format!("Parameter index {} out of range", idx))?
            .clone();
        let working_hex_arc = dev.working_hex_image.clone();
        let working_hex_path = dev.working_hex_path.clone();
        (slave_arc, param, working_hex_arc, working_hex_path)
    };

    let cal_value = if param.is_enum() {
        // Convert enum display text → raw integer via compu_method before parsing
        let text = match &value {
            Value::String(s) => s.clone(),
            Value::Array(arr) => arr
                .get(0)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            _ => value.to_string().trim_matches('"').to_string(),
        };
        match param.parse_display(&text) {
            Ok(raw) => parse_json_value_to_cal(&param.data_type, &Value::from(raw))?,
            Err(_) => parse_json_value_to_cal(&param.data_type, &value)?,
        }
    } else {
        parse_json_value_to_cal(&param.data_type, &value)?
    };

    // Convert value to bytes once (used for both XCP write and working-hex patch)
    let bytes = calibration_value_to_bytes(&cal_value, &param)?;

    let xcp_guard = slave_arc.lock().await;
    write_calibration_to_slave(&*xcp_guard, &param, &cal_value).await?;
    drop(xcp_guard);

    // Patch the working-page HEX image and persist to disk
    let hex_str_to_save = {
        let mut working_hex_guard = working_hex_arc.write().await;
        if let Some(ref mut hex) = *working_hex_guard {
            hex.add_data(param.address, &bytes);
            tracing::debug!(
                "cal_write: patched working hex at 0x{:08X} ({} bytes) for '{}'",
                param.address, bytes.len(), param.name
            );
            if !working_hex_path.is_empty() {
                Some(hex.to_intel_hex())
            } else {
                None
            }
        } else {
            None
        }
    };
    if let Some(hex_str) = hex_str_to_save {
        let path = working_hex_path.clone();
        tokio::task::spawn_blocking(move || {
            if let Err(e) = std::fs::write(&path, hex_str) {
                tracing::warn!("Failed to save working hex to '{}': {}", path, e);
            } else {
                tracing::debug!("Saved working hex to '{}'", path);
            }
        });
    }

    Ok(json!({ "success": true }))
}

/// Write a single element of a matrix/array calibration parameter.
#[tauri::command]
pub async fn cal_write_element(
    state: State<'_, SharedState>,
    device_id: String,
    idx: usize,
    row: usize,
    col: usize,
    value: String,
) -> Result<Value, String> {
    let (slave_arc, param, working_hex_arc, working_hex_path) = {
        let app = state.read().await;
        let dev = app
            .get_device(&device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id))?;
        if !dev.connected {
            return Ok(json!({ "success": false, "error": "Device not connected" }));
        }
        let sid = dev.slave_id.ok_or_else(|| "No slave ID".to_string())?;
        let slave_arc = app
            .client
            .get_slave(sid)
            .ok_or_else(|| "Slave not found".to_string())?;
        let mgr = dev.cal_manager.read().await;
        let param = mgr
            .params()
            .get(idx)
            .ok_or_else(|| format!("Parameter index {} out of range", idx))?
            .clone();
        let working_hex_arc = dev.working_hex_image.clone();
        let working_hex_path = dev.working_hex_path.clone();
        (slave_arc, param, working_hex_arc, working_hex_path)
    };

    // Convert user string to the element's raw value, then use compu_method reverse
    let raw = param.compu_method.to_raw(&value).unwrap_or(0i64);

    // Build an element-sized CalibrationValue pointing to element address
    let element_param = element_write_param(&param, row, col)?;
    let element_value = scalar_from_raw(raw, &element_param.data_type)?;
    let bytes = calibration_value_to_bytes(&element_value, &element_param)?;

    let xcp_guard = slave_arc.lock().await;
    write_calibration_to_slave(&*xcp_guard, &element_param, &element_value).await?;
    drop(xcp_guard);

    // Patch the working-page HEX image at the element's address and persist to disk
    let hex_str_to_save = {
        let mut working_hex_guard = working_hex_arc.write().await;
        if let Some(ref mut hex) = *working_hex_guard {
            hex.add_data(element_param.address, &bytes);
            tracing::debug!(
                "cal_write_element: patched working hex at 0x{:08X} ({} bytes) for '{}[{},{}]'",
                element_param.address, bytes.len(), param.name, row, col
            );
            if !working_hex_path.is_empty() {
                Some(hex.to_intel_hex())
            } else {
                None
            }
        } else {
            None
        }
    };
    if let Some(hex_str) = hex_str_to_save {
        let path = working_hex_path.clone();
        tokio::task::spawn_blocking(move || {
            if let Err(e) = std::fs::write(&path, hex_str) {
                tracing::warn!("Failed to save working hex to '{}': {}", path, e);
            } else {
                tracing::debug!("Saved working hex to '{}'", path);
            }
        });
    }

    Ok(json!({ "success": true }))
}

/// Get the cached calibration value (read from slave previously).
#[tauri::command]
pub async fn cal_value(
    state: State<'_, SharedState>,
    device_id: String,
    idx: usize,
) -> Result<Value, String> {
    let app = state.read().await;
    let dev = app
        .get_device(&device_id)
        .ok_or_else(|| format!("Device '{}' not found", device_id))?;
    let mgr = dev.cal_manager.read().await;
    let param = mgr
        .params()
        .get(idx)
        .ok_or_else(|| format!("Parameter index {} out of range", idx))?;

    let cache = dev.cal_values.read().await;
    if let Some(val) = cache.get(&param.name) {
        let is_1d = param.axis_info.len() == 1;
        Ok(json!({
            "success": true,
            "name": param.name,
            "values": calibration_value_to_json(val, is_1d),
        }))
    } else {
        Ok(json!({ "success": false, "error": "No cached value — call cal_read first" }))
    }
}

/// Get detailed metadata for a calibration parameter, including axis data.
#[tauri::command]
pub async fn cal_detail(
    state: State<'_, SharedState>,
    device_id: String,
    idx: usize,
) -> Result<Value, String> {
    let (slave_arc_opt, param, is_connected, hex_opt) = {
        let app = state.read().await;
        let dev = app
            .get_device(&device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id))?;
        let slave_arc_opt = dev.slave_id.and_then(|sid| app.client.get_slave(sid));
        let mgr = dev.cal_manager.read().await;
        let param = mgr
            .params()
            .get(idx)
            .ok_or_else(|| format!("Parameter index {} out of range", idx))?
            .clone();
        let hex_opt = dev.hex_image.read().await.clone();
        (slave_arc_opt, param, dev.connected, hex_opt)
    };

    // Read axis breakpoint values: from slave if connected, hex if available, else indices
    let axis_values: Vec<Vec<f64>> = if is_connected {
        if let Some(arc) = slave_arc_opt {
            let guard = arc.lock().await;
            read_axis_values_from_slave(&*guard, &param).await
        } else {
            vec![]
        }
    } else if let Some(hex) = &hex_opt {
        read_axis_values_from_hex(hex, &param)
    } else {
        // Static A2L values only (fixed-axis); fall back to indices
        param
            .axis_info
            .iter()
            .map(|a| {
                a.get_static_values()
                    .unwrap_or_else(|| (0..a.num_points).map(|i| i as f64).collect())
            })
            .collect()
    };

    // Always emit one entry per axis_info (don't lose metadata when values are short)
    let axis_json: Vec<Value> = param
        .axis_info
        .iter()
        .enumerate()
        .map(|(i, info)| {
            let vals = axis_values.get(i).cloned().unwrap_or_else(|| {
                info.get_static_values()
                    .unwrap_or_else(|| (0..info.num_points).map(|j| j as f64).collect())
            });
            json!({
                "name": info.name,
                "unit": info.unit,
                "num_points": info.num_points,
                "values": vals,
                "pts_ref_name": info.pts_ref_name,
            })
        })
        .collect();

    Ok(json!({
        "success": true,
        "name": param.name,
        "description": param.description,
        "type": format!("{:?}", param.data_type),
        "address_hex": format!("0x{:08X}", param.address),
        "unit": param.unit,
        "min": param.min,
        "max": param.max,
        "characteristic_type": format!("{:?}", param.characteristic_type),
        "axes": axis_json,
    }))
}

/// Get current page mode for all connected devices.
#[tauri::command]
pub async fn get_page_mode(state: State<'_, SharedState>) -> Result<Value, String> {
    let app = state.read().await;
    let mut results = Vec::new();
    for dev in app.devices.iter().filter(|d| d.connected) {
        let mode = dev.page_mode.read().await.clone();
        results.push(json!({
            "device_id": dev.id,
            "device_name": dev.display_name(),
            "page_mode": format!("{:?}", mode),
        }));
    }
    Ok(json!({ "devices": results }))
}

/// Switch working/reference page mode for a device.
///
/// When switching to the Working page, first verifies the working-page HEX
/// checksum against the ECU.  If any segment fails, the switch is blocked and
/// the caller receives `{ success: false, blocked: true, reason: "checksum_mismatch" }`.
#[tauri::command]
pub async fn switch_page_mode(
    state: State<'_, SharedState>,
    device_id: String,
    mode: String,
) -> Result<Value, String> {
    let target = match mode.to_uppercase().as_str() {
        "WORKING" | "WORK" => PageMode::Working,
        "REFERENCE" | "REF" => PageMode::Reference,
        other => return Err(format!("Unknown page mode: '{}'", other)),
    };

    let (slave_arc, page_mode_arc, working_hex_arc, cal_manager_arc) = {
        let app = state.read().await;
        let dev = app
            .get_device(&device_id)
            .ok_or_else(|| format!("Device '{}' not found", device_id))?;
        if !dev.connected {
            return Ok(json!({ "success": false, "error": "Device not connected" }));
        }
        let sid = dev.slave_id.ok_or_else(|| "No slave ID".to_string())?;
        let slave_arc = app
            .client
            .get_slave(sid)
            .ok_or_else(|| "Slave not found".to_string())?;
        (
            slave_arc,
            dev.page_mode.clone(),
            dev.working_hex_image.clone(),
            dev.cal_manager.clone(),
        )
    }; // state lock released

    // Before switching to working page, verify ECU memory against working-page HEX.
    // If the checksums don't match the switch is blocked.
    if matches!(target, PageMode::Working) {
        let working_hex_guard = working_hex_arc.read().await;
        if let Some(working_hex) = working_hex_guard.as_ref() {
            let segments = cal_manager_arc.read().await.get_memory_segments().to_vec();
            if !segments.is_empty() {
                let mut slave_guard = slave_arc.lock().await;
                let report = xcp_client::checksum::verify_segments(
                    &mut slave_guard,
                    working_hex,
                    &segments,
                    None,
                )
                .await
                .map_err(|e| format!("Checksum verification error: {}", e))?;
                drop(slave_guard);

                if !report.all_passed() {
                    let segment_results: Vec<Value> = report
                        .results
                        .iter()
                        .map(|s| {
                            json!({
                                "name": s.name,
                                "address": format!("0x{:08X}", s.address),
                                "size": s.size,
                                "master_checksum": format!("0x{:08X}", s.master_checksum),
                                "slave_checksum": format!("0x{:08X}", s.slave_checksum),
                                "match": s.passed,
                                "status": format!("{}", s.status),
                            })
                        })
                        .collect();

                    tracing::warn!(
                        "switch_page_mode: blocked switch to Working — checksum mismatch for device '{}': {}",
                        device_id, report.summary()
                    );
                    return Ok(json!({
                        "success": false,
                        "blocked": true,
                        "reason": "checksum_mismatch",
                        "error": format!(
                            "Cannot switch to working page: working-page checksum does not match ECU memory. {}",
                            report.summary()
                        ),
                        "summary": report.summary(),
                        "segments": segment_results,
                    }));
                }
            }
        }
        // No working hex or no segments → allow switch without verification
    }

    // Use switch_all_pages which handles all segments atomically (&mut self)
    let mut slave_guard = slave_arc.lock().await;
    slave_guard
        .switch_all_pages(target)
        .await
        .map_err(|e| e.to_string())?;
    drop(slave_guard);

    *page_mode_arc.write().await = target;

    Ok(json!({ "success": true, "page_mode": format!("{:?}", target) }))
}

// ─── Private helpers ──────────────────────────────────────────────────────────

/// Parse a JSON value array into a CalibrationValue matching the given CalibrationType.
fn parse_json_value_to_cal(
    data_type: &xcp_client::calibration::CalibrationType,
    json_val: &Value,
) -> Result<CalibrationValue, String> {
    use xcp_client::calibration::CalibrationType;

    // Flatten the JSON value to a 1D string list
    let flat: Vec<String> = match json_val {
        Value::Array(arr) => arr
            .iter()
            .flat_map(|v| match v {
                Value::Array(inner) => inner.iter().map(|x| x.to_string()).collect::<Vec<_>>(),
                other => vec![other.to_string().trim_matches('"').to_string()],
            })
            .collect(),
        other => vec![other.to_string().trim_matches('"').to_string()],
    };

    fn parse_f64(s: &str) -> Result<f64, String> {
        s.trim_matches('"')
            .parse::<f64>()
            .map_err(|e| format!("Parse error '{}': {}", s, e))
    }
    fn parse_i64(s: &str) -> Result<i64, String> {
        s.trim_matches('"')
            .parse::<i64>()
            .map_err(|e| format!("Parse error '{}': {}", s, e))
    }

    let v0 = flat.get(0).map(|s| s.as_str()).unwrap_or("0");
    Ok(match data_type {
        CalibrationType::U8 => CalibrationValue::U8(parse_i64(v0)? as u8),
        CalibrationType::U16 => CalibrationValue::U16(parse_i64(v0)? as u16),
        CalibrationType::U32 => CalibrationValue::U32(parse_i64(v0)? as u32),
        CalibrationType::U64 => CalibrationValue::U64(parse_i64(v0)? as u64),
        CalibrationType::I8 => CalibrationValue::I8(parse_i64(v0)? as i8),
        CalibrationType::I16 => CalibrationValue::I16(parse_i64(v0)? as i16),
        CalibrationType::I32 => CalibrationValue::I32(parse_i64(v0)? as i32),
        CalibrationType::I64 => CalibrationValue::I64(parse_i64(v0)?),
        CalibrationType::F32 => CalibrationValue::F32(parse_f64(v0)? as f32),
        CalibrationType::F64 => CalibrationValue::F64(parse_f64(v0)?),
        CalibrationType::Bool => {
            CalibrationValue::Bool(v0.to_lowercase() == "true" || v0 == "1")
        }
        CalibrationType::U8Array(n) => CalibrationValue::U8Array(
            flat.iter()
                .take(*n)
                .map(|s| parse_i64(s).map(|v| v as u8))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        CalibrationType::U16Array(n) => CalibrationValue::U16Array(
            flat.iter()
                .take(*n)
                .map(|s| parse_i64(s).map(|v| v as u16))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        CalibrationType::F32Array(n) => CalibrationValue::F32Array(
            flat.iter()
                .take(*n)
                .map(|s| parse_f64(s).map(|v| v as f32))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        CalibrationType::F64Array(n) => CalibrationValue::F64Array(
            flat.iter()
                .take(*n)
                .map(|s| parse_f64(s))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        CalibrationType::Ascii(_) => {
            CalibrationValue::Ascii(v0.to_string())
        }
        _ => return Err(format!("Unsupported write type: {:?}", data_type)),
    })
}

/// Build a CalibrationParam pointing to a single element within a matrix/array.
fn element_write_param(
    param: &xcp_client::calibration::CalibrationParam,
    row: usize,
    col: usize,
) -> Result<xcp_client::calibration::CalibrationParam, String> {
    use xcp_client::calibration::CalibrationType;

    let (elem_type, elem_size) = match &param.data_type {
        CalibrationType::U8Matrix(_, _) | CalibrationType::U8Array(_) => {
            (CalibrationType::U8, 1usize)
        }
        CalibrationType::U16Matrix(_, _) | CalibrationType::U16Array(_) => {
            (CalibrationType::U16, 2)
        }
        CalibrationType::U32Matrix(_, _) | CalibrationType::U32Array(_) => {
            (CalibrationType::U32, 4)
        }
        CalibrationType::U64Matrix(_, _) | CalibrationType::U64Array(_) => {
            (CalibrationType::U64, 8)
        }
        CalibrationType::I8Matrix(_, _) | CalibrationType::I8Array(_) => {
            (CalibrationType::I8, 1)
        }
        CalibrationType::I16Matrix(_, _) | CalibrationType::I16Array(_) => {
            (CalibrationType::I16, 2)
        }
        CalibrationType::I32Matrix(_, _) | CalibrationType::I32Array(_) => {
            (CalibrationType::I32, 4)
        }
        CalibrationType::I64Matrix(_, _) | CalibrationType::I64Array(_) => {
            (CalibrationType::I64, 8)
        }
        CalibrationType::F32Matrix(_, _) | CalibrationType::F32Array(_) => {
            (CalibrationType::F32, 4)
        }
        CalibrationType::F64Matrix(_, _) | CalibrationType::F64Array(_) => {
            (CalibrationType::F64, 8)
        }
        CalibrationType::BoolMatrix(_, _) | CalibrationType::BoolArray(_) => {
            (CalibrationType::Bool, 1)
        }
        _ => return Err("Scalar types cannot use element write".to_string()),
    };

    let cols = match &param.data_type {
        CalibrationType::U8Matrix(_, c)
        | CalibrationType::U16Matrix(_, c)
        | CalibrationType::U32Matrix(_, c)
        | CalibrationType::U64Matrix(_, c)
        | CalibrationType::I8Matrix(_, c)
        | CalibrationType::I16Matrix(_, c)
        | CalibrationType::I32Matrix(_, c)
        | CalibrationType::I64Matrix(_, c)
        | CalibrationType::F32Matrix(_, c)
        | CalibrationType::F64Matrix(_, c)
        | CalibrationType::BoolMatrix(_, c) => *c,
        _ => 1, // 1D array — col is the flat index
    };

    let flat_idx = row * cols + col;
    let offset = (flat_idx * elem_size) as u32;
    let address = param.address + offset;

    Ok(xcp_client::calibration::CalibrationParam::new(
        &param.name,
        address,
        param.address_ext,
        elem_type,
    ))
}

/// Build a scalar CalibrationValue from a raw i64 for a given element type.
fn scalar_from_raw(
    raw: i64,
    data_type: &xcp_client::calibration::CalibrationType,
) -> Result<CalibrationValue, String> {
    use xcp_client::calibration::CalibrationType;
    Ok(match data_type {
        CalibrationType::U8 => CalibrationValue::U8(raw as u8),
        CalibrationType::U16 => CalibrationValue::U16(raw as u16),
        CalibrationType::U32 => CalibrationValue::U32(raw as u32),
        CalibrationType::U64 => CalibrationValue::U64(raw as u64),
        CalibrationType::I8 => CalibrationValue::I8(raw as i8),
        CalibrationType::I16 => CalibrationValue::I16(raw as i16),
        CalibrationType::I32 => CalibrationValue::I32(raw as i32),
        CalibrationType::I64 => CalibrationValue::I64(raw),
        CalibrationType::F32 => CalibrationValue::F32(raw as f32),
        CalibrationType::F64 => CalibrationValue::F64(raw as f64),
        CalibrationType::Bool => CalibrationValue::Bool(raw != 0),
        other => return Err(format!("Expected scalar type, got {:?}", other)),
    })
}

/// Extract the underlying raw i64 from a scalar CalibrationValue (for enum display).
fn cal_value_to_raw_i64(value: &CalibrationValue) -> Option<i64> {
    match value {
        CalibrationValue::U8(v) => Some(*v as i64),
        CalibrationValue::U16(v) => Some(*v as i64),
        CalibrationValue::U32(v) => Some(*v as i64),
        CalibrationValue::U64(v) => Some(*v as i64),
        CalibrationValue::I8(v) => Some(*v as i64),
        CalibrationValue::I16(v) => Some(*v as i64),
        CalibrationValue::I32(v) => Some(*v as i64),
        CalibrationValue::I64(v) => Some(*v),
        CalibrationValue::Bool(v) => Some(*v as i64),
        _ => None,
    }
}
