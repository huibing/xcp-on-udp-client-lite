//! Calibration Parameter Management

use crate::a2l_reader::{A2lCharacteristic, A2lDataType, A2lReader};
use crate::client::XcpClient;
use crate::daq::CompuMethod;
use crate::protocol::Mta;
use std::path::Path;

/// Calibration parameter types
#[derive(Debug, Clone)]
pub enum CalibrationType {
    // Scalar types
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
    Bool,
    // Array types
    U8Array(usize),
    U16Array(usize),
    U32Array(usize),
    U64Array(usize),
    I8Array(usize),
    I16Array(usize),
    I32Array(usize),
    I64Array(usize),
    F32Array(usize),
    F64Array(usize),
    BoolArray(usize),
    // Matrix types (row-major order)
    U8Matrix(usize, usize),
    U16Matrix(usize, usize),
    U32Matrix(usize, usize),
    U64Matrix(usize, usize),
    I8Matrix(usize, usize),
    I16Matrix(usize, usize),
    I32Matrix(usize, usize),
    I64Matrix(usize, usize),
    F32Matrix(usize, usize),
    F64Matrix(usize, usize),
    BoolMatrix(usize, usize),
}

impl CalibrationType {
    /// Get the size in bytes
    pub fn size(&self) -> usize {
        match self {
            // Scalar types
            CalibrationType::U8 | CalibrationType::I8 | CalibrationType::Bool => 1,
            CalibrationType::U16 | CalibrationType::I16 => 2,
            CalibrationType::U32 | CalibrationType::I32 | CalibrationType::F32 => 4,
            CalibrationType::U64 | CalibrationType::I64 | CalibrationType::F64 => 8,
            // Array types
            CalibrationType::U8Array(n) | CalibrationType::I8Array(n) | CalibrationType::BoolArray(n) => *n,
            CalibrationType::U16Array(n) | CalibrationType::I16Array(n) => *n * 2,
            CalibrationType::U32Array(n) | CalibrationType::I32Array(n) | CalibrationType::F32Array(n) => *n * 4,
            CalibrationType::U64Array(n) | CalibrationType::I64Array(n) | CalibrationType::F64Array(n) => *n * 8,
            // Matrix types
            CalibrationType::U8Matrix(r, c) | CalibrationType::I8Matrix(r, c) | CalibrationType::BoolMatrix(r, c) => r * c,
            CalibrationType::U16Matrix(r, c) | CalibrationType::I16Matrix(r, c) => r * c * 2,
            CalibrationType::U32Matrix(r, c) | CalibrationType::I32Matrix(r, c) | CalibrationType::F32Matrix(r, c) => r * c * 4,
            CalibrationType::U64Matrix(r, c) | CalibrationType::I64Matrix(r, c) | CalibrationType::F64Matrix(r, c) => r * c * 8,
        }
    }

    /// Get type name string
    pub fn type_name(&self) -> String {
        match self {
            // Scalar types
            CalibrationType::U8 => "U8".to_string(),
            CalibrationType::U16 => "U16".to_string(),
            CalibrationType::U32 => "U32".to_string(),
            CalibrationType::U64 => "U64".to_string(),
            CalibrationType::I8 => "I8".to_string(),
            CalibrationType::I16 => "I16".to_string(),
            CalibrationType::I32 => "I32".to_string(),
            CalibrationType::I64 => "I64".to_string(),
            CalibrationType::F32 => "F32".to_string(),
            CalibrationType::F64 => "F64".to_string(),
            CalibrationType::Bool => "BOOL".to_string(),
            // Array types
            CalibrationType::U8Array(n) => format!("U8[{}]", n),
            CalibrationType::U16Array(n) => format!("U16[{}]", n),
            CalibrationType::U32Array(n) => format!("U32[{}]", n),
            CalibrationType::U64Array(n) => format!("U64[{}]", n),
            CalibrationType::I8Array(n) => format!("I8[{}]", n),
            CalibrationType::I16Array(n) => format!("I16[{}]", n),
            CalibrationType::I32Array(n) => format!("I32[{}]", n),
            CalibrationType::I64Array(n) => format!("I64[{}]", n),
            CalibrationType::F32Array(n) => format!("F32[{}]", n),
            CalibrationType::F64Array(n) => format!("F64[{}]", n),
            CalibrationType::BoolArray(n) => format!("BOOL[{}]", n),
            // Matrix types
            CalibrationType::U8Matrix(r, c) => format!("U8[{}][{}]", r, c),
            CalibrationType::U16Matrix(r, c) => format!("U16[{}][{}]", r, c),
            CalibrationType::U32Matrix(r, c) => format!("U32[{}][{}]", r, c),
            CalibrationType::U64Matrix(r, c) => format!("U64[{}][{}]", r, c),
            CalibrationType::I8Matrix(r, c) => format!("I8[{}][{}]", r, c),
            CalibrationType::I16Matrix(r, c) => format!("I16[{}][{}]", r, c),
            CalibrationType::I32Matrix(r, c) => format!("I32[{}][{}]", r, c),
            CalibrationType::I64Matrix(r, c) => format!("I64[{}][{}]", r, c),
            CalibrationType::F32Matrix(r, c) => format!("F32[{}][{}]", r, c),
            CalibrationType::F64Matrix(r, c) => format!("F64[{}][{}]", r, c),
            CalibrationType::BoolMatrix(r, c) => format!("BOOL[{}][{}]", r, c),
        }
    }
}

/// Calibration parameter value
#[derive(Debug, Clone)]
pub enum CalibrationValue {
    // Scalar types
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    Bool(bool),
    // Array types
    U8Array(Vec<u8>),
    U16Array(Vec<u16>),
    U32Array(Vec<u32>),
    U64Array(Vec<u64>),
    I8Array(Vec<i8>),
    I16Array(Vec<i16>),
    I32Array(Vec<i32>),
    I64Array(Vec<i64>),
    F32Array(Vec<f32>),
    F64Array(Vec<f64>),
    BoolArray(Vec<bool>),
    // Matrix types
    U8Matrix(Vec<Vec<u8>>),
    U16Matrix(Vec<Vec<u16>>),
    U32Matrix(Vec<Vec<u32>>),
    U64Matrix(Vec<Vec<u64>>),
    I8Matrix(Vec<Vec<i8>>),
    I16Matrix(Vec<Vec<i16>>),
    I32Matrix(Vec<Vec<i32>>),
    I64Matrix(Vec<Vec<i64>>),
    F32Matrix(Vec<Vec<f32>>),
    F64Matrix(Vec<Vec<f64>>),
    BoolMatrix(Vec<Vec<bool>>),
}

impl CalibrationValue {
    /// Format value for display
    pub fn format(&self) -> String {
        match self {
            // Scalar types
            CalibrationValue::U8(v) => v.to_string(),
            CalibrationValue::U16(v) => v.to_string(),
            CalibrationValue::U32(v) => v.to_string(),
            CalibrationValue::U64(v) => v.to_string(),
            CalibrationValue::I8(v) => v.to_string(),
            CalibrationValue::I16(v) => v.to_string(),
            CalibrationValue::I32(v) => v.to_string(),
            CalibrationValue::I64(v) => v.to_string(),
            CalibrationValue::F32(v) => format!("{:.6}", v),
            CalibrationValue::F64(v) => format!("{:.6}", v),
            CalibrationValue::Bool(v) => v.to_string(),
            // Array types
            CalibrationValue::U8Array(arr) => format!("{:?}", arr),
            CalibrationValue::U16Array(arr) => format!("{:?}", arr),
            CalibrationValue::U32Array(arr) => format!("{:?}", arr),
            CalibrationValue::U64Array(arr) => format!("{:?}", arr),
            CalibrationValue::I8Array(arr) => format!("{:?}", arr),
            CalibrationValue::I16Array(arr) => format!("{:?}", arr),
            CalibrationValue::I32Array(arr) => format!("{:?}", arr),
            CalibrationValue::I64Array(arr) => format!("{:?}", arr),
            CalibrationValue::F32Array(arr) => format!("{:?}", arr),
            CalibrationValue::F64Array(arr) => format!("{:?}", arr),
            CalibrationValue::BoolArray(arr) => format!("{:?}", arr),
            // Matrix types
            CalibrationValue::U8Matrix(mat) => format_matrix(mat),
            CalibrationValue::U16Matrix(mat) => format_matrix(mat),
            CalibrationValue::U32Matrix(mat) => format_matrix(mat),
            CalibrationValue::U64Matrix(mat) => format_matrix(mat),
            CalibrationValue::I8Matrix(mat) => format_matrix(mat),
            CalibrationValue::I16Matrix(mat) => format_matrix(mat),
            CalibrationValue::I32Matrix(mat) => format_matrix(mat),
            CalibrationValue::I64Matrix(mat) => format_matrix(mat),
            CalibrationValue::F32Matrix(mat) => format_matrix(mat),
            CalibrationValue::F64Matrix(mat) => format_matrix(mat),
            CalibrationValue::BoolMatrix(mat) => format_matrix(mat),
        }
    }
}

/// Format a matrix for display
fn format_matrix<T: std::fmt::Debug>(mat: &[Vec<T>]) -> String {
    let rows: Vec<String> = mat.iter().map(|row| format!("{:?}", row)).collect();
    format!("[\n  {}]", rows.join(",\n  "))
}

/// Calibration parameter definition
#[derive(Debug, Clone)]
pub struct CalibrationParam {
    pub name: String,
    pub description: String,
    pub address: u32,
    pub address_ext: u8,
    pub data_type: CalibrationType,
    pub unit: Option<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// COMPU_METHOD for converting between raw and physical values
    pub compu_method: CompuMethod,
}

impl CalibrationParam {
    /// Create a new calibration parameter
    pub fn new(name: &str, address: u32, address_ext: u8, data_type: CalibrationType) -> Self {
        Self {
            name: name.to_string(),
            description: String::new(),
            address,
            address_ext,
            data_type,
            unit: None,
            min: None,
            max: None,
            compu_method: CompuMethod::Identical,
        }
    }

    /// Set COMPU_METHOD
    pub fn with_compu_method(mut self, method: CompuMethod) -> Self {
        self.compu_method = method;
        self
    }

    /// Set description
    pub fn with_description(mut self, desc: &str) -> Self {
        self.description = desc.to_string();
        self
    }

    /// Set unit
    pub fn with_unit(mut self, unit: &str) -> Self {
        self.unit = Some(unit.to_string());
        self
    }

    /// Set range
    pub fn with_range(mut self, min: f64, max: f64) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self
    }

    /// Get MTA for this parameter
    pub fn mta(&self) -> Mta {
        Mta::new(self.address, self.address_ext)
    }
}

/// Calibration manager
pub struct CalibrationManager {
    params: Vec<CalibrationParam>,
}

impl CalibrationManager {
    /// Create calibration manager with empty parameters list
    /// Parameters must be loaded from A2L file
    pub fn new() -> Self {
        Self {
            params: Vec::new(),
        }
    }

    /// Create calibration manager from an A2L file
    pub fn from_a2l<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let reader = A2lReader::load(path)?;
        let characteristics = reader.get_characteristics();

        let params = characteristics
            .into_iter()
            .filter_map(Self::convert_characteristic)
            .collect();

        Ok(Self { params })
    }

    /// Convert A2L characteristic to CalibrationParam
    fn convert_characteristic(ch: A2lCharacteristic) -> Option<CalibrationParam> {
        let data_type = Self::convert_data_type(&ch.data_type)?;

        let mut param = CalibrationParam::new(
            &ch.name,
            ch.address,
            ch.address_ext,
            data_type,
        )
        .with_description(&ch.description)
        .with_range(ch.min, ch.max);

        if let Some(unit) = ch.unit {
            param = param.with_unit(&unit);
        }

        Some(param)
    }

    /// Convert A2L data type to CalibrationType
    fn convert_data_type(a2l_type: &A2lDataType) -> Option<CalibrationType> {
        match a2l_type {
            // Scalar types
            A2lDataType::U8 => Some(CalibrationType::U8),
            A2lDataType::U16 => Some(CalibrationType::U16),
            A2lDataType::U32 => Some(CalibrationType::U32),
            A2lDataType::U64 => Some(CalibrationType::U64),
            A2lDataType::I8 => Some(CalibrationType::I8),
            A2lDataType::I16 => Some(CalibrationType::I16),
            A2lDataType::I32 => Some(CalibrationType::I32),
            A2lDataType::I64 => Some(CalibrationType::I64),
            A2lDataType::F32 => Some(CalibrationType::F32),
            A2lDataType::F64 => Some(CalibrationType::F64),
            A2lDataType::Bool => Some(CalibrationType::Bool),
            // Array types
            A2lDataType::U8Array(n) => Some(CalibrationType::U8Array(*n)),
            A2lDataType::U16Array(n) => Some(CalibrationType::U16Array(*n)),
            A2lDataType::U32Array(n) => Some(CalibrationType::U32Array(*n)),
            A2lDataType::U64Array(n) => Some(CalibrationType::U64Array(*n)),
            A2lDataType::I8Array(n) => Some(CalibrationType::I8Array(*n)),
            A2lDataType::I16Array(n) => Some(CalibrationType::I16Array(*n)),
            A2lDataType::I32Array(n) => Some(CalibrationType::I32Array(*n)),
            A2lDataType::I64Array(n) => Some(CalibrationType::I64Array(*n)),
            A2lDataType::F32Array(n) => Some(CalibrationType::F32Array(*n)),
            A2lDataType::F64Array(n) => Some(CalibrationType::F64Array(*n)),
            A2lDataType::BoolArray(n) => Some(CalibrationType::BoolArray(*n)),
            // Matrix types
            A2lDataType::U8Matrix(rows, cols) => Some(CalibrationType::U8Matrix(*rows, *cols)),
            A2lDataType::U16Matrix(rows, cols) => Some(CalibrationType::U16Matrix(*rows, *cols)),
            A2lDataType::U32Matrix(rows, cols) => Some(CalibrationType::U32Matrix(*rows, *cols)),
            A2lDataType::U64Matrix(rows, cols) => Some(CalibrationType::U64Matrix(*rows, *cols)),
            A2lDataType::I8Matrix(rows, cols) => Some(CalibrationType::I8Matrix(*rows, *cols)),
            A2lDataType::I16Matrix(rows, cols) => Some(CalibrationType::I16Matrix(*rows, *cols)),
            A2lDataType::I32Matrix(rows, cols) => Some(CalibrationType::I32Matrix(*rows, *cols)),
            A2lDataType::I64Matrix(rows, cols) => Some(CalibrationType::I64Matrix(*rows, *cols)),
            A2lDataType::F32Matrix(rows, cols) => Some(CalibrationType::F32Matrix(*rows, *cols)),
            A2lDataType::F64Matrix(rows, cols) => Some(CalibrationType::F64Matrix(*rows, *cols)),
            A2lDataType::BoolMatrix(rows, cols) => Some(CalibrationType::BoolMatrix(*rows, *cols)),
        }
    }

    /// Get all parameters
    pub fn params(&self) -> &[CalibrationParam] {
        &self.params
    }

    /// Get parameter by name
    pub fn get_param(&self, name: &str) -> Option<&CalibrationParam> {
        self.params.iter().find(|p| p.name == name)
    }

    /// Add a parameter
    pub fn add_param(&mut self, param: CalibrationParam) {
        self.params.push(param);
    }

    /// Clear all parameters
    pub fn clear_params(&mut self) {
        self.params.clear();
    }

    /// Read parameter value from XCP slave
    pub async fn read_value(&self, client: &XcpClient, param: &CalibrationParam) -> Result<CalibrationValue, String> {
        let mta = param.mta();
        let size = param.data_type.size();

        // Read data (handle large reads)
        let data = if size <= 255 {
            client.read_memory(mta, size as u8).await.map_err(|e| e.to_string())?
        } else {
            client.read_u8_array(mta, size).await.map_err(|e| e.to_string())?
        };

        // Parse based on type
        let value = match &param.data_type {
            CalibrationType::U8 => CalibrationValue::U8(data.first().copied().unwrap_or(0)),
            CalibrationType::U16 => {
                CalibrationValue::U16(u16::from_le_bytes([data[0], data[1]]))
            }
            CalibrationType::U32 => {
                CalibrationValue::U32(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
            }
            CalibrationType::U64 => {
                CalibrationValue::U64(u64::from_le_bytes([
                    data[0], data[1], data[2], data[3],
                    data[4], data[5], data[6], data[7],
                ]))
            }
            CalibrationType::I8 => CalibrationValue::I8(data[0] as i8),
            CalibrationType::I16 => {
                CalibrationValue::I16(i16::from_le_bytes([data[0], data[1]]))
            }
            CalibrationType::I32 => {
                CalibrationValue::I32(i32::from_le_bytes([data[0], data[1], data[2], data[3]]))
            }
            CalibrationType::I64 => {
                CalibrationValue::I64(i64::from_le_bytes([
                    data[0], data[1], data[2], data[3],
                    data[4], data[5], data[6], data[7],
                ]))
            }
            CalibrationType::F32 => {
                CalibrationValue::F32(f32::from_le_bytes([data[0], data[1], data[2], data[3]]))
            }
            CalibrationType::F64 => {
                CalibrationValue::F64(f64::from_le_bytes([
                    data[0], data[1], data[2], data[3],
                    data[4], data[5], data[6], data[7],
                ]))
            }
            CalibrationType::Bool => CalibrationValue::Bool(data.first().copied().unwrap_or(0) != 0),
            // Array types
            CalibrationType::U8Array(n) => {
                CalibrationValue::U8Array(data.iter().take(*n).copied().collect())
            }
            CalibrationType::U16Array(n) => {
                let arr: Vec<u16> = (0..*n)
                    .map(|i| u16::from_le_bytes([data[i * 2], data[i * 2 + 1]]))
                    .collect();
                CalibrationValue::U16Array(arr)
            }
            CalibrationType::U32Array(n) => {
                let arr: Vec<u32> = (0..*n)
                    .map(|i| {
                        u32::from_le_bytes([
                            data[i * 4],
                            data[i * 4 + 1],
                            data[i * 4 + 2],
                            data[i * 4 + 3],
                        ])
                    })
                    .collect();
                CalibrationValue::U32Array(arr)
            }
            CalibrationType::U64Array(n) => {
                let arr: Vec<u64> = (0..*n)
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
                CalibrationValue::U64Array(arr)
            }
            CalibrationType::I8Array(n) => {
                let arr: Vec<i8> = data.iter().take(*n).map(|&b| b as i8).collect();
                CalibrationValue::I8Array(arr)
            }
            CalibrationType::I16Array(n) => {
                let arr: Vec<i16> = (0..*n)
                    .map(|i| i16::from_le_bytes([data[i * 2], data[i * 2 + 1]]))
                    .collect();
                CalibrationValue::I16Array(arr)
            }
            CalibrationType::I32Array(n) => {
                let arr: Vec<i32> = (0..*n)
                    .map(|i| {
                        i32::from_le_bytes([
                            data[i * 4],
                            data[i * 4 + 1],
                            data[i * 4 + 2],
                            data[i * 4 + 3],
                        ])
                    })
                    .collect();
                CalibrationValue::I32Array(arr)
            }
            CalibrationType::I64Array(n) => {
                let arr: Vec<i64> = (0..*n)
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
                CalibrationValue::I64Array(arr)
            }
            CalibrationType::F32Array(n) => {
                let arr: Vec<f32> = (0..*n)
                    .map(|i| {
                        f32::from_le_bytes([
                            data[i * 4],
                            data[i * 4 + 1],
                            data[i * 4 + 2],
                            data[i * 4 + 3],
                        ])
                    })
                    .collect();
                CalibrationValue::F32Array(arr)
            }
            CalibrationType::F64Array(n) => {
                let arr: Vec<f64> = (0..*n)
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
                CalibrationValue::F64Array(arr)
            }
            CalibrationType::BoolArray(n) => {
                let arr: Vec<bool> = data.iter().take(*n).map(|&b| b != 0).collect();
                CalibrationValue::BoolArray(arr)
            }
            // Matrix types
            CalibrationType::U8Matrix(rows, cols) => {
                let total = rows * cols;
                let flat: Vec<u8> = data.iter().take(total).copied().collect();
                let mut matrix = Vec::with_capacity(*rows);
                for r in 0..*rows {
                    let row: Vec<u8> = flat[r * cols..(r + 1) * cols].to_vec();
                    matrix.push(row);
                }
                CalibrationValue::U8Matrix(matrix)
            }
            CalibrationType::U16Matrix(rows, cols) => {
                let total = rows * cols;
                let flat: Vec<u16> = (0..total)
                    .map(|i| u16::from_le_bytes([data[i * 2], data[i * 2 + 1]]))
                    .collect();
                let mut matrix = Vec::with_capacity(*rows);
                for r in 0..*rows {
                    let row: Vec<u16> = flat[r * cols..(r + 1) * cols].to_vec();
                    matrix.push(row);
                }
                CalibrationValue::U16Matrix(matrix)
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
                let mut matrix = Vec::with_capacity(*rows);
                for r in 0..*rows {
                    let row: Vec<u32> = flat[r * cols..(r + 1) * cols].to_vec();
                    matrix.push(row);
                }
                CalibrationValue::U32Matrix(matrix)
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
                let mut matrix = Vec::with_capacity(*rows);
                for r in 0..*rows {
                    let row: Vec<u64> = flat[r * cols..(r + 1) * cols].to_vec();
                    matrix.push(row);
                }
                CalibrationValue::U64Matrix(matrix)
            }
            CalibrationType::I8Matrix(rows, cols) => {
                let total = rows * cols;
                let flat: Vec<i8> = data.iter().take(total).map(|&b| b as i8).collect();
                let mut matrix = Vec::with_capacity(*rows);
                for r in 0..*rows {
                    let row: Vec<i8> = flat[r * cols..(r + 1) * cols].to_vec();
                    matrix.push(row);
                }
                CalibrationValue::I8Matrix(matrix)
            }
            CalibrationType::I16Matrix(rows, cols) => {
                let total = rows * cols;
                let flat: Vec<i16> = (0..total)
                    .map(|i| i16::from_le_bytes([data[i * 2], data[i * 2 + 1]]))
                    .collect();
                let mut matrix = Vec::with_capacity(*rows);
                for r in 0..*rows {
                    let row: Vec<i16> = flat[r * cols..(r + 1) * cols].to_vec();
                    matrix.push(row);
                }
                CalibrationValue::I16Matrix(matrix)
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
                let mut matrix = Vec::with_capacity(*rows);
                for r in 0..*rows {
                    let row: Vec<i32> = flat[r * cols..(r + 1) * cols].to_vec();
                    matrix.push(row);
                }
                CalibrationValue::I32Matrix(matrix)
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
                let mut matrix = Vec::with_capacity(*rows);
                for r in 0..*rows {
                    let row: Vec<i64> = flat[r * cols..(r + 1) * cols].to_vec();
                    matrix.push(row);
                }
                CalibrationValue::I64Matrix(matrix)
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
                let mut matrix = Vec::with_capacity(*rows);
                for r in 0..*rows {
                    let row: Vec<f32> = flat[r * cols..(r + 1) * cols].to_vec();
                    matrix.push(row);
                }
                CalibrationValue::F32Matrix(matrix)
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
                let mut matrix = Vec::with_capacity(*rows);
                for r in 0..*rows {
                    let row: Vec<f64> = flat[r * cols..(r + 1) * cols].to_vec();
                    matrix.push(row);
                }
                CalibrationValue::F64Matrix(matrix)
            }
            CalibrationType::BoolMatrix(rows, cols) => {
                let total = rows * cols;
                let flat: Vec<bool> = data.iter().take(total).map(|&b| b != 0).collect();
                let mut matrix = Vec::with_capacity(*rows);
                for r in 0..*rows {
                    let row: Vec<bool> = flat[r * cols..(r + 1) * cols].to_vec();
                    matrix.push(row);
                }
                CalibrationValue::BoolMatrix(matrix)
            }
        };

        Ok(value)
    }

    /// Write parameter value to XCP slave
    pub async fn write_value(
        &self,
        client: &XcpClient,
        param: &CalibrationParam,
        value: &CalibrationValue,
    ) -> Result<(), String> {
        let mta = param.mta();

        // Validate type match
        match (&param.data_type, value) {
            (CalibrationType::U8, CalibrationValue::U8(v)) => {
                client.write_u8(mta, *v).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::U16, CalibrationValue::U16(v)) => {
                client.write_memory(mta, &v.to_le_bytes()).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::U32, CalibrationValue::U32(v)) => {
                client.write_u32(mta, *v).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::U64, CalibrationValue::U64(v)) => {
                client.write_memory(mta, &v.to_le_bytes()).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::I8, CalibrationValue::I8(v)) => {
                client.write_u8(mta, *v as u8).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::I16, CalibrationValue::I16(v)) => {
                client.write_memory(mta, &v.to_le_bytes()).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::I32, CalibrationValue::I32(v)) => {
                client.write_memory(mta, &v.to_le_bytes()).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::I64, CalibrationValue::I64(v)) => {
                client.write_memory(mta, &v.to_le_bytes()).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::F32, CalibrationValue::F32(v)) => {
                client.write_memory(mta, &v.to_le_bytes()).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::F64, CalibrationValue::F64(v)) => {
                client.write_memory(mta, &v.to_le_bytes()).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::Bool, CalibrationValue::Bool(v)) => {
                client.write_u8(mta, if *v { 1 } else { 0 }).await.map_err(|e| e.to_string())?
            }
            // Array types
            (CalibrationType::U8Array(_), CalibrationValue::U8Array(v)) => {
                client.write_u8_array(mta, v).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::U16Array(_), CalibrationValue::U16Array(v)) => {
                let flat: Vec<u8> = v.iter().flat_map(|&x| x.to_le_bytes()).collect();
                client.write_memory(mta, &flat).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::U32Array(_), CalibrationValue::U32Array(v)) => {
                let flat: Vec<u8> = v.iter().flat_map(|&x| x.to_le_bytes()).collect();
                client.write_memory(mta, &flat).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::U64Array(_), CalibrationValue::U64Array(v)) => {
                let flat: Vec<u8> = v.iter().flat_map(|&x| x.to_le_bytes()).collect();
                client.write_memory(mta, &flat).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::I8Array(_), CalibrationValue::I8Array(v)) => {
                let flat: Vec<u8> = v.iter().map(|&x| x as u8).collect();
                client.write_u8_array(mta, &flat).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::I16Array(_), CalibrationValue::I16Array(v)) => {
                let flat: Vec<u8> = v.iter().flat_map(|&x| x.to_le_bytes()).collect();
                client.write_memory(mta, &flat).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::I32Array(_), CalibrationValue::I32Array(v)) => {
                let flat: Vec<u8> = v.iter().flat_map(|&x| x.to_le_bytes()).collect();
                client.write_memory(mta, &flat).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::I64Array(_), CalibrationValue::I64Array(v)) => {
                let flat: Vec<u8> = v.iter().flat_map(|&x| x.to_le_bytes()).collect();
                client.write_memory(mta, &flat).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::F32Array(_), CalibrationValue::F32Array(v)) => {
                let flat: Vec<u8> = v.iter().flat_map(|&x| x.to_le_bytes()).collect();
                client.write_memory(mta, &flat).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::F64Array(_), CalibrationValue::F64Array(v)) => {
                let flat: Vec<u8> = v.iter().flat_map(|&x| x.to_le_bytes()).collect();
                client.write_memory(mta, &flat).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::BoolArray(_), CalibrationValue::BoolArray(v)) => {
                let flat: Vec<u8> = v.iter().map(|&x| if x { 1 } else { 0 }).collect();
                client.write_u8_array(mta, &flat).await.map_err(|e| e.to_string())?
            }
            // Matrix types
            (CalibrationType::U8Matrix(_, _), CalibrationValue::U8Matrix(mat)) => {
                let flat: Vec<u8> = mat.iter().flatten().copied().collect();
                client.write_u8_array(mta, &flat).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::U16Matrix(_, _), CalibrationValue::U16Matrix(mat)) => {
                let flat: Vec<u8> = mat.iter().flatten().flat_map(|&x| x.to_le_bytes()).collect();
                client.write_memory(mta, &flat).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::U32Matrix(_, _), CalibrationValue::U32Matrix(mat)) => {
                let flat: Vec<u8> = mat.iter().flatten().flat_map(|&x| x.to_le_bytes()).collect();
                client.write_memory(mta, &flat).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::U64Matrix(_, _), CalibrationValue::U64Matrix(mat)) => {
                let flat: Vec<u8> = mat.iter().flatten().flat_map(|&x| x.to_le_bytes()).collect();
                client.write_memory(mta, &flat).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::I8Matrix(_, _), CalibrationValue::I8Matrix(mat)) => {
                let flat: Vec<u8> = mat.iter().flatten().map(|&x| x as u8).collect();
                client.write_u8_array(mta, &flat).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::I16Matrix(_, _), CalibrationValue::I16Matrix(mat)) => {
                let flat: Vec<u8> = mat.iter().flatten().flat_map(|&x| x.to_le_bytes()).collect();
                client.write_memory(mta, &flat).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::I32Matrix(_, _), CalibrationValue::I32Matrix(mat)) => {
                let flat: Vec<u8> = mat.iter().flatten().flat_map(|&x| x.to_le_bytes()).collect();
                client.write_memory(mta, &flat).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::I64Matrix(_, _), CalibrationValue::I64Matrix(mat)) => {
                let flat: Vec<u8> = mat.iter().flatten().flat_map(|&x| x.to_le_bytes()).collect();
                client.write_memory(mta, &flat).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::F32Matrix(_, _), CalibrationValue::F32Matrix(mat)) => {
                let flat: Vec<u8> = mat.iter().flatten().flat_map(|&x| x.to_le_bytes()).collect();
                client.write_memory(mta, &flat).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::F64Matrix(_, _), CalibrationValue::F64Matrix(mat)) => {
                let flat: Vec<u8> = mat.iter().flatten().flat_map(|&x| x.to_le_bytes()).collect();
                client.write_memory(mta, &flat).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::BoolMatrix(_, _), CalibrationValue::BoolMatrix(mat)) => {
                let flat: Vec<u8> = mat.iter().flatten().map(|&x| if x { 1 } else { 0 }).collect();
                client.write_u8_array(mta, &flat).await.map_err(|e| e.to_string())?
            }
            _ => return Err(format!("Type mismatch for parameter {}", param.name)),
        }

        Ok(())
    }

    /// Parse value from string
    pub fn parse_value(&self, param: &CalibrationParam, input: &str) -> Result<CalibrationValue, String> {
        match &param.data_type {
            CalibrationType::U8 => {
                let v: u8 = input.parse().map_err(|_| "Invalid U8 value")?;
                Ok(CalibrationValue::U8(v))
            }
            CalibrationType::U16 => {
                let v: u16 = input.parse().map_err(|_| "Invalid U16 value")?;
                Ok(CalibrationValue::U16(v))
            }
            CalibrationType::U32 => {
                let v: u32 = input.parse().map_err(|_| "Invalid U32 value")?;
                Ok(CalibrationValue::U32(v))
            }
            CalibrationType::U64 => {
                let v: u64 = input.parse().map_err(|_| "Invalid U64 value")?;
                Ok(CalibrationValue::U64(v))
            }
            CalibrationType::I8 => {
                let v: i8 = input.parse().map_err(|_| "Invalid I8 value")?;
                Ok(CalibrationValue::I8(v))
            }
            CalibrationType::I16 => {
                let v: i16 = input.parse().map_err(|_| "Invalid I16 value")?;
                Ok(CalibrationValue::I16(v))
            }
            CalibrationType::I32 => {
                let v: i32 = input.parse().map_err(|_| "Invalid I32 value")?;
                Ok(CalibrationValue::I32(v))
            }
            CalibrationType::I64 => {
                let v: i64 = input.parse().map_err(|_| "Invalid I64 value")?;
                Ok(CalibrationValue::I64(v))
            }
            CalibrationType::F32 => {
                let v: f32 = input.parse().map_err(|_| "Invalid F32 value")?;
                Ok(CalibrationValue::F32(v))
            }
            CalibrationType::F64 => {
                let v: f64 = input.parse().map_err(|_| "Invalid F64 value")?;
                Ok(CalibrationValue::F64(v))
            }
            CalibrationType::Bool => {
                let v: bool = input.parse().map_err(|_| "Invalid BOOL value (true/false)")?;
                Ok(CalibrationValue::Bool(v))
            }
            CalibrationType::U8Array(n) => {
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != *n {
                    return Err(format!("Expected {} values, got {}", n, values.len()));
                }
                let arr: Result<Vec<u8>, _> = values.iter()
                    .map(|s| s.trim().parse())
                    .collect();
                Ok(CalibrationValue::U8Array(arr.map_err(|_| "Invalid U8 value")?))
            }
            CalibrationType::U8Matrix(rows, cols) => {
                let total = rows * cols;
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != total {
                    return Err(format!("Expected {} values, got {}", total, values.len()));
                }
                let flat: Result<Vec<u8>, _> = values.iter()
                    .map(|s| s.trim().parse())
                    .collect();
                let flat = flat.map_err(|_| "Invalid U8 value")?;
                let mut matrix = Vec::with_capacity(*rows);
                for r in 0..*rows {
                    let row: Vec<u8> = flat[r * cols..(r + 1) * cols].to_vec();
                    matrix.push(row);
                }
                Ok(CalibrationValue::U8Matrix(matrix))
            }
            CalibrationType::U16Array(n) => {
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != *n {
                    return Err(format!("Expected {} values, got {}", n, values.len()));
                }
                let arr: Result<Vec<u16>, _> = values.iter()
                    .map(|s| s.trim().parse())
                    .collect();
                Ok(CalibrationValue::U16Array(arr.map_err(|_| "Invalid U16 value")?))
            }
            CalibrationType::U32Array(n) => {
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != *n {
                    return Err(format!("Expected {} values, got {}", n, values.len()));
                }
                let arr: Result<Vec<u32>, _> = values.iter()
                    .map(|s| s.trim().parse())
                    .collect();
                Ok(CalibrationValue::U32Array(arr.map_err(|_| "Invalid U32 value")?))
            }
            CalibrationType::U64Array(n) => {
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != *n {
                    return Err(format!("Expected {} values, got {}", n, values.len()));
                }
                let arr: Result<Vec<u64>, _> = values.iter()
                    .map(|s| s.trim().parse())
                    .collect();
                Ok(CalibrationValue::U64Array(arr.map_err(|_| "Invalid U64 value")?))
            }
            CalibrationType::I8Array(n) => {
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != *n {
                    return Err(format!("Expected {} values, got {}", n, values.len()));
                }
                let arr: Result<Vec<i8>, _> = values.iter()
                    .map(|s| s.trim().parse())
                    .collect();
                Ok(CalibrationValue::I8Array(arr.map_err(|_| "Invalid I8 value")?))
            }
            CalibrationType::I16Array(n) => {
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != *n {
                    return Err(format!("Expected {} values, got {}", n, values.len()));
                }
                let arr: Result<Vec<i16>, _> = values.iter()
                    .map(|s| s.trim().parse())
                    .collect();
                Ok(CalibrationValue::I16Array(arr.map_err(|_| "Invalid I16 value")?))
            }
            CalibrationType::I32Array(n) => {
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != *n {
                    return Err(format!("Expected {} values, got {}", n, values.len()));
                }
                let arr: Result<Vec<i32>, _> = values.iter()
                    .map(|s| s.trim().parse())
                    .collect();
                Ok(CalibrationValue::I32Array(arr.map_err(|_| "Invalid I32 value")?))
            }
            CalibrationType::I64Array(n) => {
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != *n {
                    return Err(format!("Expected {} values, got {}", n, values.len()));
                }
                let arr: Result<Vec<i64>, _> = values.iter()
                    .map(|s| s.trim().parse())
                    .collect();
                Ok(CalibrationValue::I64Array(arr.map_err(|_| "Invalid I64 value")?))
            }
            CalibrationType::F32Array(n) => {
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != *n {
                    return Err(format!("Expected {} values, got {}", n, values.len()));
                }
                let arr: Result<Vec<f32>, _> = values.iter()
                    .map(|s| s.trim().parse())
                    .collect();
                Ok(CalibrationValue::F32Array(arr.map_err(|_| "Invalid F32 value")?))
            }
            CalibrationType::F64Array(n) => {
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != *n {
                    return Err(format!("Expected {} values, got {}", n, values.len()));
                }
                let arr: Result<Vec<f64>, _> = values.iter()
                    .map(|s| s.trim().parse())
                    .collect();
                Ok(CalibrationValue::F64Array(arr.map_err(|_| "Invalid F64 value")?))
            }
            CalibrationType::BoolArray(n) => {
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != *n {
                    return Err(format!("Expected {} values, got {}", n, values.len()));
                }
                let arr: Result<Vec<bool>, _> = values.iter()
                    .map(|s| s.trim().parse())
                    .collect();
                Ok(CalibrationValue::BoolArray(arr.map_err(|_| "Invalid BOOL value (true/false)")?))
            }
            CalibrationType::U16Matrix(rows, cols) => {
                let total = rows * cols;
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != total {
                    return Err(format!("Expected {} values, got {}", total, values.len()));
                }
                let flat: Result<Vec<u16>, _> = values.iter()
                    .map(|s| s.trim().parse())
                    .collect();
                let flat = flat.map_err(|_| "Invalid U16 value")?;
                let mut matrix = Vec::with_capacity(*rows);
                for r in 0..*rows {
                    let row: Vec<u16> = flat[r * cols..(r + 1) * cols].to_vec();
                    matrix.push(row);
                }
                Ok(CalibrationValue::U16Matrix(matrix))
            }
            CalibrationType::U32Matrix(rows, cols) => {
                let total = rows * cols;
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != total {
                    return Err(format!("Expected {} values, got {}", total, values.len()));
                }
                let flat: Result<Vec<u32>, _> = values.iter()
                    .map(|s| s.trim().parse())
                    .collect();
                let flat = flat.map_err(|_| "Invalid U32 value")?;
                let mut matrix = Vec::with_capacity(*rows);
                for r in 0..*rows {
                    let row: Vec<u32> = flat[r * cols..(r + 1) * cols].to_vec();
                    matrix.push(row);
                }
                Ok(CalibrationValue::U32Matrix(matrix))
            }
            CalibrationType::U64Matrix(rows, cols) => {
                let total = rows * cols;
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != total {
                    return Err(format!("Expected {} values, got {}", total, values.len()));
                }
                let flat: Result<Vec<u64>, _> = values.iter()
                    .map(|s| s.trim().parse())
                    .collect();
                let flat = flat.map_err(|_| "Invalid U64 value")?;
                let mut matrix = Vec::with_capacity(*rows);
                for r in 0..*rows {
                    let row: Vec<u64> = flat[r * cols..(r + 1) * cols].to_vec();
                    matrix.push(row);
                }
                Ok(CalibrationValue::U64Matrix(matrix))
            }
            CalibrationType::I8Matrix(rows, cols) => {
                let total = rows * cols;
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != total {
                    return Err(format!("Expected {} values, got {}", total, values.len()));
                }
                let flat: Result<Vec<i8>, _> = values.iter()
                    .map(|s| s.trim().parse())
                    .collect();
                let flat = flat.map_err(|_| "Invalid I8 value")?;
                let mut matrix = Vec::with_capacity(*rows);
                for r in 0..*rows {
                    let row: Vec<i8> = flat[r * cols..(r + 1) * cols].to_vec();
                    matrix.push(row);
                }
                Ok(CalibrationValue::I8Matrix(matrix))
            }
            CalibrationType::I16Matrix(rows, cols) => {
                let total = rows * cols;
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != total {
                    return Err(format!("Expected {} values, got {}", total, values.len()));
                }
                let flat: Result<Vec<i16>, _> = values.iter()
                    .map(|s| s.trim().parse())
                    .collect();
                let flat = flat.map_err(|_| "Invalid I16 value")?;
                let mut matrix = Vec::with_capacity(*rows);
                for r in 0..*rows {
                    let row: Vec<i16> = flat[r * cols..(r + 1) * cols].to_vec();
                    matrix.push(row);
                }
                Ok(CalibrationValue::I16Matrix(matrix))
            }
            CalibrationType::I32Matrix(rows, cols) => {
                let total = rows * cols;
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != total {
                    return Err(format!("Expected {} values, got {}", total, values.len()));
                }
                let flat: Result<Vec<i32>, _> = values.iter()
                    .map(|s| s.trim().parse())
                    .collect();
                let flat = flat.map_err(|_| "Invalid I32 value")?;
                let mut matrix = Vec::with_capacity(*rows);
                for r in 0..*rows {
                    let row: Vec<i32> = flat[r * cols..(r + 1) * cols].to_vec();
                    matrix.push(row);
                }
                Ok(CalibrationValue::I32Matrix(matrix))
            }
            CalibrationType::I64Matrix(rows, cols) => {
                let total = rows * cols;
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != total {
                    return Err(format!("Expected {} values, got {}", total, values.len()));
                }
                let flat: Result<Vec<i64>, _> = values.iter()
                    .map(|s| s.trim().parse())
                    .collect();
                let flat = flat.map_err(|_| "Invalid I64 value")?;
                let mut matrix = Vec::with_capacity(*rows);
                for r in 0..*rows {
                    let row: Vec<i64> = flat[r * cols..(r + 1) * cols].to_vec();
                    matrix.push(row);
                }
                Ok(CalibrationValue::I64Matrix(matrix))
            }
            CalibrationType::F32Matrix(rows, cols) => {
                let total = rows * cols;
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != total {
                    return Err(format!("Expected {} values, got {}", total, values.len()));
                }
                let flat: Result<Vec<f32>, _> = values.iter()
                    .map(|s| s.trim().parse())
                    .collect();
                let flat = flat.map_err(|_| "Invalid F32 value")?;
                let mut matrix = Vec::with_capacity(*rows);
                for r in 0..*rows {
                    let row: Vec<f32> = flat[r * cols..(r + 1) * cols].to_vec();
                    matrix.push(row);
                }
                Ok(CalibrationValue::F32Matrix(matrix))
            }
            CalibrationType::F64Matrix(rows, cols) => {
                let total = rows * cols;
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != total {
                    return Err(format!("Expected {} values, got {}", total, values.len()));
                }
                let flat: Result<Vec<f64>, _> = values.iter()
                    .map(|s| s.trim().parse())
                    .collect();
                let flat = flat.map_err(|_| "Invalid F64 value")?;
                let mut matrix = Vec::with_capacity(*rows);
                for r in 0..*rows {
                    let row: Vec<f64> = flat[r * cols..(r + 1) * cols].to_vec();
                    matrix.push(row);
                }
                Ok(CalibrationValue::F64Matrix(matrix))
            }
            CalibrationType::BoolMatrix(rows, cols) => {
                let total = rows * cols;
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != total {
                    return Err(format!("Expected {} values, got {}", total, values.len()));
                }
                let flat: Result<Vec<bool>, _> = values.iter()
                    .map(|s| s.trim().parse())
                    .collect();
                let flat = flat.map_err(|_| "Invalid BOOL value (true/false)")?;
                let mut matrix = Vec::with_capacity(*rows);
                for r in 0..*rows {
                    let row: Vec<bool> = flat[r * cols..(r + 1) * cols].to_vec();
                    matrix.push(row);
                }
                Ok(CalibrationValue::BoolMatrix(matrix))
            }
        }
    }
}

impl Default for CalibrationManager {
    fn default() -> Self {
        Self::new()
    }
}
