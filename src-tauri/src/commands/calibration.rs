/// Calibration Tauri commands.
///
/// Ports web/routes.rs cal_* handlers.

use serde_json::{json, Value};
use tauri::State;
use xcp_client::calibration::CalibrationValue;
use xcp_client::slave::PageMode;

use crate::commands::helpers::{
    calibration_value_to_json, read_axis_values_from_slave, read_calibration_from_slave,
    write_calibration_to_slave,
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
    // Get slave arc and param clone without holding state lock during XCP I/O
    let (slave_arc, param, dev_id) = {
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
        (slave_arc, param, dev.id.clone())
    };

    let xcp_guard = slave_arc.lock().await;
    let value = read_calibration_from_slave(&*xcp_guard, &param).await?;

    // Cache the value
    {
        let app = state.read().await;
        if let Some(dev) = app.get_device(&dev_id) {
            let mut cache = dev.cal_values.write().await;
            cache.insert(param.name.clone(), value.clone());
        }
    }

    Ok(json!({
        "success": true,
        "name": param.name,
        "value": value.format(),
    }))
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
    let (slave_arc, param) = {
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
        (slave_arc, param)
    };

    let cal_value = parse_json_value_to_cal(&param.data_type, &value)?;
    let xcp_guard = slave_arc.lock().await;
    write_calibration_to_slave(&*xcp_guard, &param, &cal_value).await?;

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
    let (slave_arc, param) = {
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
        (slave_arc, param)
    };

    // Convert user string to the element's raw value, then use compu_method reverse
    let raw = param.compu_method.to_raw(&value).unwrap_or(0i64);

    // Build an element-sized CalibrationValue pointing to element address
    let element_param = element_write_param(&param, row, col)?;
    let element_value = scalar_from_raw(raw, &element_param.data_type)?;

    let xcp_guard = slave_arc.lock().await;
    write_calibration_to_slave(&*xcp_guard, &element_param, &element_value).await?;

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
    let (slave_arc_opt, param, is_connected) = {
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
        (slave_arc_opt, param, dev.connected)
    };

    // Read axis values if connected
    let axis_values: Vec<Vec<f64>> = if is_connected {
        if let Some(arc) = slave_arc_opt {
            let guard = arc.lock().await;
            read_axis_values_from_slave(&*guard, &param).await
        } else {
            vec![]
        }
    } else {
        param
            .axis_info
            .iter()
            .filter_map(|a| a.get_static_values())
            .collect()
    };

    let axis_json: Vec<Value> = param
        .axis_info
        .iter()
        .zip(axis_values.iter())
        .map(|(info, vals)| {
            json!({
                "name": info.name,
                "unit": info.unit,
                "num_points": info.num_points,
                "values": vals,
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

    let (slave_arc, page_mode_arc) = {
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
        (slave_arc, dev.page_mode.clone())
    };

    // Use switch_all_pages which handles all segments atomically (&mut self)
    let mut slave_guard = slave_arc.lock().await;
    slave_guard
        .switch_all_pages(target)
        .await
        .map_err(|e| e.to_string())?;

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
