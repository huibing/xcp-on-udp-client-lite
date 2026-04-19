use xcp_client::protocol::ErrorCode;

pub fn build_frame(payload: &[u8], counter: u16) -> Vec<u8> {
    let len = payload.len() as u16;
    let mut frame = Vec::with_capacity(payload.len() + 4);
    frame.extend_from_slice(&len.to_le_bytes());
    frame.extend_from_slice(&counter.to_le_bytes());
    frame.extend_from_slice(payload);
    frame
}

pub fn parse_frame(datagram: &[u8]) -> Result<(u16, Vec<u8>), String> {
    if datagram.len() < 4 {
        return Err("frame too short".to_string());
    }
    let len = u16::from_le_bytes([datagram[0], datagram[1]]) as usize;
    let counter = u16::from_le_bytes([datagram[2], datagram[3]]);
    if datagram.len() != len + 4 {
        return Err(format!(
            "invalid frame length: header says {}, datagram has {} payload bytes",
            len,
            datagram.len().saturating_sub(4)
        ));
    }
    Ok((counter, datagram[4..].to_vec()))
}

pub fn positive_response(data: &[u8]) -> Vec<u8> {
    let mut response = Vec::with_capacity(1 + data.len());
    response.push(0xFF);
    response.extend_from_slice(data);
    response
}

pub fn negative_response(code: ErrorCode, params: &[u8]) -> Vec<u8> {
    let mut response = Vec::with_capacity(2 + params.len());
    response.push(0xFE);
    response.push(error_code_to_u8(code));
    response.extend_from_slice(params);
    response
}

fn error_code_to_u8(code: ErrorCode) -> u8 {
    match code {
        ErrorCode::CmdProcessorBusy => 0x00,
        ErrorCode::DaqProcessorBusy => 0x01,
        ErrorCode::InternalTimeout => 0x02,
        ErrorCode::KeyRequestSequence => 0x03,
        ErrorCode::SessionNotStarted => 0x04,
        ErrorCode::UnknownCommand => 0x20,
        ErrorCode::CommandSyntax => 0x21,
        ErrorCode::ParameterOutOfRange => 0x22,
        ErrorCode::AccessDenied => 0x23,
        ErrorCode::AccessLocked => 0x24,
        ErrorCode::PageNotValid => 0x25,
        ErrorCode::SegmentNotValid => 0x26,
        ErrorCode::MODENOTVALID => 0x27,
        ErrorCode::DaqConfig => 0x28,
        ErrorCode::MemoryOverflow => 0x29,
        ErrorCode::Verify => 0x30,
        ErrorCode::Generic => 0x31,
        ErrorCode::DAQCONFIGERR => 0x2A,
        ErrorCode::Unknown(other) => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_roundtrip_preserves_counter_and_payload() {
        let payload = vec![0xFF, 0x00, 0x10];
        let frame = build_frame(&payload, 7);
        let (counter, parsed) = parse_frame(&frame).expect("frame should parse");
        assert_eq!(counter, 7);
        assert_eq!(parsed, payload);
    }
}
