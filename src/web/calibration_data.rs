use crate::a2l::AxisPointSource;
use crate::calibration::{CalibrationParam, CalibrationType, CalibrationValue};
use crate::hex::HexImage;
use crate::StorageOrder;

fn flat_to_matrix<T: Clone>(
    flat: &[T],
    rows: usize,
    cols: usize,
    order: StorageOrder,
) -> Vec<Vec<T>> {
    let mut matrix = Vec::with_capacity(rows);
    match order {
        StorageOrder::RowDir => {
            for r in 0..rows {
                matrix.push(flat[r * cols..(r + 1) * cols].to_vec());
            }
        }
        StorageOrder::ColumnDir => {
            for r in 0..rows {
                let mut row = Vec::with_capacity(cols);
                for c in 0..cols {
                    row.push(flat[c * rows + r].clone());
                }
                matrix.push(row);
            }
        }
    }
    matrix
}

pub(crate) fn decode_calibration_bytes(
    data: &[u8],
    param: &CalibrationParam,
) -> Result<CalibrationValue, String> {
    let value = match &param.data_type {
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
        CalibrationType::U8Matrix(rows, cols) => {
            let total = rows * cols;
            let flat: Vec<u8> = data.iter().take(total).copied().collect();
            CalibrationValue::U8Matrix(flat_to_matrix(&flat, *rows, *cols, param.storage_order))
        }
        CalibrationType::U16Matrix(rows, cols) => {
            let total = rows * cols;
            let flat: Vec<u16> = (0..total)
                .map(|i| u16::from_le_bytes([data[i * 2], data[i * 2 + 1]]))
                .collect();
            CalibrationValue::U16Matrix(flat_to_matrix(&flat, *rows, *cols, param.storage_order))
        }
        CalibrationType::U32Matrix(rows, cols) => {
            let total = rows * cols;
            let flat: Vec<u32> = (0..total)
                .map(|i| {
                    u32::from_le_bytes([
                        data[i * 4],
                        data[i * 4 + 1],
                        data[i * 4 + 2],
                        data[i * 4 + 3],
                    ])
                })
                .collect();
            CalibrationValue::U32Matrix(flat_to_matrix(&flat, *rows, *cols, param.storage_order))
        }
        CalibrationType::U64Matrix(rows, cols) => {
            let total = rows * cols;
            let flat: Vec<u64> = (0..total)
                .map(|i| {
                    u64::from_le_bytes([
                        data[i * 8],
                        data[i * 8 + 1],
                        data[i * 8 + 2],
                        data[i * 8 + 3],
                        data[i * 8 + 4],
                        data[i * 8 + 5],
                        data[i * 8 + 6],
                        data[i * 8 + 7],
                    ])
                })
                .collect();
            CalibrationValue::U64Matrix(flat_to_matrix(&flat, *rows, *cols, param.storage_order))
        }
        CalibrationType::I8Matrix(rows, cols) => {
            let total = rows * cols;
            let flat: Vec<i8> = data.iter().take(total).map(|&b| b as i8).collect();
            CalibrationValue::I8Matrix(flat_to_matrix(&flat, *rows, *cols, param.storage_order))
        }
        CalibrationType::I16Matrix(rows, cols) => {
            let total = rows * cols;
            let flat: Vec<i16> = (0..total)
                .map(|i| i16::from_le_bytes([data[i * 2], data[i * 2 + 1]]))
                .collect();
            CalibrationValue::I16Matrix(flat_to_matrix(&flat, *rows, *cols, param.storage_order))
        }
        CalibrationType::I32Matrix(rows, cols) => {
            let total = rows * cols;
            let flat: Vec<i32> = (0..total)
                .map(|i| {
                    i32::from_le_bytes([
                        data[i * 4],
                        data[i * 4 + 1],
                        data[i * 4 + 2],
                        data[i * 4 + 3],
                    ])
                })
                .collect();
            CalibrationValue::I32Matrix(flat_to_matrix(&flat, *rows, *cols, param.storage_order))
        }
        CalibrationType::I64Matrix(rows, cols) => {
            let total = rows * cols;
            let flat: Vec<i64> = (0..total)
                .map(|i| {
                    i64::from_le_bytes([
                        data[i * 8],
                        data[i * 8 + 1],
                        data[i * 8 + 2],
                        data[i * 8 + 3],
                        data[i * 8 + 4],
                        data[i * 8 + 5],
                        data[i * 8 + 6],
                        data[i * 8 + 7],
                    ])
                })
                .collect();
            CalibrationValue::I64Matrix(flat_to_matrix(&flat, *rows, *cols, param.storage_order))
        }
        CalibrationType::F32Matrix(rows, cols) => {
            let total = rows * cols;
            let flat: Vec<f32> = (0..total)
                .map(|i| {
                    f32::from_le_bytes([
                        data[i * 4],
                        data[i * 4 + 1],
                        data[i * 4 + 2],
                        data[i * 4 + 3],
                    ])
                })
                .collect();
            CalibrationValue::F32Matrix(flat_to_matrix(&flat, *rows, *cols, param.storage_order))
        }
        CalibrationType::F64Matrix(rows, cols) => {
            let total = rows * cols;
            let flat: Vec<f64> = (0..total)
                .map(|i| {
                    f64::from_le_bytes([
                        data[i * 8],
                        data[i * 8 + 1],
                        data[i * 8 + 2],
                        data[i * 8 + 3],
                        data[i * 8 + 4],
                        data[i * 8 + 5],
                        data[i * 8 + 6],
                        data[i * 8 + 7],
                    ])
                })
                .collect();
            CalibrationValue::F64Matrix(flat_to_matrix(&flat, *rows, *cols, param.storage_order))
        }
        CalibrationType::BoolMatrix(rows, cols) => {
            let total = rows * cols;
            let flat: Vec<bool> = data.iter().take(total).map(|&b| b != 0).collect();
            CalibrationValue::BoolMatrix(flat_to_matrix(&flat, *rows, *cols, param.storage_order))
        }
    };

    Ok(value)
}

pub(crate) fn read_calibration_from_hex(
    hex: &HexImage,
    param: &CalibrationParam,
) -> Result<CalibrationValue, String> {
    let size = param.data_type.size();
    if size == 0 {
        return Err(format!(
            "Invalid parameter size (0 bytes) for '{}'",
            param.name
        ));
    }
    let data = hex.extract_range(param.address, size as u32, 0xFF);
    decode_calibration_bytes(&data, param)
}

pub(crate) fn read_axis_values_from_hex(hex: &HexImage, param: &CalibrationParam) -> Vec<Vec<f64>> {
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
            let element_size = match data_type {
                crate::a2l::A2lDataType::U8
                | crate::a2l::A2lDataType::I8
                | crate::a2l::A2lDataType::Bool => 1,
                crate::a2l::A2lDataType::U16 | crate::a2l::A2lDataType::I16 => 2,
                crate::a2l::A2lDataType::U32
                | crate::a2l::A2lDataType::I32
                | crate::a2l::A2lDataType::F32 => 4,
                crate::a2l::A2lDataType::U64
                | crate::a2l::A2lDataType::I64
                | crate::a2l::A2lDataType::F64 => 8,
                _ => 4,
            };
            let raw = hex.extract_range(*address, (element_size * *num_points) as u32, 0xFF);
            let values = (0..*num_points)
                .map(|i| {
                    let start = i * element_size;
                    let slice = &raw[start..start + element_size];
                    match data_type {
                        crate::a2l::A2lDataType::U8 | crate::a2l::A2lDataType::U8Array(_) => {
                            slice[0] as f64
                        }
                        crate::a2l::A2lDataType::I8 | crate::a2l::A2lDataType::I8Array(_) => {
                            slice[0] as i8 as f64
                        }
                        crate::a2l::A2lDataType::Bool | crate::a2l::A2lDataType::BoolArray(_) => {
                            if slice[0] == 0 { 0.0 } else { 1.0 }
                        }
                        crate::a2l::A2lDataType::U16 | crate::a2l::A2lDataType::U16Array(_) => {
                            u16::from_le_bytes([slice[0], slice[1]]) as f64
                        }
                        crate::a2l::A2lDataType::I16 | crate::a2l::A2lDataType::I16Array(_) => {
                            i16::from_le_bytes([slice[0], slice[1]]) as f64
                        }
                        crate::a2l::A2lDataType::U32 | crate::a2l::A2lDataType::U32Array(_) => {
                            u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]) as f64
                        }
                        crate::a2l::A2lDataType::I32 | crate::a2l::A2lDataType::I32Array(_) => {
                            i32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]) as f64
                        }
                        crate::a2l::A2lDataType::F32 | crate::a2l::A2lDataType::F32Array(_) => {
                            f32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]) as f64
                        }
                        crate::a2l::A2lDataType::U64 | crate::a2l::A2lDataType::U64Array(_) => {
                            u64::from_le_bytes([
                                slice[0], slice[1], slice[2], slice[3], slice[4], slice[5],
                                slice[6], slice[7],
                            ]) as f64
                        }
                        crate::a2l::A2lDataType::I64 | crate::a2l::A2lDataType::I64Array(_) => {
                            i64::from_le_bytes([
                                slice[0], slice[1], slice[2], slice[3], slice[4], slice[5],
                                slice[6], slice[7],
                            ]) as f64
                        }
                        crate::a2l::A2lDataType::F64 | crate::a2l::A2lDataType::F64Array(_) => {
                            f64::from_le_bytes([
                                slice[0], slice[1], slice[2], slice[3], slice[4], slice[5],
                                slice[6], slice[7],
                            ])
                        }
                        _ => i as f64,
                    }
                })
                .collect();
            result.push(values);
        }
    }
    result
}

pub(crate) fn format_raw_hex(bytes: &[u8]) -> String {
    bytes.iter()
        .map(|byte| format!("{:02X}", byte))
        .collect::<Vec<_>>()
        .join(" ")
}

#[allow(dead_code)]
pub(crate) fn calibration_value_to_json(value: &CalibrationValue, is_1d: bool) -> serde_json::Value {
    match value {
        CalibrationValue::U8(v) => serde_json::json!([*v]),
        CalibrationValue::U16(v) => serde_json::json!([*v]),
        CalibrationValue::U32(v) => serde_json::json!([*v]),
        CalibrationValue::U64(v) => serde_json::json!([*v]),
        CalibrationValue::I8(v) => serde_json::json!([*v]),
        CalibrationValue::I16(v) => serde_json::json!([*v]),
        CalibrationValue::I32(v) => serde_json::json!([*v]),
        CalibrationValue::I64(v) => serde_json::json!([*v]),
        CalibrationValue::F32(v) => serde_json::json!([format!("{:.6}", v)]),
        CalibrationValue::F64(v) => serde_json::json!([format!("{:.6}", v)]),
        CalibrationValue::Bool(v) => serde_json::json!([*v]),
        CalibrationValue::U8Array(arr) => {
            let row = arr.iter().map(|v| serde_json::json!(*v)).collect::<Vec<_>>();
            if is_1d {
                serde_json::json!(row)
            } else {
                serde_json::json!([row])
            }
        }
        CalibrationValue::U16Array(arr) => {
            let row = arr.iter().map(|v| serde_json::json!(*v)).collect::<Vec<_>>();
            if is_1d {
                serde_json::json!(row)
            } else {
                serde_json::json!([row])
            }
        }
        CalibrationValue::U32Array(arr) => {
            let row = arr.iter().map(|v| serde_json::json!(*v)).collect::<Vec<_>>();
            if is_1d {
                serde_json::json!(row)
            } else {
                serde_json::json!([row])
            }
        }
        CalibrationValue::U64Array(arr) => {
            let row = arr.iter().map(|v| serde_json::json!(*v)).collect::<Vec<_>>();
            if is_1d {
                serde_json::json!(row)
            } else {
                serde_json::json!([row])
            }
        }
        CalibrationValue::I8Array(arr) => {
            let row = arr.iter().map(|v| serde_json::json!(*v)).collect::<Vec<_>>();
            if is_1d {
                serde_json::json!(row)
            } else {
                serde_json::json!([row])
            }
        }
        CalibrationValue::I16Array(arr) => {
            let row = arr.iter().map(|v| serde_json::json!(*v)).collect::<Vec<_>>();
            if is_1d {
                serde_json::json!(row)
            } else {
                serde_json::json!([row])
            }
        }
        CalibrationValue::I32Array(arr) => {
            let row = arr.iter().map(|v| serde_json::json!(*v)).collect::<Vec<_>>();
            if is_1d {
                serde_json::json!(row)
            } else {
                serde_json::json!([row])
            }
        }
        CalibrationValue::I64Array(arr) => {
            let row = arr.iter().map(|v| serde_json::json!(*v)).collect::<Vec<_>>();
            if is_1d {
                serde_json::json!(row)
            } else {
                serde_json::json!([row])
            }
        }
        CalibrationValue::F32Array(arr) => {
            let row = arr
                .iter()
                .map(|v| serde_json::json!(format!("{:.6}", v)))
                .collect::<Vec<_>>();
            if is_1d {
                serde_json::json!(row)
            } else {
                serde_json::json!([row])
            }
        }
        CalibrationValue::F64Array(arr) => {
            let row = arr
                .iter()
                .map(|v| serde_json::json!(format!("{:.6}", v)))
                .collect::<Vec<_>>();
            if is_1d {
                serde_json::json!(row)
            } else {
                serde_json::json!([row])
            }
        }
        CalibrationValue::BoolArray(arr) => {
            let row = arr.iter().map(|v| serde_json::json!(*v)).collect::<Vec<_>>();
            if is_1d {
                serde_json::json!(row)
            } else {
                serde_json::json!([row])
            }
        }
        CalibrationValue::U8Matrix(mat) => serde_json::json!(mat
            .iter()
            .map(|row| row.iter().map(|v| serde_json::json!(*v)).collect::<Vec<_>>())
            .collect::<Vec<_>>()),
        CalibrationValue::U16Matrix(mat) => serde_json::json!(mat
            .iter()
            .map(|row| row.iter().map(|v| serde_json::json!(*v)).collect::<Vec<_>>())
            .collect::<Vec<_>>()),
        CalibrationValue::U32Matrix(mat) => serde_json::json!(mat
            .iter()
            .map(|row| row.iter().map(|v| serde_json::json!(*v)).collect::<Vec<_>>())
            .collect::<Vec<_>>()),
        CalibrationValue::U64Matrix(mat) => serde_json::json!(mat
            .iter()
            .map(|row| row.iter().map(|v| serde_json::json!(*v)).collect::<Vec<_>>())
            .collect::<Vec<_>>()),
        CalibrationValue::I8Matrix(mat) => serde_json::json!(mat
            .iter()
            .map(|row| row.iter().map(|v| serde_json::json!(*v)).collect::<Vec<_>>())
            .collect::<Vec<_>>()),
        CalibrationValue::I16Matrix(mat) => serde_json::json!(mat
            .iter()
            .map(|row| row.iter().map(|v| serde_json::json!(*v)).collect::<Vec<_>>())
            .collect::<Vec<_>>()),
        CalibrationValue::I32Matrix(mat) => serde_json::json!(mat
            .iter()
            .map(|row| row.iter().map(|v| serde_json::json!(*v)).collect::<Vec<_>>())
            .collect::<Vec<_>>()),
        CalibrationValue::I64Matrix(mat) => serde_json::json!(mat
            .iter()
            .map(|row| row.iter().map(|v| serde_json::json!(*v)).collect::<Vec<_>>())
            .collect::<Vec<_>>()),
        CalibrationValue::F32Matrix(mat) => serde_json::json!(mat
            .iter()
            .map(|row| {
                row.iter()
                    .map(|v| serde_json::json!(format!("{:.6}", v)))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()),
        CalibrationValue::F64Matrix(mat) => serde_json::json!(mat
            .iter()
            .map(|row| {
                row.iter()
                    .map(|v| serde_json::json!(format!("{:.6}", v)))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()),
        CalibrationValue::BoolMatrix(mat) => serde_json::json!(mat
            .iter()
            .map(|row| row.iter().map(|v| serde_json::json!(*v)).collect::<Vec<_>>())
            .collect::<Vec<_>>()),
    }
}
