//! XCP Protocol Types

use serde::{Deserialize, Serialize};
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

    // ============ Calibration Commands (0xEB - 0xE4) ============
    /// SET_CAL_PAGE - Set calibration page
    SetCalPage = 0xEB,
    /// GET_CAL_PAGE - Get calibration page
    GetCalPage = 0xEA,
    /// COPY_CAL_PAGE - Copy calibration page
    CopyCalPage = 0xE4,

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
    GetDaqEventInfo = 0xD7,

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
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConnectInfo {
    pub resource: ResourceFlags,
    pub comm_mode_basic: u8,
    pub max_cto: u16,
    pub max_dto: u16,
    pub protocol_version: u8,
}

impl ConnectInfo {
    pub fn parse(data: &[u8]) -> Option<Self> {
        // Format per XCP spec: [resource, comm_mode, max_cto(1B), max_dto_l, max_dto_h, proto_ver, tl_ver]
        if data.len() < 7 {
            return None;
        }
        Some(Self {
            resource: ResourceFlags::from(data[0]),
            comm_mode_basic: data[1],
            max_cto: data[2] as u16, // max_cto is 1 byte only!
            max_dto: u16::from_le_bytes([data[3], data[4]]),
            protocol_version: data[5],
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ByteOrder {
    #[default]
    MsbLast,  // Big-endian
    MsbFirst, // Little-endian
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mta_new() {
        let mta = Mta::new(0x12345678, 0x02);
        assert_eq!(mta.address, 0x12345678);
        assert_eq!(mta.extension, 0x02);
    }

    #[test]
    fn test_mta_default() {
        let mta = Mta::default();
        assert_eq!(mta.address, 0);
        assert_eq!(mta.extension, 0);
    }

    #[test]
    fn test_resource_flags_from_u8() {
        let flags = ResourceFlags::from(0x00);
        assert!(!flags.cal_pgm);
        assert!(!flags.daq);
        assert!(!flags.stim);
        assert!(!flags.pgm);

        let flags = ResourceFlags::from(0x01);
        assert!(flags.cal_pgm);
        assert!(!flags.daq);
        assert!(!flags.stim);
        assert!(!flags.pgm);

        let flags = ResourceFlags::from(0x04);
        assert!(!flags.cal_pgm);
        assert!(flags.daq);
        assert!(!flags.stim);
        assert!(!flags.pgm);

        let flags = ResourceFlags::from(0x08);
        assert!(!flags.cal_pgm);
        assert!(!flags.daq);
        assert!(flags.stim);
        assert!(!flags.pgm);

        let flags = ResourceFlags::from(0x10);
        assert!(!flags.cal_pgm);
        assert!(!flags.daq);
        assert!(!flags.stim);
        assert!(flags.pgm);

        let flags = ResourceFlags::from(0x1F);
        assert!(flags.cal_pgm);
        assert!(flags.daq);
        assert!(flags.stim);
        assert!(flags.pgm);
    }

    #[test]
    fn test_resource_flags_default() {
        let flags = ResourceFlags::default();
        assert!(!flags.cal_pgm);
        assert!(!flags.daq);
        assert!(!flags.stim);
        assert!(!flags.pgm);
    }

    #[test]
    fn test_connect_info_parse_valid() {
        // Format per XCP spec: [resource, comm_mode, max_cto(1B), max_dto_l, max_dto_h, proto_ver, tl_ver]
        // resource=0x11: cal_pgm=true (bit 0), daq=false (bit 2), pgm=true (bit 4)
        let data = vec![0x11, 0x01, 0x08, 0x10, 0x00, 0x01, 0x01];
        let info = ConnectInfo::parse(&data).unwrap();
        assert!(info.resource.cal_pgm);
        assert!(!info.resource.daq);
        assert!(info.resource.pgm);
        assert_eq!(info.comm_mode_basic, 0x01);
        assert_eq!(info.max_cto, 8);
        assert_eq!(info.max_dto, 16);
        assert_eq!(info.protocol_version, 0x01);
    }

    #[test]
    fn test_connect_info_parse_too_short() {
        let data = vec![0x05, 0x01, 0x08, 0x10, 0x00, 0x01];
        assert!(ConnectInfo::parse(&data).is_none());
    }

    #[test]
    fn test_error_code_from_u8() {
        assert_eq!(ErrorCode::from(0x00), ErrorCode::CmdProcessorBusy);
        assert_eq!(ErrorCode::from(0x01), ErrorCode::DaqProcessorBusy);
        assert_eq!(ErrorCode::from(0x02), ErrorCode::InternalTimeout);
        assert_eq!(ErrorCode::from(0x03), ErrorCode::KeyRequestSequence);
        assert_eq!(ErrorCode::from(0x04), ErrorCode::SessionNotStarted);
        assert_eq!(ErrorCode::from(0x20), ErrorCode::UnknownCommand);
        assert_eq!(ErrorCode::from(0x21), ErrorCode::CommandSyntax);
        assert_eq!(ErrorCode::from(0x22), ErrorCode::ParameterOutOfRange);
        assert_eq!(ErrorCode::from(0x23), ErrorCode::AccessDenied);
        assert_eq!(ErrorCode::from(0x24), ErrorCode::AccessLocked);
        assert_eq!(ErrorCode::from(0x25), ErrorCode::PageNotValid);
        assert_eq!(ErrorCode::from(0x26), ErrorCode::SegmentNotValid);
        assert_eq!(ErrorCode::from(0x27), ErrorCode::MODENOTVALID);
        assert_eq!(ErrorCode::from(0x28), ErrorCode::DaqConfig);
        assert_eq!(ErrorCode::from(0x29), ErrorCode::MemoryOverflow);
        assert_eq!(ErrorCode::from(0x30), ErrorCode::Verify);
        assert_eq!(ErrorCode::from(0x31), ErrorCode::Generic);
        assert_eq!(ErrorCode::from(0x2A), ErrorCode::DAQCONFIGERR);
        assert_eq!(ErrorCode::from(0xFF), ErrorCode::Unknown(0xFF));
    }

    #[test]
    fn test_error_code_display() {
        assert_eq!(format!("{}", ErrorCode::CmdProcessorBusy), "Command processor busy");
        assert_eq!(format!("{}", ErrorCode::UnknownCommand), "Unknown command");
        assert_eq!(format!("{}", ErrorCode::Unknown(0xAB)), "Undefined error: 0xAB");
    }

    #[test]
    fn test_daq_list_mode_default() {
        let mode = DaqListMode::default();
        assert_eq!(mode.mode, 0);
        assert_eq!(mode.event_channel, 0);
        assert_eq!(mode.prescaler, 0);
        assert_eq!(mode.priority, 0);
    }

    #[test]
    fn test_byte_order_default() {
        let order = ByteOrder::default();
        assert_eq!(order, ByteOrder::MsbLast);
    }

    #[test]
    fn test_address_granularity_values() {
        assert_eq!(AddressGranularity::Byte as u8, 1);
        assert_eq!(AddressGranularity::Word as u8, 2);
        assert_eq!(AddressGranularity::Dword as u8, 4);
    }

    #[test]
    fn test_u32_from_le_bytes() {
        let data = vec![0x78, 0x56, 0x34, 0x12];
        assert_eq!(u32_from_le_bytes(&data), 0x12345678);
    }

    #[test]
    fn test_u16_from_le_bytes() {
        let data = vec![0x34, 0x12];
        assert_eq!(u16_from_le_bytes(&data), 0x1234);
    }

    #[test]
    fn test_u64_from_le_bytes() {
        let data = vec![0xEF, 0xCD, 0xAB, 0x89, 0x67, 0x45, 0x23, 0x01];
        assert_eq!(u64_from_le_bytes(&data), 0x0123456789ABCDEF);
    }

    #[test]
    fn test_f32_from_le_bytes() {
        let data = vec![0x00, 0x00, 0x80, 0x3F]; // 1.0 in IEEE 754
        assert_eq!(f32_from_le_bytes(&data), 1.0);
    }

    #[test]
    fn test_f64_from_le_bytes() {
        let data = vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF0, 0x3F]; // 1.0 in IEEE 754
        assert_eq!(f64_from_le_bytes(&data), 1.0);
    }

    #[test]
    fn test_command_code_values() {
        assert_eq!(CommandCode::Connect as u8, 0xFF);
        assert_eq!(CommandCode::Disconnect as u8, 0xFE);
        assert_eq!(CommandCode::GetStatus as u8, 0xFD);
        assert_eq!(CommandCode::Synch as u8, 0xFC);
        assert_eq!(CommandCode::GetCommModeInfo as u8, 0xFB);
        assert_eq!(CommandCode::GetId as u8, 0xFA);
        assert_eq!(CommandCode::SetMta as u8, 0xF6);
        assert_eq!(CommandCode::Upload as u8, 0xF5);
        assert_eq!(CommandCode::ShortUpload as u8, 0xF4);
        assert_eq!(CommandCode::Download as u8, 0xF0);
        assert_eq!(CommandCode::ShortDownload as u8, 0xED);
        assert_eq!(CommandCode::FreeDaq as u8, 0xD6);
        assert_eq!(CommandCode::AllocDaq as u8, 0xD5);
        assert_eq!(CommandCode::AllocOdt as u8, 0xD4);
        assert_eq!(CommandCode::AllocOdtEntry as u8, 0xD3);
        assert_eq!(CommandCode::SetDaqPtr as u8, 0xE2);
        assert_eq!(CommandCode::WriteDaq as u8, 0xE1);
        assert_eq!(CommandCode::SetDaqListMode as u8, 0xE0);
        assert_eq!(CommandCode::StartStopDaqList as u8, 0xDE);
        assert_eq!(CommandCode::StartStopSynch as u8, 0xDD);
        assert_eq!(CommandCode::GetDaqClock as u8, 0xDC);
        assert_eq!(CommandCode::GetDaqProcessorInfo as u8, 0xDA);
        assert_eq!(CommandCode::GetDaqResolutionInfo as u8, 0xD9);
    }

    #[test]
    fn test_daq_entry_fields() {
        let entry = DaqEntry {
            address: 0x12345678,
            address_ext: 0x02,
            data_size: 4,
        };
        assert_eq!(entry.address, 0x12345678);
        assert_eq!(entry.address_ext, 0x02);
        assert_eq!(entry.data_size, 4);
    }
}
