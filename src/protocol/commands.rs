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
/// Query the current active calibration page for a segment
///
/// # Arguments
/// * `mode` - Access mode: 0x01 = ECU access, 0x02 = XCP access
/// * `segment` - Logical data segment number (0-255)
///
/// # Returns
/// Command packet without transport layer header
pub fn get_cal_page(mode: u8, segment: u8) -> Vec<u8> {
    vec![
        CommandCode::GetCalPage as u8,
        mode,
        segment,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connect_command() {
        let cmd = connect();
        assert_eq!(cmd.len(), 2);
        assert_eq!(cmd[0], CommandCode::Connect as u8);
        assert_eq!(cmd[1], 0x00);
    }

    #[test]
    fn test_disconnect_command() {
        let cmd = disconnect();
        assert_eq!(cmd.len(), 2);
        assert_eq!(cmd[0], CommandCode::Disconnect as u8);
        assert_eq!(cmd[1], 0x00);
    }

    #[test]
    fn test_set_mta_command() {
        let mta = Mta::new(0x12345678, 0x02);
        let cmd = set_mta(mta);
        assert_eq!(cmd.len(), 8);
        assert_eq!(cmd[0], CommandCode::SetMta as u8);
        assert_eq!(cmd[1], 0x00);
        assert_eq!(cmd[2], 0x00);
        assert_eq!(cmd[3], 0x02); // extension
        // Address in little-endian: 0x12345678 -> [0x78, 0x56, 0x34, 0x12]
        assert_eq!(cmd[4], 0x78);
        assert_eq!(cmd[5], 0x56);
        assert_eq!(cmd[6], 0x34);
        assert_eq!(cmd[7], 0x12);
    }

    #[test]
    fn test_upload_command() {
        let cmd = upload(255);
        assert_eq!(cmd.len(), 2);
        assert_eq!(cmd[0], CommandCode::Upload as u8);
        assert_eq!(cmd[1], 255);
    }

    #[test]
    fn test_short_upload_command() {
        let mta = Mta::new(0xABCDEF00, 0x00);
        let cmd = short_upload(mta, 7);
        assert_eq!(cmd.len(), 8);
        assert_eq!(cmd[0], CommandCode::ShortUpload as u8);
        assert_eq!(cmd[1], 7);
        assert_eq!(cmd[2], 0x00);
        assert_eq!(cmd[3], 0x00);
        // Address in little-endian
        assert_eq!(cmd[4], 0x00);
        assert_eq!(cmd[5], 0xEF);
        assert_eq!(cmd[6], 0xCD);
        assert_eq!(cmd[7], 0xAB);
    }

    #[test]
    fn test_download_command() {
        let data = vec![0x01, 0x02, 0x03, 0x04];
        let cmd = download(&data);
        assert_eq!(cmd.len(), 6);
        assert_eq!(cmd[0], CommandCode::Download as u8);
        assert_eq!(cmd[1], 4);
        assert_eq!(&cmd[2..], &data[..]);
    }

    #[test]
    fn test_short_download_command() {
        let mta = Mta::new(0x1000, 0x00);
        let data = vec![0xAA, 0xBB];
        let cmd = short_download(mta, &data);
        assert_eq!(cmd.len(), 10);
        assert_eq!(cmd[0], CommandCode::ShortDownload as u8);
        assert_eq!(cmd[1], 2);
        assert_eq!(cmd[2], 0x00);
        assert_eq!(cmd[3], 0x00);
        // Address: 0x1000 -> [0x00, 0x10, 0x00, 0x00] in LE
        assert_eq!(cmd[4], 0x00);
        assert_eq!(cmd[5], 0x10);
        assert_eq!(cmd[6], 0x00);
        assert_eq!(cmd[7], 0x00);
        assert_eq!(cmd[8], 0xAA);
        assert_eq!(cmd[9], 0xBB);
    }

    #[test]
    fn test_get_id_command() {
        let cmd = get_id(1);
        assert_eq!(cmd.len(), 2);
        assert_eq!(cmd[0], CommandCode::GetId as u8);
        assert_eq!(cmd[1], 1);
    }

    #[test]
    fn test_free_daq_command() {
        let cmd = free_daq();
        assert_eq!(cmd.len(), 1);
        assert_eq!(cmd[0], CommandCode::FreeDaq as u8);
    }

    #[test]
    fn test_alloc_daq_command() {
        let cmd = alloc_daq(10);
        assert_eq!(cmd.len(), 4);
        assert_eq!(cmd[0], CommandCode::AllocDaq as u8);
        assert_eq!(cmd[1], 0x00);
        // 10 in little-endian: [0x0A, 0x00]
        assert_eq!(cmd[2], 0x0A);
        assert_eq!(cmd[3], 0x00);
    }

    #[test]
    fn test_alloc_odt_command() {
        let cmd = alloc_odt(5, 8);
        assert_eq!(cmd.len(), 5);
        assert_eq!(cmd[0], CommandCode::AllocOdt as u8);
        assert_eq!(cmd[1], 0x00);
        // DAQ list 5 in LE
        assert_eq!(cmd[2], 0x05);
        assert_eq!(cmd[3], 0x00);
        assert_eq!(cmd[4], 8);
    }

    #[test]
    fn test_alloc_odt_entry_command() {
        let cmd = alloc_odt_entry(3, 2, 4);
        assert_eq!(cmd.len(), 6);
        assert_eq!(cmd[0], CommandCode::AllocOdtEntry as u8);
        assert_eq!(cmd[1], 0x00);
        // DAQ list 3 in LE
        assert_eq!(cmd[2], 0x03);
        assert_eq!(cmd[3], 0x00);
        assert_eq!(cmd[4], 2);
        assert_eq!(cmd[5], 4);
    }

    #[test]
    fn test_set_daq_ptr_command() {
        let cmd = set_daq_ptr(1, 2, 3);
        assert_eq!(cmd.len(), 6);
        assert_eq!(cmd[0], CommandCode::SetDaqPtr as u8);
        assert_eq!(cmd[1], 0x00);
        // DAQ list 1 in LE
        assert_eq!(cmd[2], 0x01);
        assert_eq!(cmd[3], 0x00);
        assert_eq!(cmd[4], 2);
        assert_eq!(cmd[5], 3);
    }

    #[test]
    fn test_write_daq_command() {
        let mta = Mta::new(0x2000, 0x00);
        let cmd = write_daq(0, mta, 4);
        assert_eq!(cmd.len(), 8);
        assert_eq!(cmd[0], CommandCode::WriteDaq as u8);
        assert_eq!(cmd[1], 0);
        assert_eq!(cmd[2], 4);
        assert_eq!(cmd[3], 0x00);
        // Address: 0x2000 -> [0x00, 0x20, 0x00, 0x00] in LE
        assert_eq!(cmd[4], 0x00);
        assert_eq!(cmd[5], 0x20);
        assert_eq!(cmd[6], 0x00);
        assert_eq!(cmd[7], 0x00);
    }

    #[test]
    fn test_set_daq_list_mode_command() {
        let cmd = set_daq_list_mode(0x01, 0, 1, 1, 0);
        assert_eq!(cmd.len(), 8);
        assert_eq!(cmd[0], CommandCode::SetDaqListMode as u8);
        assert_eq!(cmd[1], 0x01);
        // DAQ list 0 in LE
        assert_eq!(cmd[2], 0x00);
        assert_eq!(cmd[3], 0x00);
        // Event channel 1 in LE
        assert_eq!(cmd[4], 0x01);
        assert_eq!(cmd[5], 0x00);
        assert_eq!(cmd[6], 1);
        assert_eq!(cmd[7], 0);
    }

    #[test]
    fn test_start_stop_daq_list_command() {
        let cmd = start_stop_daq_list(1, 0);
        assert_eq!(cmd.len(), 4);
        assert_eq!(cmd[0], CommandCode::StartStopDaqList as u8);
        assert_eq!(cmd[1], 1);
        // DAQ list 0 in LE
        assert_eq!(cmd[2], 0x00);
        assert_eq!(cmd[3], 0x00);
    }

    #[test]
    fn test_start_stop_synch_command() {
        let cmd = start_stop_synch(1);
        assert_eq!(cmd.len(), 2);
        assert_eq!(cmd[0], CommandCode::StartStopSynch as u8);
        assert_eq!(cmd[1], 1);
    }

    #[test]
    fn test_get_daq_clock_command() {
        let cmd = get_daq_clock();
        assert_eq!(cmd.len(), 1);
        assert_eq!(cmd[0], CommandCode::GetDaqClock as u8);
    }

    #[test]
    fn test_get_daq_processor_info_command() {
        let cmd = get_daq_processor_info();
        assert_eq!(cmd.len(), 1);
        assert_eq!(cmd[0], CommandCode::GetDaqProcessorInfo as u8);
    }

    #[test]
    fn test_get_daq_resolution_info_command() {
        let cmd = get_daq_resolution_info();
        assert_eq!(cmd.len(), 1);
        assert_eq!(cmd[0], CommandCode::GetDaqResolutionInfo as u8);
    }

    #[test]
    fn test_build_checksum_command() {
        let cmd = build_checksum(0x1000);
        assert_eq!(cmd.len(), 6);
        assert_eq!(cmd[0], CommandCode::BuildChecksum as u8);
        assert_eq!(cmd[1], 0x00);
        // Block size in little-endian
        assert_eq!(cmd[2], 0x00);
        assert_eq!(cmd[3], 0x10);
        assert_eq!(cmd[4], 0x00);
        assert_eq!(cmd[5], 0x00);
    }

    #[test]
    fn test_download_max_command() {
        let data = vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        let cmd = download_max(&data);
        assert_eq!(cmd.len(), 9);
        assert_eq!(cmd[0], CommandCode::DownloadMax as u8);
        assert_eq!(&cmd[1..], &data[..]);
    }

    #[test]
    fn test_get_cal_page_command() {
        // Test GET_CAL_PAGE for XCP access on segment 0
        let cmd = get_cal_page(0x02, 0);
        assert_eq!(cmd.len(), 3);
        assert_eq!(cmd[0], CommandCode::GetCalPage as u8);
        assert_eq!(cmd[1], 0x02); // XCP access mode
        assert_eq!(cmd[2], 0);     // Segment 0
    }

    #[test]
    fn test_set_cal_page_command() {
        // Test SET_CAL_PAGE for XCP access on segment 0, page 1
        let cmd = set_cal_page(0x02, 0, 1);
        assert_eq!(cmd.len(), 4);
        assert_eq!(cmd[0], CommandCode::SetCalPage as u8);
        assert_eq!(cmd[1], 0x02); // XCP access mode
        assert_eq!(cmd[2], 0);     // Segment 0
        assert_eq!(cmd[3], 1);     // Page 1
    }
}
