//! XCP Response Parsers
//!
//! Responses include a 4-byte XCP on Ethernet transport layer header:
//! [LEN_L, LEN_H, COUNTER_L, COUNTER_H, response_data...]
//! where LEN = size of response_data, COUNTER = echoed from command (2 bytes, little-endian)

use super::types::{ConnectInfo, ErrorCode, Response};

/// Transport layer header parsed result
pub struct TlHeader {
    /// Length of the XCP response packet (after header)
    pub length: usize,
    /// Counter echoed from the command (for request/response matching)
    pub counter: u16,
}

/// Parse transport layer header
/// Returns header info and payload starts at index 4
fn parse_tl_header(data: &[u8]) -> Result<TlHeader, String> {
    if data.len() < 4 {
        return Err("Response too short for TL header".to_string());
    }
    let length = data[0] as usize | ((data[1] as usize) << 8);
    let counter = data[2] as u16 | ((data[3] as u16) << 8);
    Ok(TlHeader { length, counter })
}

/// Parse XCP response packet
/// Returns the parsed response along with the counter from the header
pub fn parse_response(data: &[u8]) -> Result<Response, String> {
    let _header = parse_tl_header(data)?;
    let payload_start = 4; // Header is always 4 bytes

    if data.len() < payload_start + 1 {
        return Err("Response too short".to_string());
    }

    let pid = data[payload_start];

    match pid {
        0xFF => {
            // Positive response: RES
            let res_data = data[payload_start + 1..].to_vec();
            // Note: response data includes the command code echo, so actual data starts at +1
            Ok(Response::Res(res_data))
        }
        0xFE => {
            // Negative response: ERR
            if data.len() < payload_start + 2 {
                return Err("Error response too short".to_string());
            }
            // ERR format: [pid, error_code, ...]
            let err_code = data[payload_start + 1];
            Ok(Response::Err(ErrorCode::from(err_code)))
        }
        0xFD => {
            // DAQ packet: PID, DAQ list number (2 bytes), ODT number (1 byte), data
            if data.len() < payload_start + 4 {
                return Err("DAQ packet too short".to_string());
            }
            let daq_list = u16::from_le_bytes([data[payload_start + 1], data[payload_start + 2]]);
            let odt = data[payload_start + 3];
            let daq_data = data[payload_start + 4..].to_vec();
            Ok(Response::Daq {
                daq_list,
                odt,
                data: daq_data,
            })
        }
        _ => Err(format!("Unknown packet ID: 0x{:02X}", pid)),
    }
}

/// Parse XCP response packet with counter
/// Returns the parsed response along with the counter from the header
pub fn parse_response_with_counter(data: &[u8]) -> Result<(Response, u16), String> {
    let header = parse_tl_header(data)?;
    let response = parse_response(data)?;
    Ok((response, header.counter))
}

/// Parse CONNECT response
pub fn parse_connect_response(data: &[u8]) -> Result<ConnectInfo, String> {
    // Response format (after TL header):
    // [resource, comm_mode_basic, max_cto_l, max_cto_h, max_dto_l, max_dto_h, protocol_version, transport_layer_version]
    if data.len() < 7 {
        return Err("Connect response too short".to_string());
    }

    Ok(ConnectInfo {
        resource: super::types::ResourceFlags::from(data[0]),
        comm_mode_basic: data[1],
        max_cto: u16::from_le_bytes([data[2], data[3]]),
        max_dto: u16::from_le_bytes([data[4], data[5]]),
        protocol_version: data[6],
    })
}

/// Parse GET_ID response
/// Returns (length, data) where data is the ID string
pub fn parse_get_id_response(data: &[u8]) -> Result<(u32, String), String> {
    // Response format (after TL header):
    // [mode, 0, 0, length_l, length_m, length_h, 0, ...upload_data]
    if data.len() < 7 {
        return Err("GET_ID response too short".to_string());
    }

    let _mode = data[0]; // 0 = data by upload, 1 = data in response
    let length = u32::from_le_bytes([data[3], data[4], data[5], 0]);

    // Data needs to be uploaded if mode is 0
    // For now, just return what we have
    let id_data = data[6..].to_vec();

    Ok((length, String::from_utf8_lossy(&id_data).to_string()))
}

/// Parse DAQ processor info response
pub fn parse_daq_processor_info(data: &[u8]) -> Result<DaqProcessorInfo, String> {
    if data.len() < 6 {
        return Err("DAQ processor info response too short".to_string());
    }

    Ok(DaqProcessorInfo {
        min_daq: u16::from_le_bytes([data[0], data[1]]),
        max_daq: u16::from_le_bytes([data[2], data[3]]),
        properties: data[4],
        key_bit: data[5],
    })
}

/// DAQ Processor Info
#[derive(Debug, Clone)]
pub struct DaqProcessorInfo {
    pub min_daq: u16,
    pub max_daq: u16,
    pub properties: u8,
    pub key_bit: u8,
}

/// Parse DAQ resolution info response
pub fn parse_daq_resolution_info(data: &[u8]) -> Result<DaqResolutionInfo, String> {
    if data.len() < 6 {
        return Err("DAQ resolution info response too short".to_string());
    }

    Ok(DaqResolutionInfo {
        granularity_odt_entry_size_daq: data[0],
        max_odt_entry_size_daq: data[1],
        granularity_odt_entry_size_stim: data[2],
        max_odt_entry_size_stim: data[3],
        timestamp_mode: data[4],
        timestamp_ticks: u16::from_le_bytes([data[5], data[6]]),
    })
}

/// DAQ Resolution Info
#[derive(Debug, Clone)]
pub struct DaqResolutionInfo {
    pub granularity_odt_entry_size_daq: u8,
    pub max_odt_entry_size_daq: u8,
    pub granularity_odt_entry_size_stim: u8,
    pub max_odt_entry_size_stim: u8,
    pub timestamp_mode: u8,
    pub timestamp_ticks: u16,
}

/// Parse GET_DAQ_CLOCK response
pub fn parse_daq_clock(data: &[u8]) -> Result<u64, String> {
    if data.len() < 6 {
        return Err("DAQ clock response too short".to_string());
    }

    // 6 bytes timestamp (little-endian)
    let ts = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], 0, 0
    ]);
    Ok(ts)
}

/// Parse BUILD_CHECKSUM response
pub fn parse_checksum(data: &[u8]) -> Result<(u8, u32), String> {
    if data.len() < 5 {
        return Err("Checksum response too short".to_string());
    }

    let checksum_type = data[0];
    let checksum_value = u32::from_le_bytes([data[1], data[2], data[3], data[4]]);

    Ok((checksum_type, checksum_value))
}
