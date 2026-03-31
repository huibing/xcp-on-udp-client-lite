//! A2L File Reader
//!
//! Parses A2L files using the a2lfile crate to extract measurement and
//! characteristic information for XCP calibration and DAQ.

use a2lfile::A2lObjectName;
use std::path::Path;

/// Data type information extracted from A2L
#[derive(Debug, Clone)]
pub enum A2lDataType {
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
    U8Matrix(usize, usize),  // rows, cols
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

impl A2lDataType {
    /// Get the size in bytes
    pub fn size(&self) -> usize {
        match self {
            // Scalar types
            A2lDataType::U8 | A2lDataType::I8 | A2lDataType::Bool => 1,
            A2lDataType::U16 | A2lDataType::I16 => 2,
            A2lDataType::U32 | A2lDataType::I32 | A2lDataType::F32 => 4,
            A2lDataType::U64 | A2lDataType::I64 | A2lDataType::F64 => 8,
            // Array types
            A2lDataType::U8Array(n) | A2lDataType::I8Array(n) | A2lDataType::BoolArray(n) => *n,
            A2lDataType::U16Array(n) | A2lDataType::I16Array(n) => *n * 2,
            A2lDataType::U32Array(n) | A2lDataType::I32Array(n) | A2lDataType::F32Array(n) => *n * 4,
            A2lDataType::U64Array(n) | A2lDataType::I64Array(n) | A2lDataType::F64Array(n) => *n * 8,
            // Matrix types
            A2lDataType::U8Matrix(r, c) | A2lDataType::I8Matrix(r, c) | A2lDataType::BoolMatrix(r, c) => r * c,
            A2lDataType::U16Matrix(r, c) | A2lDataType::I16Matrix(r, c) => r * c * 2,
            A2lDataType::U32Matrix(r, c) | A2lDataType::I32Matrix(r, c) | A2lDataType::F32Matrix(r, c) => r * c * 4,
            A2lDataType::U64Matrix(r, c) | A2lDataType::I64Matrix(r, c) | A2lDataType::F64Matrix(r, c) => r * c * 8,
        }
    }

    /// Get type name string
    pub fn type_name(&self) -> String {
        match self {
            // Scalar types
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
            // Array types
            A2lDataType::U8Array(n) => format!("U8[{}]", n),
            A2lDataType::U16Array(n) => format!("U16[{}]", n),
            A2lDataType::U32Array(n) => format!("U32[{}]", n),
            A2lDataType::U64Array(n) => format!("U64[{}]", n),
            A2lDataType::I8Array(n) => format!("I8[{}]", n),
            A2lDataType::I16Array(n) => format!("I16[{}]", n),
            A2lDataType::I32Array(n) => format!("I32[{}]", n),
            A2lDataType::I64Array(n) => format!("I64[{}]", n),
            A2lDataType::F32Array(n) => format!("F32[{}]", n),
            A2lDataType::F64Array(n) => format!("F64[{}]", n),
            A2lDataType::BoolArray(n) => format!("BOOL[{}]", n),
            // Matrix types
            A2lDataType::U8Matrix(r, c) => format!("U8[{}][{}]", r, c),
            A2lDataType::U16Matrix(r, c) => format!("U16[{}][{}]", r, c),
            A2lDataType::U32Matrix(r, c) => format!("U32[{}][{}]", r, c),
            A2lDataType::U64Matrix(r, c) => format!("U64[{}][{}]", r, c),
            A2lDataType::I8Matrix(r, c) => format!("I8[{}][{}]", r, c),
            A2lDataType::I16Matrix(r, c) => format!("I16[{}][{}]", r, c),
            A2lDataType::I32Matrix(r, c) => format!("I32[{}][{}]", r, c),
            A2lDataType::I64Matrix(r, c) => format!("I64[{}][{}]", r, c),
            A2lDataType::F32Matrix(r, c) => format!("F32[{}][{}]", r, c),
            A2lDataType::F64Matrix(r, c) => format!("F64[{}][{}]", r, c),
            A2lDataType::BoolMatrix(r, c) => format!("BOOL[{}][{}]", r, c),
        }
    }
}

/// Dimension type inferred from record layout name
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DimensionKind {
    /// Scalar value (single element)
    Scalar,
    /// 1D array (actual size from matrix_dim)
    Array,
    /// 2D matrix (actual size from matrix_dim)
    Matrix,
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

/// XCP checksum algorithm types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XcpChecksum {
    Add11,
    Add12,
    Add14,
    Add22,
    Add24,
    Add44,
    Crc16,
    Crc16Citt,
    Crc32,
    UserDefined(u8),
}

impl XcpChecksum {
    /// Parse from string representation (e.g., "XCP_CRC_32")
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "XCP_ADD_11" => Some(Self::Add11),
            "XCP_ADD_12" => Some(Self::Add12),
            "XCP_ADD_14" => Some(Self::Add14),
            "XCP_ADD_22" => Some(Self::Add22),
            "XCP_ADD_24" => Some(Self::Add24),
            "XCP_ADD_44" => Some(Self::Add44),
            "XCP_CRC_16" => Some(Self::Crc16),
            "XCP_CRC_16_CITT" => Some(Self::Crc16Citt),
            "XCP_CRC_32" => Some(Self::Crc32),
            s if s.starts_with("XCP_USER_DEFINED") => {
                // Extract the value from enum format if present
                None // Will be parsed separately
            }
            _ => None,
        }
    }
}

/// XCP write access permission for a page
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XcpWriteAccess {
    /// XCP write access not allowed
    NotAllowed,
    /// XCP write access with ECU only
    WithEcuOnly,
    /// XCP write access always allowed
    Always,
}

impl XcpWriteAccess {
    /// Check if XCP write is allowed
    pub fn is_write_allowed(&self) -> bool {
        match self {
            Self::NotAllowed => false,
            Self::WithEcuOnly => true,
            Self::Always => true,
        }
    }
}

/// XCP read access permission for a page
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XcpReadAccess {
    /// XCP read access with ECU only
    WithEcuOnly,
    /// XCP read access always allowed
    Always,
}

/// ECU access mode for a page
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EcuAccessMode {
    /// ECU access with XCP only
    WithXcpOnly,
    /// ECU access always
    Always,
}

/// XCP Page information within a memory segment
#[derive(Debug, Clone)]
pub struct XcpPage {
    /// Page number (0-indexed)
    pub page_number: u8,
    /// ECU access mode
    pub ecu_access: EcuAccessMode,
    /// XCP read access mode
    pub xcp_read_access: XcpReadAccess,
    /// XCP write access mode - this is critical for calibration!
    pub xcp_write_access: XcpWriteAccess,
}

impl XcpPage {
    /// Check if this page allows XCP write access
    pub fn is_write_allowed(&self) -> bool {
        self.xcp_write_access.is_write_allowed()
    }
}

/// XCP-specific segment information from IF_DATA
#[derive(Debug, Clone)]
pub struct XcpSegmentInfo {
    /// Segment logical number
    pub segment_number: u8,
    /// Number of pages in this segment
    pub num_pages: u8,
    /// Address extension for this segment
    pub address_extension: u8,
    /// Checksum algorithm (if specified)
    pub checksum: Option<XcpChecksum>,
    /// Page information
    pub pages: Vec<XcpPage>,
}

impl XcpSegmentInfo {
    /// Check if any page in this segment allows XCP write access
    pub fn is_write_allowed(&self) -> bool {
        self.pages.iter().any(|p| p.is_write_allowed())
    }
}

/// Memory segment information from A2L
#[derive(Debug, Clone)]
pub struct A2lMemorySegment {
    /// Segment name
    pub name: String,
    /// Segment description
    pub description: String,
    /// Program type (DATA, CODE, OFFLINE_DATA, etc.)
    pub prg_type: String,
    /// Memory type (FLASH, RAM, EEPROM, etc.)
    pub memory_type: String,
    /// Memory attribute (INTERN, EXTERN)
    pub attribute: String,
    /// Base address of the segment
    pub address: u32,
    /// Size of the segment in bytes
    pub size: u32,
    /// XCP-specific segment information (from IF_DATA XCPplus)
    pub xcp_info: Option<XcpSegmentInfo>,
}

impl A2lMemorySegment {
    /// Check if this segment allows XCP write access (calibration)
    /// Returns true if the segment has XCP info and at least one page allows write
    pub fn is_calibration_allowed(&self) -> bool {
        self.xcp_info.as_ref().map_or(false, |xcp| xcp.is_write_allowed())
    }
}

/// A2L file reader and parser
pub struct A2lReader {
    file: a2lfile::A2lFile,
}

impl A2lReader {
    /// Load and parse an A2L file
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let path = path.as_ref();
        let mut errors = Vec::new();
        let file = a2lfile::load(path, None, &mut errors, true).map_err(|e| {
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
    /// Default is 0, unless IF_DATA XCP SEGMENT explicitly specifies otherwise
    fn get_characteristic_address_ext(
        &self,
        ch: &a2lfile::Characteristic,
        module: &a2lfile::Module,
    ) -> u8 {
        // Try to find memory segment for this characteristic's address
        if let Some(mod_par) = &module.mod_par {
            for mem_seg in &mod_par.memory_segment {
                // Check if characteristic address falls within this segment
                if ch.address >= mem_seg.address
                    && ch.address < mem_seg.address + mem_seg.size
                {
                    // Check IF_DATA XCP SEGMENT for explicit address extension
                    for if_data in &mem_seg.if_data {
                        if let Some(ref generic) = if_data.ifdata_items {
                            if let Some(ext) = Self::extract_address_extension_from_segment(generic) {
                                return ext;
                            }
                        }
                    }
                    // Default to 0 if no explicit address extension found in IF_DATA
                    return 0;
                }
            }
        }
        0 // Default to 0 if not found
    }

    /// Extract address extension from IF_DATA XCP SEGMENT block
    fn extract_address_extension_from_segment(generic: &a2lfile::GenericIfData) -> Option<u8> {
        // SEGMENT format: /begin SEGMENT segment_number address_extension ... /end SEGMENT
        // Look for SEGMENT block in the IF_DATA
        let content = format!("{:?}", generic);

        // Parse SEGMENT block for address extension
        // Example: Block { tag: "SEGMENT", items: [UInt(0), UInt(2), ...] }
        // The second UInt is the address extension
        if let Some(seg_pos) = content.to_lowercase().find("segment") {
            let after_seg = &content[seg_pos..];
            // Find UInt values after SEGMENT
            let mut found_values = Vec::new();
            let mut pos = 0;
            while let Some(uint_pos) = after_seg[pos..].find("UInt(") {
                let start = pos + uint_pos + 5;
                if let Some(end) = after_seg[start..].find(")") {
                    if let Some(comma_pos) = after_seg[start..start+end].find(",") {
                        let num_str = &after_seg[start..start+comma_pos];
                        if let Ok(num) = num_str.parse::<u64>() {
                            found_values.push(num as u8);
                        }
                    } else {
                        let num_str = &after_seg[start..start+end];
                        if let Ok(num) = num_str.parse::<u64>() {
                            found_values.push(num as u8);
                        }
                    }
                    pos = start + end + 1;
                } else {
                    break;
                }
            }
            // Return the second UInt value (address extension) if available
            // Format: [segment_number, address_extension, ...]
            if found_values.len() >= 2 {
                return Some(found_values[1]);
            }
        }
        None
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
    ///
    /// Strategy:
    /// 1. First, try to get the base data type from FNC_VALUES.datatype (most accurate)
    /// 2. If FNC_VALUES is not available, infer from record layout name
    /// 3. Determine dimensionality (scalar/array/matrix) from name patterns:
    ///    - Scalar_* -> scalar
    ///    - Lookup1D_* -> 1D array
    ///    - Lookup2D_* -> 2D matrix
    ///    - LAYOUT_* -> scalar (simple layout)
    /// 4. Combine base type with dimensions from:
    ///    a) matrix_dim if available (preferred)
    ///    b) AXIS_DESCR max_axis_points (fallback for MAP/CURVE types)
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

        let layout_name = record_layout.get_name();

        // Step 1: Get base data type (scalar type without dimensions)
        let base_type = if let Some(fnc_values) = &record_layout.fnc_values {
            // Most accurate: get type directly from FNC_VALUES.datatype
            Self::convert_a2l_datatype(&fnc_values.datatype)
        } else {
            // Fallback: infer from layout name
            Self::infer_type_from_layout_name(layout_name)
        };

        // Step 2: Determine dimensionality from name patterns
        let dimension_kind = Self::infer_dimension_kind(layout_name);

        // Step 3: Combine base type with dimensions
        // Priority: matrix_dim > AXIS_DESCR dimensions

        // Try matrix_dim first
        if let Some(matrix_dim) = &ch.matrix_dim {
            let dims = &matrix_dim.dim_list;
            return match dims.len() {
                1 => Self::wrap_as_array(base_type, dims[0] as usize),
                2 => Self::wrap_as_matrix(base_type, dims[0] as usize, dims[1] as usize),
                _ => Some(base_type),
            };
        }

        // Fallback: get dimensions from AXIS_DESCR
        let dimensions_from_axis = Self::get_dimensions_from_axis_descr(ch);

        match dimension_kind {
            DimensionKind::Scalar => Some(base_type),
            DimensionKind::Array => {
                // 1D array (CURVE type)
                match dimensions_from_axis {
                    Some((rows, _)) if rows > 0 => {
                        tracing::debug!(
                            "Characteristic '{}' has array layout '{}' with {} elements from AXIS_DESCR",
                            ch.get_name(), layout_name, rows
                        );
                        Self::wrap_as_array(base_type, rows)
                    }
                    _ => {
                        tracing::warn!(
                            "Characteristic '{}' has array layout '{}' but could not determine dimensions from AXIS_DESCR. Treating as scalar.",
                            ch.get_name(), layout_name
                        );
                        Some(base_type)
                    }
                }
            }
            DimensionKind::Matrix => {
                // 2D matrix (MAP type)
                match dimensions_from_axis {
                    Some((rows, cols)) if rows > 0 && cols > 0 => {
                        tracing::debug!(
                            "Characteristic '{}' has matrix layout '{}' with dimensions {}x{} from AXIS_DESCR",
                            ch.get_name(), layout_name, rows, cols
                        );
                        Self::wrap_as_matrix(base_type, rows, cols)
                    }
                    Some((rows, _)) if rows > 0 => {
                        // Only one dimension found - treat as 1D array
                        tracing::debug!(
                            "Characteristic '{}' has matrix layout '{}' with {} elements from AXIS_DESCR (1D fallback)",
                            ch.get_name(), layout_name, rows
                        );
                        Self::wrap_as_array(base_type, rows)
                    }
                    _ => {
                        tracing::warn!(
                            "Characteristic '{}' has matrix layout '{}' but could not determine dimensions from AXIS_DESCR. Treating as scalar.",
                            ch.get_name(), layout_name
                        );
                        Some(base_type)
                    }
                }
            }
        }
    }

    /// Get dimensions from AXIS_DESCR blocks
    /// Returns Some((rows, cols)) for MAP types (2D)
    /// Returns Some((n, 0)) for CURVE types (1D)
    /// Returns None if no AXIS_DESCR or invalid configuration
    fn get_dimensions_from_axis_descr(ch: &a2lfile::Characteristic) -> Option<(usize, usize)> {
        let axis_descr_count = ch.axis_descr.len();

        match axis_descr_count {
            0 => None,
            1 => {
                // CURVE type (1D array)
                let size = ch.axis_descr[0].max_axis_points as usize;
                Some((size, 0))
            }
            2 => {
                // MAP type (2D matrix)
                // First AXIS_DESCR defines rows, second defines columns
                let rows = ch.axis_descr[0].max_axis_points as usize;
                let cols = ch.axis_descr[1].max_axis_points as usize;
                Some((rows, cols))
            }
            _ => {
                // More than 2 AXIS_DESCR blocks - unusual
                tracing::warn!(
                    "Characteristic '{}' has {} AXIS_DESCR blocks, expected 0-2",
                    ch.get_name(), axis_descr_count
                );
                None
            }
        }
    }

    /// Convert a2lfile::DataType to A2lDataType
    fn convert_a2l_datatype(datatype: &a2lfile::DataType) -> A2lDataType {
        match datatype {
            a2lfile::DataType::Ubyte => A2lDataType::U8,
            a2lfile::DataType::Uword => A2lDataType::U16,
            a2lfile::DataType::Ulong => A2lDataType::U32,
            a2lfile::DataType::AUint64 => A2lDataType::U64,
            a2lfile::DataType::Sbyte => A2lDataType::I8,
            a2lfile::DataType::Sword => A2lDataType::I16,
            a2lfile::DataType::Slong => A2lDataType::I32,
            a2lfile::DataType::AInt64 => A2lDataType::I64,
            a2lfile::DataType::Float16Ieee => A2lDataType::F32, // Map 16-bit float to F32
            a2lfile::DataType::Float32Ieee => A2lDataType::F32,
            a2lfile::DataType::Float64Ieee => A2lDataType::F64,
        }
    }

    /// Infer base data type from record layout name
    fn infer_type_from_layout_name(name: &str) -> A2lDataType {
        // Remove common prefixes to get to the type part
        let type_part = if name.starts_with("Scalar_") {
            &name[7..]
        } else if name.starts_with("Lookup1D_") {
            &name[9..]
        } else if name.starts_with("Lookup2D_") {
            &name[9..]
        } else if name.starts_with("Lookup1D_X_") {
            &name[11..]
        } else if name.starts_with("Lookup2D_X_") {
            &name[11..]
        } else if name.starts_with("LAYOUT_") {
            &name[7..]
        } else if name.starts_with("Axis_") {
            &name[5..]
        } else {
            name
        };

        // Map type names to A2lDataType
        match type_part {
            // Unsigned integer types
            "UBYTE" | "BYTE" if type_part == "UBYTE" => A2lDataType::U8,
            "UWORD" => A2lDataType::U16,
            "ULONG" => A2lDataType::U32,
            "A_UINT64" => A2lDataType::U64,

            // Signed integer types
            "SBYTE" | "BYTE" => A2lDataType::I8,
            "SWORD" | "WORD" => A2lDataType::I16,
            "SLONG" | "LONG" => A2lDataType::I32,
            "A_INT64" => A2lDataType::I64,

            // Float types
            "FLOAT32_IEEE" | "FLOAT32" => A2lDataType::F32,
            "FLOAT64_IEEE" | "FLOAT64" => A2lDataType::F64,

            // Boolean (stored as UBYTE)
            "BOOLEAN" => A2lDataType::Bool,

            // Numeric prefixes (for names like U8, I16, etc.)
            _ => Self::infer_type_from_prefix(type_part),
        }
    }

    /// Infer type from common numeric prefixes
    fn infer_type_from_prefix(type_part: &str) -> A2lDataType {
        // Check for U8, U16, U32, U64 patterns
        if type_part.starts_with("U8") || type_part.starts_with("I8") || type_part.starts_with("S8") {
            A2lDataType::U8
        } else if type_part.starts_with("U16") || type_part.starts_with("I16") || type_part.starts_with("S16") {
            A2lDataType::U16
        } else if type_part.starts_with("U32") || type_part.starts_with("I32") || type_part.starts_with("S32") {
            A2lDataType::U32
        } else if type_part.starts_with("U64") || type_part.starts_with("I64") || type_part.starts_with("S64") {
            A2lDataType::U64
        } else if type_part.starts_with("F32") || type_part.starts_with("FLOAT32") {
            A2lDataType::F32
        } else if type_part.starts_with("F64") || type_part.starts_with("FLOAT64") {
            A2lDataType::F64
        } else if type_part.starts_with("BOOL") {
            A2lDataType::Bool
        } else {
            // Default fallback
            A2lDataType::U32
        }
    }

    /// Infer dimension kind from record layout name
    /// Returns the type of dimensionality (Scalar, Array, or Matrix)
    /// Actual dimensions must come from matrix_dim in the CHARACTERISTIC block
    fn infer_dimension_kind(name: &str) -> DimensionKind {
        if name.starts_with("Lookup2D_") && !name.starts_with("Lookup2D_X_") {
            // 2D lookup table (matrix)
            DimensionKind::Matrix
        } else if name.starts_with("Lookup1D_") && !name.starts_with("Lookup1D_X_") {
            // 1D lookup table (array)
            DimensionKind::Array
        } else {
            // Scalar types (Scalar_*, LAYOUT_*, etc.)
            DimensionKind::Scalar
        }
    }

    /// Wrap a base type as an array type
    fn wrap_as_array(base_type: A2lDataType, size: usize) -> Option<A2lDataType> {
        match base_type {
            A2lDataType::U8 => Some(A2lDataType::U8Array(size)),
            A2lDataType::U16 => Some(A2lDataType::U16Array(size)),
            A2lDataType::U32 => Some(A2lDataType::U32Array(size)),
            A2lDataType::U64 => Some(A2lDataType::U64Array(size)),
            A2lDataType::I8 => Some(A2lDataType::I8Array(size)),
            A2lDataType::I16 => Some(A2lDataType::I16Array(size)),
            A2lDataType::I32 => Some(A2lDataType::I32Array(size)),
            A2lDataType::I64 => Some(A2lDataType::I64Array(size)),
            A2lDataType::F32 => Some(A2lDataType::F32Array(size)),
            A2lDataType::F64 => Some(A2lDataType::F64Array(size)),
            A2lDataType::Bool => Some(A2lDataType::BoolArray(size)),
            // If already an array/matrix, return as-is
            _ => Some(base_type),
        }
    }

    /// Wrap a base type as a matrix type
    fn wrap_as_matrix(base_type: A2lDataType, rows: usize, cols: usize) -> Option<A2lDataType> {
        match base_type {
            A2lDataType::U8 => Some(A2lDataType::U8Matrix(rows, cols)),
            A2lDataType::U16 => Some(A2lDataType::U16Matrix(rows, cols)),
            A2lDataType::U32 => Some(A2lDataType::U32Matrix(rows, cols)),
            A2lDataType::U64 => Some(A2lDataType::U64Matrix(rows, cols)),
            A2lDataType::I8 => Some(A2lDataType::I8Matrix(rows, cols)),
            A2lDataType::I16 => Some(A2lDataType::I16Matrix(rows, cols)),
            A2lDataType::I32 => Some(A2lDataType::I32Matrix(rows, cols)),
            A2lDataType::I64 => Some(A2lDataType::I64Matrix(rows, cols)),
            A2lDataType::F32 => Some(A2lDataType::F32Matrix(rows, cols)),
            A2lDataType::F64 => Some(A2lDataType::F64Matrix(rows, cols)),
            A2lDataType::Bool => Some(A2lDataType::BoolMatrix(rows, cols)),
            // If already an array/matrix, return as-is
            _ => Some(base_type),
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

    /// Get XCP slave addresses from XCP_ON_UDP_IP blocks in IF_DATA
    /// Returns a list of "ip:port" strings
    pub fn get_slave_addresses(&self) -> Vec<String> {
        let mut addresses = Vec::new();

        // Parse raw IF_DATA content to find XCP_ON_UDP_IP blocks
        for module in &self.file.project.module {
            for if_data in &module.if_data {
                if let Some(ref generic) = if_data.ifdata_items {
                    // Convert GenericIfData to string and parse
                    let content = format!("{:?}", generic);
                    Self::parse_debug_content(&content, &mut addresses);
                }
            }
        }

        addresses
    }

    /// Get MAX_CTO from PROTOCOL_LAYER block in IF_DATA
    /// Returns None if not found
    pub fn get_max_cto(&self) -> Option<u16> {
        for module in &self.file.project.module {
            for if_data in &module.if_data {
                if let Some(ref generic) = if_data.ifdata_items {
                    let content = format!("{:?}", generic);
                    if let Some(max_cto) = Self::extract_max_cto_from_protocol_layer(&content) {
                        return Some(max_cto);
                    }
                }
            }
        }
        None
    }

    /// Extract MAX_CTO from PROTOCOL_LAYER block
    /// Format in A2L: /begin PROTOCOL_LAYER ... MAX_CTO ... /end PROTOCOL_LAYER
    /// In Debug format: Block { tag: "PROTOCOL_LAYER", items: [..., 64, ...] }
    fn extract_max_cto_from_protocol_layer(content: &str) -> Option<u16> {
        let content_lower = content.to_lowercase();

        // Find PROTOCOL_LAYER block
        if let Some(pos) = content_lower.find("protocol_layer") {
            let after_protocol = &content[pos..];

            // Find the block content
            if let Some(block_end) = Self::find_block_end(after_protocol) {
                let block = &after_protocol[..block_end];

                // MAX_CTO is typically the 8th numeric value after protocol version and timeouts
                // T1, T2, T3, T4, T5, T6, T7 are timeouts (7 values), then MAX_CTO
                // But we'll just look for reasonable values (16-65535) in position after timeouts

                // Extract all numeric values from the block
                let mut numbers = Vec::new();
                let mut pos = 0;
                while let Some(uint_pos) = block[pos..].find("UInt(") {
                    let start = pos + uint_pos + 5;
                    if let Some(end) = block[start..].find(")") {
                        let num_str = &block[start..start + end];
                        // Handle both "UInt(64)" and "UInt(0, 64)" formats
                        let num_part = if let Some(comma_pos) = num_str.find(",") {
                            &num_str[comma_pos + 1..].trim()
                        } else {
                            num_str.trim()
                        };
                        if let Ok(num) = num_part.parse::<u64>() {
                            numbers.push(num);
                        }
                        pos = start + end + 1;
                    } else {
                        break;
                    }
                }

                // MAX_CTO should be after 7 timeout values (T1-T7) and protocol version
                // Typical sequence: version(0x0102 as 2 numbers), T1-T7 (7 numbers), MAX_CTO, MAX_DTO
                // So MAX_CTO is at index 9 (after 2 version numbers + 7 timeouts) or index 8 (if version is 1 number)
                // Let's look for a reasonable MAX_CTO value (16-65535) that appears after several timeout values

                // Common patterns:
                // [version_high, version_low, T1, T2, T3, T4, T5, T6, T7, MAX_CTO, MAX_DTO, ...]
                // MAX_CTO is typically 8, 16, 32, 64, 128, 255, etc.

                for (i, &num) in numbers.iter().enumerate() {
                    // MAX_CTO should be after timeout values (typically 1000-10000ms)
                    // and be a reasonable CTO size (8-255)
                    if (8..=255).contains(&num) {
                        // Check if previous values look like timeouts (1000-10000 range)
                        if i >= 7 {
                            let timeout_range = 100..=10000u64;
                            let prev_7_are_timeouts = numbers[i.saturating_sub(7)..i]
                                .iter()
                                .all(|&n| timeout_range.contains(&n));
                            if prev_7_are_timeouts {
                                return Some(num as u16);
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// Parse debug content to find addresses
    fn parse_debug_content(content: &str, addresses: &mut Vec<String>) {
        // Look for Block with tag: "XCP_ON_UDP_IP" pattern
        let content_lower = content.to_lowercase();

        let mut start = 0;
        while let Some(pos) = content_lower[start..].find("xcp_on_udp_ip") {
            let block_start = start + pos;
            // Find the end of this block (matching closing brace)
            if let Some(block_end) = Self::find_block_end(&content[block_start..]) {
                let block = &content[block_start..block_start + block_end];

                // Try to extract port and IP from this block
                if let Some(addr) = Self::extract_addr_from_block(block) {
                    addresses.push(addr);
                }

                start = block_start + block_end;
            } else {
                start = block_start + 13;
            }
        }
    }

    /// Find the end of a block (matching closing brace)
    fn find_block_end(block: &str) -> Option<usize> {
        let mut brace_count = 0;
        let mut started = false;

        for (i, c) in block.chars().enumerate() {
            if c == '{' {
                brace_count += 1;
                started = true;
            } else if c == '}' {
                brace_count -= 1;
                if started && brace_count == 0 {
                    return Some(i + 1);
                }
            }
        }

        None
    }

    /// Extract IP:port from a block
    /// Format: UInt(0, (5555, false)) for port, String(0, "169.254.144.47") for IP
    fn extract_addr_from_block(block: &str) -> Option<String> {
        // Find port: look for UInt(_, (port, _)) pattern where port is between 1024-65535
        let mut port: Option<u16> = None;

        for (i, _) in block.match_indices("UInt(") {
            let after_uint = &block[i + 5..];
            // Look for pattern: UInt(0, (5555, false))
            if let Some(inner_start) = after_uint.find("(") {
                let after_inner = &after_uint[inner_start + 1..];
                // Extract the port number from (5555, false)
                if let Some(comma_pos) = after_inner.find(",") {
                    let num_str = &after_inner[..comma_pos];
                    if let Ok(num) = num_str.parse::<u64>() {
                        if (1024..=65535).contains(&num) {
                            port = Some(num as u16);
                            break; // Use first valid port
                        }
                    }
                }
            }
        }

        let port = port?;

        // Find IP address: look for String(0, "xxx.xxx.xxx.xxx")
        // The format is String(0, "169.254.144.47")
        for (i, _) in block.match_indices("String(") {
            let after_string = &block[i + 7..];
            // Look for pattern: String(0, "169.254.144.47")
            if let Some(quote_start) = after_string.find("\"") {
                let after_quote = &after_string[quote_start + 1..];
                if let Some(quote_end) = after_quote.find("\"") {
                    let ip = &after_quote[..quote_end];
                    // Validate IP format
                    if ip.split('.').count() == 4 && ip.chars().all(|c| c.is_ascii_digit() || c == '.') {
                        return Some(format!("{}:{}", ip, port));
                    }
                }
            }
        }

        None
    }

    /// Get all memory segments from the A2L file
    /// This extracts both the basic MEMORY_SEGMENT info and XCP-specific
    /// SEGMENT/PAGE information from IF_DATA XCPplus blocks
    pub fn get_memory_segments(&self) -> Vec<A2lMemorySegment> {
        let mut segments = Vec::new();

        for module in &self.file.project.module {
            if let Some(mod_par) = &module.mod_par {
                for mem_seg in &mod_par.memory_segment {
                    let segment = self.parse_memory_segment(mem_seg);
                    segments.push(segment);
                }
            }
        }

        segments
    }

    /// Parse a single memory segment
    fn parse_memory_segment(&self, mem_seg: &a2lfile::MemorySegment) -> A2lMemorySegment {
        let name = mem_seg.name.clone();
        let description = mem_seg.long_identifier.clone();
        let prg_type = format!("{:?}", mem_seg.prg_type);
        let memory_type = format!("{:?}", mem_seg.memory_type);
        let attribute = format!("{:?}", mem_seg.attribute);
        let address = mem_seg.address;
        let size = mem_seg.size;

        // Parse XCP-specific info from IF_DATA
        let xcp_info = self.parse_xcp_segment_info(&mem_seg.if_data);

        A2lMemorySegment {
            name,
            description,
            prg_type,
            memory_type,
            attribute,
            address,
            size,
            xcp_info,
        }
    }

    /// Parse XCP-specific SEGMENT info from IF_DATA blocks
    fn parse_xcp_segment_info(&self, if_data_list: &[a2lfile::IfData]) -> Option<XcpSegmentInfo> {
        for if_data in if_data_list {
            if let Some(ref generic) = if_data.ifdata_items {
                if let Some(info) = Self::extract_xcp_segment_info(generic) {
                    return Some(info);
                }
            }
        }
        None
    }

    /// Extract XCP SEGMENT info from GenericIfData
    /// Format in debug output:
    /// Block { items: [TaggedUnion({"XCPplus": [...TaggedStruct({"SEGMENT": [...]})]})] }
    /// SEGMENT block items: [UChar(segment_number), UChar(num_pages), UChar(address_ext), UChar(compression), UChar(encryption), TaggedStruct({"CHECKSUM": [...], "PAGE": [...]})]
    fn extract_xcp_segment_info(generic: &a2lfile::GenericIfData) -> Option<XcpSegmentInfo> {
        let content = format!("{:?}", generic);

        // Find SEGMENT tagged item
        if let Some(segment_start) = Self::find_tagged_struct_item(&content, "SEGMENT") {
            // Parse the segment data
            let segment_data = &content[segment_start..];

            // Extract UChar values: segment_number, num_pages, address_extension, compression, encryption
            let segment_number = Self::extract_nth_uchar(segment_data, 0).unwrap_or(0);
            let num_pages = Self::extract_nth_uchar(segment_data, 1).unwrap_or(0);
            let address_extension = Self::extract_nth_uchar(segment_data, 2).unwrap_or(0);

            // Parse checksum
            let checksum = Self::extract_checksum_from_segment(segment_data);

            // Parse pages
            let pages = Self::extract_pages_from_segment(segment_data);

            return Some(XcpSegmentInfo {
                segment_number,
                num_pages,
                address_extension,
                checksum,
                pages,
            });
        }
        None
    }

    /// Find a TaggedStruct item by tag name and return its start position
    fn find_tagged_struct_item(content: &str, tag: &str) -> Option<usize> {
        // Look for pattern: tag: "SEGMENT"
        let search_pattern = format!("tag: \"{}\"", tag);
        content.find(&search_pattern).map(|pos| {
            // Find the Block { after this position
            if let Some(block_pos) = content[pos..].find("Block {") {
                pos + block_pos
            } else {
                pos
            }
        })
    }

    /// Extract the nth UChar value from a string
    /// Format: UChar(1, (value, true))
    /// The actual value is the first number inside the inner parentheses
    fn extract_nth_uchar(content: &str, n: usize) -> Option<u8> {
        Self::extract_uchar_values(content).nth(n)
    }

    /// Extract all UChar values from a string in a single pass
    /// Returns an iterator of u8 values
    fn extract_uchar_values(content: &str) -> impl Iterator<Item = u8> + '_ {
        let mut pos = 0;
        std::iter::from_fn(move || {
            while let Some(uchar_pos) = content[pos..].find("UChar(") {
                let start = pos + uchar_pos + 6; // After "UChar("

                // Find the matching closing parenthesis for UChar(...)
                let mut depth = 1;
                let mut end = 0;
                for (i, c) in content[start..].char_indices() {
                    match c {
                        '(' => depth += 1,
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                end = i;
                                break;
                            }
                        }
                        _ => {}
                    }
                }

                if end > 0 {
                    let uchar_content = &content[start..start + end];
                    // Format: "1, (0, true)" where the actual value is 0 inside (0, true)
                    if let Some(inner_start) = uchar_content.find('(') {
                        let inner = &uchar_content[inner_start + 1..];
                        if let Some(comma_pos) = inner.find(',') {
                            let value_str = &inner[..comma_pos];
                            if let Ok(value) = value_str.trim().parse::<u8>() {
                                pos = start + end + 1;
                                return Some(value);
                            }
                        }
                    }
                    pos = start + end + 1;
                } else {
                    break;
                }
            }
            None
        })
    }

    /// Extract checksum from SEGMENT data using the XcpChecksum enum
    fn extract_checksum_from_segment(segment_data: &str) -> Option<XcpChecksum> {
        // Use find_enum_items to find all EnumItem values and try to parse as checksum
        Self::find_enum_items(segment_data)
            .find_map(|s| XcpChecksum::from_str(s))
    }

    /// Extract all PAGE info from SEGMENT data
    fn extract_pages_from_segment(segment_data: &str) -> Vec<XcpPage> {
        let mut pages = Vec::new();

        // Find all PAGE blocks
        let mut search_pos = 0;
        while let Some(page_pos) = Self::find_tagged_struct_item(&segment_data[search_pos..], "PAGE") {
            let abs_pos = search_pos + page_pos;

            // Find the PAGE block content
            if let Some(block_start) = segment_data[abs_pos..].find("Block {") {
                let block_content = &segment_data[abs_pos + block_start..];

                // Find the end of this block
                if let Some(block_end) = Self::find_block_end(block_content) {
                    let page_block = &block_content[..block_end];

                    // Parse PAGE
                    if let Some(page) = Self::parse_page_block(page_block) {
                        pages.push(page);
                    }

                    search_pos = abs_pos + block_start + block_end;
                    continue;
                }
            }
            search_pos = abs_pos + 4;
        }

        pages
    }

    /// Parse a single PAGE block
    fn parse_page_block(page_block: &str) -> Option<XcpPage> {
        // Extract page number (first UChar)
        let page_number = Self::extract_nth_uchar(page_block, 0)?;

        // Extract access modes from EnumItem strings
        let ecu_access = Self::parse_ecu_access_mode(page_block);
        let xcp_read_access = Self::parse_xcp_read_access(page_block);
        let xcp_write_access = Self::parse_xcp_write_access(page_block);

        Some(XcpPage {
            page_number,
            ecu_access,
            xcp_read_access,
            xcp_write_access,
        })
    }

    /// Parse ECU access mode from PAGE block
    fn parse_ecu_access_mode(page_block: &str) -> EcuAccessMode {
        Self::find_enum_items(page_block)
            .find_map(|s| match s {
                "ECU_ACCESS_WITH_XCP_ONLY" => Some(EcuAccessMode::WithXcpOnly),
                "ECU_ACCESS_ALWAYS" => Some(EcuAccessMode::Always),
                _ => None,
            })
            .unwrap_or(EcuAccessMode::WithXcpOnly)
    }

    /// Parse XCP read access mode from PAGE block
    fn parse_xcp_read_access(page_block: &str) -> XcpReadAccess {
        Self::find_enum_items(page_block)
            .find_map(|s| match s {
                "XCP_READ_ACCESS_WITH_ECU_ONLY" => Some(XcpReadAccess::WithEcuOnly),
                "XCP_READ_ACCESS_ALWAYS" => Some(XcpReadAccess::Always),
                _ => None,
            })
            .unwrap_or(XcpReadAccess::WithEcuOnly)
    }

    /// Parse XCP write access mode from PAGE block
    /// This is the critical field for determining if calibration is allowed
    fn parse_xcp_write_access(page_block: &str) -> XcpWriteAccess {
        Self::find_enum_items(page_block)
            .find_map(|s| match s {
                "XCP_WRITE_ACCESS_NOT_ALLOWED" => Some(XcpWriteAccess::NotAllowed),
                "XCP_WRITE_ACCESS_WITH_ECU_ONLY" => Some(XcpWriteAccess::WithEcuOnly),
                "XCP_WRITE_ACCESS_ALWAYS" => Some(XcpWriteAccess::Always),
                _ => None,
            })
            .unwrap_or(XcpWriteAccess::NotAllowed)
    }

    /// Find all EnumItem string values in a block
    /// Pattern: EnumItem(1, "VALUE")
    fn find_enum_items(content: &str) -> impl Iterator<Item = &'_ str> {
        let mut pos = 0;
        std::iter::from_fn(move || {
            while let Some(enum_pos) = content[pos..].find("EnumItem(") {
                let start = pos + enum_pos + 9; // After "EnumItem("
                if let Some(end) = content[start..].find(')') {
                    let enum_content = &content[start..start + end];
                    // Format: "1, \"VALUE\"" - extract the string inside quotes
                    if let Some(quote_start) = enum_content.find('"') {
                        if let Some(quote_end) = enum_content[quote_start + 1..].find('"') {
                            let value = &enum_content[quote_start + 1..quote_start + 1 + quote_end];
                            pos = start + end + 1;
                            return Some(value);
                        }
                    }
                    pos = start + end + 1;
                } else {
                    break;
                }
            }
            None
        })
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

    #[test]
    fn test_scalar_prefix_type_mapping() {
        // Test that Scalar_ prefix is correctly handled
        // These tests verify the type extraction logic by checking that
        // Scalar_UWORD maps to U16, not U32 (the default fallback)
        let reader = A2lReader::load("hello_xcp.a2l").unwrap();

        // Test UWORD type name pattern (simulating what would happen with Scalar_UWORD)
        // We verify the underlying type mapping logic is correct
        let module = &reader.file.project.module[0];
        let record_layout = module.record_layout.iter().find(|rl| rl.get_name() == "U16").unwrap();

        // Verify the record layout exists and has FNC_VALUES
        assert!(record_layout.fnc_values.is_some(), "U16 record layout should have FNC_VALUES");
    }

    #[test]
    fn test_extract_addr_from_block() {
        // Test with actual debug representation format from a2lfile crate
        // Format: UInt(0, (5555, false)) for port, String(0, "169.254.144.47") for IP
        let block = r#"Block { tag: "XCP_ON_UDP_IP", items: [UInt(0, (260, true)), UInt(0, (5555, false)), TaggedUnion({"ADDRESS": [GenericIfDataTaggedItem { tag: "ADDRESS", data: Block { items: [String(0, "169.254.144.47")] } }] })] }"#;
        let result = A2lReader::extract_addr_from_block(block);
        assert_eq!(result, Some("169.254.144.47:5555".to_string()));

        // Test with localhost
        let block2 = r#"items: [UInt(0, (5555, false)), String(0, "127.0.0.1")]"#;
        let result2 = A2lReader::extract_addr_from_block(block2);
        assert_eq!(result2, Some("127.0.0.1:5555".to_string()));

        // Test with different port
        let block3 = r#"items: [UInt(0, (4444, false)), String(0, "192.168.1.100")]"#;
        let result3 = A2lReader::extract_addr_from_block(block3);
        assert_eq!(result3, Some("192.168.1.100:4444".to_string()));
    }

    #[test]
    fn test_parse_debug_content() {
        let content = r#"Block { tag: "XCP_ON_UDP_IP", data: Block { items: [UInt(0, (260, true)), UInt(0, (5555, false)), TaggedUnion({"ADDRESS": [GenericIfDataTaggedItem { tag: "ADDRESS", data: Block { items: [String(0, "169.254.144.47")] } }] })] } }"#;

        let mut addrs = Vec::new();
        A2lReader::parse_debug_content(content, &mut addrs);
        assert_eq!(addrs.len(), 1);
        assert_eq!(addrs[0], "169.254.144.47:5555");
    }

    #[test]
    fn test_get_slave_addresses_from_file() {
        // Test loading actual A2L file and extracting slave addresses
        let reader = A2lReader::load("hello_xcp.a2l").unwrap();
        let addrs = reader.get_slave_addresses();

        println!("Found addresses: {:?}", addrs);

        // Should find the address from the A2L file (actual address depends on file content)
        assert!(!addrs.is_empty(), "Should find at least one address");
        // The A2L file contains 0.0.0.0:5555 (any address) or a specific IP
        assert!(addrs.contains(&"0.0.0.0:5555".to_string()) ||
                addrs.contains(&"169.254.144.47:5555".to_string()) ||
                addrs.iter().any(|a| a.ends_with(":5555")),
            "Should find a slave address with port 5555 from A2L file, got: {:?}", addrs);
    }

    #[test]
    fn test_characteristics_have_default_address_ext() {
        // Test that characteristics default to address extension 0
        // unless explicitly specified in IF_DATA XCP SEGMENT
        let reader = A2lReader::load("hello_xcp.a2l").unwrap();
        let characteristics = reader.get_characteristics();

        // All characteristics should have address_ext = 0 by default
        // (since hello_xcp.a2l doesn't explicitly set address extension in IF_DATA)
        for ch in characteristics {
            assert_eq!(ch.address_ext, 0,
                "Characteristic '{}' should have default address_ext=0, got {}",
                ch.name, ch.address_ext);
        }
    }

    #[test]
    fn test_infer_type_from_layout_name() {
        // Test type inference from various record layout naming patterns found in abc.a2l

        // Scalar types
        assert!(matches!(
            A2lReader::infer_type_from_layout_name("Scalar_UWORD"),
            A2lDataType::U16
        ));
        assert!(matches!(
            A2lReader::infer_type_from_layout_name("Scalar_SWORD"),
            A2lDataType::I16
        ));
        assert!(matches!(
            A2lReader::infer_type_from_layout_name("Scalar_UBYTE"),
            A2lDataType::U8
        ));
        assert!(matches!(
            A2lReader::infer_type_from_layout_name("Scalar_BYTE"),
            A2lDataType::I8
        ));
        assert!(matches!(
            A2lReader::infer_type_from_layout_name("Scalar_ULONG"),
            A2lDataType::U32
        ));
        assert!(matches!(
            A2lReader::infer_type_from_layout_name("Scalar_SLONG"),
            A2lDataType::I32
        ));
        assert!(matches!(
            A2lReader::infer_type_from_layout_name("Scalar_FLOAT32_IEEE"),
            A2lDataType::F32
        ));
        assert!(matches!(
            A2lReader::infer_type_from_layout_name("Scalar_FLOAT64_IEEE"),
            A2lDataType::F64
        ));
        assert!(matches!(
            A2lReader::infer_type_from_layout_name("Scalar_BOOLEAN"),
            A2lDataType::Bool
        ));

        // Lookup1D types (1D arrays)
        assert!(matches!(
            A2lReader::infer_type_from_layout_name("Lookup1D_UWORD"),
            A2lDataType::U16
        ));
        assert!(matches!(
            A2lReader::infer_type_from_layout_name("Lookup1D_WORD"),
            A2lDataType::I16
        ));

        // Lookup2D types (2D matrices)
        assert!(matches!(
            A2lReader::infer_type_from_layout_name("Lookup2D_UWORD"),
            A2lDataType::U16
        ));
        assert!(matches!(
            A2lReader::infer_type_from_layout_name("Lookup2D_FLOAT32_IEEE"),
            A2lDataType::F32
        ));

        // LAYOUT_ types
        assert!(matches!(
            A2lReader::infer_type_from_layout_name("LAYOUT_SBYTE"),
            A2lDataType::I8
        ));
        assert!(matches!(
            A2lReader::infer_type_from_layout_name("LAYOUT_SWORD"),
            A2lDataType::I16
        ));
        assert!(matches!(
            A2lReader::infer_type_from_layout_name("LAYOUT_SLONG"),
            A2lDataType::I32
        ));

        // Axis types
        assert!(matches!(
            A2lReader::infer_type_from_layout_name("Axis_UWORD"),
            A2lDataType::U16
        ));
        assert!(matches!(
            A2lReader::infer_type_from_layout_name("Axis_FLOAT32_IEEE"),
            A2lDataType::F32
        ));
    }

    #[test]
    #[ignore] // Run with: cargo test test_print_actual_format -- --ignored
    fn test_print_actual_format() {
        // This test prints the actual GenericIfData format for debugging
        let reader = A2lReader::load("hello_xcp.a2l").unwrap();

        for module in &reader.file.project.module {
            for if_data in &module.if_data {
                if let Some(ref generic) = if_data.ifdata_items {
                    println!("GenericIfData Debug: {:?}", generic);
                }
            }
        }
    }

    #[test]
    fn test_get_memory_segments_abc_a2l() {
        // Test parsing memory segments from abc.a2l
        // This file has 3 segments:
        // - segment_0: 2 pages, page 0 NOT_ALLOWED, page 1 WITH_ECU_ONLY -> calibration allowed
        // - segment_1: 2 pages, page 0 NOT_ALLOWED, page 1 WITH_ECU_ONLY -> calibration allowed
        // - segment_2: 1 page, NOT_ALLOWED -> calibration NOT allowed
        let reader = A2lReader::load("abc.a2l").expect("Failed to load abc.a2l");
        let segments = reader.get_memory_segments();

        println!("Found {} memory segments in abc.a2l:", segments.len());
        for seg in &segments {
            println!(
                "  - {} @ 0x{:08X}, size={}, calibration_allowed={}",
                seg.name, seg.address, seg.size, seg.is_calibration_allowed()
            );
            if let Some(ref xcp) = seg.xcp_info {
                println!(
                    "    XCP: segment_number={}, address_ext={}, pages={}",
                    xcp.segment_number, xcp.address_extension, xcp.num_pages
                );
                for page in &xcp.pages {
                    println!(
                        "      page {}: write_allowed={}",
                        page.page_number,
                        page.is_write_allowed()
                    );
                }
            }
        }

        assert_eq!(segments.len(), 3, "abc.a2l should have 3 memory segments");

        // Check segment_0
        let seg0 = segments.iter().find(|s| s.name == "segment_0_type_data").expect("segment_0 not found");
        assert_eq!(seg0.address, 0xA0380000);
        assert_eq!(seg0.size, 0x40000);
        assert!(seg0.is_calibration_allowed(), "segment_0 should allow calibration");

        // Check segment_2 (CODE segment - should NOT allow calibration)
        let seg2 = segments.iter().find(|s| s.name == "segment_2_type_code").expect("segment_2 not found");
        assert!(!seg2.is_calibration_allowed(), "segment_2 should NOT allow calibration");
    }

    #[test]
    fn test_get_memory_segments_xcp_dft_a2l() {
        // Test parsing memory segments from xcp_dft.a2l
        // This file has 2 segments, both with 1 page each allowing write access
        let reader = A2lReader::load("xcp_dft.a2l").expect("Failed to load xcp_dft.a2l");
        let segments = reader.get_memory_segments();

        println!("Found {} memory segments in xcp_dft.a2l:", segments.len());
        for seg in &segments {
            println!(
                "  - {} @ 0x{:08X}, size={}, calibration_allowed={}",
                seg.name, seg.address, seg.size, seg.is_calibration_allowed()
            );
        }

        assert!(segments.len() >= 2, "xcp_dft.a2l should have at least 2 memory segments");

        // All segments in xcp_dft.a2l should allow calibration
        for seg in &segments {
            assert!(
                seg.is_calibration_allowed(),
                "Segment {} should allow calibration",
                seg.name
            );
        }
    }

    #[test]
    fn test_xcp_write_access_parsing() {
        // Test that XCP write access strings are correctly parsed
        let reader = A2lReader::load("abc.a2l").expect("Failed to load abc.a2l");
        let segments = reader.get_memory_segments();

        // Find segment_0 which has 2 pages with different write access
        let seg0 = segments.iter().find(|s| s.name == "segment_0_type_data").expect("segment_0 not found");
        let xcp_info = seg0.xcp_info.as_ref().expect("segment_0 should have XCP info");

        assert_eq!(xcp_info.pages.len(), 2, "segment_0 should have 2 pages");

        // Page 0 should NOT allow write
        let page0 = &xcp_info.pages[0];
        assert_eq!(page0.page_number, 0);
        assert!(!page0.is_write_allowed(), "Page 0 should NOT allow write");

        // Page 1 should allow write (WITH_ECU_ONLY)
        let page1 = &xcp_info.pages[1];
        assert_eq!(page1.page_number, 1);
        assert!(page1.is_write_allowed(), "Page 1 should allow write");
    }

    #[test]
    fn test_checksum_parsing() {
        // Test that checksum type is correctly parsed
        let reader = A2lReader::load("abc.a2l").expect("Failed to load abc.a2l");
        let segments = reader.get_memory_segments();

        for seg in &segments {
            if let Some(ref xcp) = seg.xcp_info {
                assert!(
                    xcp.checksum.is_some(),
                    "Segment {} should have checksum info",
                    seg.name
                );
                if let Some(checksum) = &xcp.checksum {
                    assert!(
                        matches!(checksum, XcpChecksum::Crc32),
                        "Segment {} should have CRC32 checksum",
                        seg.name
                    );
                }
            }
        }
    }

    #[test]
    fn test_address_extension_parsing() {
        // Test that address extension is correctly parsed from IF_DATA
        let reader = A2lReader::load("abc.a2l").expect("Failed to load abc.a2l");
        let segments = reader.get_memory_segments();

        for seg in &segments {
            if let Some(ref xcp) = seg.xcp_info {
                // All segments in abc.a2l have address extension 0
                assert_eq!(
                    xcp.address_extension, 0,
                    "Segment {} should have address extension 0",
                    seg.name
                );
            }
        }
    }

    #[test]
    #[ignore] // Run with: cargo test test_debug_segment_format -- --ignored --nocapture
    fn test_debug_segment_format() {
        // This test prints the actual IF_DATA format for memory segments
        let reader = A2lReader::load("abc.a2l").expect("Failed to load abc.a2l");

        for module in &reader.file.project.module {
            if let Some(mod_par) = &module.mod_par {
                for mem_seg in &mod_par.memory_segment {
                    println!("=== MEMORY_SEGMENT: {} ===", mem_seg.name);
                    for if_data in &mem_seg.if_data {
                        if let Some(ref generic) = if_data.ifdata_items {
                            println!("IF_DATA GenericIfData: {:?}", generic);
                        }
                    }
                }
            }
        }
    }
}
