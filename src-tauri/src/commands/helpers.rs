/// Shared helper functions ported from src/web/routes.rs.
/// Used by calibration, a2l, and other command modules.
use serde_json::{json, Value};
use xcp_client::a2l::{
    A2lCharacteristic, A2lDataType, A2lMeasurement, A2lReader, AxisInfo, AxisPointSource,
};
use xcp_client::calibration::{
    matrix_to_flat, CalibrationParam, CalibrationType, CalibrationValue,
};
use xcp_client::daq::Measurement;
use xcp_client::protocol::Mta;
use xcp_client::slave::XcpSlave;

/// Compute element-wise COMPU_METHOD conversion for array/matrix CalibrationValues.
///
/// Returns `Some(display_string)` for integer array/matrix types when the param has a
/// non-Identical conversion; returns `None` for scalars, float arrays, and Identical.
/// The returned string mirrors `CalibrationValue::format()` but with converted elements:
///   U32Array → "[9.800000, 17.800000, 27.000000, ...]"
pub fn cal_value_display_string(
    value: &CalibrationValue,
    param: &CalibrationParam,
) -> Option<String> {
    if param.compu_method.variant_name() == "Identical" {
        return None;
    }
    let fmt = |raw: i64| param.format_value(raw);
    match value {
        CalibrationValue::U8Array(arr) => Some(format!(
            "[{}]",
            arr.iter()
                .map(|&v| fmt(v as i64))
                .collect::<Vec<_>>()
                .join(", ")
        )),
        CalibrationValue::U16Array(arr) => Some(format!(
            "[{}]",
            arr.iter()
                .map(|&v| fmt(v as i64))
                .collect::<Vec<_>>()
                .join(", ")
        )),
        CalibrationValue::U32Array(arr) => Some(format!(
            "[{}]",
            arr.iter()
                .map(|&v| fmt(v as i64))
                .collect::<Vec<_>>()
                .join(", ")
        )),
        CalibrationValue::U64Array(arr) => Some(format!(
            "[{}]",
            arr.iter()
                .map(|&v| fmt(v as i64))
                .collect::<Vec<_>>()
                .join(", ")
        )),
        CalibrationValue::I8Array(arr) => Some(format!(
            "[{}]",
            arr.iter()
                .map(|&v| fmt(v as i64))
                .collect::<Vec<_>>()
                .join(", ")
        )),
        CalibrationValue::I16Array(arr) => Some(format!(
            "[{}]",
            arr.iter()
                .map(|&v| fmt(v as i64))
                .collect::<Vec<_>>()
                .join(", ")
        )),
        CalibrationValue::I32Array(arr) => Some(format!(
            "[{}]",
            arr.iter()
                .map(|&v| fmt(v as i64))
                .collect::<Vec<_>>()
                .join(", ")
        )),
        CalibrationValue::I64Array(arr) => Some(format!(
            "[{}]",
            arr.iter().map(|&v| fmt(v)).collect::<Vec<_>>().join(", ")
        )),
        CalibrationValue::U8Matrix(mat) => {
            let rows: Vec<String> = mat
                .iter()
                .map(|row| {
                    format!(
                        "[{}]",
                        row.iter()
                            .map(|&v| fmt(v as i64))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })
                .collect();
            Some(format!("[{}]", rows.join(", ")))
        }
        CalibrationValue::U16Matrix(mat) => {
            let rows: Vec<String> = mat
                .iter()
                .map(|row| {
                    format!(
                        "[{}]",
                        row.iter()
                            .map(|&v| fmt(v as i64))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })
                .collect();
            Some(format!("[{}]", rows.join(", ")))
        }
        CalibrationValue::U32Matrix(mat) => {
            let rows: Vec<String> = mat
                .iter()
                .map(|row| {
                    format!(
                        "[{}]",
                        row.iter()
                            .map(|&v| fmt(v as i64))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })
                .collect();
            Some(format!("[{}]", rows.join(", ")))
        }
        CalibrationValue::I32Matrix(mat) => {
            let rows: Vec<String> = mat
                .iter()
                .map(|row| {
                    format!(
                        "[{}]",
                        row.iter()
                            .map(|&v| fmt(v as i64))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })
                .collect();
            Some(format!("[{}]", rows.join(", ")))
        }
        _ => None, // scalar, float, bool — handled elsewhere or no conversion needed
    }
}

/// Produce a COMPU_METHOD-converted version of value_data (2-D JSON array).
/// Elements are converted strings (e.g., "27.000000") rather than raw integers.
/// Returns the same shape as `calibration_value_to_json`, or `None` for Identical/float.
pub fn calibration_value_to_display_json(
    value: &CalibrationValue,
    is_1d: bool,
    param: &CalibrationParam,
) -> Option<Value> {
    if param.compu_method.variant_name() == "Identical" {
        return None;
    }
    let fmt = |raw: i64| Value::String(param.format_value(raw));
    match value {
        CalibrationValue::U8Array(arr) => {
            let items: Vec<Value> = arr.iter().map(|&v| fmt(v as i64)).collect();
            Some(if is_1d { json!(items) } else { json!([items]) })
        }
        CalibrationValue::U16Array(arr) => {
            let items: Vec<Value> = arr.iter().map(|&v| fmt(v as i64)).collect();
            Some(if is_1d { json!(items) } else { json!([items]) })
        }
        CalibrationValue::U32Array(arr) => {
            let items: Vec<Value> = arr.iter().map(|&v| fmt(v as i64)).collect();
            Some(if is_1d { json!(items) } else { json!([items]) })
        }
        CalibrationValue::U64Array(arr) => {
            let items: Vec<Value> = arr.iter().map(|&v| fmt(v as i64)).collect();
            Some(if is_1d { json!(items) } else { json!([items]) })
        }
        CalibrationValue::I8Array(arr) => {
            let items: Vec<Value> = arr.iter().map(|&v| fmt(v as i64)).collect();
            Some(if is_1d { json!(items) } else { json!([items]) })
        }
        CalibrationValue::I16Array(arr) => {
            let items: Vec<Value> = arr.iter().map(|&v| fmt(v as i64)).collect();
            Some(if is_1d { json!(items) } else { json!([items]) })
        }
        CalibrationValue::I32Array(arr) => {
            let items: Vec<Value> = arr.iter().map(|&v| fmt(v as i64)).collect();
            Some(if is_1d { json!(items) } else { json!([items]) })
        }
        CalibrationValue::I64Array(arr) => {
            let items: Vec<Value> = arr.iter().map(|&v| fmt(v)).collect();
            Some(if is_1d { json!(items) } else { json!([items]) })
        }
        CalibrationValue::U8Matrix(mat) => Some(json!(mat
            .iter()
            .map(|row| { row.iter().map(|&v| fmt(v as i64)).collect::<Vec<_>>() })
            .collect::<Vec<_>>())),
        CalibrationValue::U16Matrix(mat) => Some(json!(mat
            .iter()
            .map(|row| { row.iter().map(|&v| fmt(v as i64)).collect::<Vec<_>>() })
            .collect::<Vec<_>>())),
        CalibrationValue::U32Matrix(mat) => Some(json!(mat
            .iter()
            .map(|row| { row.iter().map(|&v| fmt(v as i64)).collect::<Vec<_>>() })
            .collect::<Vec<_>>())),
        CalibrationValue::U64Matrix(mat) => Some(json!(mat
            .iter()
            .map(|row| { row.iter().map(|&v| fmt(v as i64)).collect::<Vec<_>>() })
            .collect::<Vec<_>>())),
        CalibrationValue::I8Matrix(mat) => Some(json!(mat
            .iter()
            .map(|row| { row.iter().map(|&v| fmt(v as i64)).collect::<Vec<_>>() })
            .collect::<Vec<_>>())),
        CalibrationValue::I16Matrix(mat) => Some(json!(mat
            .iter()
            .map(|row| { row.iter().map(|&v| fmt(v as i64)).collect::<Vec<_>>() })
            .collect::<Vec<_>>())),
        CalibrationValue::I32Matrix(mat) => Some(json!(mat
            .iter()
            .map(|row| { row.iter().map(|&v| fmt(v as i64)).collect::<Vec<_>>() })
            .collect::<Vec<_>>())),
        CalibrationValue::I64Matrix(mat) => Some(json!(mat
            .iter()
            .map(|row| { row.iter().map(|&v| fmt(v)).collect::<Vec<_>>() })
            .collect::<Vec<_>>())),
        _ => None,
    }
}

// ─── Byte → CalibrationValue ──────────────────────────────────────────────────

pub fn bytes_to_calibration_value(
    data: &[u8],
    cal_type: &CalibrationType,
) -> Result<CalibrationValue, String> {
    let value = match cal_type {
        CalibrationType::U8 => CalibrationValue::U8(data[0]),
        CalibrationType::U16 => CalibrationValue::U16(u16::from_le_bytes([data[0], data[1]])),
        CalibrationType::U32 => {
            CalibrationValue::U32(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
        }
        CalibrationType::U64 => CalibrationValue::U64(u64::from_le_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ])),
        CalibrationType::I8 => CalibrationValue::I8(data[0] as i8),
        CalibrationType::I16 => CalibrationValue::I16(i16::from_le_bytes([data[0], data[1]])),
        CalibrationType::I32 => {
            CalibrationValue::I32(i32::from_le_bytes([data[0], data[1], data[2], data[3]]))
        }
        CalibrationType::I64 => CalibrationValue::I64(i64::from_le_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ])),
        CalibrationType::F32 => {
            CalibrationValue::F32(f32::from_le_bytes([data[0], data[1], data[2], data[3]]))
        }
        CalibrationType::F64 => CalibrationValue::F64(f64::from_le_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ])),
        CalibrationType::Bool => CalibrationValue::Bool(data[0] != 0),
        // Array types
        CalibrationType::U8Array(_) => CalibrationValue::U8Array(data.to_vec()),
        CalibrationType::U16Array(_) => CalibrationValue::U16Array(
            data.chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect(),
        ),
        CalibrationType::U32Array(_) => CalibrationValue::U32Array(
            data.chunks_exact(4)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect(),
        ),
        CalibrationType::U64Array(_) => CalibrationValue::U64Array(
            data.chunks_exact(8)
                .map(|c| u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
                .collect(),
        ),
        CalibrationType::I8Array(_) => {
            CalibrationValue::I8Array(data.iter().map(|&v| v as i8).collect())
        }
        CalibrationType::I16Array(_) => CalibrationValue::I16Array(
            data.chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect(),
        ),
        CalibrationType::I32Array(_) => CalibrationValue::I32Array(
            data.chunks_exact(4)
                .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect(),
        ),
        CalibrationType::I64Array(_) => CalibrationValue::I64Array(
            data.chunks_exact(8)
                .map(|c| i64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
                .collect(),
        ),
        CalibrationType::F32Array(_) => CalibrationValue::F32Array(
            data.chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect(),
        ),
        CalibrationType::F64Array(_) => CalibrationValue::F64Array(
            data.chunks_exact(8)
                .map(|c| f64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
                .collect(),
        ),
        CalibrationType::BoolArray(_) => {
            CalibrationValue::BoolArray(data.iter().map(|&v| v != 0).collect())
        }
        // Matrix types (row-major)
        CalibrationType::U8Matrix(_, cols) => {
            CalibrationValue::U8Matrix(data.chunks_exact(*cols).map(|c| c.to_vec()).collect())
        }
        CalibrationType::U16Matrix(_, cols) => CalibrationValue::U16Matrix(
            data.chunks_exact(cols * 2)
                .map(|row| {
                    row.chunks_exact(2)
                        .map(|c| u16::from_le_bytes([c[0], c[1]]))
                        .collect()
                })
                .collect(),
        ),
        CalibrationType::U32Matrix(_, cols) => CalibrationValue::U32Matrix(
            data.chunks_exact(cols * 4)
                .map(|row| {
                    row.chunks_exact(4)
                        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                        .collect()
                })
                .collect(),
        ),
        CalibrationType::U64Matrix(_, cols) => CalibrationValue::U64Matrix(
            data.chunks_exact(cols * 8)
                .map(|row| {
                    row.chunks_exact(8)
                        .map(|c| {
                            u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]])
                        })
                        .collect()
                })
                .collect(),
        ),
        CalibrationType::I8Matrix(_, cols) => CalibrationValue::I8Matrix(
            data.chunks_exact(*cols)
                .map(|row| row.iter().map(|&v| v as i8).collect())
                .collect(),
        ),
        CalibrationType::I16Matrix(_, cols) => CalibrationValue::I16Matrix(
            data.chunks_exact(cols * 2)
                .map(|row| {
                    row.chunks_exact(2)
                        .map(|c| i16::from_le_bytes([c[0], c[1]]))
                        .collect()
                })
                .collect(),
        ),
        CalibrationType::I32Matrix(_, cols) => CalibrationValue::I32Matrix(
            data.chunks_exact(cols * 4)
                .map(|row| {
                    row.chunks_exact(4)
                        .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                        .collect()
                })
                .collect(),
        ),
        CalibrationType::I64Matrix(_, cols) => CalibrationValue::I64Matrix(
            data.chunks_exact(cols * 8)
                .map(|row| {
                    row.chunks_exact(8)
                        .map(|c| {
                            i64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]])
                        })
                        .collect()
                })
                .collect(),
        ),
        CalibrationType::F32Matrix(_, cols) => CalibrationValue::F32Matrix(
            data.chunks_exact(cols * 4)
                .map(|row| {
                    row.chunks_exact(4)
                        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                        .collect()
                })
                .collect(),
        ),
        CalibrationType::F64Matrix(_, cols) => CalibrationValue::F64Matrix(
            data.chunks_exact(cols * 8)
                .map(|row| {
                    row.chunks_exact(8)
                        .map(|c| {
                            f64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]])
                        })
                        .collect()
                })
                .collect(),
        ),
        CalibrationType::BoolMatrix(_, cols) => CalibrationValue::BoolMatrix(
            data.chunks_exact(*cols)
                .map(|row| row.iter().map(|&v| v != 0).collect())
                .collect(),
        ),
        CalibrationType::Ascii(n) => {
            let s = String::from_utf8_lossy(&data[..*n.min(&data.len())])
                .trim_end_matches('\0')
                .to_string();
            CalibrationValue::Ascii(s)
        }
    };
    Ok(value)
}

// ─── XCP slave read ───────────────────────────────────────────────────────────

pub async fn read_calibration_from_slave(
    slave: &XcpSlave,
    param: &CalibrationParam,
) -> Result<CalibrationValue, String> {
    let size = param.data_type.size();
    if size == 0 {
        return Err(format!(
            "Invalid parameter size (0 bytes) for '{}'",
            param.name
        ));
    }
    let mta = Mta::new(param.address, param.address_ext);
    let data = slave
        .read_large_data(mta, size, None)
        .await
        .map_err(|e| e.to_string())?;
    if data.len() != size {
        return Err(format!(
            "Data length mismatch: expected {} bytes, got {} for '{}'",
            size,
            data.len(),
            param.name
        ));
    }
    bytes_to_calibration_value(&data, &param.data_type)
}

// ─── XCP slave write ──────────────────────────────────────────────────────────

/// Convert a `CalibrationValue` to raw little-endian bytes matching `param.data_type`.
///
/// Used both by `write_calibration_to_slave` (for XCP DOWNLOAD) and by the
/// working-page HEX patching code (to keep both in sync after a write).
pub fn calibration_value_to_bytes(
    value: &CalibrationValue,
    param: &CalibrationParam,
) -> Result<Vec<u8>, String> {
    let data: Vec<u8> = match (value, &param.data_type) {
        (CalibrationValue::U8(v), CalibrationType::U8) => vec![*v],
        (CalibrationValue::U16(v), CalibrationType::U16) => v.to_le_bytes().to_vec(),
        (CalibrationValue::U32(v), CalibrationType::U32) => v.to_le_bytes().to_vec(),
        (CalibrationValue::U64(v), CalibrationType::U64) => v.to_le_bytes().to_vec(),
        (CalibrationValue::I8(v), CalibrationType::I8) => vec![*v as u8],
        (CalibrationValue::I16(v), CalibrationType::I16) => v.to_le_bytes().to_vec(),
        (CalibrationValue::I32(v), CalibrationType::I32) => v.to_le_bytes().to_vec(),
        (CalibrationValue::I64(v), CalibrationType::I64) => v.to_le_bytes().to_vec(),
        (CalibrationValue::F32(v), CalibrationType::F32) => v.to_le_bytes().to_vec(),
        (CalibrationValue::F64(v), CalibrationType::F64) => v.to_le_bytes().to_vec(),
        (CalibrationValue::Bool(v), CalibrationType::Bool) => vec![if *v { 1 } else { 0 }],
        (CalibrationValue::U8Array(arr), CalibrationType::U8Array(_)) => {
            arr.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::U16Array(arr), CalibrationType::U16Array(_)) => {
            arr.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::U32Array(arr), CalibrationType::U32Array(_)) => {
            arr.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::U64Array(arr), CalibrationType::U64Array(_)) => {
            arr.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::I8Array(arr), CalibrationType::I8Array(_)) => {
            arr.iter().map(|v| *v as u8).collect()
        }
        (CalibrationValue::I16Array(arr), CalibrationType::I16Array(_)) => {
            arr.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::I32Array(arr), CalibrationType::I32Array(_)) => {
            arr.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::I64Array(arr), CalibrationType::I64Array(_)) => {
            arr.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::F32Array(arr), CalibrationType::F32Array(_)) => {
            arr.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::F64Array(arr), CalibrationType::F64Array(_)) => {
            arr.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::BoolArray(arr), CalibrationType::BoolArray(_)) => {
            arr.iter().map(|v| if *v { 1u8 } else { 0u8 }).collect()
        }
        (CalibrationValue::U8Matrix(mat), CalibrationType::U8Matrix(rows, cols)) => {
            matrix_to_flat(mat, *rows, *cols, param.storage_order)
        }
        (CalibrationValue::U16Matrix(mat), CalibrationType::U16Matrix(rows, cols)) => {
            let flat: Vec<u16> = matrix_to_flat(mat, *rows, *cols, param.storage_order);
            flat.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::U32Matrix(mat), CalibrationType::U32Matrix(rows, cols)) => {
            let flat: Vec<u32> = matrix_to_flat(mat, *rows, *cols, param.storage_order);
            flat.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::U64Matrix(mat), CalibrationType::U64Matrix(rows, cols)) => {
            let flat: Vec<u64> = matrix_to_flat(mat, *rows, *cols, param.storage_order);
            flat.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::I8Matrix(mat), CalibrationType::I8Matrix(rows, cols)) => {
            let flat: Vec<i8> = matrix_to_flat(mat, *rows, *cols, param.storage_order);
            flat.iter().map(|v| *v as u8).collect()
        }
        (CalibrationValue::I16Matrix(mat), CalibrationType::I16Matrix(rows, cols)) => {
            let flat: Vec<i16> = matrix_to_flat(mat, *rows, *cols, param.storage_order);
            flat.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::I32Matrix(mat), CalibrationType::I32Matrix(rows, cols)) => {
            let flat: Vec<i32> = matrix_to_flat(mat, *rows, *cols, param.storage_order);
            flat.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::I64Matrix(mat), CalibrationType::I64Matrix(rows, cols)) => {
            let flat: Vec<i64> = matrix_to_flat(mat, *rows, *cols, param.storage_order);
            flat.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::F32Matrix(mat), CalibrationType::F32Matrix(rows, cols)) => {
            let flat: Vec<f32> = matrix_to_flat(mat, *rows, *cols, param.storage_order);
            flat.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::F64Matrix(mat), CalibrationType::F64Matrix(rows, cols)) => {
            let flat: Vec<f64> = matrix_to_flat(mat, *rows, *cols, param.storage_order);
            flat.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::BoolMatrix(mat), CalibrationType::BoolMatrix(rows, cols)) => {
            let flat: Vec<bool> = matrix_to_flat(mat, *rows, *cols, param.storage_order);
            flat.iter().map(|v| if *v { 1u8 } else { 0u8 }).collect()
        }
        (CalibrationValue::Ascii(s), CalibrationType::Ascii(n)) => {
            let src = s.as_bytes();
            let mut bytes = vec![0u8; *n];
            let copy_len = src.len().min(*n);
            bytes[..copy_len].copy_from_slice(&src[..copy_len]);
            bytes
        }
        _ => return Err(format!("Value/type mismatch for '{}'", param.name)),
    };
    Ok(data)
}

pub async fn write_calibration_to_slave(
    slave: &XcpSlave,
    param: &CalibrationParam,
    value: &CalibrationValue,
) -> Result<(), String> {
    let mta = Mta::new(param.address, param.address_ext);
    let data = calibration_value_to_bytes(value, param)?;

    if data.len() <= 255 {
        slave
            .write_memory_sequential(mta, &data)
            .await
            .map_err(|e| e.to_string())
    } else {
        slave
            .write_u8_array_sequential(mta, &data)
            .await
            .map_err(|e| e.to_string())
    }
}

// ─── Axis reading ─────────────────────────────────────────────────────────────

pub async fn read_axis_values_from_slave(
    slave: &XcpSlave,
    param: &CalibrationParam,
) -> Vec<Vec<f64>> {
    let mut result = Vec::new();
    for axis in &param.axis_info {
        if let Some(values) = axis.get_static_values() {
            result.push(values);
            continue;
        }
        if let AxisPointSource::Memory {
            address,
            address_ext,
            data_type,
            num_points,
        } = &axis.point_source
        {
            let element_size = scalar_data_type_size(data_type);
            let total_size = element_size * num_points;
            let mta = Mta::new(*address, *address_ext);
            match slave.read_large_data(mta, total_size, None).await {
                Ok(data) => result.push(parse_axis_values(&data, data_type, *num_points)),
                Err(e) => {
                    tracing::warn!("Failed to read axis for '{}': {}", axis.name, e);
                    result.push((0..*num_points).map(|i| i as f64).collect());
                }
            }
        } else {
            result.push((0..axis.num_points).map(|i| i as f64).collect());
        }
    }
    result
}

fn scalar_data_type_size(dt: &A2lDataType) -> usize {
    match dt {
        A2lDataType::U8 | A2lDataType::I8 | A2lDataType::Bool => 1,
        A2lDataType::U16 | A2lDataType::I16 => 2,
        A2lDataType::U32 | A2lDataType::I32 | A2lDataType::F32 => 4,
        A2lDataType::U64 | A2lDataType::I64 | A2lDataType::F64 => 8,
        _ => 4,
    }
}

pub fn parse_axis_values(data: &[u8], data_type: &A2lDataType, num_points: usize) -> Vec<f64> {
    let element_size = scalar_data_type_size(data_type);
    let mut values = Vec::with_capacity(num_points);
    for i in 0..num_points {
        let offset = i * element_size;
        if offset + element_size > data.len() {
            values.push(i as f64);
            continue;
        }
        let slice = &data[offset..offset + element_size];
        let val = match data_type {
            A2lDataType::U8 | A2lDataType::U8Array(_) => slice[0] as f64,
            A2lDataType::I8 | A2lDataType::I8Array(_) => slice[0] as i8 as f64,
            A2lDataType::U16 | A2lDataType::U16Array(_) => {
                u16::from_le_bytes([slice[0], slice[1]]) as f64
            }
            A2lDataType::I16 | A2lDataType::I16Array(_) => {
                i16::from_le_bytes([slice[0], slice[1]]) as f64
            }
            A2lDataType::U32 | A2lDataType::U32Array(_) => {
                u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]) as f64
            }
            A2lDataType::I32 | A2lDataType::I32Array(_) => {
                i32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]) as f64
            }
            A2lDataType::F32 | A2lDataType::F32Array(_) => {
                f32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]) as f64
            }
            A2lDataType::U64 | A2lDataType::U64Array(_) => u64::from_le_bytes([
                slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
            ]) as f64,
            A2lDataType::I64 | A2lDataType::I64Array(_) => i64::from_le_bytes([
                slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
            ]) as f64,
            A2lDataType::F64 | A2lDataType::F64Array(_) => f64::from_le_bytes([
                slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
            ]),
            _ => i as f64,
        };
        values.push(val);
    }
    values
}

// ─── A2L conversion helpers ───────────────────────────────────────────────────

/// Read axis breakpoint values from a HEX image (offline mode).
/// Falls back to static A2L values first, then reads from HEX memory,
/// then computes from A2L min/max, then uses indices.
pub fn read_axis_values_from_hex(
    hex: &xcp_client::hex::HexImage,
    param: &CalibrationParam,
) -> Vec<Vec<f64>> {
    let mut result = Vec::new();
    for axis in &param.axis_info {
        if let Some(values) = axis.get_static_values() {
            result.push(values);
            continue;
        }
        if let AxisPointSource::Memory {
            address,
            data_type,
            num_points,
            ..
        } = &axis.point_source
        {
            let element_size = scalar_data_type_size(data_type);
            let total_size = (element_size * *num_points) as u32;
            if hex.has_data_in_range(*address, total_size) {
                let raw = hex.extract_range(*address, total_size, 0xFF);
                let values = parse_axis_values(&raw, data_type, *num_points);
                // NaN in axis breakpoints is invalid — fall back to min/max
                if values.iter().any(|v| v.is_nan()) {
                    result.push(axis_fallback_from_min_max(axis));
                } else {
                    result.push(values);
                }
            } else {
                // HEX file doesn't cover this AXIS_PTS address — compute from A2L metadata
                result.push(axis_fallback_from_min_max(axis));
            }
        } else {
            result.push((0..axis.num_points).map(|i| i as f64).collect());
        }
    }
    result
}

/// Compute axis breakpoint values from A2L min/max limits via linear interpolation.
fn axis_fallback_from_min_max(axis: &AxisInfo) -> Vec<f64> {
    if axis.num_points <= 1 {
        vec![axis.min]
    } else {
        let step = (axis.max - axis.min) / (axis.num_points - 1) as f64;
        (0..axis.num_points)
            .map(|i| axis.min + i as f64 * step)
            .collect()
    }
}

pub fn convert_characteristic(
    ch: A2lCharacteristic,
    reader: &A2lReader,
) -> Option<CalibrationParam> {
    xcp_client::calibration::a2l_characteristic_to_param(ch, reader)
}

pub fn convert_measurement(m: A2lMeasurement, reader: &A2lReader) -> Option<Measurement> {
    Some(Measurement::with_type(
        &m.name,
        m.address,
        m.address_ext,
        m.data_type,
        m.unit,
        reader.get_compu_method(&m.conversion_name),
    ))
}

// ─── CalibrationValue → JSON ──────────────────────────────────────────────────

pub fn calibration_value_to_json(value: &CalibrationValue, is_1d: bool) -> Value {
    match value {
        CalibrationValue::U8(v) => json!([*v]),
        CalibrationValue::U16(v) => json!([*v]),
        CalibrationValue::U32(v) => json!([*v]),
        CalibrationValue::U64(v) => json!([*v]),
        CalibrationValue::I8(v) => json!([*v]),
        CalibrationValue::I16(v) => json!([*v]),
        CalibrationValue::I32(v) => json!([*v]),
        CalibrationValue::I64(v) => json!([*v]),
        CalibrationValue::F32(v) => json!([format!("{:.6}", v)]),
        CalibrationValue::F64(v) => json!([format!("{:.6}", v)]),
        CalibrationValue::Bool(v) => json!([*v]),
        CalibrationValue::U8Array(arr) => {
            let items: Vec<Value> = arr.iter().map(|v| json!(*v)).collect();
            if is_1d {
                json!(items)
            } else {
                json!([items])
            }
        }
        CalibrationValue::U16Array(arr) => {
            let items: Vec<Value> = arr.iter().map(|v| json!(*v)).collect();
            if is_1d {
                json!(items)
            } else {
                json!([items])
            }
        }
        CalibrationValue::U32Array(arr) => {
            let items: Vec<Value> = arr.iter().map(|v| json!(*v)).collect();
            if is_1d {
                json!(items)
            } else {
                json!([items])
            }
        }
        CalibrationValue::U64Array(arr) => {
            let items: Vec<Value> = arr.iter().map(|v| json!(*v)).collect();
            if is_1d {
                json!(items)
            } else {
                json!([items])
            }
        }
        CalibrationValue::I8Array(arr) => {
            let items: Vec<Value> = arr.iter().map(|v| json!(*v)).collect();
            if is_1d {
                json!(items)
            } else {
                json!([items])
            }
        }
        CalibrationValue::I16Array(arr) => {
            let items: Vec<Value> = arr.iter().map(|v| json!(*v)).collect();
            if is_1d {
                json!(items)
            } else {
                json!([items])
            }
        }
        CalibrationValue::I32Array(arr) => {
            let items: Vec<Value> = arr.iter().map(|v| json!(*v)).collect();
            if is_1d {
                json!(items)
            } else {
                json!([items])
            }
        }
        CalibrationValue::I64Array(arr) => {
            let items: Vec<Value> = arr.iter().map(|v| json!(*v)).collect();
            if is_1d {
                json!(items)
            } else {
                json!([items])
            }
        }
        CalibrationValue::F32Array(arr) => {
            let items: Vec<Value> = arr.iter().map(|v| json!(format!("{:.6}", v))).collect();
            if is_1d {
                json!(items)
            } else {
                json!([items])
            }
        }
        CalibrationValue::F64Array(arr) => {
            let items: Vec<Value> = arr.iter().map(|v| json!(format!("{:.6}", v))).collect();
            if is_1d {
                json!(items)
            } else {
                json!([items])
            }
        }
        CalibrationValue::BoolArray(arr) => {
            let items: Vec<Value> = arr.iter().map(|v| json!(*v)).collect();
            if is_1d {
                json!(items)
            } else {
                json!([items])
            }
        }
        CalibrationValue::U8Matrix(mat) => json!(mat
            .iter()
            .map(|row| row.iter().map(|v| json!(*v)).collect::<Vec<_>>())
            .collect::<Vec<_>>()),
        CalibrationValue::U16Matrix(mat) => json!(mat
            .iter()
            .map(|row| row.iter().map(|v| json!(*v)).collect::<Vec<_>>())
            .collect::<Vec<_>>()),
        CalibrationValue::U32Matrix(mat) => json!(mat
            .iter()
            .map(|row| row.iter().map(|v| json!(*v)).collect::<Vec<_>>())
            .collect::<Vec<_>>()),
        CalibrationValue::U64Matrix(mat) => json!(mat
            .iter()
            .map(|row| row.iter().map(|v| json!(*v)).collect::<Vec<_>>())
            .collect::<Vec<_>>()),
        CalibrationValue::I8Matrix(mat) => json!(mat
            .iter()
            .map(|row| row.iter().map(|v| json!(*v)).collect::<Vec<_>>())
            .collect::<Vec<_>>()),
        CalibrationValue::I16Matrix(mat) => json!(mat
            .iter()
            .map(|row| row.iter().map(|v| json!(*v)).collect::<Vec<_>>())
            .collect::<Vec<_>>()),
        CalibrationValue::I32Matrix(mat) => json!(mat
            .iter()
            .map(|row| row.iter().map(|v| json!(*v)).collect::<Vec<_>>())
            .collect::<Vec<_>>()),
        CalibrationValue::I64Matrix(mat) => json!(mat
            .iter()
            .map(|row| row.iter().map(|v| json!(*v)).collect::<Vec<_>>())
            .collect::<Vec<_>>()),
        CalibrationValue::F32Matrix(mat) => json!(mat
            .iter()
            .map(|row| {
                row.iter()
                    .map(|v| json!(format!("{:.6}", v)))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()),
        CalibrationValue::F64Matrix(mat) => json!(mat
            .iter()
            .map(|row| {
                row.iter()
                    .map(|v| json!(format!("{:.6}", v)))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()),
        CalibrationValue::BoolMatrix(mat) => json!(mat
            .iter()
            .map(|row| row.iter().map(|v| json!(*v)).collect::<Vec<_>>())
            .collect::<Vec<_>>()),
        CalibrationValue::Ascii(s) => json!([s]),
    }
}
