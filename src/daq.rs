//! DAQ (Data Acquisition) Management
//!
//! Implements XCP Dynamic DAQ mechanism:
//! 1. Configure DAQ lists, ODTs, and ODT entries
//! 2. Start DAQ list - slave sends DTO packets automatically
//! 3. Receive and parse DTO packets from slave
//!
//! MDF4 backend selection: use feature `mf4-native` (default, mf4_parse/SimpleWriter)
//! or `mf4-rs-backend` (mdf4-rs crate). Only one may be active at a time.

#[cfg(all(feature = "mf4-native", feature = "mf4-rs-backend"))]
compile_error!(
    "Features `mf4-native` and `mf4-rs-backend` are mutually exclusive. Enable only one."
);

use crate::a2l::{A2lDataType, A2lMeasurement, A2lReader, CompuMethod};
use crate::client::DAQResolutionInfo;
use crate::protocol::Mta;
use crate::slave::XcpSlave;
// Note: chrono not used - timestamps are raw slave clock values, not UTC time
#[cfg(feature = "mf4-rs-backend")]
use mdf4_rs::{DataType as MdfDataType, DecodedValue, FlushPolicy, MdfWriter};
#[cfg(feature = "mf4-native")]
use mf4_parse::writer::builder::Mf4Metadata;
#[cfg(feature = "mf4-native")]
use mf4_parse::writer::stream_writer::{
    ChannelDef, ChannelGroupDefBuilder, Mf4StreamWriter, StreamingDataGroup,
};
use serde::Serialize;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write as IoWrite;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::AsyncBufReadExt;
use tokio::sync::{Mutex as TokioMutex, RwLock};
use tokio::task::JoinHandle;

/// ODT Entry allocation - represents a single measurement entry in an ODT
#[derive(Debug, Clone)]
pub struct OdtEntryAllocation {
    /// Index into the selected_measurements array
    pub measurement_idx: usize,
}

/// ODT allocation - contains multiple entries
#[derive(Debug, Clone)]
pub struct OdtAllocation {
    /// Entries in this ODT (measurement indices)
    pub entries: Vec<OdtEntryAllocation>,
}

/// DAQ List allocation - contains multiple ODTs
#[derive(Debug, Clone)]
pub struct DaqListAllocation {
    /// ODTs in this DAQ list
    pub odts: Vec<OdtAllocation>,
}

/// Complete DAQ-ODT-ODTEntry allocation result
#[derive(Debug, Clone)]
pub struct DaqAllocation {
    /// Number of DAQ lists
    pub daq_list_count: u16,
    /// Number of ODTs per DAQ list
    pub odt_count_per_list: Vec<u8>,
    /// DAQ list allocations
    pub daq_lists: Vec<DaqListAllocation>,
    /// Event channel assigned to each DAQ list
    pub event_channel_per_list: Vec<u16>,
}

/// MF4 event group mapping — links a DAQ list to its DG in the MF4 file
#[derive(Debug, Clone)]
struct Mf4EventGroup {
    /// DAQ list index
    daq_list: u16,
    /// Data Group index in MF4 file
    dg_index: usize,
    /// Channel names in this group (time_s first, then signals)
    channel_names: Vec<String>,
}

/// Measurement definition with full type information
#[derive(Debug, Clone)]
pub struct Measurement {
    pub name: String,
    pub address: u32,
    pub address_ext: u8,
    pub size: usize,
    /// Data type for proper parsing
    pub data_type: A2lDataType,
    /// Unit for display
    pub unit: Option<String>,
    /// COMPU_METHOD for converting raw to physical values
    pub compu_method: CompuMethod,
}

#[derive(Debug, Clone)]
pub enum IdentificationFieldType {
    AbosulteODT,
    RelatetiveODTBYTE,
    RelatetiveODTWORD,
    RelatetiveODTWORDAlign,
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum TimestampUnit {
    DAQTIMESTAMPUNIT1NS = 0x0,
    DAQTIMESTAMPUNIT10NS = 0x1,
    DAQTIMESTAMPUNIT100NS = 0x2,
    DAQTIMESTAMPUNIT1US = 0x3,
    DAQTIMESTAMPUNIT10US = 0x4,
    DAQTIMESTAMPUNIT100US = 0x5,
    DAQTIMESTAMPUNIT1MS = 0x6,
    DAQTIMESTAMPUNIT10MS = 0x7,
    DAQTIMESTAMPUNIT100MS = 0x8,
    DAQTIMESTAMPUNIT1S = 0x9,
    DAQTIMESTAMPUNIT1PS = 0xa,
    DAQTIMESTAMPUNIT10PS = 0xb,
    DAQTIMESTAMPUNIT100PS = 0xc,
}

impl TimestampUnit {
    /// 获取每个 tick 对应的秒数
    pub fn to_seconds(&self) -> f64 {
        match self {
            TimestampUnit::DAQTIMESTAMPUNIT1PS => 1e-12,
            TimestampUnit::DAQTIMESTAMPUNIT10PS => 1e-11,
            TimestampUnit::DAQTIMESTAMPUNIT100PS => 1e-10,
            TimestampUnit::DAQTIMESTAMPUNIT1NS => 1e-9,
            TimestampUnit::DAQTIMESTAMPUNIT10NS => 1e-8,
            TimestampUnit::DAQTIMESTAMPUNIT100NS => 1e-7,
            TimestampUnit::DAQTIMESTAMPUNIT1US => 1e-6,
            TimestampUnit::DAQTIMESTAMPUNIT10US => 1e-5,
            TimestampUnit::DAQTIMESTAMPUNIT100US => 1e-4,
            TimestampUnit::DAQTIMESTAMPUNIT1MS => 1e-3,
            TimestampUnit::DAQTIMESTAMPUNIT10MS => 1e-2,
            TimestampUnit::DAQTIMESTAMPUNIT100MS => 1e-1,
            TimestampUnit::DAQTIMESTAMPUNIT1S => 1.0,
        }
    }

    /// 从 u16 安全转换
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x0 => Some(TimestampUnit::DAQTIMESTAMPUNIT1NS),
            0x1 => Some(TimestampUnit::DAQTIMESTAMPUNIT10NS),
            0x2 => Some(TimestampUnit::DAQTIMESTAMPUNIT100NS),
            0x3 => Some(TimestampUnit::DAQTIMESTAMPUNIT1US),
            0x4 => Some(TimestampUnit::DAQTIMESTAMPUNIT10US),
            0x5 => Some(TimestampUnit::DAQTIMESTAMPUNIT100US),
            0x6 => Some(TimestampUnit::DAQTIMESTAMPUNIT1MS),
            0x7 => Some(TimestampUnit::DAQTIMESTAMPUNIT10MS),
            0x8 => Some(TimestampUnit::DAQTIMESTAMPUNIT100MS),
            0x9 => Some(TimestampUnit::DAQTIMESTAMPUNIT1S),
            0xa => Some(TimestampUnit::DAQTIMESTAMPUNIT1PS),
            0xb => Some(TimestampUnit::DAQTIMESTAMPUNIT10PS),
            0xc => Some(TimestampUnit::DAQTIMESTAMPUNIT100PS),
            _ => None,
        }
    }
}

impl Measurement {
    pub fn new(name: &str, address: u32, address_ext: u8, size: usize) -> Self {
        Self {
            name: name.to_string(),
            address,
            address_ext,
            size,
            data_type: A2lDataType::U32,
            unit: None,
            compu_method: CompuMethod::Identical,
        }
    }

    /// Create a measurement with full type information
    pub fn with_type(
        name: &str,
        address: u32,
        address_ext: u8,
        data_type: A2lDataType,
        unit: Option<String>,
        compu_method: CompuMethod,
    ) -> Self {
        let size = data_type.size();
        Self {
            name: name.to_string(),
            address,
            address_ext,
            size,
            data_type,
            unit,
            compu_method,
        }
    }

    pub fn mta(&self) -> Mta {
        Mta::new(self.address, self.address_ext)
    }
}

/// ODT Entry configuration - tracks a single measurement's position in DAQ structure
#[derive(Debug, Clone)]
pub struct OdtEntryConfig {
    /// Measurement name
    pub name: String,
    /// DAQ list number
    pub daq_list: u16,
    /// ODT number within the DAQ list
    pub odt: u8,
    /// Entry index within the ODT
    pub entry: u8,
    /// Byte offset within the ODT data payload
    pub byte_offset: usize,
    /// Size in bytes
    pub size: usize,
    /// Data type for parsing
    pub data_type: A2lDataType,
    /// Unit string
    pub unit: Option<String>,
    /// COMPU_METHOD for conversion
    pub compu_method: CompuMethod,
}

/// DAQ Configuration - stores complete DAQ structure for decoding
#[derive(Debug, Clone)]
pub struct DaqConfiguration {
    /// ODT entry configurations (one per measurement)
    pub odt_entries: Vec<OdtEntryConfig>,
    /// Timestamp size in bytes (0, 1, 2, or 4)
    pub timestamp_size: u8,
    /// Timestamp unit for conversion
    pub timestamp_unit: Option<TimestampUnit>,
    /// Identification field type
    pub identification_type: IdentificationFieldType,
    /// Maximum ODT entry size from slave
    pub max_odt_entry_size: u8,
    /// ODT entry granularity from slave
    pub odt_entry_granularity: u8,
    /// Number of DAQ lists configured
    pub daq_list_count: u16,
    /// Number of ODTs per DAQ list
    pub odt_count_per_list: Vec<u8>,
    /// First absolute PID (firstPID) of each DAQ list, returned by the
    /// START_STOP_DAQ_LIST (0xDE) response. Used to route DTO packets in
    /// AbsoluteODT mode, where PIDs are NOT necessarily contiguous across
    /// DAQ lists (e.g. list 0 → PID 0..2, list 1 → PID 0x60..0x62).
    pub first_pid_per_list: Vec<u8>,
    /// Event channel assigned to each DAQ list (index = daq_list)
    pub event_channel_per_list: Vec<u16>,
}

impl DaqConfiguration {
    /// Create empty configuration
    pub fn new() -> Self {
        Self {
            odt_entries: Vec::new(),
            timestamp_size: 0,
            timestamp_unit: None,
            identification_type: IdentificationFieldType::AbosulteODT,
            max_odt_entry_size: 0,
            odt_entry_granularity: 1,
            daq_list_count: 0,
            odt_count_per_list: Vec::new(),
            first_pid_per_list: Vec::new(),
            event_channel_per_list: Vec::new(),
        }
    }

    /// Get configuration for a measurement by name
    pub fn get_entry(&self, name: &str) -> Option<&OdtEntryConfig> {
        self.odt_entries.iter().find(|e| e.name == name)
    }

    /// Get entries for a specific ODT
    pub fn get_odt_entries(&self, daq_list: u16, odt: u8) -> Vec<&OdtEntryConfig> {
        self.odt_entries
            .iter()
            .filter(|e| e.daq_list == daq_list && e.odt == odt)
            .collect()
    }

    /// Calculate total size of all entries (for validation)
    pub fn total_size(&self) -> usize {
        self.odt_entries.iter().map(|e| e.size).sum()
    }

    /// Resolve an absolute ODT number (PID) to (daq_list, relative_odt).
    ///
    /// In AbsoluteODT identification mode the PID carries an absolute ODT
    /// number. PIDs are NOT necessarily contiguous across DAQ lists — each
    /// DAQ list's first PID (`firstPID`) is returned by the START_STOP_DAQ_LIST
    /// (0xDE) response. For example, if DAQ list 0 has firstPID=0 with 2 ODTs
    /// and DAQ list 1 has firstPID=0x60 with 3 ODTs, then PID 0→(0,0),
    /// 1→(0,1), 0x60→(1,0), 0x61→(1,1).
    ///
    /// When `first_pid_per_list` is populated (after START_STOP_DAQ_LIST),
    /// range-based lookup is used. Otherwise it falls back to the legacy
    /// contiguous assumption (PIDs packed sequentially across lists).
    pub fn resolve_absolute_odt(&self, abs_odt: u8) -> Option<(u16, u8)> {
        if !self.first_pid_per_list.is_empty() {
            for (list_idx, &first_pid) in self.first_pid_per_list.iter().enumerate() {
                let odt_count = self.odt_count_per_list.get(list_idx).copied().unwrap_or(0) as u8;
                if abs_odt >= first_pid && abs_odt < first_pid.saturating_add(odt_count) {
                    return Some((list_idx as u16, abs_odt - first_pid));
                }
            }
            None
        } else {
            // Fallback: legacy contiguous assumption (PIDs packed sequentially).
            let mut remaining = abs_odt as u16;
            for (list_idx, &odt_count) in self.odt_count_per_list.iter().enumerate() {
                if remaining < odt_count as u16 {
                    return Some((list_idx as u16, remaining as u8));
                }
                remaining -= odt_count as u16;
            }
            None
        }
    }

    /// Clear all entries
    pub fn clear(&mut self) {
        self.odt_entries.clear();
        self.odt_count_per_list.clear();
        self.first_pid_per_list.clear();
        self.event_channel_per_list.clear();
    }
}

impl Default for DaqConfiguration {
    fn default() -> Self {
        Self::new()
    }
}

/// DAQ data record for CSV output
#[derive(Debug, Clone, Serialize)]
pub struct DaqRecord {
    /// Elapsed seconds since recording start (converted from slave ticks)
    pub timestamp_s: f64,
    /// Raw timestamp value (slave clock ticks, for debugging)
    pub timestamp_raw: u32,
    /// Measurement values (name -> physical value as f64)
    /// Values are converted from raw bytes using COMPU_METHOD
    pub values: HashMap<String, f64>,
    /// Per-signal update timestamps (name -> elapsed seconds when last updated)
    pub signal_timestamps: HashMap<String, f64>,
    /// Raw values for debugging (name -> raw bytes)
    #[serde(skip)]
    pub raw_values: HashMap<String, Vec<u8>>,
}

impl DaqRecord {
    /// Create a new DAQ record
    pub fn new(timestamp_s: f64, timestamp_raw: u32) -> Self {
        Self {
            timestamp_s,
            timestamp_raw,
            values: HashMap::new(),
            signal_timestamps: HashMap::new(),
            raw_values: HashMap::new(),
        }
    }

    /// Add a measurement value (physical value after conversion)
    pub fn add_value(&mut self, name: &str, value: f64) {
        self.values.insert(name.to_string(), value);
    }

    /// Add raw measurement bytes
    pub fn add_raw_value(&mut self, name: &str, raw: Vec<u8>) {
        self.raw_values.insert(name.to_string(), raw);
    }

    /// Get a measurement value
    pub fn get_value(&self, name: &str) -> Option<f64> {
        self.values.get(name).copied()
    }

    /// Get raw bytes for a measurement
    pub fn get_raw_value(&self, name: &str) -> Option<&Vec<u8>> {
        self.raw_values.get(name)
    }
}

/// Parsed DTO packet data
#[derive(Debug, Clone)]
pub struct DtoData {
    /// DAQ list number (from PID)
    pub daq_list: Option<u16>,
    /// ODT number (implied by sequence in DTO packets)
    pub odt: u8,
    /// Raw data bytes
    pub data: Vec<u8>,
    /// Timestamp
    pub timestamp: Option<u32>,
}

/// Tracks timestamp wrapping and converts raw slave ticks to elapsed seconds.
///
/// Uses delta-based accumulation: each DTO packet's timestamp is compared to the
/// previous one. Forward deltas (< half the counter range) are accumulated into
/// a u64 total. This correctly handles any number of counter wraps.
#[derive(Debug, Clone)]
pub struct TimestampTracker {
    /// Previous raw timestamp value (masked to DTO width)
    prev_raw: u32,
    /// Accumulated total ticks since t0
    cumulative_ticks: u64,
    /// Bit mask for DTO timestamp width (0xFF, 0xFFFF, or 0xFFFFFFFF)
    mask: u32,
    /// Half the counter range — forward/backward threshold
    half_range: u64,
    /// Seconds per tick
    tick_seconds: f64,
}

impl TimestampTracker {
    /// Create a new tracker.
    ///
    /// - `t0_ticks`: base clock from GET_DAQ_CLOCK (truncated to `timestamp_size` width)
    /// - `timestamp_size`: 1, 2, or 4 bytes
    /// - `unit`: timestamp unit for tick-to-seconds conversion
    pub fn new(t0_ticks: u32, timestamp_size: u8, unit: &TimestampUnit) -> Self {
        let max_value: u64 = match timestamp_size {
            1 => 0xFF,
            2 => 0xFFFF,
            4 => 0xFFFF_FFFF,
            _ => 0xFFFF_FFFF,
        };
        let mask = max_value as u32;
        Self {
            prev_raw: t0_ticks & mask,
            cumulative_ticks: 0,
            mask,
            half_range: (max_value + 1) / 2,
            tick_seconds: unit.to_seconds(),
        }
    }

    /// Compute elapsed seconds from a raw DTO timestamp.
    ///
    /// Compares to previous value to detect forward progress vs backward jumps.
    /// Forward deltas (< half the counter range) are accumulated; backward jumps
    /// (from out-of-order UDP packets) are ignored.
    pub fn elapsed_seconds(&mut self, ts_raw: u32) -> f64 {
        let curr = ts_raw & self.mask;
        let delta = curr.wrapping_sub(self.prev_raw) & self.mask;

        if (delta as u64) <= self.half_range {
            self.cumulative_ticks += delta as u64;
        }
        // else: backward jump (out-of-order packet), don't accumulate

        self.prev_raw = curr;
        self.cumulative_ticks as f64 * self.tick_seconds
    }
}

/// Compute elapsed seconds from a PCAN device reception timestamp
/// (microseconds since device init) relative to a baseline captured on the
/// first recorded frame. Uses wrapping subtraction so a device-clock wrap
/// (every ~49 days of `millis`) still yields a sane forward delta within a
/// single recording session.
fn device_ts_elapsed(ts_us: u64, baseline_us: Option<u64>) -> f64 {
    let base = baseline_us.unwrap_or(ts_us);
    (ts_us.wrapping_sub(base)) as f64 / 1_000_000.0
}

/// DAQ Manager - implements Dynamic DAQ
#[derive(Clone)]
pub struct DaqManager {
    /// All available measurement definitions (from A2L)
    all_measurements: Vec<Measurement>,
    /// Selected measurement indices for DAQ
    selected_indices: Vec<usize>,
    /// DAQ running flag
    running: Arc<AtomicBool>,
    /// Background task handle
    task_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
    /// CSV output path
    csv_path: PathBuf,
    /// MDF4 output path (derived from csv_path)
    mdf4_path: PathBuf,
    /// Whether CSV recording is enabled
    csv_enabled: bool,
    /// Whether MF4 recording is enabled
    mf4_enabled: bool,
    /// Slave address for DAQ connection
    slave_addr: String,
    /// Number of DAQ lists configured
    #[allow(dead_code)]
    daq_list_count: u16,
    /// Default event channel (used when no per-measurement event is set)
    event_channel: u16,
    /// Per-measurement event channel assignment (measurement index -> event channel)
    measurement_events: HashMap<usize, u16>,
    /// Available DAQ events (from A2L or slave query)
    available_events: Vec<crate::a2l::DaqEvent>,
    /// Latest DAQ sample (for UI display)
    latest_sample: Arc<RwLock<Option<DaqRecord>>>,
    /// timestamp mode and size
    timestamp_fixed: bool,
    timestamp_size: Option<u8>,
    indentification_field_type: IdentificationFieldType,
    timestamp_unit: Option<TimestampUnit>,
    /// Base timestamp from GET_DAQ_CLOCK (used for relative timestamp calculation)
    /// Note: This is a raw u32 value, not UTC epoch time (the 0 point is slave-specific)
    base_timestamp: Option<u32>,
    /// DAQ resolution info from slave (ODT entry limits)
    daq_resolution: Option<DAQResolutionInfo>,
    /// DAQ configuration - stores mapping of measurements to ODT entries
    daq_config: Arc<RwLock<DaqConfiguration>>,
    /// Flag indicating if selection has changed while DAQ was running
    selection_changed: Arc<AtomicBool>,
    /// Recording flag (file writing active)
    recording: Arc<AtomicBool>,
    /// Shared CSV writers for background task — one per event (daq_list → writer)
    csv_writers: Arc<TokioMutex<HashMap<u16, std::io::BufWriter<std::fs::File>>>>,
    /// CSV event mapping: daq_list → signal names (for writing correct columns)
    csv_event_signals: Arc<RwLock<HashMap<u16, Vec<String>>>>,
    /// Shared MF4 writer (mf4-native) for background task — multi-DG support
    #[cfg(feature = "mf4-native")]
    mf4_writer: Arc<TokioMutex<Option<Mf4StreamWriter<std::io::BufWriter<std::fs::File>>>>>,
    /// MF4 event group mapping: DAQ list → DG index + channel names
    #[cfg(feature = "mf4-native")]
    mf4_event_groups: Arc<RwLock<Vec<Mf4EventGroup>>>,
    /// Shared MF4 writer (mdf4-rs) for background task
    #[cfg(feature = "mf4-rs-backend")]
    mf4_writer: Arc<TokioMutex<Option<MdfWriter<mdf4_rs::writer::FileWriter>>>>,
    /// MDF4 channel group ID (mdf4-rs only)
    #[cfg(feature = "mf4-rs-backend")]
    mf4_cg_id: Arc<TokioMutex<Option<String>>>,
    /// Recording timestamp tracker (reset when recording starts)
    recording_ts_tracker: Arc<RwLock<Option<TimestampTracker>>>,
    /// Recording start instant (fallback when slave has no timestamps)
    recording_start_instant: Arc<RwLock<Option<std::time::Instant>>>,
    /// Recording start device timestamp baseline (PCAN hardware clock, µs),
    /// captured from the first recorded DTO packet. Used as a high-accuracy
    /// fallback when the slave does not provide its own DAQ timestamps.
    recording_start_device_ts_us: Arc<RwLock<Option<u64>>>,
    /// Measurement info for MDF4 channel setup (name, unit)
    recording_measurement_info: Arc<RwLock<Vec<(String, Option<String>)>>>,

    /// Optional shared packet log for the in-app packet browser
    pub packet_log: Option<Arc<crate::xcp_packet_log::XcpPacketLog>>,

    /// Slave display name for labeling DAQ entries
    pub slave_name: String,
}

impl DaqManager {
    /// Create a new DAQ manager with default hardcoded measurements
    pub fn new() -> Self {
        // No default measurements - must be loaded from A2L file
        let all_measurements: Vec<Measurement> = Vec::new();
        let selected_indices: Vec<usize> = Vec::new();

        Self {
            all_measurements,
            selected_indices,
            running: Arc::new(AtomicBool::new(false)),
            task_handle: Arc::new(RwLock::new(None)),
            csv_path: PathBuf::from("daq_data.csv"),
            mdf4_path: PathBuf::from("daq_data.mf4"),
            csv_enabled: true,
            mf4_enabled: true,
            slave_addr: "127.0.0.1:5555".to_string(),
            daq_list_count: 1,
            event_channel: 0,
            measurement_events: HashMap::new(),
            available_events: Vec::new(),
            latest_sample: Arc::new(RwLock::new(None)),
            timestamp_fixed: false,
            timestamp_size: None,
            indentification_field_type: IdentificationFieldType::AbosulteODT, // most commonly used
            timestamp_unit: None,
            base_timestamp: None,
            daq_resolution: None,
            daq_config: Arc::new(RwLock::new(DaqConfiguration::new())),
            selection_changed: Arc::new(AtomicBool::new(false)),
            recording: Arc::new(AtomicBool::new(false)),
            csv_writers: Arc::new(TokioMutex::new(HashMap::new())),
            csv_event_signals: Arc::new(RwLock::new(HashMap::new())),
            #[cfg(feature = "mf4-native")]
            mf4_writer: Arc::new(TokioMutex::new(None)),
            #[cfg(feature = "mf4-native")]
            mf4_event_groups: Arc::new(RwLock::new(Vec::new())),
            #[cfg(feature = "mf4-rs-backend")]
            mf4_writer: Arc::new(TokioMutex::new(None)),
            #[cfg(feature = "mf4-rs-backend")]
            mf4_cg_id: Arc::new(TokioMutex::new(None)),
            recording_ts_tracker: Arc::new(RwLock::new(None)),
            recording_start_instant: Arc::new(RwLock::new(None)),
            recording_start_device_ts_us: Arc::new(RwLock::new(None)),
            recording_measurement_info: Arc::new(RwLock::new(Vec::new())),
            packet_log: None,
            slave_name: String::new(),
        }
    }

    /// Create a DAQ manager from an A2L file
    pub fn from_a2l<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let reader = A2lReader::load(path)?;
        let a2l_measurements = reader.get_measurements();

        let all_measurements: Vec<Measurement> = a2l_measurements
            .into_iter()
            .filter_map(Self::convert_measurement)
            .collect();

        // By default, select first 8 measurements or all if less than 8
        let selected_count = all_measurements.len().min(8);
        let selected_indices = (0..selected_count).collect();

        Ok(Self {
            all_measurements,
            selected_indices,
            running: Arc::new(AtomicBool::new(false)),
            task_handle: Arc::new(RwLock::new(None)),
            csv_path: PathBuf::from("daq_data.csv"),
            mdf4_path: PathBuf::from("daq_data.mf4"),
            csv_enabled: true,
            mf4_enabled: true,
            slave_addr: "127.0.0.1:5555".to_string(),
            daq_list_count: 1,
            event_channel: 0,
            measurement_events: HashMap::new(),
            available_events: Vec::new(),
            latest_sample: Arc::new(RwLock::new(None)),
            timestamp_fixed: false,
            timestamp_size: None,
            indentification_field_type: IdentificationFieldType::AbosulteODT,
            timestamp_unit: None,
            base_timestamp: None,
            daq_resolution: None,
            daq_config: Arc::new(RwLock::new(DaqConfiguration::new())),
            selection_changed: Arc::new(AtomicBool::new(false)),
            recording: Arc::new(AtomicBool::new(false)),
            csv_writers: Arc::new(TokioMutex::new(HashMap::new())),
            csv_event_signals: Arc::new(RwLock::new(HashMap::new())),
            #[cfg(feature = "mf4-native")]
            mf4_writer: Arc::new(TokioMutex::new(None)),
            #[cfg(feature = "mf4-native")]
            mf4_event_groups: Arc::new(RwLock::new(Vec::new())),
            #[cfg(feature = "mf4-rs-backend")]
            mf4_writer: Arc::new(TokioMutex::new(None)),
            #[cfg(feature = "mf4-rs-backend")]
            mf4_cg_id: Arc::new(TokioMutex::new(None)),
            recording_ts_tracker: Arc::new(RwLock::new(None)),
            recording_start_instant: Arc::new(RwLock::new(None)),
            recording_start_device_ts_us: Arc::new(RwLock::new(None)),
            recording_measurement_info: Arc::new(RwLock::new(Vec::new())),
            packet_log: None,
            slave_name: String::new(),
        })
    }

    /// Convert A2L measurement to DAQ Measurement with full type info
    fn convert_measurement(m: A2lMeasurement) -> Option<Measurement> {
        let size = m.data_type.size();
        // Create COMPU_METHOD from A2L conversion info
        // For now, use linear conversion based on min/max if available
        let compu_method = CompuMethod::Identical;

        Some(Measurement {
            name: m.name,
            address: m.address,
            address_ext: m.address_ext,
            size,
            data_type: m.data_type,
            unit: m.unit,
            compu_method,
        })
    }

    /// Get the DAQ configuration (for use by other components)
    pub fn get_config(&self) -> Arc<RwLock<DaqConfiguration>> {
        self.daq_config.clone()
    }

    /// Get the MDF4 output file path
    pub fn get_mdf4_path(&self) -> &Path {
        &self.mdf4_path
    }

    pub fn get_csv_path(&self) -> &Path {
        &self.csv_path
    }

    /// Set the CSV output file path
    pub fn set_csv_path<P: Into<PathBuf>>(&mut self, path: P) {
        self.csv_path = path.into();
    }

    /// Set the MDF4 output file path
    pub fn set_mdf4_path<P: Into<PathBuf>>(&mut self, path: P) {
        self.mdf4_path = path.into();
    }

    /// Enable or disable CSV recording
    pub fn set_csv_enabled(&mut self, enabled: bool) {
        self.csv_enabled = enabled;
    }

    /// Enable or disable MF4 recording
    pub fn set_mf4_enabled(&mut self, enabled: bool) {
        self.mf4_enabled = enabled;
    }

    pub fn set_available_events(&mut self, events: Vec<crate::a2l::DaqEvent>) {
        self.available_events = events;
    }

    pub fn get_available_events(&self) -> &[crate::a2l::DaqEvent] {
        &self.available_events
    }

    pub fn set_event_channel(&mut self, channel: u16) {
        self.event_channel = channel;
    }

    pub fn get_event_channel(&self) -> u16 {
        self.event_channel
    }

    /// Set event channel for a specific measurement (by index)
    pub fn set_measurement_event(&mut self, measurement_idx: usize, channel: u16) {
        if measurement_idx < self.all_measurements.len() {
            self.measurement_events.insert(measurement_idx, channel);
            if self.running.load(Ordering::SeqCst) {
                self.selection_changed.store(true, Ordering::SeqCst);
            }
        }
    }

    /// Get event channel for a specific measurement (falls back to default)
    pub fn get_measurement_event(&self, measurement_idx: usize) -> u16 {
        self.measurement_events
            .get(&measurement_idx)
            .copied()
            .unwrap_or(self.event_channel)
    }

    /// Get all per-measurement event assignments
    pub fn get_measurement_events(&self) -> &HashMap<usize, u16> {
        &self.measurement_events
    }

    /// Set event channel for multiple measurements at once
    pub fn set_measurement_events_batch(&mut self, assignments: &[(usize, u16)]) {
        let was_running = self.running.load(Ordering::SeqCst);
        for &(idx, channel) in assignments {
            if idx < self.all_measurements.len() {
                self.measurement_events.insert(idx, channel);
            }
        }
        if was_running {
            self.selection_changed.store(true, Ordering::SeqCst);
        }
    }

    /// Check if DAQ needs restart due to selection changes
    pub fn needs_restart(&self) -> bool {
        self.selection_changed.load(Ordering::SeqCst)
    }

    /// Clear the restart flag
    pub fn clear_restart_flag(&self) {
        self.selection_changed.store(false, Ordering::SeqCst);
    }

    /// Get DAQ resolution info
    pub fn get_daq_resolution(&self) -> Option<&DAQResolutionInfo> {
        self.daq_resolution.as_ref()
    }

    /// Get identification field type (PID format)
    pub fn get_identification_type(&self) -> &IdentificationFieldType {
        &self.indentification_field_type
    }

    /// Get timestamp size in bytes (0 = no timestamp, or unknown before slave connect)
    pub fn get_timestamp_size(&self) -> Option<u8> {
        self.timestamp_size
    }

    /// Get timestamp unit
    pub fn get_timestamp_unit(&self) -> Option<TimestampUnit> {
        self.timestamp_unit
    }

    /// Get all available measurements
    pub fn all_measurements(&self) -> &[Measurement] {
        &self.all_measurements
    }

    /// Get selected measurement indices
    pub fn selected_indices(&self) -> &[usize] {
        &self.selected_indices
    }

    /// Get selected measurements for DAQ
    pub fn selected_measurements(&self) -> Vec<Measurement> {
        self.selected_indices
            .iter()
            .filter_map(|&idx| self.all_measurements.get(idx).cloned())
            .collect()
    }

    /// Check if a measurement is selected
    pub fn is_selected(&self, index: usize) -> bool {
        self.selected_indices.contains(&index)
    }

    /// Toggle selection of a measurement
    /// If DAQ is running, marks that restart is needed
    pub fn toggle_measurement(&mut self, index: usize) {
        let was_running = self.running.load(Ordering::SeqCst);

        if let Some(pos) = self.selected_indices.iter().position(|&i| i == index) {
            self.selected_indices.remove(pos);
        } else if index < self.all_measurements.len() {
            self.selected_indices.push(index);
            // Keep sorted for consistent display
            self.selected_indices.sort_unstable();
        }

        // Mark selection changed if DAQ was running
        if was_running {
            self.selection_changed.store(true, Ordering::SeqCst);
            tracing::info!("Selection changed while DAQ running - restart required");
        }
    }

    /// DAQ Constraints:
    /// - Each ODT: max 255 entries
    /// - Each DAQ list: max 252 ODTs
    /// - DAQ lists: max 0xFFFF (65535)
    /// - Each entry: max max_odt_entry_size bytes (checked individually)
    /// Measurements are grouped by event channel: each event gets its own DAQ list(s).
    fn calculate_odt_allocation(
        &self,
        max_odt_entry_size: u8,
        odt_granularity: u8,
        max_odt0_payload: usize,
        max_odt_payload: usize,
    ) -> Result<DaqAllocation, String> {
        const MAX_ODT_ENTRIES: usize = 255;
        const MAX_ODTS_PER_LIST: usize = 252;
        const MAX_DAQ_LISTS: usize = 0xFFFF;

        let selected = self.selected_measurements();
        if selected.is_empty() {
            return Ok(DaqAllocation {
                daq_list_count: 0,
                odt_count_per_list: Vec::new(),
                daq_lists: Vec::new(),
                event_channel_per_list: Vec::new(),
            });
        }

        // Check each measurement size against max_odt_entry_size
        for meas in selected.iter() {
            if meas.size > max_odt_entry_size as usize {
                return Err(format!(
                    "Measurement '{}' size ({} bytes) exceeds max ODT entry size ({} bytes). Please deselect this measurement or reduce data type size.",
                    meas.name, meas.size, max_odt_entry_size
                ));
            }
        }

        let granularity = std::cmp::max(1, odt_granularity as usize);

        // Group selected measurement indices by event channel
        let mut event_groups: std::collections::BTreeMap<u16, Vec<usize>> =
            std::collections::BTreeMap::new();
        for (sel_idx, &meas_idx) in self.selected_indices.iter().enumerate() {
            let event_ch = self.get_measurement_event(meas_idx);
            event_groups.entry(event_ch).or_default().push(sel_idx);
        }

        let mut daq_lists: Vec<DaqListAllocation> = Vec::new();
        let mut event_channel_per_list: Vec<u16> = Vec::new();

        // For each event group, create separate DAQ list(s)
        for (event_ch, measurement_indices) in &event_groups {
            let mut current_list: Vec<OdtAllocation> = Vec::new();
            let mut current_odt: Vec<OdtEntryAllocation> = Vec::new();
            let mut current_odt_bytes: usize = 0;

            for &idx in measurement_indices {
                let meas = &selected[idx];
                let aligned_size = meas.size.div_ceil(granularity) * granularity;

                // Determine the max payload for the current ODT
                let is_odt0 = current_list.is_empty();
                let max_payload = if is_odt0 {
                    max_odt0_payload
                } else {
                    max_odt_payload
                };

                // Check if adding this measurement would overflow the ODT payload
                let would_overflow = current_odt_bytes + aligned_size > max_payload;
                let at_entry_limit = current_odt.len() >= MAX_ODT_ENTRIES;

                if (would_overflow || at_entry_limit) && !current_odt.is_empty() {
                    // Flush current ODT
                    current_list.push(OdtAllocation {
                        entries: current_odt,
                    });
                    current_odt = Vec::new();
                    current_odt_bytes = 0;

                    // Check if we need a new DAQ list (252 ODTs max per list)
                    if current_list.len() >= MAX_ODTS_PER_LIST {
                        daq_lists.push(DaqListAllocation { odts: current_list });
                        event_channel_per_list.push(*event_ch);
                        current_list = Vec::new();
                    }

                    // Check DAQ list limit
                    if daq_lists.len() >= MAX_DAQ_LISTS {
                        return Err(format!(
                            "DAQ configuration exceeds maximum number of DAQ lists ({}). Please select fewer measurements.",
                            MAX_DAQ_LISTS
                        ));
                    }
                }

                // Add measurement to current ODT
                current_odt.push(OdtEntryAllocation {
                    measurement_idx: idx,
                });
                current_odt_bytes += aligned_size;
            }

            // Flush remaining ODT/list for this event group
            if !current_odt.is_empty() {
                current_list.push(OdtAllocation {
                    entries: current_odt,
                });
            }
            if !current_list.is_empty() {
                daq_lists.push(DaqListAllocation { odts: current_list });
                event_channel_per_list.push(*event_ch);
            }
        }

        // Final validation
        if daq_lists.len() > MAX_DAQ_LISTS {
            return Err(format!(
                "DAQ configuration requires {} DAQ lists, but maximum is {}. Please select fewer measurements.",
                daq_lists.len(), MAX_DAQ_LISTS
            ));
        }

        // Convert to return format
        let daq_list_count = daq_lists.len() as u16;
        let odt_count_per_list: Vec<u8> = daq_lists.iter().map(|l| l.odts.len() as u8).collect();

        Ok(DaqAllocation {
            daq_list_count,
            odt_count_per_list,
            daq_lists,
            event_channel_per_list,
        })
    }

    /// Select all measurements (with restart flag)
    pub fn select_all(&mut self) {
        let was_running = self.running.load(Ordering::SeqCst);
        self.selected_indices = (0..self.all_measurements.len()).collect();
        if was_running {
            self.selection_changed.store(true, Ordering::SeqCst);
        }
    }

    /// Deselect all measurements (with restart flag)
    pub fn deselect_all(&mut self) {
        let was_running = self.running.load(Ordering::SeqCst);
        self.selected_indices.clear();
        if was_running {
            self.selection_changed.store(true, Ordering::SeqCst);
        }
    }

    /// Select a measurement (with restart flag)
    pub fn select_measurement(&mut self, index: usize) {
        let was_running = self.running.load(Ordering::SeqCst);
        if index < self.all_measurements.len() && !self.selected_indices.contains(&index) {
            self.selected_indices.push(index);
            self.selected_indices.sort_unstable();
        }
        if was_running {
            self.selection_changed.store(true, Ordering::SeqCst);
        }
    }

    /// Deselect a measurement (with restart flag)
    pub fn deselect_measurement(&mut self, index: usize) {
        let was_running = self.running.load(Ordering::SeqCst);
        if let Some(pos) = self.selected_indices.iter().position(|&i| i == index) {
            self.selected_indices.remove(pos);
        }
        if was_running {
            self.selection_changed.store(true, Ordering::SeqCst);
        }
    }

    /// Clear all measurements (with restart flag if running)
    pub fn clear_measurements(&mut self) {
        let was_running = self.running.load(Ordering::SeqCst);
        self.all_measurements.clear();
        self.selected_indices.clear();
        if was_running {
            self.selection_changed.store(true, Ordering::SeqCst);
        }
    }

    /// Add a measurement to available list
    pub fn add_measurement(&mut self, measurement: Measurement) {
        self.all_measurements.push(measurement);
    }

    /// Set slave address for DAQ
    pub fn set_slave_addr(&mut self, addr: String) {
        self.slave_addr = addr;
    }

    /// Set packet log for DAQ packet capture
    pub fn set_packet_log(&mut self, log: Arc<crate::xcp_packet_log::XcpPacketLog>) {
        self.packet_log = Some(log);
    }

    /// Set slave display name
    pub fn set_slave_name(&mut self, name: String) {
        self.slave_name = name;
    }

    /// Clear the latest DAQ sample
    pub fn clear_samples(&mut self) {
        // Note: This is a synchronous method, so we use try_write or blocking_lock
        if let Ok(mut guard) = self.latest_sample.try_write() {
            *guard = None;
        }
    }

    /// Get count of all available measurements
    pub fn available_count(&self) -> usize {
        self.all_measurements.len()
    }

    /// Get count of selected measurements
    pub fn selected_count(&self) -> usize {
        self.selected_indices.len()
    }

    /// Get latest sample
    pub async fn get_latest_sample(&self) -> Option<DaqRecord> {
        self.latest_sample.read().await.clone()
    }

    pub async fn get_timestamp_mode(&mut self, slave: &XcpSlave) -> Result<(), String> {
        tracing::trace!("Getting timestamp mode...");
        // Get timestamp mode from slave through GET_DAQ_RESOLUTION_INFO
        let daqresolution_info = slave
            .get_daq_resolution_info()
            .await
            .map_err(|e| format!("GET_DAQ_RESOLUTION_INFO failed: {}", e))?;
        let timestamp_mode = daqresolution_info.timestamp_mode;
        self.timestamp_fixed = timestamp_mode & 0b1000 == 0b1000;
        let timestamp_size_bits = timestamp_mode & 0b0111;
        match timestamp_size_bits {
            0 if self.timestamp_fixed => {
                tracing::error!("Timestamp mode is fixed but timestamp size is 0.");
                return Err(
                    "Timestamp mode is fixed but timestamp size is not specified".to_string(),
                );
            }
            0 => {
                self.timestamp_size = None;
            }
            1 | 2 | 4 => {
                tracing::trace!("Timestamp size: {}", timestamp_size_bits);
                self.timestamp_size = Some(timestamp_size_bits);
            }
            _ => {
                tracing::error!("Timestamp SIZE NOT ALLOWED: {}", timestamp_size_bits);
                return Err("Timestamp SIZE NOT ALLOWED".to_string());
            }
        }
        let timestamp_unit = (timestamp_mode & 0b1111_0000) >> 4;
        self.timestamp_unit = TimestampUnit::from_u8(timestamp_unit);
        tracing::trace!("Timestamp tick: {:?}", self.timestamp_unit);
        Ok(())
    }

    pub async fn get_daq_processor_info(
        &self,
        slave: &XcpSlave,
    ) -> Result<IdentificationFieldType, String> {
        tracing::trace!("Getting DAQ processor info...");
        let daq_processor_info = slave
            .get_daq_processor_info()
            .await
            .map_err(|e| format!("GET_DAQ_PROCESSOR_INFO failed: {}", e))?;
        let daq_key_byte = daq_processor_info.daq_key_byte;
        tracing::info!("DAQ key byte: {}", daq_key_byte);
        let bits = (daq_key_byte & 0b11000000) >> 6;
        match bits {
            0 => {
                tracing::trace!("abosulte odt is used");
                Ok(IdentificationFieldType::AbosulteODT)
            }
            1 => {
                tracing::trace!("releative odt is used");
                Ok(IdentificationFieldType::RelatetiveODTBYTE)
            }
            2 => {
                tracing::trace!("releative odt is used");
                Ok(IdentificationFieldType::RelatetiveODTWORD)
            }
            3 => {
                tracing::trace!("releative odt is used");
                Ok(IdentificationFieldType::RelatetiveODTWORDAlign)
            }
            _ => {
                tracing::error!("IDENTIFICATION FIELD NOT ALLOWED: {}", bits);
                Err("IDENTIFICATION FIELD NOT ALLOWED".to_string())
            }
        }
    }

    /// Configure DAQ on the XCP slave using Dynamic DAQ
    ///
    /// Steps:
    /// 1. Get DAQ resolution info (ODT entry limits from slave)
    /// 2. FREE_DAQ - Release existing DAQ configuration
    /// 3. Calculate ODT allocation based on max_odt_entry_size
    /// 4. ALLOC_DAQ - Allocate DAQ lists
    /// 5. ALLOC_ODT - Allocate ODTs in each DAQ list
    /// 6. ALLOC_ODT_ENTRY - Allocate entries in each ODT
    /// 7. SET_DAQ_PTR + WRITE_DAQ - Configure each entry
    /// 8. SET_DAQ_LIST_MODE - Set mode and event channel
    /// 9. Build DaqConfiguration for decoding
    pub async fn configure(&mut self, slave: &XcpSlave) -> Result<(), String> {
        let identification_field_type = self.get_daq_processor_info(slave).await?;
        self.indentification_field_type = identification_field_type.clone();
        self.get_timestamp_mode(slave).await?;

        // Get DAQ resolution info for ODT entry limits
        let daq_resolution = slave
            .get_daq_resolution_info()
            .await
            .map_err(|e| format!("GET_DAQ_RESOLUTION_INFO failed: {}", e))?;
        self.daq_resolution = Some(daq_resolution);

        let max_odt_entry_size = daq_resolution.max_odt_entry_size_daq;
        let odt_granularity = daq_resolution.odt_entry_granularity_daq;

        // Get max_dto from ConnectInfo for DTO payload limit
        let max_dto = slave
            .info
            .connect_info
            .as_ref()
            .map(|c| c.max_dto)
            .unwrap_or(8);

        // Calculate DTO overhead: transport header (4 bytes) + PID + timestamp (ODT 0 only)
        let pid_size: usize = match &identification_field_type {
            IdentificationFieldType::AbosulteODT => 1,
            IdentificationFieldType::RelatetiveODTBYTE => 2,
            IdentificationFieldType::RelatetiveODTWORD => 3,
            IdentificationFieldType::RelatetiveODTWORDAlign => 4,
        };
        let ts_size = self.timestamp_size.unwrap_or(0) as usize;
        // max_dto is the XCP-level max DTO size (including PID/timestamp, excluding transport header)
        // Usable data payload per ODT = max_dto - pid_size - timestamp_size (for ODT 0)
        // For ODTs other than ODT 0, timestamp is not included, so payload = max_dto - pid_size
        let max_odt0_payload = (max_dto as usize).saturating_sub(pid_size + ts_size);
        let max_odt_payload = (max_dto as usize).saturating_sub(pid_size);

        tracing::info!("Configuring Dynamic DAQ...");
        tracing::info!(
            "ODT entry limits: max_size={}, granularity={}, max_dto={}, pid_size={}, ts_size={}, max_odt0_payload={}, max_odt_payload={}",
            max_odt_entry_size,
            odt_granularity,
            max_dto,
            pid_size,
            ts_size,
            max_odt0_payload,
            max_odt_payload
        );

        // Calculate ODT allocation based on slave limits
        let selected_measurements = self.selected_measurements();
        if selected_measurements.is_empty() {
            return Err("No measurements selected for DAQ".to_string());
        }

        // Calculate how many ODTs and DAQ lists we need
        let allocation = self.calculate_odt_allocation(
            max_odt_entry_size,
            odt_granularity,
            max_odt0_payload,
            max_odt_payload,
        )?;

        tracing::info!("ODT allocation: {} DAQ lists", allocation.daq_list_count);
        for (list_idx, odt_count) in allocation.odt_count_per_list.iter().enumerate() {
            let event_ch = allocation.event_channel_per_list[list_idx];
            tracing::info!(
                "  DAQ list {}: {} ODTs, event_channel={}",
                list_idx,
                odt_count,
                event_ch
            );
        }

        // 1. FREE_DAQ - Release any existing DAQ configuration
        tracing::trace!("FREE_DAQ");
        slave
            .free_daq()
            .await
            .map_err(|e| format!("FREE_DAQ failed: {}", e))?;

        // Clear old configuration
        {
            let mut config = self.daq_config.write().await;
            config.clear();
            config.max_odt_entry_size = max_odt_entry_size;
            config.odt_entry_granularity = odt_granularity;
            config.identification_type = identification_field_type.clone();
            config.timestamp_size = self.timestamp_size.unwrap_or(0);
            config.timestamp_unit = self.timestamp_unit;
            config.daq_list_count = allocation.daq_list_count;
            config.odt_count_per_list = allocation.odt_count_per_list.clone();
            config.event_channel_per_list = allocation.event_channel_per_list.clone();
        }

        // 2. ALLOC_DAQ - Allocate DAQ lists
        tracing::trace!("ALLOC_DAQ: {} lists", allocation.daq_list_count);
        slave
            .alloc_daq(allocation.daq_list_count)
            .await
            .map_err(|e| format!("ALLOC_DAQ failed: {}", e))?;

        // 3. For each DAQ list, ALLOC_ODT
        for (daq_list_idx, odt_count) in allocation.odt_count_per_list.iter().enumerate() {
            let daq_list = daq_list_idx as u16;
            tracing::trace!("ALLOC_ODT: DAQ list {}, {} ODTs", daq_list, odt_count);
            slave
                .alloc_odt(daq_list, *odt_count)
                .await
                .map_err(|e| format!("ALLOC_ODT failed for list {}: {}", daq_list, e))?;
        }

        // 4. For each ODT in each list, ALLOC_ODT_ENTRY
        for (daq_list_idx, _) in allocation.odt_count_per_list.iter().enumerate() {
            let daq_list = daq_list_idx as u16;
            let list_allocation = &allocation.daq_lists[daq_list_idx];
            for (odt_idx, odt_allocation) in list_allocation.odts.iter().enumerate() {
                let odt = odt_idx as u8;
                let entry_count = odt_allocation.entries.len() as u8;
                tracing::trace!(
                    "ALLOC_ODT_ENTRY: DAQ list {}, ODT {}, {} entries",
                    daq_list,
                    odt,
                    entry_count
                );
                slave
                    .alloc_odt_entry(daq_list, odt, entry_count)
                    .await
                    .map_err(|e| format!("ALLOC_ODT_ENTRY failed: {}", e))?;
            }
        }

        // 5. After all resources are allocated, SET_DAQ_PTR + WRITE_DAQ for each entry
        let mut global_entry_idx: usize = 0;

        for (daq_list_idx, _) in allocation.odt_count_per_list.iter().enumerate() {
            let daq_list = daq_list_idx as u16;
            let list_allocation = &allocation.daq_lists[daq_list_idx];

            for (odt_idx, odt_allocation) in list_allocation.odts.iter().enumerate() {
                let odt = odt_idx as u8;
                let mut byte_offset: usize = 0;

                for (entry_idx, entry_allocation) in odt_allocation.entries.iter().enumerate() {
                    let meas_idx = entry_allocation.measurement_idx;
                    let meas = &selected_measurements[meas_idx];

                    tracing::trace!(
                        "WRITE_DAQ: DAQ list {}, ODT {}, entry {}, addr 0x{:08X}, size {}",
                        daq_list,
                        odt,
                        entry_idx,
                        meas.address,
                        meas.size
                    );

                    slave
                        .set_daq_ptr(daq_list, odt, entry_idx as u8)
                        .await
                        .map_err(|e| format!("SET_DAQ_PTR failed: {}", e))?;

                    let aligned_size =
                        meas.size.div_ceil(odt_granularity as usize) * odt_granularity as usize;
                    slave
                        .write_daq(0, meas.mta(), aligned_size as u8)
                        .await
                        .map_err(|e| format!("WRITE_DAQ failed: {}", e))?;

                    {
                        let mut config = self.daq_config.write().await;
                        config.odt_entries.push(OdtEntryConfig {
                            name: meas.name.clone(),
                            daq_list,
                            odt,
                            entry: entry_idx as u8,
                            byte_offset,
                            size: meas.size,
                            data_type: meas.data_type.clone(),
                            unit: meas.unit.clone(),
                            compu_method: meas.compu_method.clone(),
                        });
                    }

                    byte_offset += meas.size;
                    global_entry_idx += 1;
                }
            }
        }

        // 6. SET_DAQ_LIST_MODE for each DAQ list
        //
        // Mode bit 0x10 requests the slave to timestamp DTOs of this DAQ list.
        // Some slaves do not support DAQ timestamping and reject mode 0x10 with
        // ERR_PARAM_OUT_OF_RANGE (0x22). To handle this robustly:
        //   - When the slave already advertised no timestamp support
        //     (timestamp_size == None), send mode 0x00 directly.
        //   - If the slave rejects the timestamp bit, fall back to mode 0x00 and
        //     disable timestamp decoding/recording for the whole session so DTO
        //     parsing and start_recording() do not expect a timestamp.
        let mut timestamp_disabled_by_slave = false;
        for (daq_list_idx, _) in allocation.odt_count_per_list.iter().enumerate() {
            let daq_list = daq_list_idx as u16;
            let list_event_channel = allocation.event_channel_per_list[daq_list_idx];
            let mode: u8 = if self.timestamp_size.is_some() && !timestamp_disabled_by_slave {
                0x10
            } else {
                0x00
            };
            tracing::trace!(
                "SET_DAQ_LIST_MODE: mode=0x{:02X}, daq_list={}, event_channel={}, prescaler=1",
                mode,
                daq_list,
                list_event_channel
            );
            match slave
                .set_daq_list_mode(mode, daq_list, list_event_channel, 1, 0)
                .await
            {
                Ok(()) => {}
                // Slave rejected the timestamp bit (e.g. ERR_PARAM_OUT_OF_RANGE).
                // Retry without timestamp and disable timestamp for the session.
                Err(crate::slave::SlaveError::Protocol(err)) if mode == 0x10 => {
                    tracing::warn!(
                        "SET_DAQ_LIST_MODE with timestamp (mode=0x10) rejected for DAQ list \
                         {}: {}. Slave does not support DAQ timestamps; retrying without \
                         timestamp (mode=0x00) and disabling timestamp for this session.",
                        daq_list,
                        err
                    );
                    timestamp_disabled_by_slave = true;
                    self.timestamp_size = None;
                    self.timestamp_unit = None;
                    self.base_timestamp = None;
                    {
                        let mut config = self.daq_config.write().await;
                        config.timestamp_size = 0;
                        config.timestamp_unit = None;
                    }
                    slave
                        .set_daq_list_mode(0x00, daq_list, list_event_channel, 1, 0)
                        .await
                        .map_err(|e| format!("SET_DAQ_LIST_MODE failed: {}", e))?;
                }
                Err(e) => {
                    return Err(format!("SET_DAQ_LIST_MODE failed: {}", e));
                }
            }
        }
        if timestamp_disabled_by_slave {
            tracing::info!(
                "Timestamp support disabled by slave; DTO timestamps will be absent \
                 and recording will use the local clock."
            );
        }

        tracing::info!(
            "DAQ configuration complete: {} entries configured",
            global_entry_idx
        );

        // Log configuration summary
        {
            let config = self.daq_config.read().await;
            tracing::info!("DAQ Configuration:");
            for entry in &config.odt_entries {
                tracing::info!(
                    "  {}: DAQ={}, ODT={}, entry={}, offset={}",
                    entry.name,
                    entry.daq_list,
                    entry.odt,
                    entry.entry,
                    entry.byte_offset
                );
            }
        }

        Ok(())
    }

    /// Start DAQ acquisition (streaming only, no file recording)
    ///
    /// This starts the DAQ list and begins receiving DTO packets from slave.
    /// Data is only streamed for live display — no files are created.
    /// Call `start_recording()` to begin file recording.
    pub async fn start(&mut self, slave: &XcpSlave, _interval_ms: u64) -> Result<(), String> {
        // If DAQ is already running, stop it first to do a clean restart
        if self.running.load(Ordering::SeqCst) {
            tracing::info!("DAQ already running, stopping first for clean restart...");
            self.stop(slave).await.ok();
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        // Configure DAQ (always clears via FREE_DAQ and rebuilds from scratch)
        self.configure(slave).await?;

        // Get DTO receiver for DAQ reception (must be done before starting DAQ)
        let dto_rx = slave.get_dto_receiver();

        // Select all configured DAQ lists (one per event group).
        // Capture firstPID from each START_STOP_DAQ_LIST response — PIDs are
        // not necessarily contiguous across DAQ lists (e.g. list 0 → 0..2,
        // list 1 → 0x60..0x62), so we need them for DTO routing.
        let daq_list_count = {
            let config = self.daq_config.read().await;
            config.daq_list_count
        };
        let mut first_pid_per_list: Vec<u8> = Vec::with_capacity(daq_list_count as usize);
        for daq_list in 0..daq_list_count {
            tracing::info!("Selecting DAQ list {}", daq_list);
            let first_pid = slave
                .start_stop_daq_list(2, daq_list) // mode=2 (select DAQ list)
                .await
                .map_err(|e| format!("START_STOP_DAQ_LIST failed for list {}: {}", daq_list, e))?;
            tracing::info!(
                "DAQ list {} selected, firstPID=0x{:02X}",
                daq_list,
                first_pid
            );
            first_pid_per_list.push(first_pid);
        }
        {
            let mut config = self.daq_config.write().await;
            config.first_pid_per_list = first_pid_per_list;
        }

        tracing::info!("All {} DAQ lists selected, starting DAQ...", daq_list_count);

        // Query timestamp mode info for later use by start_recording()
        if self.timestamp_unit.is_some() && self.timestamp_size.is_some() {
            tracing::info!(
                "Timestamp mode enabled (size={:?}, unit={:?})",
                self.timestamp_size,
                self.timestamp_unit
            );
        } else {
            tracing::trace!("Timestamp mode not enabled");
        }

        slave
            .start_stop_synch(0x01)
            .await
            .map_err(|e| format!("START_STOP_SYNCH failed: {}", e))?;

        self.running.store(true, Ordering::SeqCst);

        // Store measurement info for recording (needed by start_recording later)
        let selected_measurements = self.selected_measurements();
        if selected_measurements.is_empty() {
            return Err("No measurements selected for DAQ".to_string());
        }
        {
            let measurement_info: Vec<(String, Option<String>)> = selected_measurements
                .iter()
                .map(|m| (m.name.clone(), m.unit.clone()))
                .collect();
            *self.recording_measurement_info.write().await = measurement_info;
        }

        let running = self.running.clone();
        let recording = self.recording.clone();
        let latest_sample = self.latest_sample.clone();
        let daq_config = self.daq_config.clone();
        let timestamp_size = self.timestamp_size.unwrap_or(0);
        let iden_field_type = self.indentification_field_type.clone();
        let selection_changed = self.selection_changed.clone();
        let csv_writers = self.csv_writers.clone();
        let csv_event_signals = self.csv_event_signals.clone();
        let recording_ts_tracker = self.recording_ts_tracker.clone();
        let recording_start_instant = self.recording_start_instant.clone();
        let recording_start_device_ts_us = self.recording_start_device_ts_us.clone();

        #[cfg(feature = "mf4-native")]
        let mf4_writer = self.mf4_writer.clone();
        #[cfg(feature = "mf4-native")]
        let mf4_event_groups = self.mf4_event_groups.clone();
        #[cfg(feature = "mf4-rs-backend")]
        let mf4_writer = self.mf4_writer.clone();
        #[cfg(feature = "mf4-rs-backend")]
        let mf4_cg_id = self.mf4_cg_id.clone();
        #[cfg(feature = "mf4-rs-backend")]
        let recording_meas_info = self.recording_measurement_info.clone();

        let packet_log = self.packet_log.clone();
        let slave_name = self.slave_name.clone();

        // Build TimestampTracker for live display using local clock
        let display_start = std::time::Instant::now();

        // Spawn DTO reception task
        let handle = tokio::spawn(async move {
            tracing::info!("DAQ started, waiting for DTO packets...");

            // Per-DAQ-list accumulated signal values.
            // When a DAQ list has multiple ODTs, each DTO only carries a subset
            // of the signals. We keep the latest known value for every signal so
            // that MF4 and CSV records are always complete — no artificial zeros.
            let mut acc_values: std::collections::HashMap<
                u16,
                std::collections::HashMap<String, f64>,
            > = std::collections::HashMap::new();

            while running.load(Ordering::SeqCst) {
                // Check if selection changed - if so, we need to restart
                if selection_changed.load(Ordering::SeqCst) {
                    tracing::info!("Selection changed - DAQ task will exit for restart");
                    break;
                }

                // Receive DTO packet from channel
                let msg = {
                    let mut rx = dto_rx.lock().await;
                    tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await
                };

                match msg {
                    Ok(Some(msg)) => {
                        // Parse single DTO packet (PID + raw data, no timestamp parsed yet)
                        if let Some(mut data) = parse_dto_packet(&msg.payload, &iden_field_type) {
                            // Get configuration for this DAQ list and ODT
                            let config = daq_config.read().await;

                            // Resolve (daq_list, odt) — for AbsoluteODT mode,
                            // the PID is a global ODT index that we must map to
                            // the concrete DAQ list + relative ODT.
                            let (resolved_daq_list, resolved_odt) = match data.daq_list {
                                Some(dl) => (dl, data.odt),
                                None => match config.resolve_absolute_odt(data.odt) {
                                    Some((dl, odt)) => (dl, odt),
                                    None => {
                                        tracing::warn!(
                                                "Cannot resolve absolute ODT {} (odt_count_per_list={:?})",
                                                data.odt, config.odt_count_per_list
                                            );
                                        continue;
                                    }
                                },
                            };

                            // Per XCP spec, timestamp only exists in ODT 0 of each DAQ list.
                            // Strip timestamp bytes from data and parse them.
                            if resolved_odt == 0 && timestamp_size > 0 {
                                let (ts, remaining) =
                                    extract_odt0_timestamp(&data.data, timestamp_size);
                                data.timestamp = ts;
                                data.data = remaining;
                            }

                            // Log DAQ packet to packet browser (if enabled)
                            if let Some(log) = &packet_log {
                                if log.is_enabled() {
                                    let hex = crate::protocol::packet_display::format_hex_truncated(
                                        &msg.payload,
                                        crate::xcp_packet_log::DAQ_HEX_MAX_BYTES,
                                    );
                                    let entry = crate::xcp_packet_log::XcpPacketEntry {
                                        id: log.next_id(),
                                        timestamp: chrono::Local::now()
                                            .format("%H:%M:%S%.3f")
                                            .to_string(),
                                        source: format!(
                                            "{} DAQ {}/{}",
                                            slave_name, resolved_daq_list, resolved_odt
                                        ),
                                        direction: crate::xcp_packet_log::PacketDirection::Incoming,
                                        hex,
                                        description: "DAQ".to_string(),
                                        packet_class: crate::xcp_packet_log::PacketClass::Daq,
                                        daq_timestamp: if resolved_odt == 0 {
                                            data.timestamp.map(|t| t as u64)
                                        } else {
                                            None
                                        },
                                    };
                                    log.push(entry);
                                }
                            }

                            // Find entries for this ODT
                            let entries: Vec<&OdtEntryConfig> = config
                                .odt_entries
                                .iter()
                                .filter(|e| {
                                    e.daq_list == resolved_daq_list && e.odt == resolved_odt
                                })
                                .collect();

                            if entries.is_empty() {
                                tracing::warn!("No configuration found for DAQ list {}, ODT {} (raw daq_list={:?}, odt={})",
                                    resolved_daq_list, resolved_odt, data.daq_list, data.odt);
                                continue;
                            }

                            tracing::trace!(
                                "Received DTO: daq_list={}, odt={}, data_len={}, entries={}",
                                resolved_daq_list,
                                resolved_odt,
                                data.data.len(),
                                entries.len()
                            );

                            // Parse measurement data using configuration
                            let mut record = DaqRecord::new(0.0, 0);

                            for entry in &entries {
                                let offset = entry.byte_offset;
                                if offset + entry.size <= data.data.len() {
                                    let raw_bytes = &data.data[offset..offset + entry.size];
                                    let is_float = entry.data_type.is_float();
                                    let is_signed = entry.data_type.is_signed_integer();
                                    let physical_value = entry
                                        .compu_method
                                        .bytes_to_display(raw_bytes, is_float, is_signed);
                                    record.add_value(&entry.name, physical_value);
                                    record.add_raw_value(&entry.name, raw_bytes.to_vec());
                                } else {
                                    tracing::warn!("Data too short for {}: need {} bytes at offset {}, have {}",
                                        entry.name, entry.size, offset, data.data.len());
                                }
                            }

                            // Calculate display timestamp (always relative to DAQ start)
                            let display_timestamp_s = display_start.elapsed().as_secs_f64();
                            record.timestamp_s = display_timestamp_s;

                            // Update latest sample for UI (merge with existing to preserve values from other event groups)
                            {
                                let mut latest = latest_sample.write().await;
                                match latest.as_mut() {
                                    Some(existing) => {
                                        existing.timestamp_s = record.timestamp_s;
                                        existing.timestamp_raw = record.timestamp_raw;
                                        for (name, value) in &record.values {
                                            existing.values.insert(name.clone(), *value);
                                            existing
                                                .signal_timestamps
                                                .insert(name.clone(), display_timestamp_s);
                                        }
                                        for (name, raw) in &record.raw_values {
                                            existing.raw_values.insert(name.clone(), raw.clone());
                                        }
                                    }
                                    None => {
                                        let mut r = record.clone();
                                        for name in r.values.keys() {
                                            r.signal_timestamps
                                                .insert(name.clone(), display_timestamp_s);
                                        }
                                        *latest = Some(r);
                                    }
                                }
                            }

                            // If recording is active, write to files
                            if recording.load(Ordering::SeqCst) {
                                // Merge current ODT values into per-DAQ-list
                                // accumulation so that signals from other ODTs of
                                // the same DAQ list keep their last-known value
                                // instead of being zero-filled.
                                {
                                    let daq_values =
                                        acc_values.entry(resolved_daq_list).or_default();
                                    for (name, value) in &record.values {
                                        daq_values.insert(name.clone(), *value);
                                    }
                                }

                                // Calculate recording timestamp (relative to recording start)
                                let rec_ts = {
                                    let mut tracker = recording_ts_tracker.write().await;
                                    let start_instant = recording_start_instant.read().await;
                                    let mut dev_start =
                                        recording_start_device_ts_us.write().await;
                                    let dev_ts = msg.device_timestamp_us;
                                    if let Some(ref mut tracker) = *tracker {
                                        if let Some(raw_ts) = data.timestamp {
                                            tracker.elapsed_seconds(raw_ts)
                                        } else if let Some(ts) = dev_ts {
                                            // Slave has no timestamp: fall back to the
                                            // PCAN device hardware timestamp (µs since
                                            // device init), relative to the first
                                            // recorded frame.
                                            if dev_start.is_none() {
                                                *dev_start = Some(ts);
                                            }
                                            device_ts_elapsed(ts, *dev_start)
                                        } else if let Some(ref start) = *start_instant {
                                            start.elapsed().as_secs_f64()
                                        } else {
                                            display_timestamp_s
                                        }
                                    } else if let Some(ts) = dev_ts {
                                        if dev_start.is_none() {
                                            *dev_start = Some(ts);
                                        }
                                        device_ts_elapsed(ts, *dev_start)
                                    } else if let Some(ref start) = *start_instant {
                                        start.elapsed().as_secs_f64()
                                    } else {
                                        display_timestamp_s
                                    }
                                };

                                // Write CSV line to the correct event file
                                {
                                    let signals = csv_event_signals.read().await;
                                    if let Some(signal_names) = signals.get(&resolved_daq_list) {
                                        let mut csv_guard = csv_writers.lock().await;
                                        if let Some(writer) = csv_guard.get_mut(&resolved_daq_list)
                                        {
                                            let mut csv_line = format!("{:.6}", rec_ts);
                                            let daq_values =
                                                acc_values.entry(resolved_daq_list).or_default();
                                            for name in signal_names {
                                                csv_line.push(',');
                                                if let Some(value) = daq_values.get(name).copied() {
                                                    // Find data type from config entries
                                                    let is_float = config
                                                        .odt_entries
                                                        .iter()
                                                        .find(|e| &e.name == name)
                                                        .map(|e| e.data_type.is_float())
                                                        .unwrap_or(false);
                                                    if is_float {
                                                        csv_line.push_str(&format!("{:.6}", value));
                                                    } else {
                                                        csv_line.push_str(&format!("{:.0}", value));
                                                    }
                                                }
                                            }
                                            if let Err(e) = writeln!(writer, "{}", csv_line) {
                                                tracing::error!("Failed to write CSV: {}", e);
                                            }
                                        }
                                    }
                                }

                                // Write MDF4 record to the correct DG for this event
                                #[cfg(feature = "mf4-native")]
                                {
                                    let event_groups = mf4_event_groups.read().await;
                                    if let Some(eg) = event_groups
                                        .iter()
                                        .find(|g| g.daq_list == resolved_daq_list)
                                    {
                                        let mut mf4_guard = mf4_writer.lock().await;
                                        if let Some(ref mut writer) = *mf4_guard {
                                            if let Err(e) = writer.start_record(eg.dg_index, 0) {
                                                tracing::error!("MF4 start_record failed: {}", e);
                                            } else {
                                                // Set time channel
                                                if let Err(e) =
                                                    writer.set_channel_value("time_s", rec_ts)
                                                {
                                                    tracing::error!("MF4 set time failed: {}", e);
                                                }
                                                // Set signal channels (skip time_s at index 0)
                                                // Read back accumulated values so that
                                                // signals from other ODTs of this DAQ list
                                                // keep their last-known value.
                                                if let Some(daq_values) =
                                                    acc_values.get(&resolved_daq_list)
                                                {
                                                    for ch_name in eg.channel_names.iter().skip(1) {
                                                        let v = daq_values
                                                            .get(ch_name)
                                                            .copied()
                                                            .unwrap_or(0.0);
                                                        if let Err(e) =
                                                            writer.set_channel_value(ch_name, v)
                                                        {
                                                            tracing::error!(
                                                                "MF4 set {} failed: {}",
                                                                ch_name,
                                                                e
                                                            );
                                                        }
                                                    }
                                                } else {
                                                    for ch_name in eg.channel_names.iter().skip(1) {
                                                        let v = record
                                                            .get_value(ch_name)
                                                            .unwrap_or(0.0);
                                                        if let Err(e) =
                                                            writer.set_channel_value(ch_name, v)
                                                        {
                                                            tracing::error!(
                                                                "MF4 set {} failed: {}",
                                                                ch_name,
                                                                e
                                                            );
                                                        }
                                                    }
                                                }
                                                if let Err(e) = writer.flush_record() {
                                                    tracing::error!(
                                                        "MF4 flush_record failed: {}",
                                                        e
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }

                                #[cfg(feature = "mf4-rs-backend")]
                                {
                                    let mut mf4_guard = mf4_writer.lock().await;
                                    let cg_guard = mf4_cg_id.lock().await;
                                    if let (Some(ref mut writer), Some(ref cg_id)) =
                                        (&mut *mf4_guard, &*cg_guard)
                                    {
                                        let meas_info = recording_meas_info.read().await;
                                        let mut mdf_values =
                                            Vec::with_capacity(meas_info.len() + 1);
                                        mdf_values.push(DecodedValue::Float(rec_ts));
                                        let daq_values =
                                            acc_values.entry(resolved_daq_list).or_default();
                                        for (name, _) in meas_info.iter() {
                                            let v = daq_values.get(name).copied().unwrap_or(0.0);
                                            mdf_values.push(DecodedValue::Float(v));
                                        }
                                        if let Err(e) = writer.write_record(cg_id, &mdf_values) {
                                            tracing::error!("Failed to write MDF4 record: {}", e);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        tracing::warn!("DTO channel closed");
                        break;
                    }
                    Err(_) => {
                        // Timeout, continue to check running flag
                        continue;
                    }
                }
            }

            // If recording was still active when task exits, finalize files
            if recording.load(Ordering::SeqCst) {
                recording.store(false, Ordering::SeqCst);

                // Flush all CSV writers
                {
                    let mut csv_guard = csv_writers.lock().await;
                    csv_guard.clear();
                }

                // Finalize MDF4
                #[cfg(feature = "mf4-native")]
                {
                    let mut mf4_guard = mf4_writer.lock().await;
                    if let Some(mut writer) = mf4_guard.take() {
                        if let Err(e) = writer.finalize_with_compact(false) {
                            tracing::warn!("Failed to finalize MDF4 file: {}", e);
                        } else {
                            tracing::info!("MDF4 file finalized successfully (mf4_parse)");
                        }
                    }
                }

                #[cfg(feature = "mf4-rs-backend")]
                {
                    let mut mf4_guard = mf4_writer.lock().await;
                    let cg_guard = mf4_cg_id.lock().await;
                    if let (Some(ref mut writer), Some(ref cg_id)) = (&mut *mf4_guard, &*cg_guard) {
                        if let Err(e) = writer.finish_data_block(cg_id) {
                            tracing::warn!("Failed to finish MDF4 data block: {}", e);
                        }
                        if let Err(e) = writer.finalize() {
                            tracing::warn!("Failed to finalize MDF4 file: {}", e);
                        } else {
                            tracing::info!("MDF4 file finalized successfully (mdf4-rs)");
                        }
                    }
                }
            }

            tracing::info!("DAQ reception stopped");
        });

        // Store task handle
        {
            let mut handle_guard = self.task_handle.write().await;
            *handle_guard = Some(handle);
        }

        Ok(())
    }

    /// Start file recording while DAQ is already streaming
    ///
    /// Sends GET_DAQ_CLOCK to the slave, creates CSV/MF4 files, and begins
    /// writing data. Recording timestamps start from 0.
    pub async fn start_recording(&mut self, slave: &XcpSlave) -> Result<(), String> {
        if !self.running.load(Ordering::SeqCst) {
            return Err("DAQ not running — start DAQ first".to_string());
        }
        if self.recording.load(Ordering::SeqCst) {
            return Err("Already recording".to_string());
        }

        // Get base timestamp from slave if timestamp mode is enabled
        let has_slave_timestamp = self.timestamp_unit.is_some() && self.timestamp_size.is_some();
        if has_slave_timestamp {
            match slave.get_daq_clock().await {
                Ok(base_ts) => {
                    tracing::info!(
                        "Recording: got base timestamp from slave: {} ticks",
                        base_ts
                    );
                    self.base_timestamp = Some(base_ts);
                    let tracker = TimestampTracker::new(
                        base_ts,
                        self.timestamp_size.unwrap(),
                        self.timestamp_unit.as_ref().unwrap(),
                    );
                    *self.recording_ts_tracker.write().await = Some(tracker);
                }
                Err(e) => {
                    tracing::warn!("Failed to get DAQ clock: {}. Using local time.", e);
                    *self.recording_ts_tracker.write().await = None;
                }
            }
        } else {
            *self.recording_ts_tracker.write().await = None;
        }

        // Set recording start instant (fallback for local-clock timestamps)
        *self.recording_start_instant.write().await = Some(std::time::Instant::now());

        // Reset the device-timestamp baseline; it is captured from the first
        // recorded DTO packet (used when the slave provides no timestamps).
        *self.recording_start_device_ts_us.write().await = None;

        // Get selected measurements for CSV header
        let selected_measurements = self.selected_measurements();
        if selected_measurements.is_empty() {
            return Err("No measurements selected for recording".to_string());
        }

        // Create per-event CSV files (only if CSV recording is enabled)
        if self.csv_enabled {
            let config = self.daq_config.read().await;
            let mut event_map: std::collections::BTreeMap<u16, Vec<String>> =
                std::collections::BTreeMap::new();
            let mut seen_per_list: std::collections::HashMap<
                u16,
                std::collections::HashSet<String>,
            > = std::collections::HashMap::new();
            for entry in &config.odt_entries {
                let seen = seen_per_list.entry(entry.daq_list).or_default();
                if seen.insert(entry.name.clone()) {
                    event_map
                        .entry(entry.daq_list)
                        .or_default()
                        .push(entry.name.clone());
                }
            }

            let csv_base = self.csv_path.with_extension("");
            let csv_ext = self
                .csv_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("csv");
            let mut writers = self.csv_writers.lock().await;
            let mut signals = self.csv_event_signals.write().await;
            writers.clear();
            signals.clear();

            for (&daq_list, signal_names) in &event_map {
                let csv_path = if event_map.len() == 1 {
                    self.csv_path.clone()
                } else {
                    PathBuf::from(format!(
                        "{}_event{}.{}",
                        csv_base.display(),
                        daq_list,
                        csv_ext
                    ))
                };

                let mut file = OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&csv_path)
                    .map_err(|e| format!("Failed to create CSV for event {}: {}", daq_list, e))?;

                let mut header = String::from("time_s");
                for name in signal_names {
                    header.push(',');
                    header.push_str(name);
                }
                writeln!(file, "{}", header)
                    .map_err(|e| format!("Failed to write CSV header: {}", e))?;

                let csv_file = OpenOptions::new()
                    .append(true)
                    .open(&csv_path)
                    .map_err(|e| format!("Failed to open CSV for appending: {}", e))?;
                writers.insert(daq_list, std::io::BufWriter::new(csv_file));
                signals.insert(daq_list, signal_names.clone());

                tracing::info!(
                    "CSV: {} for event {} with {} signals",
                    csv_path.display(),
                    daq_list,
                    signal_names.len()
                );
            }
        }

        // Create MDF4 writer (only if MF4 recording is enabled)

        #[cfg(feature = "mf4-native")]
        if self.mf4_enabled {
            let mf4_path = self.mdf4_path.clone();
            let config = self.daq_config.read().await;

            // Group OdtEntryConfig by daq_list to build per-event DGs
            let mut event_map: std::collections::BTreeMap<u16, Vec<&OdtEntryConfig>> =
                std::collections::BTreeMap::new();
            for entry in &config.odt_entries {
                event_map.entry(entry.daq_list).or_default().push(entry);
            }

            let metadata = Mf4Metadata::default();
            match Mf4StreamWriter::new(mf4_path.clone(), metadata) {
                Ok(mut writer) => {
                    let mut event_groups = Vec::new();

                    for (dg_index, (&daq_list, entries)) in event_map.iter().enumerate() {
                        // Look up the real event name from available_events
                        let event_channel = config
                            .event_channel_per_list
                            .get(daq_list as usize)
                            .copied()
                            .unwrap_or(daq_list);
                        let group_name = self
                            .available_events
                            .iter()
                            .find(|ev| ev.channel == event_channel)
                            .map(|ev| ev.name.clone())
                            .unwrap_or_else(|| format!("event_{}", daq_list));
                        let mut cg_builder = ChannelGroupDefBuilder::new()
                            .name(&group_name)
                            .with_time_channel("time_s");

                        let mut channel_names = vec!["time_s".to_string()];
                        // Deduplicate signal names within same DAQ list
                        let mut seen = std::collections::HashSet::new();
                        for entry in entries {
                            if seen.insert(&entry.name) {
                                let unit = entry.unit.as_deref().unwrap_or("");
                                if let CompuMethod::CompuVtab {
                                    entries: vtab_entries,
                                    ..
                                } = &entry.compu_method
                                {
                                    // Enum channel: FLOAT64 raw data + Value2Text CC block
                                    let (keys, texts): (Vec<f64>, Vec<String>) = vtab_entries
                                        .iter()
                                        .map(|e| (e.raw_value as f64, e.text_value.clone()))
                                        .unzip();
                                    let ch = ChannelDef::new(&entry.name)
                                        .data_type(4)
                                        .bit_count(64)
                                        .unit(unit)
                                        .vtab(keys, texts, String::new());
                                    cg_builder = cg_builder.channel(ch);
                                } else {
                                    cg_builder = cg_builder.add_f64_channel(&entry.name, unit);
                                }
                                channel_names.push(entry.name.clone());
                            }
                        }

                        match cg_builder.build() {
                            Ok(cg) => match StreamingDataGroup::new(cg) {
                                Ok(dg) => {
                                    if let Err(e) = writer.add_data_group(dg) {
                                        tracing::warn!(
                                            "Failed to add DG for event {}: {}",
                                            daq_list,
                                            e
                                        );
                                        continue;
                                    }
                                    event_groups.push(Mf4EventGroup {
                                        daq_list,
                                        dg_index,
                                        channel_names,
                                    });
                                    tracing::info!(
                                        "MF4: DG {} for event {} with {} channels",
                                        dg_index,
                                        daq_list,
                                        entries.len()
                                    );
                                }
                                Err(e) => tracing::warn!(
                                    "Failed to create DG for event {}: {}",
                                    daq_list,
                                    e
                                ),
                            },
                            Err(e) => {
                                tracing::warn!("Failed to build CG for event {}: {}", daq_list, e)
                            }
                        }
                    }

                    if !event_groups.is_empty() {
                        if let Err(e) = writer.finalize_structure() {
                            tracing::warn!("Failed to finalize MF4 structure: {}", e);
                        } else {
                            *self.mf4_writer.lock().await = Some(writer);
                            *self.mf4_event_groups.write().await = event_groups;
                            tracing::info!("MDF4 recording started with multi-DG (mf4_parse)");
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to create MDF4 file: {}", e);
                }
            }
        }

        #[cfg(feature = "mf4-rs-backend")]
        if self.mf4_enabled {
            let mf4_path = self.mdf4_path.clone();
            let mf4_measurement_info = self.recording_measurement_info.read().await.clone();
            match MdfWriter::new(mf4_path.to_str().unwrap_or("daq_data.mf4")) {
                Ok(w) => {
                    let mut writer = w.with_flush_policy(FlushPolicy::EveryNRecords(1000));
                    match Self::init_mdf4_channels(&mut writer, &mf4_measurement_info) {
                        Ok(cg_id) => {
                            *self.mf4_writer.lock().await = Some(writer);
                            *self.mf4_cg_id.lock().await = Some(cg_id);
                            tracing::info!("MDF4 recording started (mdf4-rs)");
                        }
                        Err(e) => {
                            tracing::warn!("Failed to init MDF4 channels: {}", e);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to create MDF4 file: {}", e);
                }
            }
        }

        self.recording.store(true, Ordering::SeqCst);
        let csv_info = if self.csv_enabled {
            self.csv_path.display().to_string()
        } else {
            "(disabled)".to_string()
        };
        let mf4_info = if self.mf4_enabled {
            self.mdf4_path.display().to_string()
        } else {
            "(disabled)".to_string()
        };
        tracing::info!("Recording started — CSV: {}, MF4: {}", csv_info, mf4_info);
        Ok(())
    }

    /// Stop file recording (DAQ streaming continues)
    ///
    /// Finalizes CSV and MDF4 files. DAQ remains running for live display.
    pub async fn stop_recording(&self) -> Result<(), String> {
        if !self.recording.load(Ordering::SeqCst) {
            return Err("Not recording".to_string());
        }

        // Signal background task to stop writing
        self.recording.store(false, Ordering::SeqCst);

        // Small delay to let any in-flight writes complete
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Close all CSV writers
        {
            let mut csv_guard = self.csv_writers.lock().await;
            csv_guard.clear(); // flush & close all
        }
        self.csv_event_signals.write().await.clear();

        // Finalize MDF4
        #[cfg(feature = "mf4-native")]
        {
            let mut mf4_guard = self.mf4_writer.lock().await;
            if let Some(mut writer) = mf4_guard.take() {
                if let Err(e) = writer.finalize_with_compact(false) {
                    tracing::warn!("Failed to finalize MDF4 file: {}", e);
                } else {
                    tracing::info!("MDF4 file finalized successfully (mf4_parse)");
                }
            }
            self.mf4_event_groups.write().await.clear();
        }

        #[cfg(feature = "mf4-rs-backend")]
        {
            let mut mf4_guard = self.mf4_writer.lock().await;
            let cg_guard = self.mf4_cg_id.lock().await;
            if let (Some(ref mut writer), Some(ref cg_id)) = (&mut *mf4_guard, &*cg_guard) {
                if let Err(e) = writer.finish_data_block(cg_id) {
                    tracing::warn!("Failed to finish MDF4 data block: {}", e);
                }
                if let Err(e) = writer.finalize() {
                    tracing::warn!("Failed to finalize MDF4 file: {}", e);
                } else {
                    tracing::info!("MDF4 file finalized successfully (mdf4-rs)");
                }
            }
        }

        // Clear recording state
        *self.recording_ts_tracker.write().await = None;
        *self.recording_start_instant.write().await = None;
        *self.recording_start_device_ts_us.write().await = None;

        tracing::info!(
            "Recording stopped. Data saved to {} and {}",
            self.csv_path.display(),
            self.mdf4_path.display()
        );
        Ok(())
    }

    /// Check if recording is active
    pub fn is_recording(&self) -> bool {
        self.recording.load(Ordering::SeqCst)
    }

    /// Get recording statistics (duration in seconds, file sizes in bytes)
    pub async fn recording_stats(&self) -> (f64, u64, u64) {
        let duration = if let Some(start) = *self.recording_start_instant.read().await {
            start.elapsed().as_secs_f64()
        } else {
            0.0
        };

        let csv_size = std::fs::metadata(&self.csv_path)
            .map(|m| m.len())
            .unwrap_or(0);
        let mf4_size = std::fs::metadata(&self.mdf4_path)
            .map(|m| m.len())
            .unwrap_or(0);

        (duration, csv_size, mf4_size)
    }

    /// Stop DAQ acquisition
    ///
    /// If recording is active, stops recording first (finalizes files).
    pub async fn stop(&self, slave: &XcpSlave) -> Result<(), String> {
        if !self.running.load(Ordering::SeqCst) {
            return Err("DAQ not running".to_string());
        }

        // Stop recording first if active
        if self.recording.load(Ordering::SeqCst) {
            if let Err(e) = self.stop_recording().await {
                tracing::warn!("Error stopping recording during DAQ stop: {}", e);
            }
        }

        // First, stop DAQ on slave side to prevent further DTO packets
        tracing::info!("Stopping DAQ list on slave...");
        if let Err(e) = slave.start_stop_synch(0x00).await {
            tracing::warn!("Failed to send STOP_SYNCH: {}", e);
        }

        // Small delay to allow any in-flight packets to arrive
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Signal reception task to stop
        self.running.store(false, Ordering::SeqCst);

        // Wait for task to finish
        {
            let mut handle_guard = self.task_handle.write().await;
            if let Some(handle) = handle_guard.take() {
                handle.await.ok();
            }
        }

        tracing::info!("DAQ acquisition stopped");
        Ok(())
    }

    /// Initialize MDF4 file structure with time channel and measurement channels (mdf4-rs backend).
    /// Returns the channel group ID for use in `write_record`.
    #[cfg(feature = "mf4-rs-backend")]
    fn init_mdf4_channels(
        writer: &mut MdfWriter<mdf4_rs::writer::FileWriter>,
        measurements: &[(String, Option<String>)],
    ) -> Result<String, String> {
        writer
            .init_mdf_file()
            .map_err(|e| format!("MDF4 init: {}", e))?;

        let cg_id = writer
            .add_channel_group(None, |_| {})
            .map_err(|e| format!("MDF4 add_channel_group: {}", e))?;

        // Time master channel (f64, seconds)
        let time_ch = writer
            .add_channel(&cg_id, None, |ch| {
                ch.data_type = MdfDataType::FloatLE;
                ch.name = Some("time_s".into());
                ch.bit_count = 64;
            })
            .map_err(|e| format!("MDF4 add time channel: {}", e))?;
        writer
            .set_time_channel(&time_ch)
            .map_err(|e| format!("MDF4 set_time_channel: {}", e))?;
        writer
            .set_channel_unit(&time_ch, "s")
            .map_err(|e| format!("MDF4 set_channel_unit: {}", e))?;

        // Signal channels (all f64 — physical values after COMPU_METHOD conversion)
        let mut prev_ch = time_ch;
        for (name, unit) in measurements {
            let ch = writer
                .add_channel(&cg_id, Some(&prev_ch), |ch| {
                    ch.data_type = MdfDataType::FloatLE;
                    ch.name = Some(name.clone());
                    ch.bit_count = 64;
                })
                .map_err(|e| format!("MDF4 add channel '{}': {}", name, e))?;
            if let Some(u) = unit {
                writer
                    .set_channel_unit(&ch, u)
                    .map_err(|e| format!("MDF4 set unit: {}", e))?;
            }
            prev_ch = ch;
        }

        // Start the data block for streaming writes
        writer
            .start_data_block_for_cg(&cg_id, 0)
            .map_err(|e| format!("MDF4 start_data_block: {}", e))?;

        Ok(cg_id)
    }

    /// Check if DAQ is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Monitor stdin for a quit command ("q" followed by Enter).
    ///
    /// Reads stdin line by line. When a line containing exactly "q" (after trimming)
    /// is received, this method:
    /// 1. Stops recording (finalizes CSV and MF4 files)
    /// 2. Stops the DAQ acquisition
    ///
    /// This is a blocking async method — it will not return until EOF or "q" is entered.
    /// Callers typically spawn this in a background task.
    pub async fn wait_stdin_quit(&self, slave: &XcpSlave) -> Result<(), String> {
        let mut reader = tokio::io::BufReader::new(tokio::io::stdin());
        let mut line = String::new();

        loop {
            line.clear();
            let n = reader
                .read_line(&mut line)
                .await
                .map_err(|e| format!("Failed to read stdin: {}", e))?;

            if n == 0 {
                // EOF
                tracing::info!("stdin EOF reached, stopping DAQ...");
                break;
            }

            let trimmed = line.trim();
            if trimmed == "q" {
                tracing::info!("Received quit command via stdin, stopping DAQ and saving files...");

                // Stop recording first (finalizes CSV and MF4 files)
                if self.recording.load(Ordering::SeqCst) {
                    if let Err(e) = self.stop_recording().await {
                        tracing::warn!("Error stopping recording during stdin quit: {}", e);
                    }
                }

                // Stop DAQ
                if self.running.load(Ordering::SeqCst) {
                    if let Err(e) = self.stop(slave).await {
                        tracing::warn!("Error stopping DAQ during stdin quit: {}", e);
                    }
                }

                tracing::info!("DAQ stopped via stdin quit command");
                break;
            }
        }

        Ok(())
    }

    /// Spawn a background task to monitor stdin for quit command.
    ///
    /// When the user types "q" + Enter on stdin, the task stops recording
    /// (saving MF4 file) and stops DAQ acquisition.
    ///
    /// Returns a `JoinHandle` that the caller can await or detach.
    pub fn spawn_stdin_quit_monitor(
        self: Arc<Self>,
        slave: Arc<TokioMutex<XcpSlave>>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(tokio::io::stdin());
            let mut line = String::new();

            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        tracing::info!("stdin EOF reached (spawned monitor)");
                        break;
                    }
                    Ok(_) => {
                        let trimmed = line.trim();
                        if trimmed == "q" {
                            tracing::info!("Received quit command via stdin (spawned monitor), stopping DAQ...");

                            // Stop recording first (finalizes CSV and MF4 files)
                            if self.recording.load(Ordering::SeqCst) {
                                if let Err(e) = self.stop_recording().await {
                                    tracing::warn!("Error stopping recording during stdin quit: {}", e);
                                }
                            }

                            // Stop DAQ
                            if self.running.load(Ordering::SeqCst) {
                                let guard = slave.lock().await;
                                if let Err(e) = self.stop(&*guard).await {
                                    tracing::warn!("Error stopping DAQ during stdin quit: {}", e);
                                }
                            }

                            tracing::info!("DAQ stopped via stdin quit command (spawned monitor)");
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to read stdin in spawned monitor: {}", e);
                        break;
                    }
                }
            }
        })
    }

    /// Restart DAQ if selection changed
    /// This should be called periodically to check if DAQ needs restart
    pub async fn restart_if_needed(&mut self, slave: &XcpSlave) -> Result<bool, String> {
        if self.selection_changed.load(Ordering::SeqCst) && self.running.load(Ordering::SeqCst) {
            tracing::info!("Restarting DAQ due to selection change...");

            // Stop DAQ
            self.stop(slave).await.ok();

            // Clear the flag
            self.selection_changed.store(false, Ordering::SeqCst);

            // Wait a moment for cleanup
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            // Restart DAQ
            self.start(slave, 0).await?;

            tracing::info!("DAQ restarted successfully");
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

impl Default for DaqManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse a single DTO packet from slave
///
/// DTO packet payload format:
/// - 1 byte: PID / identification start
/// - N bytes: ODT data (including identification field and timestamp)
fn parse_dto_packet(packet: &[u8], ind_field_type: &IdentificationFieldType) -> Option<DtoData> {
    if packet.is_empty() {
        tracing::error!("DTO packet too short: {} bytes", packet.len());
        return None;
    }

    let mut cur = Cursor::new(packet);

    // Parse identification field based on type
    let (daq_list, odt, pid_size) = match ind_field_type {
        IdentificationFieldType::AbosulteODT => {
            let mut odt_buff = [0u8];
            cur.read_exact(&mut odt_buff).ok()?;
            (None, odt_buff[0], 1usize)
        }
        IdentificationFieldType::RelatetiveODTBYTE => {
            let mut odt_buff = [0u8];
            cur.read_exact(&mut odt_buff).ok()?;
            let mut daq_buff = [0u8];
            cur.read_exact(&mut daq_buff).ok()?;
            (Some(daq_buff[0] as u16), odt_buff[0], 2usize)
        }
        IdentificationFieldType::RelatetiveODTWORD => {
            let mut odt_buff = [0u8];
            cur.read_exact(&mut odt_buff).ok()?;
            let mut daq_buff = [0u8; 2];
            cur.read_exact(&mut daq_buff).ok()?;
            (Some(u16::from_le_bytes(daq_buff)), odt_buff[0], 3usize)
        }
        IdentificationFieldType::RelatetiveODTWORDAlign => {
            let mut odt_buff = [0u8; 2]; // FILL byte + ODT
            cur.read_exact(&mut odt_buff).ok()?;
            let mut daq_buff = [0u8; 2];
            cur.read_exact(&mut daq_buff).ok()?;
            (Some(u16::from_le_bytes(daq_buff)), odt_buff[0], 4usize)
        }
    };

    // NOTE: Timestamp is NOT parsed here. Per XCP spec, timestamp only
    // appears in ODT 0 of each DAQ list. The caller must strip the
    // timestamp bytes after resolving which ODT is ODT 0.
    let mut data = Vec::with_capacity(packet.len().saturating_sub(pid_size));
    cur.read_to_end(&mut data).ok()?;

    Some(DtoData {
        daq_list,
        odt,
        timestamp: None,
        data,
    })
}

/// Extract timestamp from the beginning of a DTO data buffer (ODT 0 only).
/// Returns `(timestamp, data_without_timestamp)`.
fn extract_odt0_timestamp(data: &[u8], timestamp_size: u8) -> (Option<u32>, Vec<u8>) {
    let ts_len = timestamp_size as usize;
    if ts_len == 0 || data.len() < ts_len {
        return (None, data.to_vec());
    }
    let timestamp = match timestamp_size {
        1 => Some(data[0] as u32),
        2 => Some(u16::from_le_bytes([data[0], data[1]]) as u32),
        4 => Some(u32::from_le_bytes([data[0], data[1], data[2], data[3]])),
        _ => None,
    };
    (timestamp, data[ts_len..].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a DaqManager with N dummy U32 measurements, all selected
    fn make_daq_manager(n: usize) -> DaqManager {
        let mut mgr = DaqManager::new();
        mgr.all_measurements = (0..n)
            .map(|i| Measurement {
                name: format!("sig_{}", i),
                address: 0x1000 + (i as u32) * 4,
                address_ext: 0,
                size: 4,
                data_type: A2lDataType::U32,
                unit: None,
                compu_method: CompuMethod::Identical,
            })
            .collect();
        mgr.selected_indices = (0..n).collect();
        mgr
    }

    // ── Per-measurement event methods ──────────────────────────

    #[test]
    fn test_get_measurement_event_fallback_to_default() {
        let mut mgr = make_daq_manager(4);
        mgr.event_channel = 5;
        // No per-measurement override → returns default
        assert_eq!(mgr.get_measurement_event(0), 5);
        assert_eq!(mgr.get_measurement_event(3), 5);
    }

    #[test]
    fn test_set_and_get_measurement_event() {
        let mut mgr = make_daq_manager(4);
        mgr.event_channel = 0;
        mgr.set_measurement_event(1, 10);
        mgr.set_measurement_event(3, 20);
        assert_eq!(mgr.get_measurement_event(0), 0); // default
        assert_eq!(mgr.get_measurement_event(1), 10);
        assert_eq!(mgr.get_measurement_event(2), 0); // default
        assert_eq!(mgr.get_measurement_event(3), 20);
    }

    #[test]
    fn test_set_measurement_event_out_of_bounds_ignored() {
        let mut mgr = make_daq_manager(2);
        mgr.set_measurement_event(99, 7);
        // Out-of-bounds index should not be inserted
        assert!(mgr.measurement_events.is_empty());
    }

    #[test]
    fn test_set_measurement_events_batch() {
        let mut mgr = make_daq_manager(5);
        mgr.set_measurement_events_batch(&[(0, 1), (2, 2), (4, 3), (99, 9)]);
        assert_eq!(mgr.get_measurement_event(0), 1);
        assert_eq!(mgr.get_measurement_event(1), 0); // default
        assert_eq!(mgr.get_measurement_event(2), 2);
        assert_eq!(mgr.get_measurement_event(4), 3);
        // out-of-bounds 99 not stored
        assert!(!mgr.measurement_events.contains_key(&99));
    }

    // ── calculate_odt_allocation tests ────────────────────────

    #[test]
    fn test_allocation_empty_selection() {
        let mut mgr = make_daq_manager(5);
        mgr.selected_indices.clear();
        let alloc = mgr.calculate_odt_allocation(255, 1, 65535, 65535).unwrap();
        assert_eq!(alloc.daq_list_count, 0);
        assert!(alloc.daq_lists.is_empty());
        assert!(alloc.event_channel_per_list.is_empty());
    }

    #[test]
    fn test_allocation_single_event_all_signals() {
        // 5 signals, all on default event 0 → 1 DAQ list, 1 ODT, 5 entries
        let mgr = make_daq_manager(5);
        let alloc = mgr.calculate_odt_allocation(255, 1, 65535, 65535).unwrap();
        assert_eq!(alloc.daq_list_count, 1);
        assert_eq!(alloc.event_channel_per_list, vec![0]);
        assert_eq!(alloc.daq_lists.len(), 1);
        assert_eq!(alloc.daq_lists[0].odts.len(), 1);
        assert_eq!(alloc.daq_lists[0].odts[0].entries.len(), 5);
    }

    #[test]
    fn test_allocation_two_events_separate_lists() {
        // 6 signals: [0,1,2] on event 1, [3,4,5] on event 2
        let mut mgr = make_daq_manager(6);
        mgr.set_measurement_event(0, 1);
        mgr.set_measurement_event(1, 1);
        mgr.set_measurement_event(2, 1);
        mgr.set_measurement_event(3, 2);
        mgr.set_measurement_event(4, 2);
        mgr.set_measurement_event(5, 2);

        let alloc = mgr.calculate_odt_allocation(255, 1, 65535, 65535).unwrap();
        assert_eq!(alloc.daq_list_count, 2);
        // BTreeMap orders by key, so event 1 first, then event 2
        assert_eq!(alloc.event_channel_per_list, vec![1, 2]);
        assert_eq!(alloc.daq_lists[0].odts[0].entries.len(), 3);
        assert_eq!(alloc.daq_lists[1].odts[0].entries.len(), 3);
    }

    #[test]
    fn test_allocation_three_events_mixed() {
        // 8 signals: indices 0,1 → event 10; 2,3,4 → event 5; 5,6,7 → event 20
        let mut mgr = make_daq_manager(8);
        mgr.set_measurement_event(0, 10);
        mgr.set_measurement_event(1, 10);
        mgr.set_measurement_event(2, 5);
        mgr.set_measurement_event(3, 5);
        mgr.set_measurement_event(4, 5);
        mgr.set_measurement_event(5, 20);
        mgr.set_measurement_event(6, 20);
        mgr.set_measurement_event(7, 20);

        let alloc = mgr.calculate_odt_allocation(255, 1, 65535, 65535).unwrap();
        assert_eq!(alloc.daq_list_count, 3);
        // BTreeMap: sorted by event channel → 5, 10, 20
        assert_eq!(alloc.event_channel_per_list, vec![5, 10, 20]);
        // event 5 has 3 signals, event 10 has 2, event 20 has 3
        assert_eq!(alloc.daq_lists[0].odts[0].entries.len(), 3);
        assert_eq!(alloc.daq_lists[1].odts[0].entries.len(), 2);
        assert_eq!(alloc.daq_lists[2].odts[0].entries.len(), 3);
    }

    #[test]
    fn test_allocation_partial_overrides_use_default() {
        // 4 signals: 0 → event 3, rest use default event 0
        let mut mgr = make_daq_manager(4);
        mgr.event_channel = 0;
        mgr.set_measurement_event(0, 3);

        let alloc = mgr.calculate_odt_allocation(255, 1, 65535, 65535).unwrap();
        assert_eq!(alloc.daq_list_count, 2);
        // event 0 (default, 3 signals), event 3 (1 signal)
        assert_eq!(alloc.event_channel_per_list, vec![0, 3]);
        assert_eq!(alloc.daq_lists[0].odts[0].entries.len(), 3);
        assert_eq!(alloc.daq_lists[1].odts[0].entries.len(), 1);
    }

    #[test]
    fn test_allocation_measurement_too_large() {
        let mut mgr = DaqManager::new();
        mgr.all_measurements.push(Measurement {
            name: "big_signal".to_string(),
            address: 0x1000,
            address_ext: 0,
            size: 16, // 16 bytes
            data_type: A2lDataType::U8,
            unit: None,
            compu_method: CompuMethod::Identical,
        });
        mgr.selected_indices = vec![0];
        // max_odt_entry_size = 8, signal is 16 → should fail
        let result = mgr.calculate_odt_allocation(8, 1, 65535, 65535);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("exceeds max ODT entry size"));
    }

    #[test]
    fn test_allocation_preserves_measurement_indices() {
        // Verify that measurement_idx in entries refers to selected_measurements position
        let mut mgr = make_daq_manager(4);
        mgr.set_measurement_event(0, 1);
        mgr.set_measurement_event(1, 1);
        mgr.set_measurement_event(2, 2);
        mgr.set_measurement_event(3, 2);

        let alloc = mgr.calculate_odt_allocation(255, 1, 65535, 65535).unwrap();
        // event 1 list: sel_idx 0, 1
        let entries_ev1: Vec<usize> = alloc.daq_lists[0].odts[0]
            .entries
            .iter()
            .map(|e| e.measurement_idx)
            .collect();
        assert_eq!(entries_ev1, vec![0, 1]);
        // event 2 list: sel_idx 2, 3
        let entries_ev2: Vec<usize> = alloc.daq_lists[1].odts[0]
            .entries
            .iter()
            .map(|e| e.measurement_idx)
            .collect();
        assert_eq!(entries_ev2, vec![2, 3]);
    }

    // ── ODT overflow / boundary tests ─────────────────────────

    #[test]
    fn test_allocation_exactly_255_entries_one_odt() {
        let mgr = make_daq_manager(255);
        let alloc = mgr.calculate_odt_allocation(255, 1, 65535, 65535).unwrap();
        assert_eq!(alloc.daq_list_count, 1);
        assert_eq!(alloc.daq_lists[0].odts.len(), 1);
        assert_eq!(alloc.daq_lists[0].odts[0].entries.len(), 255);
    }

    #[test]
    fn test_allocation_256_entries_spills_to_second_odt() {
        let mgr = make_daq_manager(256);
        let alloc = mgr.calculate_odt_allocation(255, 1, 65535, 65535).unwrap();
        assert_eq!(alloc.daq_list_count, 1);
        assert_eq!(alloc.daq_lists[0].odts.len(), 2);
        assert_eq!(alloc.daq_lists[0].odts[0].entries.len(), 255);
        assert_eq!(alloc.daq_lists[0].odts[1].entries.len(), 1);
    }

    #[test]
    fn test_allocation_510_entries_two_full_odts() {
        let mgr = make_daq_manager(510);
        let alloc = mgr.calculate_odt_allocation(255, 1, 65535, 65535).unwrap();
        assert_eq!(alloc.daq_list_count, 1);
        assert_eq!(alloc.daq_lists[0].odts.len(), 2);
        assert_eq!(alloc.daq_lists[0].odts[0].entries.len(), 255);
        assert_eq!(alloc.daq_lists[0].odts[1].entries.len(), 255);
    }

    #[test]
    fn test_allocation_single_measurement() {
        let mgr = make_daq_manager(1);
        let alloc = mgr.calculate_odt_allocation(255, 1, 65535, 65535).unwrap();
        assert_eq!(alloc.daq_list_count, 1);
        assert_eq!(alloc.daq_lists[0].odts.len(), 1);
        assert_eq!(alloc.daq_lists[0].odts[0].entries.len(), 1);
        assert_eq!(alloc.event_channel_per_list, vec![0]);
    }

    #[test]
    fn test_allocation_odt_overflow_per_event_group() {
        // 256 signals on event 1, 2 signals on event 2
        // Event 1 should get 2 ODTs (255+1), event 2 gets 1 ODT (2)
        let mut mgr = make_daq_manager(258);
        for i in 0..256 {
            mgr.set_measurement_event(i, 1);
        }
        mgr.set_measurement_event(256, 2);
        mgr.set_measurement_event(257, 2);

        let alloc = mgr.calculate_odt_allocation(255, 1, 65535, 65535).unwrap();
        assert_eq!(alloc.daq_list_count, 2);
        assert_eq!(alloc.event_channel_per_list, vec![1, 2]);
        // Event 1 list: 2 ODTs
        assert_eq!(alloc.daq_lists[0].odts.len(), 2);
        assert_eq!(alloc.daq_lists[0].odts[0].entries.len(), 255);
        assert_eq!(alloc.daq_lists[0].odts[1].entries.len(), 1);
        // Event 2 list: 1 ODT
        assert_eq!(alloc.daq_lists[1].odts.len(), 1);
        assert_eq!(alloc.daq_lists[1].odts[0].entries.len(), 2);
    }

    #[test]
    fn test_allocation_odt_count_per_list_accurate() {
        // 300 signals on event 0 → 1 list, 2 ODTs (255 + 45)
        let mgr = make_daq_manager(300);
        let alloc = mgr.calculate_odt_allocation(255, 1, 65535, 65535).unwrap();
        assert_eq!(alloc.odt_count_per_list, vec![2]);
        assert_eq!(alloc.daq_lists[0].odts[0].entries.len(), 255);
        assert_eq!(alloc.daq_lists[0].odts[1].entries.len(), 45);
    }

    #[test]
    fn test_allocation_mixed_sizes_same_event() {
        let mut mgr = DaqManager::new();
        // Mix of U8 (1 byte), U16 (2 bytes), U32 (4 bytes)
        mgr.all_measurements = vec![
            Measurement {
                name: "byte_sig".into(),
                address: 0x1000,
                address_ext: 0,
                size: 1,
                data_type: A2lDataType::U8,
                unit: None,
                compu_method: CompuMethod::Identical,
            },
            Measurement {
                name: "word_sig".into(),
                address: 0x1001,
                address_ext: 0,
                size: 2,
                data_type: A2lDataType::U16,
                unit: None,
                compu_method: CompuMethod::Identical,
            },
            Measurement {
                name: "dword_sig".into(),
                address: 0x1003,
                address_ext: 0,
                size: 4,
                data_type: A2lDataType::U32,
                unit: None,
                compu_method: CompuMethod::Identical,
            },
        ];
        mgr.selected_indices = vec![0, 1, 2];

        let alloc = mgr.calculate_odt_allocation(4, 1, 65535, 65535).unwrap();
        assert_eq!(alloc.daq_list_count, 1);
        assert_eq!(alloc.daq_lists[0].odts[0].entries.len(), 3);
    }

    #[test]
    fn test_allocation_mixed_sizes_one_too_large() {
        let mut mgr = DaqManager::new();
        mgr.all_measurements = vec![
            Measurement {
                name: "ok_sig".into(),
                address: 0x1000,
                address_ext: 0,
                size: 4,
                data_type: A2lDataType::U32,
                unit: None,
                compu_method: CompuMethod::Identical,
            },
            Measurement {
                name: "big_sig".into(),
                address: 0x1004,
                address_ext: 0,
                size: 8,
                data_type: A2lDataType::F64,
                unit: None,
                compu_method: CompuMethod::Identical,
            },
        ];
        mgr.selected_indices = vec![0, 1];
        // max_odt_entry_size = 4, F64 is 8 → error
        let result = mgr.calculate_odt_allocation(4, 1, 65535, 65535);
        assert!(result.is_err());
    }

    // ── DaqRecord merging tests ───────────────────────────────

    #[test]
    fn test_daq_record_value_storage() {
        let mut record = DaqRecord::new(1.234, 1234);
        assert!((record.timestamp_s - 1.234).abs() < 1e-9);
        assert_eq!(record.timestamp_raw, 1234);
        record.add_value("temp", 25.5);
        record.add_value("rpm", 1500.0);
        assert_eq!(record.get_value("temp"), Some(25.5));
        assert_eq!(record.get_value("rpm"), Some(1500.0));
        assert_eq!(record.get_value("unknown"), None);
    }

    #[test]
    fn test_daq_record_overwrite_value() {
        let mut record = DaqRecord::new(0.0, 0);
        record.add_value("temp", 20.0);
        record.add_value("temp", 30.0);
        assert_eq!(record.get_value("temp"), Some(30.0));
    }

    // ── TimestampTracker tests ────────────────────────────────

    #[test]
    fn test_tracker_u16_basic_no_wrap() {
        // u16, 1ms/tick, t0=60000
        let mut tracker = TimestampTracker::new(60000, 2, &TimestampUnit::DAQTIMESTAMPUNIT1MS);
        let t = tracker.elapsed_seconds(60100);
        assert!((t - 0.100).abs() < 1e-9, "expected 0.100, got {}", t);
        let t = tracker.elapsed_seconds(60200);
        assert!((t - 0.200).abs() < 1e-9, "expected 0.200, got {}", t);
    }

    #[test]
    fn test_tracker_u16_single_wrap() {
        // u16, 1ms/tick, t0=60000. After wrapping past 65535 → 100
        let mut tracker = TimestampTracker::new(60000, 2, &TimestampUnit::DAQTIMESTAMPUNIT1MS);
        // Before wrap
        let t = tracker.elapsed_seconds(65400);
        assert!((t - 5.400).abs() < 1e-9, "expected 5.400, got {}", t);
        // At max
        let t = tracker.elapsed_seconds(65535);
        assert!((t - 5.535).abs() < 1e-9, "expected 5.535, got {}", t);
        // After wrap
        let t = tracker.elapsed_seconds(100);
        assert!((t - 5.636).abs() < 1e-6, "expected 5.636, got {}", t);
        let t = tracker.elapsed_seconds(200);
        assert!((t - 5.736).abs() < 1e-6, "expected 5.736, got {}", t);
    }

    #[test]
    fn test_tracker_u16_multi_wrap() {
        // u16, 1ms/tick, t0=0. Simulate regular packet arrival (step=10000) through 2+ wraps.
        let mut tracker = TimestampTracker::new(0, 2, &TimestampUnit::DAQTIMESTAMPUNIT1MS);

        // 13 steps × 10000 ticks = 130000 ticks elapsed (2 full u16 cycles)
        let mut raw: u32 = 0;
        for _ in 0..13 {
            raw = (raw + 10000) % 65536;
            tracker.elapsed_seconds(raw);
        }
        // One more step of 5000 → total 135000 ticks = 135.0s
        raw = (raw + 5000) % 65536;
        let t = tracker.elapsed_seconds(raw);
        assert!((t - 135.0).abs() < 1e-3, "expected 135.0, got {}", t);
    }

    #[test]
    fn test_tracker_u8_fast_wrap() {
        // u8, 1ms/tick, t0=200
        let mut tracker = TimestampTracker::new(200, 1, &TimestampUnit::DAQTIMESTAMPUNIT1MS);
        let t = tracker.elapsed_seconds(250);
        assert!((t - 0.050).abs() < 1e-9, "expected 0.050, got {}", t);
        // Wrap: 250 → 10
        let t = tracker.elapsed_seconds(10);
        assert!((t - 0.066).abs() < 1e-3, "expected ~0.066, got {}", t);
    }

    #[test]
    fn test_tracker_u32_no_wrap() {
        // u32, 1µs/tick, t0=1_000_000
        let mut tracker = TimestampTracker::new(1_000_000, 4, &TimestampUnit::DAQTIMESTAMPUNIT1US);
        let t = tracker.elapsed_seconds(2_000_000);
        assert!((t - 1.0).abs() < 1e-9, "expected 1.0s, got {}", t);
    }

    #[test]
    fn test_tracker_u32_wrap() {
        // u32, 1µs/tick, t0 = 0xFFFF_FF00 (near max)
        let mut tracker =
            TimestampTracker::new(0xFFFF_FF00, 4, &TimestampUnit::DAQTIMESTAMPUNIT1US);
        // After wrap: raw = 0x100
        let t = tracker.elapsed_seconds(0x0000_0100);
        // expected: 0x200 = 512 ticks = 512µs = 0.000512s
        assert!((t - 0.000512).abs() < 1e-9, "expected 0.000512, got {}", t);
    }

    #[test]
    fn test_tracker_t0_truncation() {
        // GET_DAQ_CLOCK returns u32=0x0001_EA60 (125536), but DTO is u16
        // Truncated t0 = 0xEA60 = 60000
        let mut tracker =
            TimestampTracker::new(0x0001_EA60, 2, &TimestampUnit::DAQTIMESTAMPUNIT1MS);
        let t = tracker.elapsed_seconds(60100);
        assert!((t - 0.100).abs() < 1e-9, "expected 0.100, got {}", t);
    }

    // ── Selection / restart flag tests ────────────────────────

    #[test]
    fn test_toggle_measurement_restart_flag() {
        let mut mgr = make_daq_manager(4);
        mgr.running.store(true, Ordering::SeqCst);
        mgr.toggle_measurement(0); // deselect while running
        assert!(mgr.needs_restart());
        mgr.clear_restart_flag();
        assert!(!mgr.needs_restart());
    }

    #[test]
    fn test_set_measurement_event_while_running() {
        let mut mgr = make_daq_manager(4);
        mgr.running.store(true, Ordering::SeqCst);
        mgr.set_measurement_event(0, 5);
        assert!(mgr.needs_restart());
    }

    #[test]
    fn test_batch_event_while_running() {
        let mut mgr = make_daq_manager(4);
        mgr.running.store(true, Ordering::SeqCst);
        mgr.set_measurement_events_batch(&[(0, 1), (1, 2)]);
        assert!(mgr.needs_restart());
    }

    #[test]
    fn test_event_reassignment_changes_allocation() {
        let mut mgr = make_daq_manager(4);
        // Initially all on default event 0 → 1 list
        let alloc1 = mgr.calculate_odt_allocation(255, 1, 65535, 65535).unwrap();
        assert_eq!(alloc1.daq_list_count, 1);

        // Reassign half to event 5 → 2 lists
        mgr.set_measurement_event(0, 5);
        mgr.set_measurement_event(1, 5);
        let alloc2 = mgr.calculate_odt_allocation(255, 1, 65535, 65535).unwrap();
        assert_eq!(alloc2.daq_list_count, 2);
        assert_eq!(alloc2.event_channel_per_list, vec![0, 5]);
    }

    #[test]
    fn test_allocation_splits_odt_on_payload_overflow() {
        // 10 U32 measurements (4 bytes each) = 40 bytes total
        // With max_odt_payload=12, each ODT can hold 3 measurements (12 bytes)
        // So we need ceil(10/3) = 4 ODTs
        let mgr = make_daq_manager(10);
        let alloc = mgr.calculate_odt_allocation(255, 1, 12, 12).unwrap();
        assert_eq!(alloc.daq_list_count, 1);
        // 10 measurements × 4 bytes = 40 bytes; 12 bytes per ODT → 4 ODTs (3+3+3+1)
        assert_eq!(alloc.daq_lists[0].odts.len(), 4);
        assert_eq!(alloc.daq_lists[0].odts[0].entries.len(), 3);
        assert_eq!(alloc.daq_lists[0].odts[1].entries.len(), 3);
        assert_eq!(alloc.daq_lists[0].odts[2].entries.len(), 3);
        assert_eq!(alloc.daq_lists[0].odts[3].entries.len(), 1);
    }

    #[test]
    fn test_allocation_odt0_smaller_payload_due_to_timestamp() {
        // ODT 0 has smaller payload due to timestamp overhead
        // max_odt0_payload=8 (fits 2×4-byte), max_odt_payload=12 (fits 3×4-byte)
        // 8 measurements → ODT0: 2, ODT1: 3, ODT2: 3
        let mgr = make_daq_manager(8);
        let alloc = mgr.calculate_odt_allocation(255, 1, 8, 12).unwrap();
        assert_eq!(alloc.daq_list_count, 1);
        assert_eq!(alloc.daq_lists[0].odts.len(), 3);
        assert_eq!(alloc.daq_lists[0].odts[0].entries.len(), 2); // ODT 0 fits 2
        assert_eq!(alloc.daq_lists[0].odts[1].entries.len(), 3); // ODT 1 fits 3
        assert_eq!(alloc.daq_lists[0].odts[2].entries.len(), 3); // ODT 2 fits 3
    }

    #[test]
    fn test_allocation_respects_granularity() {
        // 2-byte measurements with granularity=4 → aligned to 4 bytes each
        let mut mgr = DaqManager::new();
        mgr.all_measurements = (0..5)
            .map(|i| Measurement {
                name: format!("sig_{}", i),
                address: 0x1000 + i as u32 * 2,
                address_ext: 0,
                size: 2, // 2 bytes, but aligned to 4 with granularity=4
                data_type: A2lDataType::U16,
                unit: None,
                compu_method: CompuMethod::default(),
            })
            .collect();
        mgr.selected_indices = (0..5).collect();

        // max payload=12, granularity=4 → each meas takes 4 bytes → 3 fit per ODT
        let alloc = mgr.calculate_odt_allocation(255, 4, 12, 12).unwrap();
        assert_eq!(alloc.daq_lists[0].odts.len(), 2); // 3 + 2
        assert_eq!(alloc.daq_lists[0].odts[0].entries.len(), 3);
        assert_eq!(alloc.daq_lists[0].odts[1].entries.len(), 2);
    }

    #[test]
    fn test_parse_dto_packet() {
        // Valid DTO payload: ODT (0x00) + data (AbosulteODT mode)
        // Note: In AbosulteODT mode, the ODT number is absolute, daq_list is None
        let packet = [0x00, 0x01, 0x02, 0x03, 0x04];
        let result = parse_dto_packet(&packet, &IdentificationFieldType::AbosulteODT);
        assert!(result.is_some());
        let data = result.unwrap();
        assert_eq!(data.daq_list, None); // AbosulteODT doesn't include daq_list
        assert_eq!(data.odt, 0x00);
        // Timestamp is no longer parsed here — caller strips it for ODT 0
        assert_eq!(data.data, vec![0x01, 0x02, 0x03, 0x04]);

        // Test with RelativeODT mode - includes DAQ list
        // Format: ODT (1 byte) + DAQ list (1 byte) + data
        let packet = [0x01, 0x01, 0xAA, 0xBB, 0xCC, 0xDD];
        let result = parse_dto_packet(&packet, &IdentificationFieldType::RelatetiveODTBYTE);
        assert!(result.is_some());
        let data = result.unwrap();
        assert_eq!(data.daq_list, Some(1));
        assert_eq!(data.odt, 0x01);

        // Packet too short (missing PID/data payload bytes)
        let packet = [];
        let result = parse_dto_packet(&packet, &IdentificationFieldType::AbosulteODT);
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_odt0_timestamp() {
        // 4-byte timestamp
        let data = vec![0x10, 0x27, 0x00, 0x00, 0xAA, 0xBB, 0xCC];
        let (ts, remaining) = extract_odt0_timestamp(&data, 4);
        assert_eq!(ts, Some(10000)); // 0x00002710 = 10000
        assert_eq!(remaining, vec![0xAA, 0xBB, 0xCC]);

        // 2-byte timestamp
        let data = vec![0xE8, 0x03, 0xDD, 0xEE];
        let (ts, remaining) = extract_odt0_timestamp(&data, 2);
        assert_eq!(ts, Some(1000)); // 0x03E8 = 1000
        assert_eq!(remaining, vec![0xDD, 0xEE]);

        // No timestamp
        let data = vec![0xAA, 0xBB];
        let (ts, remaining) = extract_odt0_timestamp(&data, 0);
        assert_eq!(ts, None);
        assert_eq!(remaining, vec![0xAA, 0xBB]);
    }

    #[test]
    fn test_resolve_absolute_odt() {
        let mut config = DaqConfiguration::new();
        // DAQ list 0 has 2 ODTs, DAQ list 1 has 1 ODT, DAQ list 2 has 3 ODTs
        config.odt_count_per_list = vec![2, 1, 3];

        // Absolute ODT 0 → DAQ 0, ODT 0
        assert_eq!(config.resolve_absolute_odt(0), Some((0, 0)));
        // Absolute ODT 1 → DAQ 0, ODT 1
        assert_eq!(config.resolve_absolute_odt(1), Some((0, 1)));
        // Absolute ODT 2 → DAQ 1, ODT 0
        assert_eq!(config.resolve_absolute_odt(2), Some((1, 0)));
        // Absolute ODT 3 → DAQ 2, ODT 0
        assert_eq!(config.resolve_absolute_odt(3), Some((2, 0)));
        // Absolute ODT 5 → DAQ 2, ODT 2
        assert_eq!(config.resolve_absolute_odt(5), Some((2, 2)));
        // Absolute ODT 6 → out of range
        assert_eq!(config.resolve_absolute_odt(6), None);
    }

    #[test]
    fn test_resolve_absolute_odt_non_contiguous_first_pid() {
        // PIDs are NOT contiguous across DAQ lists: list 0 starts at PID 0
        // with 3 ODTs, list 1 starts at PID 0x60 with 3 ODTs.
        let mut config = DaqConfiguration::new();
        config.odt_count_per_list = vec![3, 3];
        config.first_pid_per_list = vec![0x00, 0x60];

        // List 0: PIDs 0x00..0x02
        assert_eq!(config.resolve_absolute_odt(0x00), Some((0, 0)));
        assert_eq!(config.resolve_absolute_odt(0x01), Some((0, 1)));
        assert_eq!(config.resolve_absolute_odt(0x02), Some((0, 2)));
        // List 1: PIDs 0x60..0x62
        assert_eq!(config.resolve_absolute_odt(0x60), Some((1, 0)));
        assert_eq!(config.resolve_absolute_odt(0x61), Some((1, 1)));
        assert_eq!(config.resolve_absolute_odt(0x62), Some((1, 2)));
        // Gap between lists (e.g. 0x03..0x5F) is unmapped → None
        assert_eq!(config.resolve_absolute_odt(0x03), None);
        assert_eq!(config.resolve_absolute_odt(0x5F), None);
        // Beyond last list
        assert_eq!(config.resolve_absolute_odt(0x63), None);
    }

    /// Verify that enum measurements produce MF4 channels with Value2Text CC blocks.
    ///
    /// Simulates the channel-building logic from `start_recording()` using a mix of
    /// numeric and enum `OdtEntryConfig` entries, writes a small MF4 file, then reads
    /// it back and checks CC types.
    #[cfg(feature = "mf4-native")]
    #[test]
    fn test_mf4_enum_channel_has_value2text_cc() {
        use crate::CompuVtabEntry;
        use mf4_parse::writer::builder::Mf4Metadata;
        use mf4_parse::writer::stream_writer::{
            ChannelDef, ChannelGroupDefBuilder, Mf4StreamWriter, StreamingDataGroup,
        };
        use mf4_parse::{components::cc::conversion::CcType, Mf4Wrapper};
        use std::path::PathBuf;

        let path = PathBuf::from("test_enum_channel_mf4.mf4");
        let _ = std::fs::remove_file(&path);

        // Build the same channel group that start_recording() would create:
        // - "speed"  : numeric (Identical), stored as f64
        // - "status" : enum (CompuVtab), stored as f64 + Value2Text CC block
        let vtab_entries = vec![
            CompuVtabEntry {
                raw_value: 0,
                text_value: "OFF".to_string(),
            },
            CompuVtabEntry {
                raw_value: 1,
                text_value: "ACTIVE".to_string(),
            },
            CompuVtabEntry {
                raw_value: 2,
                text_value: "ERROR".to_string(),
            },
        ];
        let (keys, texts): (Vec<f64>, Vec<String>) = vtab_entries
            .iter()
            .map(|e| (e.raw_value as f64, e.text_value.clone()))
            .unzip();

        let cg = ChannelGroupDefBuilder::new()
            .name("test_group")
            .with_time_channel("time_s")
            .add_f64_channel("speed", "km/h")
            .channel(ChannelDef::new("status").data_type(4).bit_count(64).vtab(
                keys,
                texts,
                String::new(),
            ))
            .build()
            .unwrap();

        let mut writer = Mf4StreamWriter::new(path.clone(), Mf4Metadata::default()).unwrap();
        writer
            .add_data_group(StreamingDataGroup::new(cg).unwrap())
            .unwrap();
        writer.finalize_structure().unwrap();

        // Write two sample records
        for i in 0..2u8 {
            writer.start_record(0, 0).unwrap();
            writer.set_channel_value("time_s", i as f64 * 0.01).unwrap();
            writer.set_channel_value("speed", 50.0 + i as f64).unwrap();
            writer.set_channel_value("status", i as f64).unwrap();
            writer.flush_record().unwrap();
        }
        writer.finalize().unwrap();

        // Read back and verify CC types
        let mf4 = Mf4Wrapper::new::<fn(f64)>(path.clone(), None).unwrap();
        let names = mf4.get_channel_names();
        assert!(
            names.contains(&"speed".to_string()),
            "speed channel missing"
        );
        assert!(
            names.contains(&"status".to_string()),
            "status channel missing"
        );

        // "speed" must have no conversion (OneToOne)
        let speed_link = mf4.get_channel_link("speed").expect("speed link");
        assert!(
            matches!(
                speed_link.0.get_conversion().get_cc_type(),
                CcType::OneToOne
            ),
            "speed should have OneToOne CC, got {:?}",
            speed_link.0.get_conversion().get_cc_type()
        );

        // "status" must have Value2Text CC with keys 0,1,2 → OFF,ACTIVE,ERROR
        let status_link = mf4.get_channel_link("status").expect("status link");
        let cc_type = status_link.0.get_conversion().get_cc_type();
        if let CcType::Value2Text((got_keys, got_texts)) = cc_type {
            assert_eq!(got_keys.len(), 3, "expected 3 vtab keys");
            assert!((got_keys[0] - 0.0).abs() < f64::EPSILON);
            assert!((got_keys[1] - 1.0).abs() < f64::EPSILON);
            assert!((got_keys[2] - 2.0).abs() < f64::EPSILON);
            use mf4_parse::components::cc::conversion::TextOrScale;
            let expected = ["OFF", "ACTIVE", "ERROR"];
            for (i, exp) in expected.iter().enumerate() {
                if let TextOrScale::Text(t) = &got_texts[i] {
                    assert_eq!(t.as_str(), *exp, "text[{}] mismatch", i);
                } else {
                    panic!("expected TextOrScale::Text at index {}", i);
                }
            }
        } else {
            panic!("expected CcType::Value2Text for status, got {:?}", cc_type);
        }

        let _ = std::fs::remove_file(&path);
    }
}
