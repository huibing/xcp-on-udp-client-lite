//! Calibration Parameter Management

use crate::a2l_reader::{A2lCharacteristic, A2lDataType, A2lReader};
use crate::client::XcpClient;
use crate::protocol::Mta;
use std::path::Path;

/// Calibration parameter types
#[derive(Debug, Clone)]
pub enum CalibrationType {
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
    U8Array(usize),
    U8Matrix(usize, usize), // rows, cols
}

impl CalibrationType {
    /// Get the size in bytes
    pub fn size(&self) -> usize {
        match self {
            CalibrationType::U8 | CalibrationType::I8 | CalibrationType::Bool => 1,
            CalibrationType::U16 | CalibrationType::I16 => 2,
            CalibrationType::U32 | CalibrationType::I32 | CalibrationType::F32 => 4,
            CalibrationType::U64 | CalibrationType::I64 | CalibrationType::F64 => 8,
            CalibrationType::U8Array(n) => *n,
            CalibrationType::U8Matrix(rows, cols) => rows * cols,
        }
    }

    /// Get type name string
    pub fn type_name(&self) -> String {
        match self {
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
            CalibrationType::U8Array(n) => format!("U8[{}]", n),
            CalibrationType::U8Matrix(rows, cols) => format!("U8[{}][{}]", rows, cols),
        }
    }
}

/// Calibration parameter value
#[derive(Debug, Clone)]
pub enum CalibrationValue {
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
    U8Array(Vec<u8>),
    U8Matrix(Vec<Vec<u8>>),
}

impl CalibrationValue {
    /// Format value for display
    pub fn format(&self) -> String {
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
            CalibrationValue::U8Array(arr) => format!("{:?}", arr),
            CalibrationValue::U8Matrix(mat) => {
                let rows: Vec<String> = mat.iter().map(|row| format!("{:?}", row)).collect();
                format!("[\n  {}]", rows.join(",\n  "))
            }
        }
    }
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
        }
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
    /// Create calibration manager with hardcoded default parameters
    pub fn new() -> Self {
        Self {
            params: Self::get_default_params(),
        }
    }

    /// Create calibration manager from an A2L file
    pub fn from_a2l<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let reader = A2lReader::load(path)?;
        let characteristics = reader.get_characteristics();

        let params = characteristics
            .into_iter()
            .filter_map(|ch| Self::convert_characteristic(ch))
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
            A2lDataType::U8Matrix(rows, cols) => Some(CalibrationType::U8Matrix(*rows, *cols)),
        }
    }

    /// Get parameters defined in hello_xcp.a2l (fallback when A2L file is not available)
    fn get_default_params() -> Vec<CalibrationParam> {
        vec![
            CalibrationParam::new("my_params.counter_max", 0x80010000, 0, CalibrationType::U32)
                .with_description("Max counter value")
                .with_range(0.0, 1023.0),
            CalibrationParam::new("my_params.counter_on", 0x8001002C, 0, CalibrationType::Bool)
                .with_description("Start/stop counter"),
            CalibrationParam::new("my_params.delay", 0x80010004, 0, CalibrationType::U32)
                .with_description("Task delay time in us")
                .with_unit("us")
                .with_range(0.00001, 2.0),
            CalibrationParam::new("my_params.array", 0x80010008, 0, CalibrationType::U8Array(4))
                .with_description("Demo array"),
            CalibrationParam::new("my_params.matrix", 0x8001000C, 0, CalibrationType::U8Matrix(8, 4))
                .with_description("Demo matrix"),
        ]
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
            CalibrationType::U8 => CalibrationValue::U8(data.get(0).copied().unwrap_or(0)),
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
            CalibrationType::Bool => CalibrationValue::Bool(data.get(0).copied().unwrap_or(0) != 0),
            CalibrationType::U8Array(n) => {
                CalibrationValue::U8Array(data.iter().take(*n).copied().collect())
            }
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
            (CalibrationType::U8Array(_), CalibrationValue::U8Array(v)) => {
                client.write_u8_array(mta, v).await.map_err(|e| e.to_string())?
            }
            (CalibrationType::U8Matrix(_, _), CalibrationValue::U8Matrix(mat)) => {
                let flat: Vec<u8> = mat.iter().flatten().copied().collect();
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
        }
    }
}

impl Default for CalibrationManager {
    fn default() -> Self {
        Self::new()
    }
}
