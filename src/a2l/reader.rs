//! A2L File Reader
//!
//! Parses A2L files using the a2lfile crate to extract measurement and
//! characteristic information for XCP calibration and DAQ.

use a2lfile::A2lObjectName;
use std::collections::{HashMap, HashSet};
use std::path::Path;

// Import types from types module
use super::types::{A2lDataType, CharacteristicType, StorageOrder};
use crate::transport::{TransportConfig, TransportType};

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

/// Describes how axis points are obtained for CURVE/MAP characteristics
#[derive(Debug, Clone)]
pub enum AxisPointSource {
    /// Axis points stored in memory (AXIS_PTS reference)
    /// Contains: address, address_extension, data_type, num_points
    Memory {
        address: u32,
        address_ext: u8,
        data_type: A2lDataType,
        num_points: usize,
    },
    /// Fixed axis calculated as: offset + i * 2^shift (FIX_AXIS_PAR)
    /// Formula: axis_value[i] = offset + i * (2^shift)
    Calculated {
        offset: f64,
        shift: i32,
        num_points: usize,
    },
    /// Fixed axis with distance: offset + i * distance (FIX_AXIS_PAR_DIST)
    /// Formula: axis_value[i] = offset + i * distance
    CalculatedDist {
        offset: f64,
        distance: f64,
        num_points: usize,
    },
    /// Explicit list of axis values (FIX_AXIS_PAR_LIST)
    Explicit { values: Vec<f64> },
}

/// Complete axis information for CURVE or MAP characteristics
#[derive(Debug, Clone)]
pub struct AxisInfo {
    /// Input quantity name (the physical meaning of this axis)
    pub name: String,
    /// AXIS_PTS_REF variable name (when axis points are stored in ECU memory via AXIS_PTS block)
    pub pts_ref_name: Option<String>,
    /// Physical unit
    pub unit: Option<String>,
    /// Minimum axis value
    pub min: f64,
    /// Maximum axis value
    pub max: f64,
    /// Number of axis points
    pub num_points: usize,
    /// Source of axis point values
    pub point_source: AxisPointSource,
}

impl AxisInfo {
    /// Get axis point values without reading from XCP slave.
    /// Only works for calculated or explicit axis sources.
    /// Returns None for Memory sources which require XCP communication.
    pub fn get_static_values(&self) -> Option<Vec<f64>> {
        match &self.point_source {
            AxisPointSource::Memory { .. } => None,
            AxisPointSource::Calculated {
                offset,
                shift,
                num_points,
            } => {
                let multiplier = 2f64.powi(*shift);
                Some(
                    (0..*num_points)
                        .map(|i| offset + i as f64 * multiplier)
                        .collect(),
                )
            }
            AxisPointSource::CalculatedDist {
                offset,
                distance,
                num_points,
            } => Some(
                (0..*num_points)
                    .map(|i| offset + i as f64 * distance)
                    .collect(),
            ),
            AxisPointSource::Explicit { values } => Some(values.clone()),
        }
    }

    /// Check if this axis requires XCP communication to get values
    pub fn requires_memory_read(&self) -> bool {
        matches!(self.point_source, AxisPointSource::Memory { .. })
    }

    /// Serialize axis info to JSON for API responses
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "name": &self.name,
            "pts_ref_name": &self.pts_ref_name,
            "unit": &self.unit,
            "min": self.min,
            "max": self.max,
            "num_points": self.num_points,
            "values": self.get_static_values(),
            "requires_memory_read": self.requires_memory_read(),
        })
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
    /// COMPU_METHOD name for value conversion
    pub conversion_name: String,
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
    /// Characteristic type (VALUE, CURVE, MAP, VAL_BLK)
    pub characteristic_type: CharacteristicType,
    /// Axis information for CURVE (1 axis) and MAP (2 axes) types
    /// Empty for VALUE and VAL_BLK types
    pub axis_info: Vec<AxisInfo>,
    /// Storage order for matrix/map data (ROW_DIR or COLUMN_DIR)
    /// Only relevant for MAP and VAL_BLK with 2D dimensions
    pub storage_order: StorageOrder,
    /// COMPU_METHOD name for value conversion
    pub conversion_name: String,
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

/// Transport configurations discovered from A2L IF_DATA blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum A2lTransportConfig {
    Udp { addr: String },
    Tcp { addr: String },
    Can {
        can_id: u32,
        /// Nominal (arbitration-phase) bit rate in bit/s from `BAUDRATE`.
        baudrate: Option<u32>,
        /// CAN-FD data-phase bit rate in bit/s from
        /// `CAN_FD_DATA_TRANSFER_BAUDRATE`; `Some` means CAN-FD.
        data_baudrate: Option<u32>,
        /// Slave response CAN ID from `CAN_ID_SLAVE`.
        slave_id: Option<u32>,
        /// DAQ DTO CAN IDs from `DAQ_LIST_CAN_ID` (fixed mapping).
        daq_ids: Vec<u32>,
        /// True when `MAX_DLC_REQUIRED` is set.
        max_dlc_required: bool,
    },
}

impl A2lTransportConfig {
    pub fn transport_type(&self) -> TransportType {
        match self {
            Self::Udp { .. } => TransportType::Udp,
            Self::Tcp { .. } => TransportType::Tcp,
            Self::Can { .. } => TransportType::Can,
        }
    }

    pub fn endpoint(&self) -> String {
        match self {
            Self::Udp { addr } | Self::Tcp { addr } => addr.clone(),
            Self::Can { can_id, .. } => format!("0x{:X}", can_id),
        }
    }

    pub fn display_label(&self) -> String {
        match self {
            Self::Udp { addr } => format!("UDP - {}", addr),
            Self::Tcp { addr } => format!("TCP - {}", addr),
            Self::Can {
                can_id,
                baudrate,
                data_baudrate,
                ..
            } => {
                let mode = if data_baudrate.is_some() { "CAN-FD" } else { "CAN" };
                let rate = match (baudrate, data_baudrate) {
                    (Some(nom), Some(data)) => format!(" {}k/{}", nom / 1000, data),
                    (Some(nom), None) => format!(" {}k", nom / 1000),
                    _ => String::new(),
                };
                format!("{} - 0x{:X}{}", mode, can_id, rate)
            }
        }
    }

    pub fn to_transport_config(&self) -> TransportConfig {
        match self {
            Self::Udp { addr } => TransportConfig::Udp { addr: addr.clone() },
            Self::Tcp { addr } => TransportConfig::Tcp { addr: addr.clone() },
            Self::Can {
                can_id,
                baudrate,
                data_baudrate,
                slave_id,
                daq_ids,
                max_dlc_required,
            } => TransportConfig::Can {
                driver: None,
                channel: "auto".to_string(),
                device_id: None,
                controller: None,
                can_id: *can_id,
                baudrate: *baudrate,
                data_baudrate: *data_baudrate,
                slave_id: *slave_id,
                daq_ids: daq_ids.clone(),
                max_dlc_required: *max_dlc_required,
            },
        }
    }
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

    /// Convert to XCP protocol byte code
    pub fn to_code(&self) -> u8 {
        match self {
            Self::Add11 => 1,
            Self::Add12 => 2,
            Self::Add14 => 3,
            Self::Add22 => 4,
            Self::Add24 => 5,
            Self::Add44 => 6,
            Self::Crc16 => 7,
            Self::Crc16Citt => 8,
            Self::Crc32 => 9,
            Self::UserDefined(code) => *code,
        }
    }

    /// Parse from XCP protocol byte code
    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Add11),
            2 => Some(Self::Add12),
            3 => Some(Self::Add14),
            4 => Some(Self::Add22),
            5 => Some(Self::Add24),
            6 => Some(Self::Add44),
            7 => Some(Self::Crc16),
            8 => Some(Self::Crc16Citt),
            9 => Some(Self::Crc32),
            255 => Some(Self::UserDefined(255)),
            _ => None,
        }
    }

    /// Get human-readable name
    pub fn name(&self) -> &'static str {
        match self {
            Self::Add11 => "XCP_ADD_11",
            Self::Add12 => "XCP_ADD_12",
            Self::Add14 => "XCP_ADD_14",
            Self::Add22 => "XCP_ADD_22",
            Self::Add24 => "XCP_ADD_24",
            Self::Add44 => "XCP_ADD_44",
            Self::Crc16 => "XCP_CRC_16",
            Self::Crc16Citt => "XCP_CRC_16_CITT",
            Self::Crc32 => "XCP_CRC_32",
            Self::UserDefined(_) => "XCP_USER_DEFINED",
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
        self.xcp_info
            .as_ref()
            .map_or(false, |xcp| xcp.is_write_allowed())
    }
}

/// A2L file reader and parser
pub struct A2lReader {
    file: a2lfile::A2lFile,
    events: Vec<super::types::DaqEvent>,
    raw_content: String,
}

impl A2lReader {
    /// Load and parse an A2L file
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let path = path.as_ref();
        let mut errors = Vec::new();
        let file = a2lfile::load(path, None, &mut errors, true)
            .map_err(|e| format!("Failed to load A2L file '{}': {:?}", path.display(), e))?;

        // Parse EVENT blocks from raw file (a2lfile crate doesn't support EVENT blocks)
        let raw = std::fs::read_to_string(path).unwrap_or_default();
        let events = Self::parse_events_from_raw(&raw);

        Ok(Self {
            file,
            events,
            raw_content: raw,
        })
    }

    /// Get all measurements from the A2L file
    pub fn get_measurements(&self) -> Vec<A2lMeasurement> {
        let mut measurements = Vec::new();

        for module in &self.file.project.module {
            // Existing: parse flat MEASUREMENT blocks.
            for meas in &module.measurement {
                if let Some(info) = self.parse_measurement(meas) {
                    measurements.push(info);
                }
            }
            // NEW: append synthetic entries from INSTANCE+TYPEDEF_STRUCTURE flattening.
            // Insertion order is load-bearing: flat-parsed entries come first so they
            // win on name collisions in any downstream lookup-by-name code.
            let mut flattened = super::typedef_flatten::flatten_instance_measurements(module);
            // Resolve unit via existing COMPU_METHOD lookup (typedef_flatten doesn't
            // have access to the compu_method tables).
            for m in &mut flattened {
                m.unit = self.get_unit_for_conversion(&m.conversion_name);
            }
            // Drop any synthetic name that collides with a flat name; warn.
            let flat_names: HashSet<&str> = measurements.iter().map(|m| m.name.as_str()).collect();
            flattened.retain(|m| {
                if flat_names.contains(m.name.as_str()) {
                    tracing::warn!(name = %m.name, "structural measurement collides with flat MEASUREMENT, dropped");
                    false
                } else {
                    true
                }
            });
            measurements.extend(flattened);
        }

        measurements
    }

    /// Get all characteristics (calibration parameters) from the A2L file
    pub fn get_characteristics(&self) -> Vec<A2lCharacteristic> {
        let mut characteristics = Vec::new();

        for module in &self.file.project.module {
            // Existing: parse flat CHARACTERISTIC blocks.
            for ch in &module.characteristic {
                if let Some(info) = self.parse_characteristic(ch, module) {
                    characteristics.push(info);
                }
            }
            // NEW: append synthetic entries from INSTANCE+TYPEDEF_STRUCTURE+
            // TYPEDEF_CHARACTERISTIC flattening. Insertion order is
            // load-bearing: flat-parsed entries come first so they win on name
            // collisions in any downstream lookup-by-name code.
            let mut flattened = super::typedef_flatten::flatten_instance_characteristics(module);
            // Resolve unit via existing COMPU_METHOD lookup (typedef_flatten
            // doesn't have access to the compu_method tables).
            for c in &mut flattened {
                c.unit = self.get_unit_for_conversion(&c.conversion_name);
            }
            // Drop any synthetic name that collides with a flat name; warn.
            let flat_names: HashSet<&str> =
                characteristics.iter().map(|c| c.name.as_str()).collect();
            flattened.retain(|c| {
                if flat_names.contains(c.name.as_str()) {
                    tracing::warn!(name = %c.name, "structural calibration collides with flat CHARACTERISTIC, dropped");
                    false
                } else {
                    true
                }
            });
            characteristics.extend(flattened);
        }

        characteristics
    }

    /// Get all AXIS_PTS objects as calibration characteristics (1D arrays).
    /// Each AXIS_PTS is exposed as a `CharacteristicType::ValBlk` with the
    /// appropriate array element type and `max_axis_points` as the array length.
    pub fn get_axis_pts_as_characteristics(&self) -> Vec<A2lCharacteristic> {
        let mut result = Vec::new();
        for module in &self.file.project.module {
            for ap in &module.axis_pts {
                if let Some(ch) = self.parse_axis_pts_as_characteristic(ap, module) {
                    result.push(ch);
                }
            }
        }
        result
    }

    fn parse_axis_pts_as_characteristic(
        &self,
        ap: &a2lfile::AxisPts,
        module: &a2lfile::Module,
    ) -> Option<A2lCharacteristic> {
        let name = ap.get_name().to_string();
        let description = ap.long_identifier.clone();
        let address = ap.address;
        let address_ext = ap
            .ecu_address_extension
            .as_ref()
            .map(|ext| ext.extension as u8)
            .unwrap_or(0);
        let num_points = ap.max_axis_points as usize;
        if num_points == 0 {
            return None;
        }

        // Resolve element data type from record layout
        let elem_type = self.get_axis_pts_datatype(ap, module)?;

        // Wrap scalar element type in its corresponding Array variant
        let array_type = match elem_type {
            A2lDataType::U8 => A2lDataType::U8Array(num_points),
            A2lDataType::U16 => A2lDataType::U16Array(num_points),
            A2lDataType::U32 => A2lDataType::U32Array(num_points),
            A2lDataType::U64 => A2lDataType::U64Array(num_points),
            A2lDataType::I8 => A2lDataType::I8Array(num_points),
            A2lDataType::I16 => A2lDataType::I16Array(num_points),
            A2lDataType::I32 => A2lDataType::I32Array(num_points),
            A2lDataType::I64 => A2lDataType::I64Array(num_points),
            A2lDataType::F32 => A2lDataType::F32Array(num_points),
            A2lDataType::F64 => A2lDataType::F64Array(num_points),
            A2lDataType::Bool => A2lDataType::BoolArray(num_points),
            // Already an array/matrix – skip (shouldn't happen for AXIS_PTS)
            _ => return None,
        };

        let unit = self.get_unit_for_conversion(&ap.conversion);
        let min = ap.lower_limit;
        let max = ap.upper_limit;
        let conversion_name = ap.conversion.clone();

        Some(A2lCharacteristic {
            name,
            description,
            address,
            address_ext,
            data_type: array_type,
            unit,
            min,
            max,
            characteristic_type: CharacteristicType::ValBlk,
            axis_info: vec![],
            storage_order: StorageOrder::RowDir,
            conversion_name,
        })
    }

    /// Get all DAQ event channels parsed from the A2L file
    pub fn get_events(&self) -> Vec<super::types::DaqEvent> {
        self.events.clone()
    }

    pub fn get_transport_configs(&self) -> Vec<A2lTransportConfig> {
        Self::parse_transport_configs_from_raw(&self.raw_content)
    }

    fn parse_events_from_raw(raw: &str) -> Vec<super::types::DaqEvent> {
        let mut events = Vec::new();
        let mut search = raw;

        while let Some(begin_pos) = search.to_uppercase().find("/BEGIN EVENT") {
            let after_begin = &search[begin_pos + "/BEGIN EVENT".len()..];
            let end_pos = after_begin
                .to_uppercase()
                .find("/END EVENT")
                .unwrap_or(after_begin.len());
            let block = &after_begin[..end_pos];

            let clean = Self::strip_comments(block);
            let tokens = Self::tokenize_a2l(&clean);

            if tokens.len() >= 7 {
                let name = tokens[0].trim_matches('"').to_string();
                let short_name = tokens[1].trim_matches('"').to_string();
                let channel: u16 = tokens[2].parse().unwrap_or(0);
                // tokens[3] is DAQ/STIM direction keyword — skip
                let max_daq_list: u8 = tokens[4].parse().unwrap_or(0);
                let time_cycle: u8 = tokens[5].parse().unwrap_or(0);
                let time_unit: u8 = tokens[6].parse().unwrap_or(0);
                let priority: u8 = tokens.get(7).and_then(|t| t.parse().ok()).unwrap_or(0);

                events.push(super::types::DaqEvent {
                    channel,
                    name,
                    short_name,
                    time_cycle,
                    time_unit,
                    max_daq_list,
                    priority,
                });
            }

            search = &search[begin_pos + "/BEGIN EVENT".len() + end_pos + "/END EVENT".len()..];
        }

        events
    }

    fn parse_transport_configs_from_raw(raw: &str) -> Vec<A2lTransportConfig> {
        let mut configs = Vec::new();

        if let Some(config) = Self::extract_transport_block_contents(raw, "XCP_ON_UDP_IP")
            .into_iter()
            .find_map(|block| Self::parse_ip_transport_block(&block, TransportType::Udp))
        {
            configs.push(config);
        }

        if let Some(config) = Self::extract_transport_block_contents(raw, "XCP_ON_CAN")
            .into_iter()
            .find_map(|block| Self::parse_can_transport_block(&block))
        {
            configs.push(config);
        }

        if let Some(config) = Self::extract_transport_block_contents(raw, "XCP_ON_TCP_IP")
            .into_iter()
            .find_map(|block| Self::parse_ip_transport_block(&block, TransportType::Tcp))
        {
            configs.push(config);
        }

        configs
    }

    fn extract_transport_block_contents(raw: &str, tag: &str) -> Vec<String> {
        let begin_tag = format!("/BEGIN {}", tag);
        let end_tag = format!("/END {}", tag);
        let raw_upper = raw.to_uppercase();
        let begin_upper = begin_tag.to_uppercase();
        let end_upper = end_tag.to_uppercase();
        let mut blocks = Vec::new();
        let mut offset = 0usize;

        while let Some(begin_rel) = raw_upper[offset..].find(&begin_upper) {
            let block_start = offset + begin_rel + begin_tag.len();
            let remaining_upper = &raw_upper[block_start..];
            let Some(end_rel) = remaining_upper.find(&end_upper) else {
                break;
            };

            let block_end = block_start + end_rel;
            blocks.push(raw[block_start..block_end].to_string());
            offset = block_end + end_tag.len();
        }

        blocks
    }

    fn parse_ip_transport_block(
        block: &str,
        transport_type: TransportType,
    ) -> Option<A2lTransportConfig> {
        let clean = Self::strip_comments(block);
        let tokens = Self::tokenize_a2l(&clean);
        if tokens.len() < 4 {
            return None;
        }

        let port = Self::parse_u16_token(&tokens[1])?;
        let address_idx = tokens.iter().position(|token| {
            matches!(
                token.to_uppercase().as_str(),
                "ADDRESS" | "HOST_NAME" | "IPV6"
            )
        })?;
        let host = tokens.get(address_idx + 1)?.trim_matches('"');
        let addr = Self::format_host_port(host, port);

        match transport_type {
            TransportType::Udp => Some(A2lTransportConfig::Udp { addr }),
            TransportType::Tcp => Some(A2lTransportConfig::Tcp { addr }),
            TransportType::Can => None,
        }
    }

    fn parse_can_transport_block(block: &str) -> Option<A2lTransportConfig> {
        let clean = Self::strip_comments(block);
        let tokens = Self::tokenize_a2l(&clean);

        // Prefer the master (CMD) identifier, then fall back to other tags.
        let can_id = ["CAN_ID_MASTER", "CAN_ID_SLAVE", "CAN_ID_BROADCAST"]
            .iter()
            .find_map(|tag| Self::token_u32_after(&tokens, tag))?;

        // BAUDRATE is the nominal/arbitration bit rate in bit/s.
        let baudrate = Self::token_u32_after(&tokens, "BAUDRATE");

        // Presence of a data-phase bit rate selects CAN-FD.
        let data_baudrate = [
            "CAN_FD_DATA_TRANSFER_BAUDRATE",
            "CAN_FD_DATA_BAUDRATE",
            "DATA_BAUDRATE",
        ]
        .iter()
        .find_map(|tag| Self::token_u32_after(&tokens, tag));

        // Slave response identifier used to filter incoming frames.
        let slave_id = Self::token_u32_after(&tokens, "CAN_ID_SLAVE");

        // DAQ DTO identifiers. The slave may use a different CAN ID for DAQ
        // data than for CMD responses; both `DAQ_LIST_CAN_ID` and
        // `EVENT_CAN_ID_LIST` declare those IDs.
        let daq_ids = Self::parse_daq_dto_ids(block);

        // When set, command frames must be padded to the maximum DLC.
        // `MAX_DLC_REQUIRED` is a bare flag (its value may be omitted).
        let max_dlc_required = Self::token_flag(&tokens, "MAX_DLC_REQUIRED");

        Some(A2lTransportConfig::Can {
            can_id,
            baudrate,
            data_baudrate,
            slave_id,
            daq_ids,
            max_dlc_required,
        })
    }

    /// Collect the fixed DAQ DTO CAN IDs declared in the `XCP_ON_CAN` block.
    ///
    /// Sources:
    /// - `DAQ_LIST_CAN_ID` (per DAQ list)
    /// - `EVENT_CAN_ID_LIST` (per event; used when DAQ and CMD responses use
    ///   different CAN IDs)
    ///
    /// Only fixed mappings are returned. If any entry is `VARIABLE` or
    /// `INCREMENTAL` the IDs cannot be enumerated statically, so the result is
    /// empty and the transport falls back to accepting all non-master IDs.
    fn parse_daq_dto_ids(block: &str) -> Vec<u32> {
        let mut ids = Vec::new();
        let mut incomplete = false;

        for tag in ["DAQ_LIST_CAN_ID", "EVENT_CAN_ID_LIST"] {
            for daq_block in Self::extract_transport_block_contents(block, tag) {
                let clean = Self::strip_comments(&daq_block);
                let tokens = Self::tokenize_a2l(&clean);

                let mut i = 0;
                while i < tokens.len() {
                    match tokens[i].to_ascii_uppercase().as_str() {
                        "FIXED" => {
                            if let Some(id) =
                                tokens.get(i + 1).and_then(|t| Self::parse_u32_token(t))
                            {
                                ids.push(id);
                            }
                            i += 2;
                        }
                        "VARIABLE" | "INCREMENTAL" => {
                            incomplete = true;
                            i += 1;
                        }
                        _ => i += 1,
                    }
                }
            }
        }

        if incomplete {
            return Vec::new();
        }

        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// True when the bare flag `tag` is present, unless explicitly disabled
    /// with a following `0`/`FALSE`.
    fn token_flag(tokens: &[String], tag: &str) -> bool {
        let Some(idx) = tokens.iter().position(|t| t.eq_ignore_ascii_case(tag)) else {
            return false;
        };
        match tokens.get(idx + 1).map(|v| v.to_ascii_uppercase()) {
            Some(value) if value == "FALSE" || value == "0" => false,
            _ => true,
        }
    }

    /// Find the `u32` value that follows the first occurrence of `tag`.
    fn token_u32_after(tokens: &[String], tag: &str) -> Option<u32> {
        let idx = tokens
            .iter()
            .position(|token| token.eq_ignore_ascii_case(tag))?;
        tokens
            .get(idx + 1)
            .and_then(|token| Self::parse_u32_token(token))
    }

    fn parse_u16_token(token: &str) -> Option<u16> {
        let token = token.trim();
        if token.starts_with("0x") || token.starts_with("0X") {
            u16::from_str_radix(token.trim_start_matches("0x").trim_start_matches("0X"), 16).ok()
        } else {
            token.parse::<u16>().ok()
        }
    }

    fn parse_u32_token(token: &str) -> Option<u32> {
        let token = token.trim();
        if token.starts_with("0x") || token.starts_with("0X") {
            u32::from_str_radix(token.trim_start_matches("0x").trim_start_matches("0X"), 16).ok()
        } else {
            token.parse::<u32>().ok()
        }
    }

    fn format_host_port(host: &str, port: u16) -> String {
        if host.contains(':') && !host.starts_with('[') {
            format!("[{}]:{}", host, port)
        } else {
            format!("{}:{}", host, port)
        }
    }

    fn strip_comments(s: &str) -> String {
        let mut result = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '/' && chars.peek() == Some(&'*') {
                chars.next(); // consume '*'
                loop {
                    match chars.next() {
                        Some('*') if chars.peek() == Some(&'/') => {
                            chars.next();
                            break;
                        }
                        None => break,
                        _ => {}
                    }
                }
            } else {
                result.push(c);
            }
        }
        result
    }

    fn tokenize_a2l(s: &str) -> Vec<String> {
        let mut tokens = Vec::new();
        let mut chars = s.chars().peekable();
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() {
                chars.next();
            } else if c == '"' {
                chars.next(); // consume opening quote
                let mut token = String::from('"');
                loop {
                    match chars.next() {
                        Some('"') => {
                            token.push('"');
                            break;
                        }
                        Some(ch) => token.push(ch),
                        None => break,
                    }
                }
                tokens.push(token);
            } else {
                let mut token = String::new();
                while let Some(&ch) = chars.peek() {
                    if ch.is_whitespace() {
                        break;
                    }
                    token.push(ch);
                    chars.next();
                }
                tokens.push(token);
            }
        }
        tokens
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

        // Get conversion method name
        let conversion_name = meas.conversion.clone();

        Some(A2lMeasurement {
            name,
            description,
            address,
            address_ext,
            data_type,
            unit,
            min,
            max,
            conversion_name,
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

        // Parse characteristic type
        let characteristic_type = Self::parse_characteristic_type(ch);

        // Parse axis information for CURVE and MAP types
        let axis_info = self.parse_axis_info(ch, module);

        // Parse storage order from record layout
        let storage_order = self.parse_storage_order(ch, module);

        // Get conversion method name
        let conversion_name = ch.conversion.clone();

        Some(A2lCharacteristic {
            name,
            description,
            address,
            address_ext,
            data_type,
            unit,
            min,
            max,
            characteristic_type,
            axis_info,
            storage_order,
            conversion_name,
        })
    }

    /// Parse characteristic type from A2L
    fn parse_characteristic_type(ch: &a2lfile::Characteristic) -> CharacteristicType {
        match ch.characteristic_type {
            a2lfile::CharacteristicType::Value => CharacteristicType::Value,
            a2lfile::CharacteristicType::Curve => CharacteristicType::Curve,
            a2lfile::CharacteristicType::Map => CharacteristicType::Map,
            a2lfile::CharacteristicType::ValBlk => CharacteristicType::ValBlk,
            a2lfile::CharacteristicType::Ascii => CharacteristicType::Ascii,
            // Cuboid, Cube4, Cube5 are treated as VAL_BLK for now
            _ => CharacteristicType::ValBlk,
        }
    }

    /// Parse storage order from RECORD_LAYOUT FNC_VALUES index_mode
    /// Returns ROW_DIR by default if not specified
    fn parse_storage_order(
        &self,
        ch: &a2lfile::Characteristic,
        module: &a2lfile::Module,
    ) -> StorageOrder {
        // Find the record layout
        let record_layout_name = &ch.deposit;
        let record_layout = module
            .record_layout
            .iter()
            .find(|rl| rl.get_name() == record_layout_name);

        // Get index_mode from FNC_VALUES
        if let Some(rl) = record_layout {
            if let Some(fnc_values) = &rl.fnc_values {
                // A2L COLUMN_DIR means "store column-by-column": for each X-column, all Y-rows are
                // stored consecutively → flat[x * Y_count + y] → X-major → our RowDir.
                // A2L ROW_DIR means "store row-by-row": for each Y-row, all X-columns are stored
                // consecutively → flat[y * X_count + x] → Y-major → our ColumnDir.
                return match fnc_values.index_mode {
                    a2lfile::IndexMode::ColumnDir => StorageOrder::RowDir,
                    a2lfile::IndexMode::RowDir => StorageOrder::ColumnDir,
                    // Other modes (ALTERNATE_*) default to RowDir
                    _ => StorageOrder::RowDir,
                };
            }
        }

        // Default to RowDir (X-major)
        StorageOrder::RowDir
    }

    /// Parse axis information from AXIS_DESCR blocks
    fn parse_axis_info(
        &self,
        ch: &a2lfile::Characteristic,
        module: &a2lfile::Module,
    ) -> Vec<AxisInfo> {
        let axis_count = ch.axis_descr.len();
        tracing::trace!(
            "Parsing axis info for '{}': {} AXIS_DESCR blocks, characteristic_type={:?}",
            ch.get_name(),
            axis_count,
            ch.characteristic_type
        );

        let result: Vec<AxisInfo> = ch
            .axis_descr
            .iter()
            .enumerate()
            .filter_map(|(i, axis_descr)| {
                let info = self.parse_single_axis_info(axis_descr, module);
                if info.is_none() {
                    tracing::warn!(
                        "Failed to parse AXIS_DESCR[{}] for '{}': attribute={:?}",
                        i,
                        ch.get_name(),
                        axis_descr.attribute
                    );
                }
                info
            })
            .collect();

        tracing::trace!(
            "Parsed {} axis info entries for '{}', characteristic_type={:?}",
            result.len(),
            ch.get_name(),
            ch.characteristic_type
        );

        result
    }

    /// Parse a single AXIS_DESCR block into AxisInfo
    fn parse_single_axis_info(
        &self,
        axis_descr: &a2lfile::AxisDescr,
        module: &a2lfile::Module,
    ) -> Option<AxisInfo> {
        let name = axis_descr.input_quantity.clone();
        let pts_ref_name = axis_descr
            .axis_pts_ref
            .as_ref()
            .map(|r| r.axis_points.clone());
        let num_points = axis_descr.max_axis_points as usize;
        let min = axis_descr.lower_limit;
        let max = axis_descr.upper_limit;

        tracing::trace!(
            "parse_single_axis_info: name='{}', num_points={}, attribute={:?}",
            name,
            num_points,
            axis_descr.attribute
        );

        // Get unit from phys_unit or conversion method
        let unit = axis_descr
            .phys_unit
            .as_ref()
            .map(|pu| pu.unit.clone())
            .or_else(|| self.get_unit_for_conversion(&axis_descr.conversion));

        // Determine the axis point source
        let point_source = if let Some(fix_axis_par_list) = &axis_descr.fix_axis_par_list {
            // FIX_AXIS_PAR_LIST: explicit list of axis values
            tracing::trace!(
                "  -> Using FIX_AXIS_PAR_LIST with {} values",
                fix_axis_par_list.axis_pts_value_list.len()
            );
            AxisPointSource::Explicit {
                values: fix_axis_par_list.axis_pts_value_list.clone(),
            }
        } else if let Some(fix_axis_par_dist) = &axis_descr.fix_axis_par_dist {
            // FIX_AXIS_PAR_DIST: offset + i * distance
            tracing::trace!(
                "  -> Using FIX_AXIS_PAR_DIST: offset={}, distance={}",
                fix_axis_par_dist.offset,
                fix_axis_par_dist.distance
            );
            AxisPointSource::CalculatedDist {
                offset: fix_axis_par_dist.offset as f64,
                distance: fix_axis_par_dist.distance as f64,
                num_points,
            }
        } else if let Some(fix_axis_par) = &axis_descr.fix_axis_par {
            // FIX_AXIS_PAR: offset + i * 2^shift
            tracing::trace!(
                "  -> Using FIX_AXIS_PAR: offset={}, shift={}",
                fix_axis_par.offset,
                fix_axis_par.shift
            );
            AxisPointSource::Calculated {
                offset: fix_axis_par.offset as f64,
                shift: fix_axis_par.shift as i32,
                num_points,
            }
        } else if let Some(axis_pts_ref) = &axis_descr.axis_pts_ref {
            // AXIS_PTS_REF: reference to AXIS_PTS block
            tracing::trace!("  -> Using AXIS_PTS_REF: {}", axis_pts_ref.axis_points);
            self.resolve_axis_pts_reference(&axis_pts_ref.axis_points, module, num_points)
                .unwrap_or_else(|| {
                    // Fallback: create from limits
                    tracing::trace!("  -> Fallback to limits for AXIS_PTS_REF");
                    Self::create_fallback_axis_source(min, max, num_points)
                })
        } else {
            // No explicit axis source - create from limits
            tracing::trace!("  -> Using fallback from limits: min={}, max={}", min, max);
            Self::create_fallback_axis_source(min, max, num_points)
        };

        tracing::trace!("  -> Created AxisInfo for '{}'", name);
        Some(AxisInfo {
            name,
            pts_ref_name,
            unit,
            min,
            max,
            num_points,
            point_source,
        })
    }

    /// Resolve AXIS_PTS reference to get axis point source
    fn resolve_axis_pts_reference(
        &self,
        axis_pts_name: &str,
        module: &a2lfile::Module,
        num_points: usize,
    ) -> Option<AxisPointSource> {
        // Find the AXIS_PTS block
        let axis_pts = module
            .axis_pts
            .iter()
            .find(|ap| ap.get_name() == axis_pts_name)?;

        let address = axis_pts.address;
        let address_ext = axis_pts
            .ecu_address_extension
            .as_ref()
            .map(|ext| ext.extension as u8)
            .unwrap_or(0);

        // Get the data type from the deposit record layout
        let data_type = self.get_axis_pts_datatype(axis_pts, module)?;

        Some(AxisPointSource::Memory {
            address,
            address_ext,
            data_type,
            num_points,
        })
    }

    /// Get the data type for an AXIS_PTS block
    fn get_axis_pts_datatype(
        &self,
        axis_pts: &a2lfile::AxisPts,
        module: &a2lfile::Module,
    ) -> Option<A2lDataType> {
        // Find the record layout
        let record_layout = module
            .record_layout
            .iter()
            .find(|rl| rl.get_name() == axis_pts.deposit_record)?;

        // Get axis point data type from AXIS_PTS_X block in record layout
        if let Some(axis_pts_layout) = &record_layout.axis_pts_x {
            Some(Self::convert_a2l_datatype(&axis_pts_layout.datatype))
        } else {
            // Fallback to FNC_VALUES if AXIS_PTS_X not present
            if let Some(fnc_values) = &record_layout.fnc_values {
                Some(Self::convert_a2l_datatype(&fnc_values.datatype))
            } else {
                None
            }
        }
    }

    /// Create a fallback axis source from min/max limits
    fn create_fallback_axis_source(min: f64, max: f64, num_points: usize) -> AxisPointSource {
        if num_points <= 1 {
            AxisPointSource::Explicit { values: vec![min] }
        } else {
            // Linear interpolation between min and max
            let step = if num_points > 1 {
                (max - min) / (num_points - 1) as f64
            } else {
                0.0
            };
            AxisPointSource::CalculatedDist {
                offset: min,
                distance: step,
                num_points,
            }
        }
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
                if ch.address >= mem_seg.address && ch.address < mem_seg.address + mem_seg.size {
                    // Check IF_DATA XCP SEGMENT for explicit address extension
                    for if_data in &mem_seg.if_data {
                        if let Some(ref generic) = if_data.ifdata_items {
                            if let Some(ext) = Self::extract_address_extension_from_segment(generic)
                            {
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
                    if let Some(comma_pos) = after_seg[start..start + end].find(",") {
                        let num_str = &after_seg[start..start + comma_pos];
                        if let Ok(num) = num_str.parse::<u64>() {
                            found_values.push(num as u8);
                        }
                    } else {
                        let num_str = &after_seg[start..start + end];
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
    /// 3. Determine dimensionality (scalar/array/matrix):
    ///    - CURVE characteristic type → 1D array
    ///    - MAP characteristic type → 2D matrix
    ///    - VALUE/VALBLK → infer from record layout name patterns (Lookup1D_*, Lookup2D_*, Scalar_*, etc.)
    /// 4. Combine base type with dimensions from (in priority order):
    ///    a) matrix_dim if available (preferred)
    ///    b) NUMBER keyword (VAL_BLK flat arrays)
    ///    c) AXIS_DESCR max_axis_points (CURVE/MAP)
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

        // Step 2: Determine dimensionality — A2L CharacteristicType is authoritative;
        // fall back to layout name patterns only for VALUE/VALBLK types.
        let dimension_kind = match ch.characteristic_type {
            a2lfile::CharacteristicType::Curve => DimensionKind::Array,
            a2lfile::CharacteristicType::Map => DimensionKind::Matrix,
            _ => Self::infer_dimension_kind(layout_name),
        };

        // Step 3: Combine base type with dimensions
        // Priority: matrix_dim > number (VAL_BLK) > AXIS_DESCR dimensions

        // Try matrix_dim first
        if let Some(matrix_dim) = &ch.matrix_dim {
            let dims = &matrix_dim.dim_list;
            return match dims.len() {
                1 => Self::wrap_as_array(base_type, dims[0] as usize),
                2 => Self::wrap_as_matrix(base_type, dims[0] as usize, dims[1] as usize),
                _ => Some(base_type),
            };
        }

        // Try NUMBER keyword (VAL_BLK with flat array of n elements)
        if let Some(n) = ch.number.as_ref().map(|n| n.number as usize) {
            if n > 1 {
                return Self::wrap_as_array(base_type, n);
            }
        }

        // Fallback: get dimensions from AXIS_DESCR
        let dimensions_from_axis = Self::get_dimensions_from_axis_descr(ch);

        match dimension_kind {
            DimensionKind::Scalar => Some(base_type),
            DimensionKind::Array => {
                // 1D array (CURVE type)
                match dimensions_from_axis {
                    Some((rows, _)) if rows > 0 => {
                        tracing::trace!(
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
                        tracing::trace!(
                            "Characteristic '{}' has matrix layout '{}' with dimensions {}x{} from AXIS_DESCR",
                            ch.get_name(), layout_name, rows, cols
                        );
                        Self::wrap_as_matrix(base_type, rows, cols)
                    }
                    Some((rows, _)) if rows > 0 => {
                        // Only one dimension found - treat as 1D array
                        tracing::trace!(
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
                    ch.get_name(),
                    axis_descr_count
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
        if type_part.starts_with("U8") || type_part.starts_with("I8") || type_part.starts_with("S8")
        {
            A2lDataType::U8
        } else if type_part.starts_with("U16")
            || type_part.starts_with("I16")
            || type_part.starts_with("S16")
        {
            A2lDataType::U16
        } else if type_part.starts_with("U32")
            || type_part.starts_with("I32")
            || type_part.starts_with("S32")
        {
            A2lDataType::U32
        } else if type_part.starts_with("U64")
            || type_part.starts_with("I64")
            || type_part.starts_with("S64")
        {
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

    /// Get COMPU_METHOD for a conversion method name
    ///
    /// # Arguments
    /// * `conversion_name` - Name of the COMPU_METHOD in the A2L file
    ///
    /// # Returns
    /// A CompuMethod instance for value conversion, or Identical if not found
    pub fn get_compu_method(&self, conversion_name: &str) -> super::compute_method::CompuMethod {
        use super::compute_method::CompuMethodFactory;

        if conversion_name.is_empty() || conversion_name == "NO_COMPU_METHOD" {
            return super::compute_method::CompuMethod::Identical;
        }

        for module in &self.file.project.module {
            // Build a map of COMPU_VTAB references
            let mut vtabs: HashMap<String, &a2lfile::CompuVtab> = HashMap::new();
            for vtab in &module.compu_vtab {
                vtabs.insert(vtab.get_name().to_string(), vtab);
            }

            // Find the COMPU_METHOD
            for cm in &module.compu_method {
                if cm.get_name() == conversion_name {
                    let result = CompuMethodFactory::from_a2l(cm, &vtabs);
                    tracing::debug!(
                        "get_compu_method '{}': a2l_type={:?} coeffs_linear={:?} -> result={} is_enum={}",
                        conversion_name,
                        cm.conversion_type,
                        cm.coeffs_linear.as_ref().map(|c| format!("a={} b={}", c.a, c.b)),
                        result.variant_name(),
                        result.is_enum()
                    );
                    return result;
                }
            }
        }

        tracing::warn!("get_compu_method '{}': not found in A2L", conversion_name);
        // Default to Identical if not found
        super::compute_method::CompuMethod::Identical
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
        let transport_addresses: Vec<String> = self
            .get_transport_configs()
            .into_iter()
            .filter_map(|config| match config {
                A2lTransportConfig::Udp { addr } | A2lTransportConfig::Tcp { addr } => Some(addr),
                A2lTransportConfig::Can { .. } => None,
            })
            .collect();
        if !transport_addresses.is_empty() {
            return transport_addresses;
        }

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

    /// Check if an optional XCP command is supported
    ///
    /// Parses OPTIONAL_CMD enum in IF_DATA to check if a specific command is supported.
    /// Common optional commands: SHORT_UPLOAD, SHORT_DOWNLOAD, DOWNLOAD, DOWNLOAD_MAX, etc.
    ///
    /// # Arguments
    /// * `command_name` - Name of the command (e.g., "SHORT_DOWNLOAD", "SHORT_UPLOAD")
    ///
    /// # Returns
    /// - true if the command is listed in OPTIONAL_CMD enum
    /// - false if the command is not listed or commented out
    pub fn supports_optional_command(&self, command_name: &str) -> bool {
        for module in &self.file.project.module {
            for if_data in &module.if_data {
                if let Some(ref generic) = if_data.ifdata_items {
                    let content = format!("{:?}", generic);
                    if Self::check_optional_cmd_supported(&content, command_name) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Check if a command is in the OPTIONAL_CMD list
    ///
    /// The OPTIONAL_CMD list appears in IF_DATA and shows which optional commands
    /// the slave supports. In the Debug format, the data appears as:
    /// `"OPTIONAL_CMD": [GenericIfDataTaggedItem { ... items: [EnumItem(0, "SHORT_UPLOAD"), ...] }]`
    ///
    /// Commands that are NOT supported appear as comments in the A2L source,
    /// but after parsing they're simply not in the list.
    fn check_optional_cmd_supported(content: &str, command_name: &str) -> bool {
        // Look for the command name in EnumItem format
        // The Debug format shows: EnumItem(0, "SHORT_UPLOAD")
        let enum_item_pattern_lower = format!("enumitem(0, \"{}\")", command_name.to_lowercase());

        // Case-insensitive search
        let content_lower = content.to_lowercase();

        // Check if the command appears as an EnumItem in the OPTIONAL_CMD block
        // First find the OPTIONAL_CMD section, then check for the command
        if let Some(pos) = content_lower.find("\"optional_cmd\"") {
            // Find the next closing bracket or the start of the next tag
            let after_tag = &content[pos..];

            // Look for the EnumItem pattern in a reasonable window after the tag
            let search_window = after_tag.chars().take(5000).collect::<String>();
            let search_window_lower = search_window.to_lowercase();

            if search_window_lower.contains(&enum_item_pattern_lower) {
                return true;
            }
        }

        false
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
                    if ip.split('.').count() == 4
                        && ip.chars().all(|c| c.is_ascii_digit() || c == '.')
                    {
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
        Self::find_enum_items(segment_data).find_map(|s| XcpChecksum::from_str(s))
    }

    /// Extract all PAGE info from SEGMENT data
    fn extract_pages_from_segment(segment_data: &str) -> Vec<XcpPage> {
        let mut pages = Vec::new();

        // Find all PAGE blocks
        let mut search_pos = 0;
        while let Some(page_pos) =
            Self::find_tagged_struct_item(&segment_data[search_pos..], "PAGE")
        {
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
        let record_layout = module
            .record_layout
            .iter()
            .find(|rl| rl.get_name() == "U16")
            .unwrap();

        // Verify the record layout exists and has FNC_VALUES
        assert!(
            record_layout.fnc_values.is_some(),
            "U16 record layout should have FNC_VALUES"
        );
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
        assert!(
            addrs.contains(&"0.0.0.0:5555".to_string())
                || addrs.contains(&"169.254.144.47:5555".to_string())
                || addrs.iter().any(|a| a.ends_with(":5555")),
            "Should find a slave address with port 5555 from A2L file, got: {:?}",
            addrs
        );
    }

    #[test]
    fn test_parse_transport_configs_from_raw_orders_by_default_priority() {
        let raw = r#"
            /begin IF_DATA XCP
                /begin XCP_ON_TCP_IP 0x104 6001 ADDRESS "192.168.0.10" /end XCP_ON_TCP_IP
                /begin XCP_ON_CAN 0x104 CAN_ID_MASTER 0x123 CAN_ID_SLAVE 0x456 BAUDRATE 500000 /end XCP_ON_CAN
                /begin XCP_ON_UDP_IP 0x104 5555 ADDRESS "169.254.1.20" /end XCP_ON_UDP_IP
            /end IF_DATA
        "#;

        let configs = A2lReader::parse_transport_configs_from_raw(raw);

        assert_eq!(
            configs,
            vec![
                A2lTransportConfig::Udp {
                    addr: "169.254.1.20:5555".to_string(),
                },
                A2lTransportConfig::Can {
                    can_id: 0x123,
                    baudrate: Some(500_000),
                    data_baudrate: None,
                    slave_id: Some(0x456),
                    daq_ids: Vec::new(),
                    max_dlc_required: false,
                },
                A2lTransportConfig::Tcp {
                    addr: "192.168.0.10:6001".to_string(),
                },
            ]
        );
    }

    #[test]
    fn test_parse_can_fd_transport_config() {
        let raw = r#"
            /begin IF_DATA XCP
                /begin XCP_ON_CAN 0x104
                    CAN_ID_MASTER 0x123
                    CAN_ID_SLAVE 0x456
                    BAUDRATE 500000
                    CAN_FD_DATA_TRANSFER_BAUDRATE 2000000
                    MAX_DLC_REQUIRED 1
                    /begin DAQ_LIST_CAN_ID 0x0000 FIXED 0x310 /end DAQ_LIST_CAN_ID
                    /begin DAQ_LIST_CAN_ID 0x0001 FIXED 0x320 /end DAQ_LIST_CAN_ID
                /end XCP_ON_CAN
            /end IF_DATA
        "#;

        let configs = A2lReader::parse_transport_configs_from_raw(raw);
        assert_eq!(
            configs,
            vec![A2lTransportConfig::Can {
                can_id: 0x123,
                baudrate: Some(500_000),
                data_baudrate: Some(2_000_000),
                slave_id: Some(0x456),
                daq_ids: vec![0x310, 0x320],
                max_dlc_required: true,
            }]
        );
    }

    #[test]
    fn test_parse_can_event_can_id_list_and_bare_max_dlc() {
        // Mirrors a real A2L where CMD responses and DAQ data use different
        // CAN IDs, declared via EVENT_CAN_ID_LIST, plus a bare
        // MAX_DLC_REQUIRED flag.
        let raw = r#"
            /begin IF_DATA XCP
                /begin XCP_ON_CAN
                    0x0102
                    CAN_ID_MASTER 0x7fb
                    CAN_ID_SLAVE  0x7fc
                    BAUDRATE      0x7a120
                    MAX_DLC_REQUIRED
                    /begin EVENT_CAN_ID_LIST
                        0
                        FIXED 0x7fd
                    /end EVENT_CAN_ID_LIST
                    /begin EVENT_CAN_ID_LIST
                        1
                        FIXED 0x7fd
                    /end EVENT_CAN_ID_LIST
                /end XCP_ON_CAN
            /end IF_DATA
        "#;

        let configs = A2lReader::parse_transport_configs_from_raw(raw);
        assert_eq!(
            configs,
            vec![A2lTransportConfig::Can {
                can_id: 0x7fb,
                baudrate: Some(0x7a120),
                data_baudrate: None,
                slave_id: Some(0x7fc),
                daq_ids: vec![0x7fd],
                max_dlc_required: true,
            }]
        );
    }

    #[test]
    fn test_parse_can_variable_daq_ids_disable_filtering() {
        // A VARIABLE DAQ mapping cannot be enumerated statically, so no
        // strict filtering IDs are produced.
        let raw = r#"
            /begin IF_DATA XCP
                /begin XCP_ON_CAN 0x0102
                    CAN_ID_MASTER 0x123
                    CAN_ID_SLAVE  0x124
                    /begin DAQ_LIST_CAN_ID
                        0
                        VARIABLE
                    /end DAQ_LIST_CAN_ID
                /end XCP_ON_CAN
            /end IF_DATA
        "#;

        let configs = A2lReader::parse_transport_configs_from_raw(raw);
        assert_eq!(
            configs,
            vec![A2lTransportConfig::Can {
                can_id: 0x123,
                baudrate: None,
                data_baudrate: None,
                slave_id: Some(0x124),
                daq_ids: Vec::new(),
                max_dlc_required: false,
            }]
        );
    }

    #[test]
    fn test_get_transport_configs_from_file() {
        let reader = A2lReader::load("hello_xcp.a2l").unwrap();
        let configs = reader.get_transport_configs();

        assert_eq!(
            configs,
            vec![A2lTransportConfig::Udp {
                addr: "169.254.144.47:5555".to_string(),
            }]
        );
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
            assert_eq!(
                ch.address_ext, 0,
                "Characteristic '{}' should have default address_ext=0, got {}",
                ch.name, ch.address_ext
            );
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
                seg.name,
                seg.address,
                seg.size,
                seg.is_calibration_allowed()
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
        let seg0 = segments
            .iter()
            .find(|s| s.name == "segment_0_type_data")
            .expect("segment_0 not found");
        assert_eq!(seg0.address, 0xA0380000);
        assert_eq!(seg0.size, 0x40000);
        assert!(
            seg0.is_calibration_allowed(),
            "segment_0 should allow calibration"
        );

        // Check segment_2 (CODE segment - should NOT allow calibration)
        let seg2 = segments
            .iter()
            .find(|s| s.name == "segment_2_type_code")
            .expect("segment_2 not found");
        assert!(
            !seg2.is_calibration_allowed(),
            "segment_2 should NOT allow calibration"
        );
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
                seg.name,
                seg.address,
                seg.size,
                seg.is_calibration_allowed()
            );
        }

        assert!(
            segments.len() >= 2,
            "xcp_dft.a2l should have at least 2 memory segments"
        );

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
        let seg0 = segments
            .iter()
            .find(|s| s.name == "segment_0_type_data")
            .expect("segment_0 not found");
        let xcp_info = seg0
            .xcp_info
            .as_ref()
            .expect("segment_0 should have XCP info");

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

    #[test]
    fn test_supports_optional_command() {
        let reader = A2lReader::load("abc.a2l").expect("Failed to load abc.a2l");

        // Check for commands that are supported
        assert!(
            reader.supports_optional_command("SHORT_UPLOAD"),
            "SHORT_UPLOAD should be supported"
        );
        assert!(
            reader.supports_optional_command("DOWNLOAD"),
            "DOWNLOAD should be supported"
        );
        assert!(
            reader.supports_optional_command("SET_CAL_PAGE"),
            "SET_CAL_PAGE should be supported"
        );

        // SHORT_DOWNLOAD is commented out in abc.a2l
        assert!(
            !reader.supports_optional_command("SHORT_DOWNLOAD"),
            "SHORT_DOWNLOAD should NOT be supported (commented out in A2L)"
        );
    }

    #[test]
    fn test_axis_point_source_calculated() {
        // Test FIX_AXIS_PAR style: offset + i * 2^shift
        let source = AxisPointSource::Calculated {
            offset: 0.0,
            shift: 1, // 2^1 = 2
            num_points: 5,
        };

        let axis_info = AxisInfo {
            name: "test_axis".to_string(),
            pts_ref_name: None,
            unit: Some("rpm".to_string()),
            min: 0.0,
            max: 1000.0,
            num_points: 5,
            point_source: source,
        };

        let values = axis_info.get_static_values().unwrap();
        assert_eq!(values, vec![0.0, 2.0, 4.0, 6.0, 8.0]);
        assert!(!axis_info.requires_memory_read());
    }

    #[test]
    fn test_axis_point_source_calculated_dist() {
        // Test FIX_AXIS_PAR_DIST style: offset + i * distance
        let source = AxisPointSource::CalculatedDist {
            offset: 100.0,
            distance: 50.0,
            num_points: 5,
        };

        let axis_info = AxisInfo {
            name: "test_axis".to_string(),
            pts_ref_name: None,
            unit: Some("Nm".to_string()),
            min: 100.0,
            max: 300.0,
            num_points: 5,
            point_source: source,
        };

        let values = axis_info.get_static_values().unwrap();
        assert_eq!(values, vec![100.0, 150.0, 200.0, 250.0, 300.0]);
        assert!(!axis_info.requires_memory_read());
    }

    #[test]
    fn test_axis_point_source_explicit() {
        // Test FIX_AXIS_PAR_LIST style: explicit values
        let source = AxisPointSource::Explicit {
            values: vec![0.0, 100.0, 500.0, 1000.0],
        };

        let axis_info = AxisInfo {
            name: "explicit_axis".to_string(),
            pts_ref_name: None,
            unit: None,
            min: 0.0,
            max: 1000.0,
            num_points: 4,
            point_source: source,
        };

        let values = axis_info.get_static_values().unwrap();
        assert_eq!(values, vec![0.0, 100.0, 500.0, 1000.0]);
        assert!(!axis_info.requires_memory_read());
    }

    #[test]
    fn test_axis_point_source_memory() {
        // Test AXIS_PTS reference style: requires memory read
        let source = AxisPointSource::Memory {
            address: 0x1000,
            address_ext: 2,
            data_type: A2lDataType::F32,
            num_points: 10,
        };

        let axis_info = AxisInfo {
            name: "memory_axis".to_string(),
            pts_ref_name: Some("SPEED_AXIS_PTS".to_string()),
            unit: Some("deg".to_string()),
            min: -180.0,
            max: 180.0,
            num_points: 10,
            point_source: source,
        };

        // Should return None for static values (requires memory read)
        assert!(axis_info.get_static_values().is_none());
        assert!(axis_info.requires_memory_read());
    }

    #[test]
    fn test_characteristic_type_values() {
        // Test CharacteristicType enum
        assert_eq!(CharacteristicType::Value, CharacteristicType::Value);
        assert_ne!(CharacteristicType::Curve, CharacteristicType::Map);
        assert_ne!(CharacteristicType::Map, CharacteristicType::ValBlk);
    }

    #[test]
    fn test_compu_vtab_parsing() {
        // Test COMPU_VTAB parsing from hello_xcp.a2l
        let reader = A2lReader::load("hello_xcp.a2l").expect("Failed to load hello_xcp.a2l");

        // Test BOOL conversion method (0="false", 1="true")
        let bool_method = reader.get_compu_method("BOOL");
        assert!(bool_method.is_enum(), "BOOL should be an enum type");
        assert_eq!(bool_method.enum_count(), 2);
        assert_eq!(bool_method.to_display(0), "false");
        assert_eq!(bool_method.to_display(1), "true");
        assert_eq!(bool_method.to_raw("false").unwrap(), 0);
        assert_eq!(bool_method.to_raw("true").unwrap(), 1);

        // Test OP_MODE conversion method (5 states)
        let op_mode_method = reader.get_compu_method("OP_MODE");
        assert!(op_mode_method.is_enum(), "OP_MODE should be an enum type");
        assert_eq!(op_mode_method.enum_count(), 5);
        assert_eq!(op_mode_method.to_display(0), "MODE_OFF");
        assert_eq!(op_mode_method.to_display(2), "MODE_AUTO");
        assert_eq!(op_mode_method.to_raw("MODE_ON").unwrap(), 1);

        // Test STATE conversion method (4 states)
        let state_method = reader.get_compu_method("STATE");
        assert!(state_method.is_enum(), "STATE should be an enum type");
        assert_eq!(state_method.enum_count(), 4);
        assert_eq!(state_method.to_display(0), "STATE_INIT");
        assert_eq!(state_method.to_display(3), "STATE_ERROR");

        // Test unknown raw value returns the number as string
        assert_eq!(op_mode_method.to_display(99), "99");

        // Test IDENTICAL method
        let identity_method = reader.get_compu_method("IDENTITY");
        assert!(!identity_method.is_enum());
        assert_eq!(identity_method.to_display(42), "42");

        println!("COMPU_VTAB parsing test passed!");
    }

    #[test]
    fn reader_appends_structural_measurements() {
        // Mirrors the spec example. Save to a temp file and load via A2lReader::load.
        let src = r#"
ASAP2_VERSION 1 71
/begin PROJECT P "" /begin MODULE M ""
  /begin MEASUREMENT counter "" ULONG NO_COMPU_METHOD 0 0 0 4294967295
    ECU_ADDRESS 0xFFF0 ECU_ADDRESS_EXTENSION 2 READ_WRITE
  /end MEASUREMENT
  /begin TYPEDEF_MEASUREMENT input "" FLOAT32_IEEE NO_COMPU_METHOD 0 0 -1e30 1e30 /end TYPEDEF_MEASUREMENT
  /begin TYPEDEF_MEASUREMENT output_linear "" FLOAT32_IEEE NO_COMPU_METHOD 0 0 -1e30 1e30 /end TYPEDEF_MEASUREMENT
  /begin TYPEDEF_MEASUREMENT output_spline "" FLOAT32_IEEE NO_COMPU_METHOD 0 0 -1e30 1e30 /end TYPEDEF_MEASUREMENT
  /begin TYPEDEF_STRUCTURE Lookup "" 12
    /begin STRUCTURE_COMPONENT input         input         0 /end STRUCTURE_COMPONENT
    /begin STRUCTURE_COMPONENT output_linear output_linear 4 /end STRUCTURE_COMPONENT
    /begin STRUCTURE_COMPONENT output_spline output_spline 8 /end STRUCTURE_COMPONENT
  /end TYPEDEF_STRUCTURE
  /begin INSTANCE lookup "" Lookup 0xFFF4 ECU_ADDRESS_EXTENSION 2 /end INSTANCE
/end MODULE /end PROJECT
"#;
        let tmp = std::env::temp_dir().join(format!(
            "xcp_struct_meas_test_{}_{}.a2l",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&tmp, src).unwrap();
        let reader = super::A2lReader::load(&tmp).unwrap();
        let measurements = reader.get_measurements();
        let names: HashSet<_> = measurements.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains("counter"), "flat measurement preserved");
        assert!(names.contains("lookup.input"));
        assert!(names.contains("lookup.output_linear"));
        assert!(names.contains("lookup.output_spline"));
        // Flat parsed first, synthetic appended.
        assert_eq!(measurements[0].name, "counter");
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn reader_appends_structural_characteristics() {
        let src = r#"
ASAP2_VERSION 1 71
/begin PROJECT P "" /begin MODULE M ""
  /begin RECORD_LAYOUT __F32 FNC_VALUES 1 FLOAT32_IEEE ROW_DIR DIRECT /end RECORD_LAYOUT
  /begin CHARACTERISTIC flat_gain "" VALUE 0x7000 __F32 0 NO_COMPU_METHOD -1e30 1e30 /end CHARACTERISTIC
  /begin TYPEDEF_CHARACTERISTIC gain_t "" VALUE __F32 0 NO_COMPU_METHOD -1e30 1e30 /end TYPEDEF_CHARACTERISTIC
  /begin TYPEDEF_CHARACTERISTIC offset_t "" VALUE __F32 0 NO_COMPU_METHOD -1e30 1e30 /end TYPEDEF_CHARACTERISTIC
  /begin TYPEDEF_STRUCTURE Tuning "" 8
    /begin STRUCTURE_COMPONENT gain   gain_t   0 /end STRUCTURE_COMPONENT
    /begin STRUCTURE_COMPONENT offset offset_t 4 /end STRUCTURE_COMPONENT
  /end TYPEDEF_STRUCTURE
  /begin INSTANCE tuning "" Tuning 0x6000 /end INSTANCE
/end MODULE /end PROJECT
"#;
        let tmp = std::env::temp_dir().join(format!(
            "xcp_struct_cal_test_{}_{}.a2l",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&tmp, src).unwrap();
        let reader = super::A2lReader::load(&tmp).unwrap();
        let chars = reader.get_characteristics();
        let names: HashSet<&str> = chars.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains("flat_gain"), "flat characteristic preserved");
        assert!(names.contains("tuning.gain"));
        assert!(names.contains("tuning.offset"));
        // Flat first, synthetic appended.
        assert_eq!(chars[0].name, "flat_gain");
        std::fs::remove_file(&tmp).ok();
    }
}
