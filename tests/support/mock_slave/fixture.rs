use std::collections::HashMap;

pub const MOCK_SLAVE_ADDR: &str = "127.0.0.1:5555";
pub const MOCK_ID_STRING: &str = "mock-xcp-slave";
pub const MOCK_EVENT_NAME: &str = "my_event";
pub const MOCK_EVENT_CHANNEL: u16 = 0;
pub const MOCK_SEGMENT_EPK: u8 = 0;
pub const MOCK_SEGMENT_PARAMS: u8 = 1;

pub const COUNTER_ADDR: u32 = 0x0000_FFFC;
pub const COUNTER_EXT: u8 = 2;
pub const EPK_BASE_ADDR: u32 = 0x8000_0000;
pub const PARAM_BASE_ADDR: u32 = 0x8001_0000;

pub const PARAM_COUNTER_MAX_ADDR: u32 = 0x8001_0000;
pub const PARAM_DELAY_ADDR: u32 = 0x8001_0004;
pub const PARAM_ARRAY_ADDR: u32 = 0x8001_0008;
pub const PARAM_ARRAY_LEN: usize = 4;
pub const PARAM_MATRIX_ADDR: u32 = 0x8001_000C;
pub const PARAM_MATRIX_ROWS: usize = 4;
pub const PARAM_MATRIX_COLS: usize = 8;
pub const PARAM_MATRIX_LEN: usize = PARAM_MATRIX_ROWS * PARAM_MATRIX_COLS;
pub const PARAM_COUNTER_ON_ADDR: u32 = 0x8001_002C;

#[derive(Debug, Clone)]
pub struct MockFixture {
    pub id_string: String,
    pub event_name: String,
    pub connect_resource: u8,
    pub max_cto: u8,
    pub max_dto: u16,
    pub protocol_version: u8,
    pub transport_version: u8,
    pub checksum_type: u8,
    pub max_checksum_block: u32,
    pub daq_properties: u8,
    pub max_daq: u16,
    pub max_event_channel: u16,
    pub min_daq: u8,
    pub daq_key_byte: u8,
    pub timestamp_mode: u8,
    pub timestamp_ticks: u16,
    pub segment_pages: HashMap<u8, u8>,
    pub params_working_page: Vec<u8>,
    pub params_reference_page: Vec<u8>,
}

impl Default for MockFixture {
    fn default() -> Self {
        let mut segment_pages = HashMap::new();
        segment_pages.insert(MOCK_SEGMENT_EPK, 0);
        segment_pages.insert(MOCK_SEGMENT_PARAMS, 0);

        let mut params_working_page = vec![0u8; 64];
        params_working_page[0..4].copy_from_slice(&100u32.to_le_bytes());
        params_working_page[4..8].copy_from_slice(&10_000u32.to_le_bytes());
        params_working_page[8..12].copy_from_slice(&[0, 1, 2, 3]);
        for (idx, byte) in params_working_page[12..12 + PARAM_MATRIX_LEN]
            .iter_mut()
            .enumerate()
        {
            *byte = idx as u8;
        }
        params_working_page[0x2C] = 1;
        params_working_page[0x30] = 2;
        params_working_page[0x31] = 1;

        let mut params_reference_page = params_working_page.clone();
        params_reference_page[0..4].copy_from_slice(&42u32.to_le_bytes());
        params_reference_page[4..8].copy_from_slice(&20_000u32.to_le_bytes());
        params_reference_page[8..12].copy_from_slice(&[9, 8, 7, 6]);
        for (idx, byte) in params_reference_page[12..12 + PARAM_MATRIX_LEN]
            .iter_mut()
            .enumerate()
        {
            *byte = 200u8.saturating_add(idx as u8);
        }
        params_reference_page[0x2C] = 0;
        params_reference_page[0x30] = 4;
        params_reference_page[0x31] = 3;

        Self {
            id_string: MOCK_ID_STRING.to_string(),
            event_name: MOCK_EVENT_NAME.to_string(),
            connect_resource: 0x05,
            max_cto: 16,
            max_dto: 64,
            protocol_version: 1,
            transport_version: 1,
            checksum_type: 0x01,
            max_checksum_block: 8,
            daq_properties: 0,
            max_daq: 8,
            max_event_channel: 1,
            min_daq: 0,
            daq_key_byte: 0,
            timestamp_mode: 0x3C,
            timestamp_ticks: 1,
            segment_pages,
            params_working_page,
            params_reference_page,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_matches_a2l_contract() {
        let fixture = MockFixture::default();
        assert_eq!(fixture.max_cto, 16);
        assert_eq!(PARAM_COUNTER_MAX_ADDR, PARAM_BASE_ADDR);
        assert_eq!(PARAM_DELAY_ADDR, PARAM_BASE_ADDR + 4);
        assert_eq!(PARAM_ARRAY_ADDR, PARAM_BASE_ADDR + 8);
        assert_eq!(PARAM_MATRIX_ADDR, PARAM_BASE_ADDR + 12);
        assert_eq!(PARAM_COUNTER_ON_ADDR, PARAM_BASE_ADDR + 0x2C);
        assert_eq!(fixture.segment_pages[&MOCK_SEGMENT_PARAMS], 0);
    }
}
