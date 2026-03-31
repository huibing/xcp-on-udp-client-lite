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
            // EV (Event) packet - same format as DAQ
            // PID, DAQ list number (2 bytes), ODT number (1 byte), data
            if data.len() < payload_start + 4 {
                return Err("EV packet too short".to_string());
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
        0xFC => {
            // SERV (Service Request) packet
            Err(format!("SERV packet not implemented: 0x{:02X}", pid))
        }
        // DAQ packets: PID < 0xFC (0x00 - 0xFB)
        _ if pid < 0xFC => {
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
    // Response format (after TL header) per XCP Protocol Specification:
    // BYTE 0: resource
    // BYTE 1: comm_mode_basic
    // BYTE 2: max_cto (1 byte!)
    // BYTE 3-4: max_dto (2 bytes, little-endian)
    // BYTE 5: protocol_version
    // BYTE 6: transport_layer_version
    if data.len() < 7 {
        return Err("Connect response too short".to_string());
    }

    Ok(ConnectInfo {
        resource: super::types::ResourceFlags::from(data[0]),
        comm_mode_basic: data[1],
        max_cto: data[2] as u16, // max_cto is 1 byte only!
        max_dto: u16::from_le_bytes([data[3], data[4]]),
        protocol_version: data[5],
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

/// Parse GET_CAL_PAGE response
/// Response format (after RES byte):
/// BYTE 0: reserved
/// BYTE 1: reserved
/// BYTE 2: Logical data page number (current active page)
pub fn parse_cal_page_response(data: &[u8]) -> Result<u8, String> {
    if data.len() < 3 {
        return Err("GET_CAL_PAGE response too short".to_string());
    }

    // Byte 0, 1: reserved
    // Byte 2: page number
    Ok(data[2])
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::types::{ErrorCode, Response};

    #[test]
    fn test_parse_tl_header_valid() {
        // TL header: len=10, counter=42
        let data = vec![0x0A, 0x00, 0x2A, 0x00, 0xFF];
        let header = parse_tl_header(&data).unwrap();
        assert_eq!(header.length, 10);
        assert_eq!(header.counter, 42);
    }

    #[test]
    fn test_parse_tl_header_too_short() {
        let data = vec![0x01, 0x02, 0x03];
        assert!(parse_tl_header(&data).is_err());
    }

    #[test]
    fn test_parse_tl_header_large_values() {
        // len=65535 (max u16), counter=65535
        let data = vec![0xFF, 0xFF, 0xFF, 0xFF, 0x00];
        let header = parse_tl_header(&data).unwrap();
        assert_eq!(header.length, 65535);
        assert_eq!(header.counter, 65535);
    }

    #[test]
    fn test_parse_response_positive() {
        // TL header: len=4, counter=1
        // Payload: PID=0xFF (RES), data=[0x01, 0x02, 0x03]
        let data = vec![0x04, 0x00, 0x01, 0x00, 0xFF, 0x01, 0x02, 0x03];
        let response = parse_response(&data).unwrap();
        match response {
            Response::Res(res_data) => {
                assert_eq!(res_data, vec![0x01, 0x02, 0x03]);
            }
            _ => panic!("Expected Res response"),
        }
    }

    #[test]
    fn test_parse_response_error() {
        // TL header: len=2, counter=1
        // Payload: PID=0xFE (ERR), error_code=0x21 (CommandSyntax)
        let data = vec![0x02, 0x00, 0x01, 0x00, 0xFE, 0x21];
        let response = parse_response(&data).unwrap();
        match response {
            Response::Err(err_code) => {
                assert_eq!(err_code, ErrorCode::CommandSyntax);
            }
            _ => panic!("Expected Err response"),
        }
    }

    #[test]
    fn test_parse_response_daq() {
        // TL header: len=6, counter=1
        // Payload: PID=0xC0, daq_list=0x0001, odt=0x02, data=[0xAB, 0xCD]
        let data = vec![0x06, 0x00, 0x01, 0x00, 0xC0, 0x01, 0x00, 0x02, 0xAB, 0xCD];
        let response = parse_response(&data).unwrap();
        match response {
            Response::Daq { daq_list, odt, data } => {
                assert_eq!(daq_list, 1);
                assert_eq!(odt, 2);
                assert_eq!(data, vec![0xAB, 0xCD]);
            }
            _ => panic!("Expected Daq response"),
        }
    }

    #[test]
    fn test_parse_response_serv_packet() {
        // SERV packet (0xFC) should return not implemented error
        let data = vec![0x01, 0x00, 0x01, 0x00, 0xFC];
        let result = parse_response(&data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not implemented"));
    }

    #[test]
    fn test_parse_response_too_short() {
        // Only TL header, no payload
        let data = vec![0x00, 0x00, 0x01, 0x00];
        assert!(parse_response(&data).is_err());
    }

    #[test]
    fn test_parse_response_error_too_short() {
        // ERR response with no error code
        let data = vec![0x01, 0x00, 0x01, 0x00, 0xFE];
        assert!(parse_response(&data).is_err());
    }

    #[test]
    fn test_parse_response_with_counter() {
        // TL header: len=1, counter=12345
        let data = vec![0x01, 0x00, 0x39, 0x30, 0xFF];
        let (response, counter) = parse_response_with_counter(&data).unwrap();
        assert_eq!(counter, 12345);
        match response {
            Response::Res(res_data) => {
                assert!(res_data.is_empty()); // No data after PID
            }
            _ => panic!("Expected Res response"),
        }
    }

    #[test]
    fn test_parse_connect_response_valid() {
        // Mock CONNECT response data (without TL header)
        // Format per XCP spec: [resource, comm_mode, max_cto(1B), max_dto_l, max_dto_h, proto_ver, tl_ver]
        // resource=0x11: cal_pgm=true (bit 0), daq=false (bit 2), pgm=true (bit 4)
        let data = vec![0x11, 0x00, 0x08, 0x08, 0x00, 0x01, 0x01];
        let info = parse_connect_response(&data).unwrap();
        assert!(info.resource.cal_pgm);
        assert!(!info.resource.daq);
        assert!(info.resource.pgm);
        assert_eq!(info.comm_mode_basic, 0x00);
        assert_eq!(info.max_cto, 8);
        assert_eq!(info.max_dto, 8);
        assert_eq!(info.protocol_version, 0x01);
    }

    #[test]
    fn test_parse_connect_response_all_resources() {
        // resource=0x1F (all resources enabled)
        // Format: [resource, comm_mode, max_cto(1B), max_dto_l, max_dto_h, proto_ver, tl_ver]
        let data = vec![0x1F, 0x01, 0xFF, 0xFF, 0x00, 0x02, 0x01];
        let info = parse_connect_response(&data).unwrap();
        assert!(info.resource.cal_pgm);
        assert!(info.resource.daq);
        assert!(info.resource.stim);
        assert!(info.resource.pgm);
        assert_eq!(info.comm_mode_basic, 0x01);
        assert_eq!(info.max_cto, 255);
        assert_eq!(info.max_dto, 255);
        assert_eq!(info.protocol_version, 0x02);
    }

    #[test]
    fn test_parse_connect_response_too_short() {
        let data = vec![0x05, 0x00, 0x08, 0x00];
        assert!(parse_connect_response(&data).is_err());
    }

    #[test]
    fn test_parse_get_id_response_valid() {
        // GET_ID response: [mode=0, 0, 0, len_l, len_m, len_h, data...]
        let data = vec![0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x54, 0x45, 0x53, 0x54];
        let (length, id_string) = parse_get_id_response(&data).unwrap();
        assert_eq!(length, 4);
        assert_eq!(id_string, "TEST");
    }

    #[test]
    fn test_parse_get_id_response_too_short() {
        let data = vec![0x00, 0x00, 0x00];
        assert!(parse_get_id_response(&data).is_err());
    }

    #[test]
    fn test_parse_daq_processor_info_valid() {
        // [min_daq=1, max_daq=10, properties=0x03, key_bit=0x01]
        let data = vec![0x01, 0x00, 0x0A, 0x00, 0x03, 0x01];
        let info = parse_daq_processor_info(&data).unwrap();
        assert_eq!(info.min_daq, 1);
        assert_eq!(info.max_daq, 10);
        assert_eq!(info.properties, 0x03);
        assert_eq!(info.key_bit, 0x01);
    }

    #[test]
    fn test_parse_daq_processor_info_too_short() {
        let data = vec![0x01, 0x00, 0x0A];
        assert!(parse_daq_processor_info(&data).is_err());
    }

    #[test]
    fn test_parse_daq_resolution_info_valid() {
        // [gran_daq, max_daq, gran_stim, max_stim, ts_mode, ts_ticks_l, ts_ticks_h]
        let data = vec![0x01, 0xFF, 0x01, 0xFF, 0x02, 0xE8, 0x03];
        let info = parse_daq_resolution_info(&data).unwrap();
        assert_eq!(info.granularity_odt_entry_size_daq, 0x01);
        assert_eq!(info.max_odt_entry_size_daq, 0xFF);
        assert_eq!(info.granularity_odt_entry_size_stim, 0x01);
        assert_eq!(info.max_odt_entry_size_stim, 0xFF);
        assert_eq!(info.timestamp_mode, 0x02);
        assert_eq!(info.timestamp_ticks, 1000); // 0x03E8
    }

    #[test]
    fn test_parse_daq_clock_valid() {
        // 6-byte timestamp: 0x010203040506
        let data = vec![0x06, 0x05, 0x04, 0x03, 0x02, 0x01];
        let ts = parse_daq_clock(&data).unwrap();
        assert_eq!(ts, 0x010203040506);
    }

    #[test]
    fn test_parse_daq_clock_too_short() {
        let data = vec![0x01, 0x02, 0x03, 0x04, 0x05];
        assert!(parse_daq_clock(&data).is_err());
    }

    #[test]
    fn test_parse_checksum_valid() {
        // [type=0x01, value=0x12345678]
        let data = vec![0x01, 0x78, 0x56, 0x34, 0x12];
        let (checksum_type, checksum_value) = parse_checksum(&data).unwrap();
        assert_eq!(checksum_type, 0x01);
        assert_eq!(checksum_value, 0x12345678);
    }

    #[test]
    fn test_parse_checksum_too_short() {
        let data = vec![0x01, 0x78, 0x56, 0x34];
        assert!(parse_checksum(&data).is_err());
    }

    #[test]
    fn test_error_code_conversion() {
        assert_eq!(ErrorCode::from(0x00), ErrorCode::CmdProcessorBusy);
        assert_eq!(ErrorCode::from(0x20), ErrorCode::UnknownCommand);
        assert_eq!(ErrorCode::from(0x21), ErrorCode::CommandSyntax);
        assert_eq!(ErrorCode::from(0x22), ErrorCode::ParameterOutOfRange);
        assert_eq!(ErrorCode::from(0x23), ErrorCode::AccessDenied);
        assert_eq!(ErrorCode::from(0x24), ErrorCode::AccessLocked);
        assert_eq!(ErrorCode::from(0xFF), ErrorCode::Unknown(0xFF));
    }

    #[test]
    fn test_parse_cal_page_response_valid() {
        // Response: [reserved, reserved, page_number]
        let data = vec![0x00, 0x00, 0x01];
        let page = parse_cal_page_response(&data).unwrap();
        assert_eq!(page, 1);
    }

    #[test]
    fn test_parse_cal_page_response_page_zero() {
        // Page 0 is valid
        let data = vec![0x00, 0x00, 0x00];
        let page = parse_cal_page_response(&data).unwrap();
        assert_eq!(page, 0);
    }

    #[test]
    fn test_parse_cal_page_response_too_short() {
        let data = vec![0x00, 0x00];
        assert!(parse_cal_page_response(&data).is_err());
    }
}
