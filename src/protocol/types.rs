//! XCP Protocol Types

use thiserror::Error;

/// XCP Command Codes (ASAM MCD-1 XCP Protocol Layer V1.5.0)
///
/// Reference: ASAM_AE_MCD-1-XCP_BS_Protocol-Layer_V1-5-0.pdf
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CommandCode {
    // ============ Standard Commands (0xFF - 0xF0) ============
    /// CONNECT - Establish connection to slave
    Connect = 0xFF,
    /// DISCONNECT - Terminate connection
    Disconnect = 0xFE,
    /// GET_STATUS - Get current status
    GetStatus = 0xFD,
    /// SYNCH - Synchronize communication
    Synch = 0xFC,
    /// GET_COMM_MODE_INFO - Get communication mode info
    GetCommModeInfo = 0xFB,
    /// GET_ID - Get identification data
    GetId = 0xFA,
    /// SET_REQUEST - Set request mode
    SetRequest = 0xF9,
    /// GET_SEED - Get seed for unlock
    GetSeed = 0xF8,
    /// UNLOCK - Unlock protected resources
    Unlock = 0xF7,
    /// SET_MTA - Set memory transfer address
    SetMta = 0xF6,
    /// UPLOAD - Upload data from slave
    Upload = 0xF5,
    /// SHORT_UPLOAD - Upload up to (MAX_CTO-2) bytes
    ShortUpload = 0xF4,
    /// BUILD_CHECKSUM - Build checksum over memory
    BuildChecksum = 0xF3,
    /// TRANSPORT_LAYER_CMD - Transport layer specific command
    TransportLayerCmd = 0xF2,
    /// USER_CMD - User defined command
    UserCmd = 0xF1,
    /// DOWNLOAD - Download data to slave
    Download = 0xF0,

    // ============ Download Commands (0xEF - 0xED) ============
    /// DOWNLOAD_NEXT - Continue download
    DownloadNext = 0xEF,
    /// DOWNLOAD_MAX - Download max block
    DownloadMax = 0xEE,
    /// SHORT_DOWNLOAD - Download up to (MAX_CTO-8) bytes
    ShortDownload = 0xED,
    /// MODIFY_BITS - Modify bits in memory
    ModifyBits = 0xEC,

    // ============ Calibration Commands (0xEB - 0xE9) ============
    /// SET_CAL_PAGE - Set calibration page
    SetCalPage = 0xEB,
    /// GET_CAL_PAGE - Get calibration page
    GetCalPage = 0xEA,
    /// COPY_CAL_PAGE - Copy calibration page
    CopyCalPage = 0xE9,

    // ============ DAQ Commands ============
    /// FREE_DAQ - Free DAQ lists
    FreeDaq = 0xD6,
    /// ALLOC_DAQ - Allocate DAQ list
    AllocDaq = 0xD5,
    /// ALLOC_ODT - Allocate ODT
    AllocOdt = 0xD4,
    /// ALLOC_ODT_ENTRY - Allocate ODT entry
    AllocOdtEntry = 0xD3,
    /// SET_DAQ_PTR - Set DAQ pointer
    SetDaqPtr = 0xE2,
    /// WRITE_DAQ - Write DAQ entry
    WriteDaq = 0xE1,
    /// SET_DAQ_LIST_MODE - Set DAQ list mode
    SetDaqListMode = 0xE0,
    /// GET_DAQ_LIST_MODE - Get DAQ list mode
    GetDaqListMode = 0xDF,
    /// START_STOP_DAQ_LIST - Start/stop DAQ list
    StartStopDaqList = 0xDE,
    /// START_STOP_SYNCH - Start/stop all DAQ lists synchronously
    StartStopSynch = 0xDD,
    /// GET_DAQ_CLOCK - Get DAQ clock
    GetDaqClock = 0xDC,
    /// READ_DAQ - Read DAQ entry
    ReadDaq = 0xDB,
    /// GET_DAQ_PROCESSOR_INFO - Get DAQ processor info
    GetDaqProcessorInfo = 0xDA,
    /// GET_DAQ_RESOLUTION_INFO - Get DAQ resolution info
    GetDaqResolutionInfo = 0xD9,
    /// GET_DAQ_LIST_INFO - Get DAQ list info
    GetDaqListInfo = 0xD8,
    /// GET_DAQ_EVENT_INFO - Get DAQ event info
    GetDaqEventInfo = 0xE4,

    // ============ PGM Commands (0xD2 - 0xCE) ============
    /// PROGRAM_START - Start programming session
    ProgramStart = 0xD2,
    /// PROGRAM_CLEAR - Clear program memory
    ProgramClear = 0xD1,
    /// PROGRAM - Program data
    Program = 0xD0,
    /// PROGRAM_RESET - Reset after programming
    ProgramReset = 0xCF,
    /// GET_PGM_PROCESSOR_INFO - Get program processor info
    GetPgmProcessorInfo = 0xCE,

    // ============ Time Correlation ============
    /// TIME_CORRELATION_PROPERTIES - Get time correlation properties
    TimeCorrelationProperties = 0xC6,

    // ============ DAQ Multiple ============
    /// DAQ_MULTIPLE - Multiple DAQ
    DaqMultiple = 0xC4,
}

/// XCP Error Codes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[repr(u8)]
pub enum ErrorCode {
    #[error("Command processor busy")]
    CmdProcessorBusy = 0x00,
    #[error("DAQ processor busy")]
    DaqProcessorBusy = 0x01,
    #[error("Internal timeout")]
    InternalTimeout = 0x02,
    #[error("Key request sequence error")]
    KeyRequestSequence = 0x03,
    #[error("Session not started")]
    SessionNotStarted = 0x04,
    #[error("Unknown command")]
    UnknownCommand = 0x20,
    #[error("Command syntax invalid")]
    CommandSyntax = 0x21,
    #[error("Parameter out of range")]
    ParameterOutOfRange = 0x22,
    #[error("Access denied")]
    AccessDenied = 0x23,
    #[error("Resource locked")]
    AccessLocked = 0x24,
    #[error("Page not valid")]
    PageNotValid = 0x25,
    #[error("Segment not valid")]
    SegmentNotValid = 0x26,
    #[error("Mode not valid")]
    MODENOTVALID = 0x27,
    #[error("DAQ configuration not valid")]
    DaqConfig = 0x28,
    #[error("Memory overflow")]
    MemoryOverflow = 0x29,
    #[error("Generic error")]
    Generic = 0x31,
    #[error("Verify error")]
    Verify = 0x30,
    #[error("DAQ config not valid")]
    DAQCONFIGERR = 0x2A,
    #[error("Undefined error: 0x{0:02X}")]
    Unknown(u8),
}

impl From<u8> for ErrorCode {
    fn from(value: u8) -> Self {
        match value {
            0x00 => ErrorCode::CmdProcessorBusy,
            0x01 => ErrorCode::DaqProcessorBusy,
            0x02 => ErrorCode::InternalTimeout,
            0x03 => ErrorCode::KeyRequestSequence,
            0x04 => ErrorCode::SessionNotStarted,
            0x20 => ErrorCode::UnknownCommand,
            0x21 => ErrorCode::CommandSyntax,
            0x22 => ErrorCode::ParameterOutOfRange,
            0x23 => ErrorCode::AccessDenied,
            0x24 => ErrorCode::AccessLocked,
            0x25 => ErrorCode::PageNotValid,
            0x26 => ErrorCode::SegmentNotValid,
            0x27 => ErrorCode::MODENOTVALID,
            0x28 => ErrorCode::DaqConfig,
            0x29 => ErrorCode::MemoryOverflow,
            0x31 => ErrorCode::Generic,
            0x30 => ErrorCode::Verify,
            0x2A => ErrorCode::DAQCONFIGERR,
            other => ErrorCode::Unknown(other),
        }
    }
}

/// XCP Response Packet
#[derive(Debug, Clone)]
pub enum Response {
    /// Positive response
    Res(Vec<u8>),
    /// Negative response (error)
    Err(ErrorCode),
    /// DAQ data packet
    Daq { daq_list: u16, odt: u8, data: Vec<u8> },
}

/// Memory Transfer Address
#[derive(Debug, Clone, Copy, Default)]
pub struct Mta {
    pub address: u32,
    pub extension: u8,
}

impl Mta {
    pub fn new(address: u32, extension: u8) -> Self {
        Self { address, extension }
    }
}

/// XCP Resource Flags
#[derive(Debug, Clone, Copy, Default)]
pub struct ResourceFlags {
    pub cal_pgm: bool,
    pub daq: bool,
    pub stim: bool,
    pub pgm: bool,
}

impl From<u8> for ResourceFlags {
    fn from(value: u8) -> Self {
        Self {
            cal_pgm: (value & 0x01) != 0,
            daq: (value & 0x04) != 0,
            stim: (value & 0x08) != 0,
            pgm: (value & 0x10) != 0,
        }
    }
}

/// XCP Connection Info (from CONNECT response)
#[derive(Debug, Clone)]
pub struct ConnectInfo {
    pub resource: ResourceFlags,
    pub comm_mode_basic: u8,
    pub max_cto: u16,
    pub max_dto: u16,
    pub protocol_version: u8,
}

impl ConnectInfo {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        Some(Self {
            resource: ResourceFlags::from(data[0]),
            comm_mode_basic: data[1],
            max_cto: data[2] as u16 | ((data[3] as u16) << 8),
            max_dto: data[4] as u16 | ((data[5] as u16) << 8),
            protocol_version: data[6],
        })
    }
}

/// DAQ List Mode
#[derive(Debug, Clone, Copy, Default)]
pub struct DaqListMode {
    pub mode: u8,
    pub event_channel: u16,
    pub prescaler: u8,
    pub priority: u8,
}

/// DAQ Properties (from WRITE_DAQ)
#[derive(Debug, Clone, Copy)]
pub struct DaqEntry {
    pub address: u32,
    pub address_ext: u8,
    pub data_size: u8,
}

/// XCP Address Granularity
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressGranularity {
    Byte = 1,
    Word = 2,
    Dword = 4,
}

/// Byte Order
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteOrder {
    MsbLast,  // Big-endian
    MsbFirst, // Little-endian
}

impl Default for ByteOrder {
    fn default() -> Self {
        Self::MsbLast // A2L specifies MSB_LAST
    }
}

/// Convert u32 from little-endian bytes
pub fn u32_from_le_bytes(data: &[u8]) -> u32 {
    u32::from_le_bytes([data[0], data[1], data[2], data[3]])
}

/// Convert u16 from little-endian bytes
pub fn u16_from_le_bytes(data: &[u8]) -> u16 {
    u16::from_le_bytes([data[0], data[1]])
}

/// Convert u64 from little-endian bytes
pub fn u64_from_le_bytes(data: &[u8]) -> u64 {
    u64::from_le_bytes([
        data[0], data[1], data[2], data[3],
        data[4], data[5], data[6], data[7],
    ])
}

/// Convert f32 from little-endian bytes
pub fn f32_from_le_bytes(data: &[u8]) -> f32 {
    f32::from_le_bytes([data[0], data[1], data[2], data[3]])
}

/// Convert f64 from little-endian bytes
pub fn f64_from_le_bytes(data: &[u8]) -> f64 {
    f64::from_le_bytes([
        data[0], data[1], data[2], data[3],
        data[4], data[5], data[6], data[7],
    ])
}
