//! XCP Command Builders
//!
//! These functions build XCP command packets WITHOUT the transport layer header.
//! The transport layer (e.g., XCP on Ethernet) will add its own header including
//! the packet counter.
//!
//! Command format: [command_code, parameters...]

use super::types::{CommandCode, Mta};

/// Build CONNECT command
pub fn connect() -> Vec<u8> {
    vec![
        CommandCode::Connect as u8,
        0x00, // connect_mode = 0 (normal mode)
    ]
}

/// Build DISCONNECT command
pub fn disconnect() -> Vec<u8> {
    vec![
        CommandCode::Disconnect as u8,
        0x00, // reserved
    ]
}

/// Build SET_MTA command
/// Sets the Memory Transfer Address for subsequent UPLOAD/DOWNLOAD operations
pub fn set_mta(mta: Mta) -> Vec<u8> {
    let mut cmd = Vec::with_capacity(8);
    cmd.push(CommandCode::SetMta as u8);
    cmd.push(0x00); // reserved
    cmd.push(0x00); // reserved
    cmd.push(mta.extension);
    cmd.extend_from_slice(&mta.address.to_le_bytes()); // Little-endian for xcp-lite
    cmd
}

/// Build UPLOAD command
/// Request to read data from slave (uses current MTA)
pub fn upload(length: u8) -> Vec<u8> {
    vec![
        CommandCode::Upload as u8,
        length,
    ]
}

/// Build SHORT_UPLOAD command
/// One-shot read from specified address
pub fn short_upload(mta: Mta, length: u8) -> Vec<u8> {
    let mut cmd = Vec::with_capacity(8);
    cmd.push(CommandCode::ShortUpload as u8);
    cmd.push(length);
    cmd.push(0x00); // reserved
    cmd.push(mta.extension);
    cmd.extend_from_slice(&mta.address.to_le_bytes());
    cmd
}

/// Build DOWNLOAD command
/// Write data to current MTA
pub fn download(data: &[u8]) -> Vec<u8> {
    let len = data.len() as u8;
    let mut cmd = Vec::with_capacity(2 + data.len());
    cmd.push(CommandCode::Download as u8);
    cmd.push(len);
    cmd.extend_from_slice(data);
    cmd
}

/// Build DOWNLOAD_MAX command
/// Write max size data block to current MTA
pub fn download_max(data: &[u8]) -> Vec<u8> {
    let mut cmd = Vec::with_capacity(1 + data.len());
    cmd.push(CommandCode::DownloadMax as u8);
    cmd.extend_from_slice(data);
    cmd
}

/// Build SHORT_DOWNLOAD command
/// One-shot write to specified address
pub fn short_download(mta: Mta, data: &[u8]) -> Vec<u8> {
    let len = data.len() as u8;
    let mut cmd = Vec::with_capacity(8 + data.len());
    cmd.push(CommandCode::ShortDownload as u8);
    cmd.push(len);
    cmd.push(0x00); // reserved
    cmd.push(mta.extension);
    cmd.extend_from_slice(&mta.address.to_le_bytes());
    cmd.extend_from_slice(data);
    cmd
}

/// Build GET_ID command
/// Get identification data from slave
pub fn get_id(id_type: u8) -> Vec<u8> {
    vec![
        CommandCode::GetId as u8,
        id_type,
    ]
}

/// Build GET_COMM_MODE_INFO command
pub fn get_comm_mode_info() -> Vec<u8> {
    vec![
        CommandCode::GetCommModeInfo as u8,
        0x00, // reserved
    ]
}

/// Build FREE_DAQ command
/// Release all DAQ resources
pub fn free_daq() -> Vec<u8> {
    vec![CommandCode::FreeDaq as u8]
}

/// Build ALLOC_DAQ command
/// Allocate DAQ lists
pub fn alloc_daq(daq_count: u16) -> Vec<u8> {
    let mut cmd = Vec::with_capacity(4);
    cmd.push(CommandCode::AllocDaq as u8);
    cmd.push(0x00); // reserved
    cmd.extend_from_slice(&daq_count.to_le_bytes());
    cmd
}

/// Build ALLOC_ODT command
/// Allocate ODTs within a DAQ list
pub fn alloc_odt(daq_list: u16, odt_count: u8) -> Vec<u8> {
    let mut cmd = Vec::with_capacity(5);
    cmd.push(CommandCode::AllocOdt as u8);
    cmd.push(0x00); // reserved
    cmd.extend_from_slice(&daq_list.to_le_bytes());
    cmd.push(odt_count);
    cmd
}

/// Build ALLOC_ODT_ENTRY command
/// Allocate entries within an ODT
pub fn alloc_odt_entry(daq_list: u16, odt: u8, entry_count: u8) -> Vec<u8> {
    let mut cmd = Vec::with_capacity(6);
    cmd.push(CommandCode::AllocOdtEntry as u8);
    cmd.push(0x00); // reserved
    cmd.extend_from_slice(&daq_list.to_le_bytes());
    cmd.push(odt);
    cmd.push(entry_count);
    cmd
}

/// Build SET_DAQ_PTR command
/// Set the DAQ list pointer for WRITE_DAQ
pub fn set_daq_ptr(daq_list: u16, odt: u8, entry: u8) -> Vec<u8> {
    let mut cmd = Vec::with_capacity(6);
    cmd.push(CommandCode::SetDaqPtr as u8);
    cmd.push(0x00); // reserved
    cmd.extend_from_slice(&daq_list.to_le_bytes());
    cmd.push(odt);
    cmd.push(entry);
    cmd
}

/// Build WRITE_DAQ command
/// Configure a DAQ entry
/// bit_offset: usually 0 for byte-aligned data
/// data_size: 1, 2, or 4 bytes
pub fn write_daq(bit_offset: u8, mta: Mta, data_size: u8) -> Vec<u8> {
    let mut cmd = Vec::with_capacity(8);
    cmd.push(CommandCode::WriteDaq as u8);
    cmd.push(bit_offset);
    cmd.push(data_size);
    cmd.push(mta.extension);
    cmd.extend_from_slice(&mta.address.to_le_bytes());
    cmd
}

/// Build SET_DAQ_LIST_MODE command
/// Configure DAQ list mode
pub fn set_daq_list_mode(
    mode: u8,
    daq_list: u16,
    event_channel: u16,
    prescaler: u8,
    priority: u8,
) -> Vec<u8> {
    let mut cmd = Vec::with_capacity(9);
    cmd.push(CommandCode::SetDaqListMode as u8);
    cmd.push(mode);
    cmd.extend_from_slice(&daq_list.to_le_bytes());
    cmd.extend_from_slice(&event_channel.to_le_bytes());
    cmd.push(prescaler);
    cmd.push(priority);
    cmd
}

/// Build START_STOP_DAQ_LIST command
/// mode: 0=stop, 1=start, 2=start selected
pub fn start_stop_daq_list(mode: u8, daq_list: u16) -> Vec<u8> {
    let mut cmd = Vec::with_capacity(4);
    cmd.push(CommandCode::StartStopDaqList as u8);
    cmd.push(mode);
    cmd.extend_from_slice(&daq_list.to_le_bytes());
    cmd
}

/// Build START_STOP_SYNCH command
/// Start or stop all DAQ lists synchronously
pub fn start_stop_synch(mode: u8) -> Vec<u8> {
    vec![
        CommandCode::StartStopSynch as u8,
        mode,
    ]
}

/// Build GET_DAQ_CLOCK command
pub fn get_daq_clock() -> Vec<u8> {
    vec![CommandCode::GetDaqClock as u8]
}

/// Build GET_DAQ_PROCESSOR_INFO command
pub fn get_daq_processor_info() -> Vec<u8> {
    vec![CommandCode::GetDaqProcessorInfo as u8]
}

/// Build GET_DAQ_RESOLUTION_INFO command
pub fn get_daq_resolution_info() -> Vec<u8> {
    vec![CommandCode::GetDaqResolutionInfo as u8]
}

/// Build GET_CAL_PAGE command
pub fn get_cal_page(mode: u8, segment: u8, page: u8) -> Vec<u8> {
    vec![
        CommandCode::GetCalPage as u8,
        mode,
        segment,
        page,
    ]
}

/// Build SET_CAL_PAGE command
pub fn set_cal_page(mode: u8, segment: u8, page: u8) -> Vec<u8> {
    vec![
        CommandCode::SetCalPage as u8,
        mode,
        segment,
        page,
    ]
}

/// Build BUILD_CHECKSUM command
pub fn build_checksum(block_size: u32) -> Vec<u8> {
    let mut cmd = Vec::with_capacity(5);
    cmd.push(CommandCode::BuildChecksum as u8);
    cmd.push(0x00); // reserved
    cmd.extend_from_slice(&block_size.to_le_bytes());
    cmd
}
