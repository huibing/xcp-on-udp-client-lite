//! XCP packet display formatting — pure functions for hex dump + human-readable
//! descriptions used by the in-app packet browser. All inputs are pure XCP
//! payload bytes (first byte = PID/command code). Transport-layer framing
//! (len/counter) must already be stripped.

use super::responses::parse_response;
use super::types::Response;

/// Format bytes as space-separated uppercase hex.
pub fn format_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ")
}

/// Hex dump truncated to `max` bytes. Appends ` ... +N more` when truncated.
pub fn format_hex_truncated(bytes: &[u8], max: usize) -> String {
    if bytes.len() <= max {
        format_hex(bytes)
    } else {
        let head = format_hex(&bytes[..max]);
        let extra = bytes.len() - max;
        format!("{} ... +{} more", head, extra)
    }
}

/// Human-readable XCP command name from command-code byte.
pub fn command_name(code: u8) -> &'static str {
    match code {
        0xFF => "CONNECT",
        0xFE => "DISCONNECT",
        0xFD => "GET_STATUS",
        0xFC => "SYNCH",
        0xFB => "GET_COMM_MODE_INFO",
        0xFA => "GET_ID",
        0xF9 => "SET_REQUEST",
        0xF8 => "GET_SEED",
        0xF7 => "UNLOCK",
        0xF6 => "SET_MTA",
        0xF5 => "UPLOAD",
        0xF4 => "SHORT_UPLOAD",
        0xF3 => "BUILD_CHECKSUM",
        0xF2 => "TRANSPORT_LAYER_CMD",
        0xF1 => "USER_CMD",
        0xF0 => "DOWNLOAD",
        0xEF => "DOWNLOAD_NEXT",
        0xEE => "DOWNLOAD_MAX",
        0xED => "SHORT_DOWNLOAD",
        0xEC => "MODIFY_BITS",
        0xEB => "SET_CAL_PAGE",
        0xEA => "GET_CAL_PAGE",
        0xE4 => "COPY_CAL_PAGE",
        0xE2 => "SET_DAQ_PTR",
        0xE1 => "WRITE_DAQ",
        0xE0 => "SET_DAQ_LIST_MODE",
        0xDF => "GET_DAQ_LIST_MODE",
        0xDE => "START_STOP_DAQ_LIST",
        0xDD => "START_STOP_SYNCH",
        0xDC => "GET_DAQ_CLOCK",
        0xDB => "READ_DAQ",
        0xDA => "GET_DAQ_PROCESSOR_INFO",
        0xD9 => "GET_DAQ_RESOLUTION_INFO",
        0xD8 => "GET_DAQ_LIST_INFO",
        0xD7 => "GET_DAQ_EVENT_INFO",
        0xD6 => "FREE_DAQ",
        0xD5 => "ALLOC_DAQ",
        0xD4 => "ALLOC_ODT",
        0xD3 => "ALLOC_ODT_ENTRY",
        0xD2 => "PROGRAM_START",
        0xD1 => "PROGRAM_CLEAR",
        0xD0 => "PROGRAM",
        0xCF => "PROGRAM_RESET",
        0xCE => "GET_PGM_PROCESSOR_INFO",
        0xC6 => "TIME_CORRELATION_PROPERTIES",
        0xC4 => "DAQ_MULTIPLE",
        _ => "UNKNOWN",
    }
}

/// Describe an outgoing CTO command packet. Returns e.g. `"CONNECT"` or
/// `"UNKNOWN(0x12)"`.
pub fn describe_cto_command(bytes: &[u8]) -> String {
    match bytes.first() {
        Some(&code) => {
            let name = command_name(code);
            if name == "UNKNOWN" {
                format!("UNKNOWN(0x{:02X})", code)
            } else {
                name.to_string()
            }
        }
        None => "<empty>".to_string(),
    }
}

/// Describe an incoming CTO response packet, correlating with `last_cmd` for
/// positive responses.
pub fn describe_cto_response(bytes: &[u8], last_cmd: Option<u8>) -> String {
    match parse_response(bytes) {
        Ok(Response::Res(_)) => match last_cmd {
            Some(code) => format!("RES: {}", command_name(code)),
            None => "RES".to_string(),
        },
        Ok(Response::Err(code)) => format!("ERR: {}", code),
        Ok(Response::Daq { .. }) => "EV/SERV/DAQ".to_string(),
        Err(e) => format!("<parse error: {}>", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_hex() {
        assert_eq!(format_hex(&[0xFF, 0x00, 0x01]), "FF 00 01");
        assert_eq!(format_hex(&[]), "");
    }

    #[test]
    fn test_format_hex_truncated() {
        let data = [0u8; 80];
        let s = format_hex_truncated(&data, 64);
        assert!(s.contains("+16 more"));
        assert_eq!(format_hex_truncated(&[0xAB], 64), "AB");
    }

    #[test]
    fn test_command_name() {
        assert_eq!(command_name(0xFF), "CONNECT");
        assert_eq!(command_name(0xFE), "DISCONNECT");
        assert_eq!(command_name(0x00), "UNKNOWN");
    }

    #[test]
    fn test_describe_cto_command() {
        assert_eq!(describe_cto_command(&[0xFF, 0x00]), "CONNECT");
        assert_eq!(describe_cto_command(&[0x12]), "UNKNOWN(0x12)");
        assert_eq!(describe_cto_command(&[]), "<empty>");
    }

    #[test]
    fn test_describe_cto_response_positive_with_correlation() {
        let resp = vec![0xFF, 0x11, 0x00, 0x08];
        assert_eq!(describe_cto_response(&resp, Some(0xFF)), "RES: CONNECT");
        assert_eq!(describe_cto_response(&resp, None), "RES");
    }

    #[test]
    fn test_describe_cto_response_error() {
        let resp = vec![0xFE, 0x20];
        assert_eq!(describe_cto_response(&resp, Some(0xFF)), "ERR: Unknown command");
    }
}
