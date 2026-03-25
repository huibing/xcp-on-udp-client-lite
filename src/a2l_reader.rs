//! A2L File Reader
//!
//! Parses A2L files using the a2lfile crate to extract measurement and
//! characteristic information for XCP calibration and DAQ.

use a2lfile::A2lObjectName;
use std::path::Path;

/// Data type information extracted from A2L
#[derive(Debug, Clone)]
pub enum A2lDataType {
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

impl A2lDataType {
    /// Get the size in bytes
    pub fn size(&self) -> usize {
        match self {
            A2lDataType::U8 | A2lDataType::I8 | A2lDataType::Bool => 1,
            A2lDataType::U16 | A2lDataType::I16 => 2,
            A2lDataType::U32 | A2lDataType::I32 | A2lDataType::F32 => 4,
            A2lDataType::U64 | A2lDataType::I64 | A2lDataType::F64 => 8,
            A2lDataType::U8Array(n) => *n,
            A2lDataType::U8Matrix(rows, cols) => rows * cols,
        }
    }

    /// Get type name string
    pub fn type_name(&self) -> String {
        match self {
            A2lDataType::U8 => "U8".to_string(),
            A2lDataType::U16 => "U16".to_string(),
            A2lDataType::U32 => "U32".to_string(),
            A2lDataType::U64 => "U64".to_string(),
            A2lDataType::I8 => "I8".to_string(),
            A2lDataType::I16 => "I16".to_string(),
            A2lDataType::I32 => "I32".to_string(),
            A2lDataType::I64 => "I64".to_string(),
            A2lDataType::F32 => "F32".to_string(),
            A2lDataType::F64 => "F64".to_string(),
            A2lDataType::Bool => "BOOL".to_string(),
            A2lDataType::U8Array(n) => format!("U8[{}]", n),
            A2lDataType::U8Matrix(rows, cols) => format!("U8[{}][{}]", rows, cols),
        }
    }
}

/// Measurement information from A2L
#[derive(Debug, Clone)]
pub struct A2lMeasurement {
    pub name: String,
    pub description: String,
    pub address: u32,
    pub address_ext: u8,
    pub data_type: A2lDataType,
    pub unit: Option<String>,
    pub min: f64,
    pub max: f64,
}

/// Characteristic (calibration parameter) information from A2L
#[derive(Debug, Clone)]
pub struct A2lCharacteristic {
    pub name: String,
    pub description: String,
    pub address: u32,
    pub address_ext: u8,
    pub data_type: A2lDataType,
    pub unit: Option<String>,
    pub min: f64,
    pub max: f64,
}

/// A2L file reader and parser
pub struct A2lReader {
    file: a2lfile::A2lFile,
}

impl A2lReader {
    /// Load and parse an A2L file
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let path = path.as_ref();
        let (file, errors) = a2lfile::load(path, None, true).map_err(|e| {
            format!("Failed to load A2L file '{}': {:?}", path.display(), e)
        })?;

        if !errors.is_empty() {
            for err in &errors {
                tracing::warn!("A2L parsing warning: {:?}", err);
            }
        }

        Ok(Self { file })
    }

    /// Get all measurements from the A2L file
    pub fn get_measurements(&self) -> Vec<A2lMeasurement> {
        let mut measurements = Vec::new();

        for module in &self.file.project.module {
            for meas in &module.measurement {
                if let Some(info) = self.parse_measurement(meas) {
                    measurements.push(info);
                }
            }
        }

        measurements
    }

    /// Get all characteristics (calibration parameters) from the A2L file
    pub fn get_characteristics(&self) -> Vec<A2lCharacteristic> {
        let mut characteristics = Vec::new();

        for module in &self.file.project.module {
            for ch in &module.characteristic {
                if let Some(info) = self.parse_characteristic(ch, module) {
                    characteristics.push(info);
                }
            }
        }

        characteristics
    }

    /// Parse a single measurement
    fn parse_measurement(&self, meas: &a2lfile::Measurement) -> Option<A2lMeasurement> {
        let name = meas.get_name().to_string();
        let description = meas.long_identifier.clone();

        // Get address from ECU_ADDRESS
        let ecu_addr = meas.ecu_address.as_ref()?;
        let address = ecu_addr.address;

        // Get address extension (default to 0 if not specified)
        let address_ext = meas
            .ecu_address_extension
            .as_ref()
            .map(|ext| ext.extension as u8)
            .unwrap_or(0);

        // Parse data type from datatype field
        let data_type = self.parse_datatype(&meas.datatype)?;

        // Get unit from conversion method if available
        let unit = self.get_unit_for_conversion(&meas.conversion);

        // Get min/max values
        let min = meas.lower_limit;
        let max = meas.upper_limit;

        Some(A2lMeasurement {
            name,
            description,
            address,
            address_ext,
            data_type,
            unit,
            min,
            max,
        })
    }

    /// Parse a single characteristic
    fn parse_characteristic(
        &self,
        ch: &a2lfile::Characteristic,
        module: &a2lfile::Module,
    ) -> Option<A2lCharacteristic> {
        let name = ch.get_name().to_string();
        let description = ch.long_identifier.clone();

        // Get address
        let address = ch.address;

        // Get address extension from IF_DATA XCP SEGMENT if available, otherwise default to 0
        let address_ext = self.get_characteristic_address_ext(ch, module);

        // Parse data type from deposit (record layout)
        let data_type = self.parse_characteristic_datatype(ch, module)?;

        // Get unit from conversion method
        let unit = self.get_unit_for_conversion(&ch.conversion);

        // Get min/max values
        let min = ch.lower_limit;
        let max = ch.upper_limit;

        Some(A2lCharacteristic {
            name,
            description,
            address,
            address_ext,
            data_type,
            unit,
            min,
            max,
        })
    }

    /// Get address extension for a characteristic from its memory segment
    fn get_characteristic_address_ext(
        &self,
        ch: &a2lfile::Characteristic,
        module: &a2lfile::Module,
    ) -> u8 {
        // Try to find memory segment for this characteristic's address
        if let Some(mod_par) = &module.mod_par {
            for mem_seg in &mod_par.memory_segment {
                // Check IF_DATA for XCP segment info
                for _if_data in &mem_seg.if_data {
                    // The IF_DATA content is stored as raw items, we need to check XCP-specific data
                    // For now, use a simplified approach based on address ranges
                    // Check if characteristic address falls within this segment
                    if ch.address >= mem_seg.address
                        && ch.address < mem_seg.address + mem_seg.size
                    {
                        // Return 2 for FLASH/calibration segments (based on A2L file pattern)
                        if mem_seg.memory_type == a2lfile::MemoryType::Flash {
                            return 2;
                        }
                        return 0;
                    }
                }
            }
        }
        0 // Default to 0 if not found
    }

    /// Parse data type
    fn parse_datatype(&self, datatype: &a2lfile::DataType) -> Option<A2lDataType> {
        match datatype {
            a2lfile::DataType::Ubyte => Some(A2lDataType::U8),
            a2lfile::DataType::Uword => Some(A2lDataType::U16),
            a2lfile::DataType::Ulong => Some(A2lDataType::U32),
            a2lfile::DataType::AUint64 => Some(A2lDataType::U64),
            a2lfile::DataType::Sbyte => Some(A2lDataType::I8),
            a2lfile::DataType::Sword => Some(A2lDataType::I16),
            a2lfile::DataType::Slong => Some(A2lDataType::I32),
            a2lfile::DataType::AInt64 => Some(A2lDataType::I64),
            a2lfile::DataType::Float32Ieee => Some(A2lDataType::F32),
            a2lfile::DataType::Float64Ieee => Some(A2lDataType::F64),
            _ => None,
        }
    }

    /// Parse characteristic data type from deposit (record layout)
    fn parse_characteristic_datatype(
        &self,
        ch: &a2lfile::Characteristic,
        module: &a2lfile::Module,
    ) -> Option<A2lDataType> {
        // Find the record layout
        let record_layout_name = &ch.deposit;
        let record_layout = module
            .record_layout
            .iter()
            .find(|rl| rl.get_name() == record_layout_name)?;

        // Determine data type from record layout's FNC_VALUES
        let base_type = if record_layout.fnc_values.is_some() {
            // Check the datatype of FNC_VALUES
            // Look up the corresponding typedef if available
            self.get_type_from_record_layout(record_layout, module)
        } else {
            // Fallback: try to infer from characteristic type
            match ch.characteristic_type {
                a2lfile::CharacteristicType::Value => Some(A2lDataType::U32),
                a2lfile::CharacteristicType::ValBlk => {
                    // Handle array/matrix
                    if let Some(matrix_dim) = &ch.matrix_dim {
                        self.parse_matrix_dim(matrix_dim)
                    } else {
                        Some(A2lDataType::U8Array(1))
                    }
                }
                _ => Some(A2lDataType::U32),
            }
        };

        // Handle matrix dimensions for arrays
        if let Some(matrix_dim) = &ch.matrix_dim {
            if let Some(A2lDataType::U8) = base_type {
                return self.parse_matrix_dim(matrix_dim);
            }
        }

        base_type
    }

    /// Get data type from record layout
    fn get_type_from_record_layout(
        &self,
        record_layout: &a2lfile::RecordLayout,
        _module: &a2lfile::Module,
    ) -> Option<A2lDataType> {
        // Try to get type from record layout name
        let name = record_layout.get_name();

        // Common naming patterns in A2L files
        if name.starts_with("U8") || name == "U8" {
            Some(A2lDataType::U8)
        } else if name.starts_with("U16") || name == "U16" {
            Some(A2lDataType::U16)
        } else if name.starts_with("U32") || name == "U32" || name.starts_with("ULONG") {
            Some(A2lDataType::U32)
        } else if name.starts_with("U64") || name == "U64" {
            Some(A2lDataType::U64)
        } else if name.starts_with("I8") || name == "I8" || name.starts_with("SBYTE") {
            Some(A2lDataType::I8)
        } else if name.starts_with("I16") || name == "I16" || name.starts_with("SWORD") {
            Some(A2lDataType::I16)
        } else if name.starts_with("I32") || name == "I32" || name.starts_with("SLONG") {
            Some(A2lDataType::I32)
        } else if name.starts_with("I64") || name == "I64" {
            Some(A2lDataType::I64)
        } else if name.starts_with("F32") || name == "F32" || name.starts_with("FLOAT32") {
            Some(A2lDataType::F32)
        } else if name.starts_with("F64") || name == "F64" || name.starts_with("FLOAT64") {
            Some(A2lDataType::F64)
        } else if name.starts_with("BOOL") || name == "BOOL" {
            Some(A2lDataType::Bool)
        } else {
            // Default to U32 if unknown
            Some(A2lDataType::U32)
        }
    }

    /// Parse matrix dimensions
    fn parse_matrix_dim(&self, matrix_dim: &a2lfile::MatrixDim) -> Option<A2lDataType> {
        let dims = &matrix_dim.dim_list;
        match dims.len() {
            1 => Some(A2lDataType::U8Array(dims[0] as usize)),
            2 => Some(A2lDataType::U8Matrix(dims[0] as usize, dims[1] as usize)),
            _ => None,
        }
    }

    /// Get unit string for a conversion method name
    fn get_unit_for_conversion(&self, conversion_name: &str) -> Option<String> {
        if conversion_name.is_empty() || conversion_name == "NO_COMPU_METHOD" {
            return None;
        }

        for module in &self.file.project.module {
            // Check COMPU_METHOD
            for cm in &module.compu_method {
                if cm.get_name() == conversion_name {
                    if !cm.unit.is_empty() {
                        return Some(cm.unit.clone());
                    }
                    break;
                }
            }
        }

        None
    }

    /// Get the project name
    pub fn project_name(&self) -> &str {
        &self.file.project.name
    }

    /// Get module names
    pub fn module_names(&self) -> Vec<&str> {
        self.file
            .project
            .module
            .iter()
            .map(|m| m.get_name())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_a2l_data_type_size() {
        assert_eq!(A2lDataType::U8.size(), 1);
        assert_eq!(A2lDataType::U16.size(), 2);
        assert_eq!(A2lDataType::U32.size(), 4);
        assert_eq!(A2lDataType::U64.size(), 8);
        assert_eq!(A2lDataType::F32.size(), 4);
        assert_eq!(A2lDataType::F64.size(), 8);
        assert_eq!(A2lDataType::Bool.size(), 1);
        assert_eq!(A2lDataType::U8Array(10).size(), 10);
        assert_eq!(A2lDataType::U8Matrix(8, 4).size(), 32);
    }

    #[test]
    fn test_a2l_data_type_name() {
        assert_eq!(A2lDataType::U8.type_name(), "U8");
        assert_eq!(A2lDataType::U32.type_name(), "U32");
        assert_eq!(A2lDataType::F32.type_name(), "F32");
        assert_eq!(A2lDataType::Bool.type_name(), "BOOL");
        assert_eq!(A2lDataType::U8Array(4).type_name(), "U8[4]");
        assert_eq!(A2lDataType::U8Matrix(8, 4).type_name(), "U8[8][4]");
    }
}
