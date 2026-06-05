//! Calibration Parameter Management

use crate::a2l::{
    A2lCharacteristic, A2lDataType, A2lMemorySegment, A2lReader, AxisInfo, CharacteristicType,
    CompuMethod, StorageOrder,
};
use crate::client::XcpClient;
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
    /// ASCII string of fixed byte length n (each char = 1 byte, null-padded)
    Ascii(usize),
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
            CalibrationType::U8Array(n)
            | CalibrationType::I8Array(n)
            | CalibrationType::BoolArray(n) => *n,
            CalibrationType::U16Array(n) | CalibrationType::I16Array(n) => *n * 2,
            CalibrationType::U32Array(n)
            | CalibrationType::I32Array(n)
            | CalibrationType::F32Array(n) => *n * 4,
            CalibrationType::U64Array(n)
            | CalibrationType::I64Array(n)
            | CalibrationType::F64Array(n) => *n * 8,
            // Matrix types
            CalibrationType::U8Matrix(r, c)
            | CalibrationType::I8Matrix(r, c)
            | CalibrationType::BoolMatrix(r, c) => r * c,
            CalibrationType::U16Matrix(r, c) | CalibrationType::I16Matrix(r, c) => r * c * 2,
            CalibrationType::U32Matrix(r, c)
            | CalibrationType::I32Matrix(r, c)
            | CalibrationType::F32Matrix(r, c) => r * c * 4,
            CalibrationType::U64Matrix(r, c)
            | CalibrationType::I64Matrix(r, c)
            | CalibrationType::F64Matrix(r, c) => r * c * 8,
            CalibrationType::Ascii(n) => *n,
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
            CalibrationType::Ascii(n) => format!("ASCII[{}]", n),
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
    /// ASCII string value
    Ascii(String),
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
            CalibrationValue::Ascii(s) => s.clone(),
        }
    }

    /// Format scalar value as a plain string for change-tracking comparison.
    /// For scalar types returns a simple numeric/bool string.
    /// For array/matrix types returns an empty string (not used for scalar comparison).
    pub fn format_scalar_str(&self) -> String {
        match self {
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
            CalibrationValue::Ascii(s) => s.clone(),
            _ => String::new(),
        }
    }
}

/// Format a matrix for display
fn format_matrix<T: std::fmt::Debug>(mat: &[Vec<T>]) -> String {
    let rows: Vec<String> = mat.iter().map(|row| format!("{:?}", row)).collect();
    format!("[\n  {}]", rows.join(",\n  "))
}

/// Convert a flat array to a 2D matrix based on storage order
///
/// For ROW_DIR: matrix[r][c] = flat[r * cols + c]
/// For COLUMN_DIR: matrix[r][c] = flat[c * rows + r]
fn flat_to_matrix<T: Clone>(
    flat: &[T],
    rows: usize,
    cols: usize,
    order: StorageOrder,
) -> Vec<Vec<T>> {
    let mut matrix = Vec::with_capacity(rows);
    match order {
        StorageOrder::RowDir => {
            // Row-major: data stored row by row
            for r in 0..rows {
                let row: Vec<T> = flat[r * cols..(r + 1) * cols].to_vec();
                matrix.push(row);
            }
        }
        StorageOrder::ColumnDir => {
            // Column-major: data stored column by column
            // matrix[r][c] = flat[c * rows + r]
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

/// Convert a 2D matrix to a flat array based on storage order
///
/// For ROW_DIR: flat[r * cols + c] = matrix[r][c]
/// For COLUMN_DIR: flat[c * rows + r] = matrix[r][c]
pub fn matrix_to_flat<T: Clone>(
    matrix: &[Vec<T>],
    rows: usize,
    cols: usize,
    order: StorageOrder,
) -> Vec<T> {
    let mut flat = Vec::with_capacity(rows * cols);
    match order {
        StorageOrder::RowDir => {
            // Row-major: store row by row
            for row in matrix {
                flat.extend(row.iter().cloned());
            }
        }
        StorageOrder::ColumnDir => {
            // Column-major: store column by column
            // flat[c * rows + r] = matrix[r][c]
            for c in 0..cols {
                for r in 0..rows {
                    flat.push(matrix[r][c].clone());
                }
            }
        }
    }
    flat
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
    /// Characteristic type (VALUE, CURVE, MAP, VAL_BLK)
    pub characteristic_type: CharacteristicType,
    /// Axis information for CURVE (1 axis) and MAP (2 axes) types
    /// Empty for VALUE and VAL_BLK types
    pub axis_info: Vec<AxisInfo>,
    /// Storage order for matrix/map data (ROW_DIR or COLUMN_DIR)
    /// Only relevant for MAP and VAL_BLK with 2D dimensions
    pub storage_order: StorageOrder,
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
            characteristic_type: CharacteristicType::Value,
            axis_info: Vec::new(),
            storage_order: StorageOrder::RowDir,
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

    /// Check if this is a CURVE type (1D lookup table)
    pub fn is_curve(&self) -> bool {
        self.characteristic_type == CharacteristicType::Curve
    }

    /// Check if this is a MAP type (2D lookup table)
    pub fn is_map(&self) -> bool {
        self.characteristic_type == CharacteristicType::Map
    }

    /// Check if this has axis information
    pub fn has_axis(&self) -> bool {
        !self.axis_info.is_empty()
    }

    /// Check if this parameter uses enum conversion (COMPU_VTAB)
    pub fn is_enum(&self) -> bool {
        self.compu_method.is_enum()
    }

    /// Get number of enum options
    pub fn enum_count(&self) -> usize {
        self.compu_method.enum_count()
    }

    /// Get enum options for dropdown display
    /// Returns None if not an enum type
    pub fn enum_options(&self) -> Option<&[crate::a2l::CompuVtabEntry]> {
        self.compu_method.enum_options()
    }

    /// Convert raw value to display string using COMPU_METHOD
    pub fn format_value(&self, raw: i64) -> String {
        self.compu_method.to_display(raw)
    }

    /// Convert display string to raw value using COMPU_METHOD
    pub fn parse_display(&self, display: &str) -> Result<i64, String> {
        self.compu_method.to_raw(display)
    }

    /// Convert raw bytes to physical value for DAQ display
    pub fn bytes_to_physical(&self, bytes: &[u8]) -> f64 {
        let is_float = matches!(
            self.data_type,
            CalibrationType::F32
                | CalibrationType::F64
                | CalibrationType::F32Array(_)
                | CalibrationType::F64Array(_)
                | CalibrationType::F32Matrix(_, _)
                | CalibrationType::F64Matrix(_, _)
        );
        let is_signed = matches!(
            self.data_type,
            CalibrationType::I8
                | CalibrationType::I16
                | CalibrationType::I32
                | CalibrationType::I64
        );
        self.compu_method
            .bytes_to_display(bytes, is_float, is_signed)
    }

    /// Serialize parameter metadata to JSON (without value).
    /// Used for API responses that need parameter info but not the current value.
    pub fn to_json(
        &self,
        device_id: &str,
        device_name: &str,
        idx: usize,
        connected: bool,
    ) -> serde_json::Value {
        let axis_info: Vec<serde_json::Value> =
            self.axis_info.iter().map(|axis| axis.to_json()).collect();
        let enum_options: Vec<serde_json::Value> = self
            .compu_method
            .enum_options()
            .map(|opts| {
                opts.iter()
                    .map(|opt| {
                        serde_json::json!({"raw": opt.raw_value, "text": opt.text_value})
                    })
                    .collect()
            })
            .unwrap_or_default();
        serde_json::json!({
            "device_id": device_id,
            "device_name": device_name,
            "idx": idx,
            "name": self.name,
            "description": self.description,
            "type": self.data_type.type_name(),
            "address": self.address,
            "address_hex": format!("0x{:08X}", self.address),
            "address_ext": self.address_ext,
            "connected": connected,
            "unit": self.unit,
            "min": self.min,
            "max": self.max,
            "characteristic_type": self.characteristic_type.as_str(),
            "axis_info": axis_info,
            "storage_order": self.storage_order.as_str(),
            "data_size": self.data_type.size(),
            "is_enum": self.compu_method.is_enum(),
            "enum_options": enum_options,
        })
    }

    /// Serialize full parameter details for info dialog.
    /// Includes all metadata with better formatting for display.
    pub fn to_detail_json(
        &self,
        device_id: &str,
        device_name: &str,
        idx: usize,
        connected: bool,
    ) -> serde_json::Value {
        let axis_info: Vec<serde_json::Value> =
            self.axis_info.iter().map(|axis| axis.to_json()).collect();
        let axis_values = self.get_static_axis_values();

        serde_json::json!({
            "device_id": device_id,
            "device_name": device_name,
            "idx": idx,
            "name": self.name,
            "description": if self.description.is_empty() { "N/A" } else { &self.description },
            "type": self.data_type.type_name(),
            "address": format!("0x{:08X}", self.address),
            "address_ext": self.address_ext,
            "connected": connected,
            "unit": self.unit.as_deref().unwrap_or("N/A"),
            "range": if self.min.is_some() && self.max.is_some() {
                format!("{} ~ {}", self.min.unwrap(), self.max.unwrap())
            } else {
                "N/A".to_string()
            },
            "characteristic_type": self.characteristic_type.as_str(),
            "axis_info": axis_info,
            "axis_values": axis_values,
            "storage_order": self.storage_order.as_str(),
            "data_size": format!("{} bytes", self.data_type.size()),
        })
    }

    /// Get axis values that don't require XCP communication.
    /// Returns None for axes that require memory reads.
    pub fn get_static_axis_values(&self) -> Option<Vec<Vec<f64>>> {
        if self.axis_info.is_empty() {
            return None;
        }

        let mut result = Vec::new();
        for axis in &self.axis_info {
            if let Some(values) = axis.get_static_values() {
                result.push(values);
            } else {
                // Axis requires memory read
                return None;
            }
        }
        Some(result)
    }
}

/// Calibration manager
#[derive(Clone)]
pub struct CalibrationManager {
    params: Vec<CalibrationParam>,
    /// Memory segments from A2L file
    memory_segments: Vec<A2lMemorySegment>,
    /// Whether SHORT_DOWNLOAD command is supported
    /// If false, must use DOWNLOAD command instead
    supports_short_download: bool,
}

impl CalibrationManager {
    /// Create calibration manager with empty parameters list
    /// Parameters must be loaded from A2L file
    pub fn new() -> Self {
        Self {
            params: Vec::new(),
            memory_segments: Vec::new(),
            supports_short_download: true, // Default to true for backwards compatibility
        }
    }

    /// Create calibration manager from an A2L file
    pub fn from_a2l<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let reader = A2lReader::load(path)?;
        let characteristics = reader.get_characteristics();
        let memory_segments = reader.get_memory_segments();

        // Check if SHORT_DOWNLOAD is supported
        let supports_short_download = reader.supports_optional_command("SHORT_DOWNLOAD");

        let params = characteristics
            .into_iter()
            .filter_map(|ch| Self::convert_characteristic(&reader, ch))
            .collect();

        Ok(Self {
            params,
            memory_segments,
            supports_short_download,
        })
    }

    /// Convert A2L characteristic to CalibrationParam
    fn convert_characteristic(
        reader: &A2lReader,
        ch: A2lCharacteristic,
    ) -> Option<CalibrationParam> {
        a2l_characteristic_to_param(ch, reader)
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
    pub async fn read_value(
        &self,
        client: &XcpClient,
        param: &CalibrationParam,
    ) -> Result<CalibrationValue, String> {
        let mta = param.mta();
        let size = param.data_type.size();

        // Read data (handle large reads)
        let data = if size <= 255 {
            client
                .read_memory(mta, size as u8)
                .await
                .map_err(|e| e.to_string())?
        } else {
            client
                .read_u8_array(mta, size)
                .await
                .map_err(|e| e.to_string())?
        };

        // Parse based on type
        let value = match &param.data_type {
            CalibrationType::U8 => CalibrationValue::U8(data.first().copied().unwrap_or(0)),
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
            CalibrationType::Bool => {
                CalibrationValue::Bool(data.first().copied().unwrap_or(0) != 0)
            }
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
                CalibrationValue::U8Matrix(flat_to_matrix(&flat, *rows, *cols, param.storage_order))
            }
            CalibrationType::U16Matrix(rows, cols) => {
                let total = rows * cols;
                let flat: Vec<u16> = (0..total)
                    .map(|i| u16::from_le_bytes([data[i * 2], data[i * 2 + 1]]))
                    .collect();
                CalibrationValue::U16Matrix(flat_to_matrix(
                    &flat,
                    *rows,
                    *cols,
                    param.storage_order,
                ))
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
                CalibrationValue::U32Matrix(flat_to_matrix(
                    &flat,
                    *rows,
                    *cols,
                    param.storage_order,
                ))
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
                CalibrationValue::U64Matrix(flat_to_matrix(
                    &flat,
                    *rows,
                    *cols,
                    param.storage_order,
                ))
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
                CalibrationValue::I16Matrix(flat_to_matrix(
                    &flat,
                    *rows,
                    *cols,
                    param.storage_order,
                ))
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
                CalibrationValue::I32Matrix(flat_to_matrix(
                    &flat,
                    *rows,
                    *cols,
                    param.storage_order,
                ))
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
                CalibrationValue::I64Matrix(flat_to_matrix(
                    &flat,
                    *rows,
                    *cols,
                    param.storage_order,
                ))
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
                CalibrationValue::F32Matrix(flat_to_matrix(
                    &flat,
                    *rows,
                    *cols,
                    param.storage_order,
                ))
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
                CalibrationValue::F64Matrix(flat_to_matrix(
                    &flat,
                    *rows,
                    *cols,
                    param.storage_order,
                ))
            }
            CalibrationType::BoolMatrix(rows, cols) => {
                let total = rows * cols;
                let flat: Vec<bool> = data.iter().take(total).map(|&b| b != 0).collect();
                CalibrationValue::BoolMatrix(flat_to_matrix(
                    &flat,
                    *rows,
                    *cols,
                    param.storage_order,
                ))
            }
            CalibrationType::Ascii(n) => {
                let s = String::from_utf8_lossy(data.iter().take(*n).copied().collect::<Vec<u8>>().as_slice())
                    .trim_end_matches('\0')
                    .to_string();
                CalibrationValue::Ascii(s)
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
            (CalibrationType::U16, CalibrationValue::U16(v)) => client
                .write_memory(mta, &v.to_le_bytes())
                .await
                .map_err(|e| e.to_string())?,
            (CalibrationType::U32, CalibrationValue::U32(v)) => {
                client.write_u32(mta, *v).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::U64, CalibrationValue::U64(v)) => client
                .write_memory(mta, &v.to_le_bytes())
                .await
                .map_err(|e| e.to_string())?,
            (CalibrationType::I8, CalibrationValue::I8(v)) => client
                .write_u8(mta, *v as u8)
                .await
                .map_err(|e| e.to_string())?,
            (CalibrationType::I16, CalibrationValue::I16(v)) => client
                .write_memory(mta, &v.to_le_bytes())
                .await
                .map_err(|e| e.to_string())?,
            (CalibrationType::I32, CalibrationValue::I32(v)) => client
                .write_memory(mta, &v.to_le_bytes())
                .await
                .map_err(|e| e.to_string())?,
            (CalibrationType::I64, CalibrationValue::I64(v)) => client
                .write_memory(mta, &v.to_le_bytes())
                .await
                .map_err(|e| e.to_string())?,
            (CalibrationType::F32, CalibrationValue::F32(v)) => client
                .write_memory(mta, &v.to_le_bytes())
                .await
                .map_err(|e| e.to_string())?,
            (CalibrationType::F64, CalibrationValue::F64(v)) => client
                .write_memory(mta, &v.to_le_bytes())
                .await
                .map_err(|e| e.to_string())?,
            (CalibrationType::Bool, CalibrationValue::Bool(v)) => client
                .write_u8(mta, if *v { 1 } else { 0 })
                .await
                .map_err(|e| e.to_string())?,
            // Array types
            (CalibrationType::U8Array(_), CalibrationValue::U8Array(v)) => client
                .write_u8_array(mta, v)
                .await
                .map_err(|e| e.to_string())?,
            (CalibrationType::U16Array(_), CalibrationValue::U16Array(v)) => {
                let flat: Vec<u8> = v.iter().flat_map(|&x| x.to_le_bytes()).collect();
                client
                    .write_memory(mta, &flat)
                    .await
                    .map_err(|e| e.to_string())?
            }
            (CalibrationType::U32Array(_), CalibrationValue::U32Array(v)) => {
                let flat: Vec<u8> = v.iter().flat_map(|&x| x.to_le_bytes()).collect();
                client
                    .write_memory(mta, &flat)
                    .await
                    .map_err(|e| e.to_string())?
            }
            (CalibrationType::U64Array(_), CalibrationValue::U64Array(v)) => {
                let flat: Vec<u8> = v.iter().flat_map(|&x| x.to_le_bytes()).collect();
                client
                    .write_memory(mta, &flat)
                    .await
                    .map_err(|e| e.to_string())?
            }
            (CalibrationType::I8Array(_), CalibrationValue::I8Array(v)) => {
                let flat: Vec<u8> = v.iter().map(|&x| x as u8).collect();
                client
                    .write_u8_array(mta, &flat)
                    .await
                    .map_err(|e| e.to_string())?
            }
            (CalibrationType::I16Array(_), CalibrationValue::I16Array(v)) => {
                let flat: Vec<u8> = v.iter().flat_map(|&x| x.to_le_bytes()).collect();
                client
                    .write_memory(mta, &flat)
                    .await
                    .map_err(|e| e.to_string())?
            }
            (CalibrationType::I32Array(_), CalibrationValue::I32Array(v)) => {
                let flat: Vec<u8> = v.iter().flat_map(|&x| x.to_le_bytes()).collect();
                client
                    .write_memory(mta, &flat)
                    .await
                    .map_err(|e| e.to_string())?
            }
            (CalibrationType::I64Array(_), CalibrationValue::I64Array(v)) => {
                let flat: Vec<u8> = v.iter().flat_map(|&x| x.to_le_bytes()).collect();
                client
                    .write_memory(mta, &flat)
                    .await
                    .map_err(|e| e.to_string())?
            }
            (CalibrationType::F32Array(_), CalibrationValue::F32Array(v)) => {
                let flat: Vec<u8> = v.iter().flat_map(|&x| x.to_le_bytes()).collect();
                client
                    .write_memory(mta, &flat)
                    .await
                    .map_err(|e| e.to_string())?
            }
            (CalibrationType::F64Array(_), CalibrationValue::F64Array(v)) => {
                let flat: Vec<u8> = v.iter().flat_map(|&x| x.to_le_bytes()).collect();
                client
                    .write_memory(mta, &flat)
                    .await
                    .map_err(|e| e.to_string())?
            }
            (CalibrationType::BoolArray(_), CalibrationValue::BoolArray(v)) => {
                let flat: Vec<u8> = v.iter().map(|&x| if x { 1 } else { 0 }).collect();
                client
                    .write_u8_array(mta, &flat)
                    .await
                    .map_err(|e| e.to_string())?
            }
            // Matrix types
            (CalibrationType::U8Matrix(rows, cols), CalibrationValue::U8Matrix(mat)) => {
                let flat: Vec<u8> = matrix_to_flat(mat, *rows, *cols, param.storage_order);
                client
                    .write_u8_array(mta, &flat)
                    .await
                    .map_err(|e| e.to_string())?
            }
            (CalibrationType::U16Matrix(rows, cols), CalibrationValue::U16Matrix(mat)) => {
                let flat: Vec<u16> = matrix_to_flat(mat, *rows, *cols, param.storage_order);
                let bytes: Vec<u8> = flat.iter().flat_map(|&x| x.to_le_bytes()).collect();
                client
                    .write_memory(mta, &bytes)
                    .await
                    .map_err(|e| e.to_string())?
            }
            (CalibrationType::U32Matrix(rows, cols), CalibrationValue::U32Matrix(mat)) => {
                let flat: Vec<u32> = matrix_to_flat(mat, *rows, *cols, param.storage_order);
                let bytes: Vec<u8> = flat.iter().flat_map(|&x| x.to_le_bytes()).collect();
                client
                    .write_memory(mta, &bytes)
                    .await
                    .map_err(|e| e.to_string())?
            }
            (CalibrationType::U64Matrix(rows, cols), CalibrationValue::U64Matrix(mat)) => {
                let flat: Vec<u64> = matrix_to_flat(mat, *rows, *cols, param.storage_order);
                let bytes: Vec<u8> = flat.iter().flat_map(|&x| x.to_le_bytes()).collect();
                client
                    .write_memory(mta, &bytes)
                    .await
                    .map_err(|e| e.to_string())?
            }
            (CalibrationType::I8Matrix(rows, cols), CalibrationValue::I8Matrix(mat)) => {
                let flat: Vec<i8> = matrix_to_flat(mat, *rows, *cols, param.storage_order);
                let bytes: Vec<u8> = flat.iter().map(|&x| x as u8).collect();
                client
                    .write_u8_array(mta, &bytes)
                    .await
                    .map_err(|e| e.to_string())?
            }
            (CalibrationType::I16Matrix(rows, cols), CalibrationValue::I16Matrix(mat)) => {
                let flat: Vec<i16> = matrix_to_flat(mat, *rows, *cols, param.storage_order);
                let bytes: Vec<u8> = flat.iter().flat_map(|&x| x.to_le_bytes()).collect();
                client
                    .write_memory(mta, &bytes)
                    .await
                    .map_err(|e| e.to_string())?
            }
            (CalibrationType::I32Matrix(rows, cols), CalibrationValue::I32Matrix(mat)) => {
                let flat: Vec<i32> = matrix_to_flat(mat, *rows, *cols, param.storage_order);
                let bytes: Vec<u8> = flat.iter().flat_map(|&x| x.to_le_bytes()).collect();
                client
                    .write_memory(mta, &bytes)
                    .await
                    .map_err(|e| e.to_string())?
            }
            (CalibrationType::I64Matrix(rows, cols), CalibrationValue::I64Matrix(mat)) => {
                let flat: Vec<i64> = matrix_to_flat(mat, *rows, *cols, param.storage_order);
                let bytes: Vec<u8> = flat.iter().flat_map(|&x| x.to_le_bytes()).collect();
                client
                    .write_memory(mta, &bytes)
                    .await
                    .map_err(|e| e.to_string())?
            }
            (CalibrationType::F32Matrix(rows, cols), CalibrationValue::F32Matrix(mat)) => {
                let flat: Vec<f32> = matrix_to_flat(mat, *rows, *cols, param.storage_order);
                let bytes: Vec<u8> = flat.iter().flat_map(|&x| x.to_le_bytes()).collect();
                client
                    .write_memory(mta, &bytes)
                    .await
                    .map_err(|e| e.to_string())?
            }
            (CalibrationType::F64Matrix(rows, cols), CalibrationValue::F64Matrix(mat)) => {
                let flat: Vec<f64> = matrix_to_flat(mat, *rows, *cols, param.storage_order);
                let bytes: Vec<u8> = flat.iter().flat_map(|&x| x.to_le_bytes()).collect();
                client
                    .write_memory(mta, &bytes)
                    .await
                    .map_err(|e| e.to_string())?
            }
            (CalibrationType::BoolMatrix(rows, cols), CalibrationValue::BoolMatrix(mat)) => {
                let flat: Vec<bool> = matrix_to_flat(mat, *rows, *cols, param.storage_order);
                let bytes: Vec<u8> = flat.iter().map(|&x| if x { 1 } else { 0 }).collect();
                client
                    .write_u8_array(mta, &bytes)
                    .await
                    .map_err(|e| e.to_string())?
            }
            (CalibrationType::Ascii(n), CalibrationValue::Ascii(s)) => {
                let src = s.as_bytes();
                let mut bytes = vec![0u8; *n];
                let copy_len = src.len().min(*n);
                bytes[..copy_len].copy_from_slice(&src[..copy_len]);
                client
                    .write_u8_array(mta, &bytes)
                    .await
                    .map_err(|e| e.to_string())?
            }
            _ => return Err(format!("Type mismatch for parameter {}", param.name)),
        }

        Ok(())
    }

    /// Parse value from string
    ///
    /// For enum types, this will convert enum text to raw value using COMPU_METHOD.
    /// For numeric types, this will parse the string directly.
    pub fn parse_value(
        &self,
        param: &CalibrationParam,
        input: &str,
    ) -> Result<CalibrationValue, String> {
        // For enum types, convert display text to raw value first
        let numeric_input = if param.is_enum() {
            // Use CompuMethod to convert enum text to raw value
            let raw = param.parse_display(input)?;
            raw.to_string()
        } else {
            input.to_string()
        };

        match &param.data_type {
            CalibrationType::U8 => {
                let v: u8 = numeric_input.parse().map_err(|_| "Invalid U8 value")?;
                Ok(CalibrationValue::U8(v))
            }
            CalibrationType::U16 => {
                let v: u16 = numeric_input.parse().map_err(|_| "Invalid U16 value")?;
                Ok(CalibrationValue::U16(v))
            }
            CalibrationType::U32 => {
                let v: u32 = numeric_input.parse().map_err(|_| "Invalid U32 value")?;
                Ok(CalibrationValue::U32(v))
            }
            CalibrationType::U64 => {
                let v: u64 = numeric_input.parse().map_err(|_| "Invalid U64 value")?;
                Ok(CalibrationValue::U64(v))
            }
            CalibrationType::I8 => {
                let v: i8 = numeric_input.parse().map_err(|_| "Invalid I8 value")?;
                Ok(CalibrationValue::I8(v))
            }
            CalibrationType::I16 => {
                let v: i16 = numeric_input.parse().map_err(|_| "Invalid I16 value")?;
                Ok(CalibrationValue::I16(v))
            }
            CalibrationType::I32 => {
                let v: i32 = numeric_input.parse().map_err(|_| "Invalid I32 value")?;
                Ok(CalibrationValue::I32(v))
            }
            CalibrationType::I64 => {
                let v: i64 = numeric_input.parse().map_err(|_| "Invalid I64 value")?;
                Ok(CalibrationValue::I64(v))
            }
            CalibrationType::F32 => {
                let v: f32 = numeric_input.parse().map_err(|_| "Invalid F32 value")?;
                Ok(CalibrationValue::F32(v))
            }
            CalibrationType::F64 => {
                let v: f64 = numeric_input.parse().map_err(|_| "Invalid F64 value")?;
                Ok(CalibrationValue::F64(v))
            }
            CalibrationType::Bool => {
                let v: bool = numeric_input
                    .parse()
                    .map_err(|_| "Invalid BOOL value (true/false)")?;
                Ok(CalibrationValue::Bool(v))
            }
            CalibrationType::U8Array(n) => {
                let values: Vec<&str> = numeric_input.split(',').collect();
                if values.len() != *n {
                    return Err(format!("Expected {} values, got {}", n, values.len()));
                }
                let arr: Result<Vec<u8>, _> = values.iter().map(|s| s.trim().parse()).collect();
                Ok(CalibrationValue::U8Array(
                    arr.map_err(|_| "Invalid U8 value")?,
                ))
            }
            CalibrationType::U8Matrix(rows, cols) => {
                let total = rows * cols;
                let values: Vec<&str> = numeric_input.split(',').collect();
                if values.len() != total {
                    return Err(format!("Expected {} values, got {}", total, values.len()));
                }
                let flat: Result<Vec<u8>, _> = values.iter().map(|s| s.trim().parse()).collect();
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
                let arr: Result<Vec<u16>, _> = values.iter().map(|s| s.trim().parse()).collect();
                Ok(CalibrationValue::U16Array(
                    arr.map_err(|_| "Invalid U16 value")?,
                ))
            }
            CalibrationType::U32Array(n) => {
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != *n {
                    return Err(format!("Expected {} values, got {}", n, values.len()));
                }
                let arr: Result<Vec<u32>, _> = values.iter().map(|s| s.trim().parse()).collect();
                Ok(CalibrationValue::U32Array(
                    arr.map_err(|_| "Invalid U32 value")?,
                ))
            }
            CalibrationType::U64Array(n) => {
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != *n {
                    return Err(format!("Expected {} values, got {}", n, values.len()));
                }
                let arr: Result<Vec<u64>, _> = values.iter().map(|s| s.trim().parse()).collect();
                Ok(CalibrationValue::U64Array(
                    arr.map_err(|_| "Invalid U64 value")?,
                ))
            }
            CalibrationType::I8Array(n) => {
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != *n {
                    return Err(format!("Expected {} values, got {}", n, values.len()));
                }
                let arr: Result<Vec<i8>, _> = values.iter().map(|s| s.trim().parse()).collect();
                Ok(CalibrationValue::I8Array(
                    arr.map_err(|_| "Invalid I8 value")?,
                ))
            }
            CalibrationType::I16Array(n) => {
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != *n {
                    return Err(format!("Expected {} values, got {}", n, values.len()));
                }
                let arr: Result<Vec<i16>, _> = values.iter().map(|s| s.trim().parse()).collect();
                Ok(CalibrationValue::I16Array(
                    arr.map_err(|_| "Invalid I16 value")?,
                ))
            }
            CalibrationType::I32Array(n) => {
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != *n {
                    return Err(format!("Expected {} values, got {}", n, values.len()));
                }
                let arr: Result<Vec<i32>, _> = values.iter().map(|s| s.trim().parse()).collect();
                Ok(CalibrationValue::I32Array(
                    arr.map_err(|_| "Invalid I32 value")?,
                ))
            }
            CalibrationType::I64Array(n) => {
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != *n {
                    return Err(format!("Expected {} values, got {}", n, values.len()));
                }
                let arr: Result<Vec<i64>, _> = values.iter().map(|s| s.trim().parse()).collect();
                Ok(CalibrationValue::I64Array(
                    arr.map_err(|_| "Invalid I64 value")?,
                ))
            }
            CalibrationType::F32Array(n) => {
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != *n {
                    return Err(format!("Expected {} values, got {}", n, values.len()));
                }
                let arr: Result<Vec<f32>, _> = values.iter().map(|s| s.trim().parse()).collect();
                Ok(CalibrationValue::F32Array(
                    arr.map_err(|_| "Invalid F32 value")?,
                ))
            }
            CalibrationType::F64Array(n) => {
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != *n {
                    return Err(format!("Expected {} values, got {}", n, values.len()));
                }
                let arr: Result<Vec<f64>, _> = values.iter().map(|s| s.trim().parse()).collect();
                Ok(CalibrationValue::F64Array(
                    arr.map_err(|_| "Invalid F64 value")?,
                ))
            }
            CalibrationType::BoolArray(n) => {
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != *n {
                    return Err(format!("Expected {} values, got {}", n, values.len()));
                }
                let arr: Result<Vec<bool>, _> = values.iter().map(|s| s.trim().parse()).collect();
                Ok(CalibrationValue::BoolArray(
                    arr.map_err(|_| "Invalid BOOL value (true/false)")?,
                ))
            }
            CalibrationType::U16Matrix(rows, cols) => {
                let total = rows * cols;
                let values: Vec<&str> = input.split(',').collect();
                if values.len() != total {
                    return Err(format!("Expected {} values, got {}", total, values.len()));
                }
                let flat: Result<Vec<u16>, _> = values.iter().map(|s| s.trim().parse()).collect();
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
                let flat: Result<Vec<u32>, _> = values.iter().map(|s| s.trim().parse()).collect();
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
                let flat: Result<Vec<u64>, _> = values.iter().map(|s| s.trim().parse()).collect();
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
                let flat: Result<Vec<i8>, _> = values.iter().map(|s| s.trim().parse()).collect();
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
                let flat: Result<Vec<i16>, _> = values.iter().map(|s| s.trim().parse()).collect();
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
                let flat: Result<Vec<i32>, _> = values.iter().map(|s| s.trim().parse()).collect();
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
                let flat: Result<Vec<i64>, _> = values.iter().map(|s| s.trim().parse()).collect();
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
                let flat: Result<Vec<f32>, _> = values.iter().map(|s| s.trim().parse()).collect();
                let flat = flat.map_err(|_| "Invalid F32 value")?;
                tracing::debug!(
                    "parse_value F32Matrix: rows={}, cols={}, flat values (first 16): {:?}",
                    rows, cols, &flat[..flat.len().min(16)]
                );
                let mut matrix = Vec::with_capacity(*rows);
                for r in 0..*rows {
                    let row: Vec<f32> = flat[r * cols..(r + 1) * cols].to_vec();
                    tracing::debug!("parse_value F32Matrix: row[{}] = {:?}", r, row);
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
                let flat: Result<Vec<f64>, _> = values.iter().map(|s| s.trim().parse()).collect();
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
                let flat: Result<Vec<bool>, _> = values.iter().map(|s| s.trim().parse()).collect();
                let flat = flat.map_err(|_| "Invalid BOOL value (true/false)")?;
                let mut matrix = Vec::with_capacity(*rows);
                for r in 0..*rows {
                    let row: Vec<bool> = flat[r * cols..(r + 1) * cols].to_vec();
                    matrix.push(row);
                }
                Ok(CalibrationValue::BoolMatrix(matrix))
            }
            CalibrationType::Ascii(_) => {
                Ok(CalibrationValue::Ascii(input.to_string()))
            }
        }
    }

    /// Get the number of memory segments
    pub fn get_memory_segment_count(&self) -> usize {
        self.memory_segments.len()
    }

    /// Get all memory segments
    pub fn get_memory_segments(&self) -> &[A2lMemorySegment] {
        &self.memory_segments
    }

    /// Set memory segments directly (avoids re-parsing A2L)
    pub fn set_memory_segments(&mut self, segments: Vec<A2lMemorySegment>) {
        self.memory_segments = segments;
    }

    /// Check if SHORT_DOWNLOAD command is supported
    ///
    /// If false, must use DOWNLOAD command instead (which is mandatory in XCP).
    /// This is determined from the OPTIONAL_CMD enum in A2L IF_DATA.
    pub fn supports_short_download(&self) -> bool {
        self.supports_short_download
    }

    /// Find a memory segment by its segment number
    fn find_segment_by_number(&self, segment: u8) -> Option<&A2lMemorySegment> {
        self.memory_segments
            .iter()
            .find(|s| s.xcp_info.as_ref().map(|x| x.segment_number) == Some(segment))
    }

    /// Check if a specific page in a segment allows write access
    ///
    /// # Arguments
    /// * `segment` - Segment number (0-255)
    /// * `page` - Page number (0-255)
    ///
    /// # Returns
    /// true if the page allows XCP write access
    pub fn is_page_write_allowed(&self, segment: u8, page: u8) -> bool {
        self.find_segment_by_number(segment)
            .and_then(|s| s.xcp_info.as_ref())
            .and_then(|xcp| xcp.pages.iter().find(|p| p.page_number == page))
            .map(|p| p.is_write_allowed())
            .unwrap_or(false)
    }

    /// Check if a segment allows calibration (has at least one page with write access)
    ///
    /// # Arguments
    /// * `segment` - Segment number (0-255)
    ///
    /// # Returns
    /// true if the segment has at least one page that allows XCP write access
    pub fn is_segment_calibration_allowed(&self, segment: u8) -> bool {
        self.find_segment_by_number(segment)
            .map(|s| s.is_calibration_allowed())
            .unwrap_or(false)
    }

    /// Find a page that allows XCP write access for a segment
    ///
    /// Returns the first page number that allows XCP write access,
    /// or None if no such page exists.
    ///
    /// # Arguments
    /// * `segment` - Segment number (0-255)
    ///
    /// # Returns
    /// - Some(page_number) if a write-allowed page is found
    /// - None if no pages allow write access
    pub fn find_write_allowed_page(&self, segment: u8) -> Option<u8> {
        self.find_segment_by_number(segment)
            .and_then(|s| s.xcp_info.as_ref())
            .and_then(|xcp| {
                xcp.pages
                    .iter()
                    .find(|p| p.is_write_allowed())
                    .map(|p| p.page_number)
            })
    }

    /// Find a read-only (reference) page for a segment
    ///
    /// Returns the first page number that does NOT allow XCP write access,
    /// or None if all pages allow write access.
    ///
    /// # Arguments
    /// * `segment` - Segment number (0-255)
    ///
    /// # Returns
    /// - Some(page_number) if a read-only page is found
    /// - None if all pages allow write access
    pub fn find_read_only_page(&self, segment: u8) -> Option<u8> {
        self.find_segment_by_number(segment)
            .and_then(|s| s.xcp_info.as_ref())
            .and_then(|xcp| {
                xcp.pages
                    .iter()
                    .find(|p| !p.is_write_allowed())
                    .map(|p| p.page_number)
            })
    }

    /// Find which segment a characteristic belongs to based on its address
    ///
    /// # Returns
    /// The segment number if found, or None if not in any segment
    pub fn find_segment_for_address(&self, address: u32, address_ext: u8) -> Option<u8> {
        for seg in &self.memory_segments {
            // Check if address falls within this segment
            if address >= seg.address && address < seg.address + seg.size {
                // Check address extension matches
                if let Some(ref xcp) = seg.xcp_info {
                    if xcp.address_extension == address_ext {
                        return Some(xcp.segment_number);
                    }
                }
            }
        }
        None
    }

    /// Check if write access is allowed for a characteristic at the given address
    ///
    /// This combines segment lookup with page write permission check
    pub fn is_write_allowed_for_address(
        &self,
        address: u32,
        address_ext: u8,
        current_page: u8,
    ) -> bool {
        if let Some(segment) = self.find_segment_for_address(address, address_ext) {
            self.is_page_write_allowed(segment, current_page)
        } else {
            // If not in any segment, default to allowing write
            // (for backwards compatibility with A2L files without segment info)
            true
        }
    }
}

impl Default for CalibrationManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert an [`A2lDataType`] to the corresponding [`CalibrationType`].
///
/// Returns `None` only if the variant mapping is unrecognised (should not happen in practice).
pub fn a2l_type_to_calibration(a2l_type: &A2lDataType) -> Option<CalibrationType> {
    match a2l_type {
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

/// Canonical conversion of an [`A2lCharacteristic`] into a [`CalibrationParam`].
///
/// This is the single authoritative implementation.  All other copies in the
/// codebase (`web/routes.rs`, `src-tauri/commands/helpers.rs`, `tui/app.rs`)
/// delegate to this function so that changes to type mapping or ASCII handling
/// propagate everywhere automatically.
pub fn a2l_characteristic_to_param(
    ch: A2lCharacteristic,
    reader: &A2lReader,
) -> Option<CalibrationParam> {
    let mut data_type = a2l_type_to_calibration(&ch.data_type)?;

    // ASCII characteristics: override data_type to Ascii(n) using the byte count
    // from the already-resolved array size (parse_characteristic_datatype already
    // encoded the NUMBER keyword as U8Array(n) for us).
    if ch.characteristic_type == CharacteristicType::Ascii {
        let byte_len = data_type.size().max(1);
        data_type = CalibrationType::Ascii(byte_len);
    }

    let mut param = CalibrationParam::new(&ch.name, ch.address, ch.address_ext, data_type)
        .with_description(&ch.description)
        .with_range(ch.min, ch.max);

    if let Some(unit) = ch.unit {
        param = param.with_unit(&unit);
    }

    param.characteristic_type = ch.characteristic_type;
    param.axis_info = ch.axis_info;
    param.storage_order = ch.storage_order;
    param.compu_method = reader.get_compu_method(&ch.conversion_name);

    Some(param)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calibration_type_size() {
        // Scalar types
        assert_eq!(CalibrationType::U8.size(), 1);
        assert_eq!(CalibrationType::I8.size(), 1);
        assert_eq!(CalibrationType::Bool.size(), 1);
        assert_eq!(CalibrationType::U16.size(), 2);
        assert_eq!(CalibrationType::I16.size(), 2);
        assert_eq!(CalibrationType::U32.size(), 4);
        assert_eq!(CalibrationType::I32.size(), 4);
        assert_eq!(CalibrationType::F32.size(), 4);
        assert_eq!(CalibrationType::U64.size(), 8);
        assert_eq!(CalibrationType::I64.size(), 8);
        assert_eq!(CalibrationType::F64.size(), 8);
    }

    #[test]
    fn test_calibration_type_array_size() {
        assert_eq!(CalibrationType::U8Array(10).size(), 10);
        assert_eq!(CalibrationType::U16Array(10).size(), 20);
        assert_eq!(CalibrationType::U32Array(10).size(), 40);
        assert_eq!(CalibrationType::U64Array(10).size(), 80);
        assert_eq!(CalibrationType::F32Array(5).size(), 20);
        assert_eq!(CalibrationType::F64Array(5).size(), 40);
    }

    #[test]
    fn test_calibration_type_matrix_size() {
        assert_eq!(CalibrationType::U8Matrix(3, 4).size(), 12);
        assert_eq!(CalibrationType::U16Matrix(3, 4).size(), 24);
        assert_eq!(CalibrationType::U32Matrix(3, 4).size(), 48);
        assert_eq!(CalibrationType::F32Matrix(2, 2).size(), 16);
    }

    #[test]
    fn test_calibration_type_name() {
        assert_eq!(CalibrationType::U8.type_name(), "U8");
        assert_eq!(CalibrationType::U32.type_name(), "U32");
        assert_eq!(CalibrationType::F32.type_name(), "F32");
        assert_eq!(CalibrationType::Bool.type_name(), "BOOL");
        assert_eq!(CalibrationType::U8Array(10).type_name(), "U8[10]");
        assert_eq!(CalibrationType::F32Array(5).type_name(), "F32[5]");
        assert_eq!(CalibrationType::U16Matrix(3, 4).type_name(), "U16[3][4]");
    }

    #[test]
    fn test_calibration_value_format_scalar() {
        assert_eq!(CalibrationValue::U8(42).format(), "42");
        assert_eq!(CalibrationValue::U32(12345).format(), "12345");
        assert_eq!(CalibrationValue::I32(-100).format(), "-100");
        assert_eq!(CalibrationValue::F32(3.14159).format(), "3.141590");
        assert_eq!(CalibrationValue::F64(2.71828).format(), "2.718280");
        assert_eq!(CalibrationValue::Bool(true).format(), "true");
        assert_eq!(CalibrationValue::Bool(false).format(), "false");
    }

    #[test]
    fn test_calibration_value_format_array() {
        let arr = CalibrationValue::U8Array(vec![1, 2, 3, 4]);
        assert_eq!(arr.format(), "[1, 2, 3, 4]");

        let arr = CalibrationValue::U32Array(vec![100, 200, 300]);
        assert_eq!(arr.format(), "[100, 200, 300]");
    }

    #[test]
    fn test_calibration_param_creation() {
        let param = CalibrationParam::new("test_param", 0x1000, 0x00, CalibrationType::U32);
        assert_eq!(param.name, "test_param");
        assert_eq!(param.address, 0x1000);
        assert_eq!(param.address_ext, 0x00);
        assert_eq!(param.data_type.size(), 4);
        assert_eq!(param.description, "");
        assert!(param.unit.is_none());
        assert!(param.min.is_none());
        assert!(param.max.is_none());
    }

    #[test]
    fn test_calibration_param_builder() {
        let param = CalibrationParam::new("test", 0x1000, 0x00, CalibrationType::F32)
            .with_description("Test parameter")
            .with_unit("Nm")
            .with_range(0.0, 100.0);

        assert_eq!(param.description, "Test parameter");
        assert_eq!(param.unit, Some("Nm".to_string()));
        assert_eq!(param.min, Some(0.0));
        assert_eq!(param.max, Some(100.0));
    }

    #[test]
    fn test_calibration_param_mta() {
        let param = CalibrationParam::new("test", 0x12345678, 0x02, CalibrationType::U32);
        let mta = param.mta();
        assert_eq!(mta.address, 0x12345678);
        assert_eq!(mta.extension, 0x02);
    }

    #[test]
    fn test_calibration_manager_new() {
        let manager = CalibrationManager::new();
        assert_eq!(manager.params().len(), 0);
    }

    #[test]
    fn test_calibration_manager_add_and_get() {
        let mut manager = CalibrationManager::new();
        let param1 = CalibrationParam::new("param1", 0x1000, 0x00, CalibrationType::U32);
        let param2 = CalibrationParam::new("param2", 0x2000, 0x00, CalibrationType::F64);

        manager.add_param(param1);
        manager.add_param(param2);

        assert_eq!(manager.params().len(), 2);

        let p1 = manager.get_param("param1");
        assert!(p1.is_some());
        assert_eq!(p1.unwrap().address, 0x1000);

        let p2 = manager.get_param("param2");
        assert!(p2.is_some());
        assert_eq!(p2.unwrap().data_type.size(), 8);

        let p3 = manager.get_param("nonexistent");
        assert!(p3.is_none());
    }

    #[test]
    fn test_calibration_manager_clear() {
        let mut manager = CalibrationManager::new();
        let param = CalibrationParam::new("test", 0x1000, 0x00, CalibrationType::U32);
        manager.add_param(param);

        assert_eq!(manager.params().len(), 1);
        manager.clear_params();
        assert_eq!(manager.params().len(), 0);
    }

    #[test]
    fn test_parse_value_scalar() {
        let manager = CalibrationManager::new();

        let param_u8 = CalibrationParam::new("u8_test", 0x1000, 0x00, CalibrationType::U8);
        assert_eq!(
            manager.parse_value(&param_u8, "255").unwrap().format(),
            "255"
        );
        assert!(manager.parse_value(&param_u8, "256").is_err()); // overflow

        let param_u32 = CalibrationParam::new("u32_test", 0x1000, 0x00, CalibrationType::U32);
        assert_eq!(
            manager.parse_value(&param_u32, "12345").unwrap().format(),
            "12345"
        );

        let param_i32 = CalibrationParam::new("i32_test", 0x1000, 0x00, CalibrationType::I32);
        assert_eq!(
            manager.parse_value(&param_i32, "-100").unwrap().format(),
            "-100"
        );

        let param_f32 = CalibrationParam::new("f32_test", 0x1000, 0x00, CalibrationType::F32);
        assert_eq!(
            manager.parse_value(&param_f32, "3.14").unwrap().format(),
            "3.140000"
        );

        let param_bool = CalibrationParam::new("bool_test", 0x1000, 0x00, CalibrationType::Bool);
        assert_eq!(
            manager.parse_value(&param_bool, "true").unwrap().format(),
            "true"
        );
        assert_eq!(
            manager.parse_value(&param_bool, "false").unwrap().format(),
            "false"
        );
    }

    #[test]
    fn test_parse_value_array() {
        let manager = CalibrationManager::new();

        let param = CalibrationParam::new("arr_test", 0x1000, 0x00, CalibrationType::U32Array(3));
        let value = manager.parse_value(&param, "1, 2, 3").unwrap();
        match value {
            CalibrationValue::U32Array(arr) => {
                assert_eq!(arr.len(), 3);
                assert_eq!(arr[0], 1);
                assert_eq!(arr[1], 2);
                assert_eq!(arr[2], 3);
            }
            _ => panic!("Expected U32Array"),
        }

        // Wrong count should fail
        assert!(manager.parse_value(&param, "1, 2").is_err());
    }

    #[test]
    fn test_parse_value_matrix() {
        let manager = CalibrationManager::new();

        let param =
            CalibrationParam::new("mat_test", 0x1000, 0x00, CalibrationType::U16Matrix(2, 3));
        let value = manager.parse_value(&param, "1, 2, 3, 4, 5, 6").unwrap();
        match value {
            CalibrationValue::U16Matrix(mat) => {
                assert_eq!(mat.len(), 2);
                assert_eq!(mat[0], vec![1, 2, 3]);
                assert_eq!(mat[1], vec![4, 5, 6]);
            }
            _ => panic!("Expected U16Matrix"),
        }

        // Wrong count should fail
        assert!(manager.parse_value(&param, "1, 2, 3").is_err());
    }

    #[test]
    fn test_memory_segment_from_a2l() {
        // Test that CalibrationManager loads memory segments from A2L
        let manager = CalibrationManager::from_a2l("abc.a2l").expect("Failed to load abc.a2l");

        // Should have 3 segments
        assert_eq!(manager.get_memory_segment_count(), 3);

        // Check segment names
        let segments = manager.get_memory_segments();
        assert!(segments.iter().any(|s| s.name == "segment_0_type_data"));
        assert!(segments
            .iter()
            .any(|s| s.name == "segment_1_type_offline_data"));
        assert!(segments.iter().any(|s| s.name == "segment_2_type_code"));
    }

    #[test]
    fn test_is_page_write_allowed() {
        let manager = CalibrationManager::from_a2l("abc.a2l").expect("Failed to load abc.a2l");

        // Segment 0:
        // - Page 0: XCP_WRITE_ACCESS_NOT_ALLOWED
        // - Page 1: XCP_WRITE_ACCESS_WITH_ECU_ONLY
        assert!(
            !manager.is_page_write_allowed(0, 0),
            "Page 0 should not allow write"
        );
        assert!(
            manager.is_page_write_allowed(0, 1),
            "Page 1 should allow write"
        );

        // Segment 2 (CODE segment):
        // - Page 0: XCP_WRITE_ACCESS_NOT_ALLOWED
        assert!(
            !manager.is_page_write_allowed(2, 0),
            "CODE segment page 0 should not allow write"
        );
    }

    #[test]
    fn test_find_segment_for_address() {
        let manager = CalibrationManager::from_a2l("abc.a2l").expect("Failed to load abc.a2l");

        // Segment 0: address 0xA0380000, size 0x40000
        // An address within segment 0
        let segment = manager.find_segment_for_address(0xA0380000 + 0x1000, 0);
        assert_eq!(segment, Some(0));

        // Address outside all segments
        let segment = manager.find_segment_for_address(0x00000000, 0);
        assert_eq!(segment, None);
    }

    #[test]
    fn test_empty_manager_segment_count() {
        let manager = CalibrationManager::new();
        assert_eq!(manager.get_memory_segment_count(), 0);
    }

    #[test]
    fn test_is_segment_calibration_allowed() {
        let manager = CalibrationManager::from_a2l("abc.a2l").expect("Failed to load abc.a2l");

        // Segment 0: has pages 0 (no write) and 1 (write allowed)
        // -> segment allows calibration
        assert!(
            manager.is_segment_calibration_allowed(0),
            "Segment 0 should allow calibration"
        );

        // Segment 1: has pages 0 (no write) and 1 (write allowed)
        // -> segment allows calibration
        assert!(
            manager.is_segment_calibration_allowed(1),
            "Segment 1 should allow calibration"
        );

        // Segment 2 (CODE): has only page 0 (no write)
        // -> segment does NOT allow calibration
        assert!(
            !manager.is_segment_calibration_allowed(2),
            "Segment 2 (CODE) should NOT allow calibration"
        );

        // Non-existent segment
        assert!(
            !manager.is_segment_calibration_allowed(99),
            "Non-existent segment should not allow calibration"
        );
    }

    #[test]
    fn test_find_write_allowed_page() {
        let manager = CalibrationManager::from_a2l("abc.a2l").expect("Failed to load abc.a2l");

        // Segment 0: Page 0 no write, Page 1 has write
        // -> should find page 1
        let page = manager.find_write_allowed_page(0);
        assert_eq!(page, Some(1), "Should find page 1 for segment 0");

        // Segment 1: Page 0 no write, Page 1 has write
        // -> should find page 1
        let page = manager.find_write_allowed_page(1);
        assert_eq!(page, Some(1), "Should find page 1 for segment 1");

        // Segment 2 (CODE): No pages with write
        // -> should return None
        let page = manager.find_write_allowed_page(2);
        assert_eq!(
            page, None,
            "Should not find any write-allowed page for CODE segment"
        );

        // Non-existent segment
        let page = manager.find_write_allowed_page(99);
        assert_eq!(page, None, "Should return None for non-existent segment");
    }

    #[test]
    fn test_find_read_only_page() {
        let manager = CalibrationManager::from_a2l("abc.a2l").expect("Failed to load abc.a2l");

        // Segment 0: Page 0 no write (read-only), Page 1 has write
        // -> should find page 0 as read-only
        let page = manager.find_read_only_page(0);
        assert_eq!(
            page,
            Some(0),
            "Should find page 0 as read-only for segment 0"
        );

        // Segment 1: Page 0 no write, Page 1 has write
        // -> should find page 0 as read-only
        let page = manager.find_read_only_page(1);
        assert_eq!(
            page,
            Some(0),
            "Should find page 0 as read-only for segment 1"
        );

        // Segment 2 (CODE): Page 0 no write
        // -> should find page 0 as read-only
        let page = manager.find_read_only_page(2);
        assert_eq!(
            page,
            Some(0),
            "Should find page 0 as read-only for CODE segment"
        );

        // Non-existent segment
        let page = manager.find_read_only_page(99);
        assert_eq!(page, None, "Should return None for non-existent segment");
    }

    #[test]
    fn test_supports_short_download() {
        let manager = CalibrationManager::from_a2l("abc.a2l").expect("Failed to load abc.a2l");

        // SHORT_DOWNLOAD is commented out in abc.a2l OPTIONAL_CMD enum
        assert!(
            !manager.supports_short_download(),
            "SHORT_DOWNLOAD should NOT be supported based on A2L OPTIONAL_CMD"
        );
    }

    #[test]
    fn test_calibration_param_characteristic_type() {
        let mut param = CalibrationParam::new("test_param", 0x1000, 0, CalibrationType::U8);
        assert_eq!(param.characteristic_type, CharacteristicType::Value);
        assert!(!param.is_curve());
        assert!(!param.is_map());
        assert!(!param.has_axis());

        // Change to CURVE type
        param.characteristic_type = CharacteristicType::Curve;
        assert!(param.is_curve());
        assert!(!param.is_map());

        // Change to MAP type
        param.characteristic_type = CharacteristicType::Map;
        assert!(!param.is_curve());
        assert!(param.is_map());
    }

    #[test]
    fn test_calibration_param_with_axis_info() {
        let mut param =
            CalibrationParam::new("curve_param", 0x2000, 0, CalibrationType::F32Array(10));

        // Add axis info
        let axis = crate::a2l::AxisInfo {
            name: "x_axis".to_string(),
            pts_ref_name: None,
            unit: Some("rpm".to_string()),
            min: 0.0,
            max: 1000.0,
            num_points: 10,
            point_source: crate::a2l::AxisPointSource::CalculatedDist {
                offset: 0.0,
                distance: 100.0,
                num_points: 10,
            },
        };

        param.characteristic_type = CharacteristicType::Curve;
        param.axis_info.push(axis);

        assert!(param.has_axis());
        assert!(param.is_curve());

        // Get static axis values
        let axis_values = param.get_static_axis_values();
        assert!(axis_values.is_some());
        let values = axis_values.unwrap();
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].len(), 10);
        assert_eq!(values[0][0], 0.0);
        assert_eq!(values[0][9], 900.0);
    }

    #[test]
    fn test_calibration_param_axis_requires_memory_read() {
        let mut param =
            CalibrationParam::new("map_param", 0x3000, 0, CalibrationType::F32Matrix(5, 10));

        // Add axis that requires memory read
        let axis = crate::a2l::AxisInfo {
            name: "memory_axis".to_string(),
            pts_ref_name: None,
            unit: None,
            min: 0.0,
            max: 100.0,
            num_points: 5,
            point_source: crate::a2l::AxisPointSource::Memory {
                address: 0x4000,
                address_ext: 0,
                data_type: crate::a2l::A2lDataType::F32,
                num_points: 5,
            },
        };

        param.characteristic_type = CharacteristicType::Map;
        param.axis_info.push(axis);

        assert!(param.has_axis());
        assert!(param.is_map());

        // Should return None because axis requires memory read
        let axis_values = param.get_static_axis_values();
        assert!(axis_values.is_none());
    }

    #[test]
    fn test_flat_to_matrix_row_dir() {
        // Test row-major order: [[1,2,3],[4,5,6],[7,8,9]]
        // Stored as: [1,2,3,4,5,6,7,8,9]
        let flat = vec![1u32, 2, 3, 4, 5, 6, 7, 8, 9];
        let matrix = flat_to_matrix(&flat, 3, 3, StorageOrder::RowDir);

        assert_eq!(matrix.len(), 3);
        assert_eq!(matrix[0], vec![1, 2, 3]);
        assert_eq!(matrix[1], vec![4, 5, 6]);
        assert_eq!(matrix[2], vec![7, 8, 9]);
    }

    #[test]
    fn test_flat_to_matrix_column_dir() {
        // Test column-major order: [[1,2,3],[4,5,6],[7,8,9]]
        // Stored as: [1,4,7,2,5,8,3,6,9]
        let flat = vec![1u32, 4, 7, 2, 5, 8, 3, 6, 9];
        let matrix = flat_to_matrix(&flat, 3, 3, StorageOrder::ColumnDir);

        assert_eq!(matrix.len(), 3);
        assert_eq!(matrix[0], vec![1, 2, 3]);
        assert_eq!(matrix[1], vec![4, 5, 6]);
        assert_eq!(matrix[2], vec![7, 8, 9]);
    }

    #[test]
    fn test_matrix_to_flat_row_dir() {
        let matrix = vec![vec![1u32, 2, 3], vec![4, 5, 6], vec![7, 8, 9]];

        let flat = matrix_to_flat(&matrix, 3, 3, StorageOrder::RowDir);
        assert_eq!(flat, vec![1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn test_matrix_to_flat_column_dir() {
        let matrix = vec![vec![1u32, 2, 3], vec![4, 5, 6], vec![7, 8, 9]];

        let flat = matrix_to_flat(&matrix, 3, 3, StorageOrder::ColumnDir);
        assert_eq!(flat, vec![1, 4, 7, 2, 5, 8, 3, 6, 9]);
    }

    #[test]
    fn test_roundtrip_row_dir() {
        let original = vec![vec![1.0f32, 2.0, 3.0], vec![4.0, 5.0, 6.0]];

        let flat = matrix_to_flat(&original, 2, 3, StorageOrder::RowDir);
        let recovered = flat_to_matrix(&flat, 2, 3, StorageOrder::RowDir);

        assert_eq!(original, recovered);
    }

    #[test]
    fn test_roundtrip_column_dir() {
        let original = vec![vec![1.0f32, 2.0, 3.0], vec![4.0, 5.0, 6.0]];

        let flat = matrix_to_flat(&original, 2, 3, StorageOrder::ColumnDir);
        let recovered = flat_to_matrix(&flat, 2, 3, StorageOrder::ColumnDir);

        assert_eq!(original, recovered);
    }

    #[test]
    fn test_non_square_matrix_column_dir() {
        // 2x3 matrix: [[1,2,3],[4,5,6]]
        // Column-major: [1,4,2,5,3,6]
        let flat = vec![1u16, 4, 2, 5, 3, 6];
        let matrix = flat_to_matrix(&flat, 2, 3, StorageOrder::ColumnDir);

        assert_eq!(matrix.len(), 2);
        assert_eq!(matrix[0], vec![1, 2, 3]);
        assert_eq!(matrix[1], vec![4, 5, 6]);

        // Verify roundtrip
        let back_to_flat = matrix_to_flat(&matrix, 2, 3, StorageOrder::ColumnDir);
        assert_eq!(flat, back_to_flat);
    }

    #[test]
    fn test_enum_characteristic_from_a2l() {
        // Test loading characteristics with enum COMPU_METHOD from hello_xcp.a2l
        let manager = CalibrationManager::from_a2l("hello_xcp.a2l")
            .expect("Failed to load calibration manager");

        // Find my_params.counter_on (uses BOOL enum)
        let counter_on = manager
            .params()
            .iter()
            .find(|p| p.name == "my_params.counter_on");
        assert!(counter_on.is_some(), "Should find my_params.counter_on");

        let param = counter_on.unwrap();
        assert!(param.is_enum(), "counter_on should be an enum");
        assert_eq!(param.enum_count(), 2);
        assert_eq!(param.format_value(0), "false");
        assert_eq!(param.format_value(1), "true");

        // Find my_params.op_mode (uses OP_MODE enum with 5 states)
        let op_mode = manager
            .params()
            .iter()
            .find(|p| p.name == "my_params.op_mode");
        assert!(op_mode.is_some(), "Should find my_params.op_mode");

        let op_param = op_mode.unwrap();
        assert!(op_param.is_enum(), "op_mode should be an enum");
        assert_eq!(op_param.enum_count(), 5);
        assert_eq!(op_param.format_value(0), "MODE_OFF");
        assert_eq!(op_param.format_value(2), "MODE_AUTO");
        assert_eq!(op_param.parse_display("MODE_ON").unwrap(), 1);

        // Find my_params.state (uses STATE enum with 4 states)
        let state = manager
            .params()
            .iter()
            .find(|p| p.name == "my_params.state");
        assert!(state.is_some(), "Should find my_params.state");

        let state_param = state.unwrap();
        assert!(state_param.is_enum(), "state should be an enum");
        assert_eq!(state_param.enum_count(), 4);
        assert_eq!(state_param.format_value(0), "STATE_INIT");
        assert_eq!(state_param.format_value(3), "STATE_ERROR");

        println!("Enum characteristic test passed!");
    }
}
