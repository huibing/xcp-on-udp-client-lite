use std::collections::{HashMap, HashSet};

use xcp_client::protocol::Mta;

use super::fixture::{
    MockFixture, COUNTER_ADDR, COUNTER_EXT, EPK_BASE_ADDR, MOCK_EVENT_CHANNEL, MOCK_SEGMENT_EPK,
    MOCK_SEGMENT_PARAMS, PARAM_ARRAY_ADDR, PARAM_ARRAY_LEN, PARAM_BASE_ADDR, PARAM_COUNTER_MAX_ADDR,
    PARAM_COUNTER_ON_ADDR, PARAM_MATRIX_ADDR, PARAM_MATRIX_LEN,
};

#[derive(Debug, Clone)]
pub struct DaqEntry {
    pub address: u32,
    pub address_ext: u8,
    pub size: u8,
}

#[derive(Debug, Clone)]
pub struct DaqPointer {
    pub daq_list: u16,
    pub odt: u8,
    pub entry: u8,
}

#[derive(Debug, Clone)]
pub struct MockXcpState {
    pub fixture: MockFixture,
    pub current_mta: Mta,
    pub measurement_counter: u32,
    pub timestamp_counter: u32,
    pub daq_pointer: Option<DaqPointer>,
    pub daq_lists: HashMap<u16, Vec<Vec<Option<DaqEntry>>>>,
    pub selected_daq_lists: HashSet<u16>,
    pub daq_running: bool,
    upload_buffer: Option<Vec<u8>>,
    upload_offset: usize,
    pending_download: Option<PendingDownload>,
    daq_event_channel: HashMap<u16, u16>,
}

#[derive(Debug, Clone)]
struct PendingDownload {
    target: Mta,
    total_size: usize,
    data: Vec<u8>,
}

impl MockXcpState {
    pub fn new(fixture: MockFixture) -> Self {
        Self {
            fixture,
            current_mta: Mta::new(0, 0),
            measurement_counter: 0,
            timestamp_counter: 0,
            daq_pointer: None,
            daq_lists: HashMap::new(),
            selected_daq_lists: HashSet::new(),
            daq_running: false,
            upload_buffer: None,
            upload_offset: 0,
            pending_download: None,
            daq_event_channel: HashMap::new(),
        }
    }

    pub fn get_cal_page(&self, segment: u8) -> Option<u8> {
        self.fixture.segment_pages.get(&segment).copied()
    }

    pub fn set_cal_page(&mut self, segment: u8, page: u8) {
        self.fixture.segment_pages.insert(segment, page);
    }

    pub fn set_mta(&mut self, mta: Mta) {
        self.current_mta = mta;
    }

    pub fn read_memory(&self, mta: Mta, length: usize) -> Result<Vec<u8>, String> {
        if mta.extension == COUNTER_EXT && mta.address == COUNTER_ADDR {
            let bytes = self.measurement_counter.to_le_bytes();
            return Ok(bytes[..length.min(bytes.len())].to_vec());
        }

        if mta.extension != 0 {
            return Err(format!(
                "unsupported address extension {} for address 0x{:08X}",
                mta.extension, mta.address
            ));
        }

        if let Some(offset) = self.segment_offset(MOCK_SEGMENT_PARAMS, mta.address) {
            let page = self
                .get_cal_page(MOCK_SEGMENT_PARAMS)
                .ok_or_else(|| "missing params segment page".to_string())?;
            let page_data = self.params_page(page)?;
            let end = offset + length;
            if end > page_data.len() {
                return Err(format!(
                    "params read out of range: offset={} length={} page_len={}",
                    offset,
                    length,
                    page_data.len()
                ));
            }
            return Ok(page_data[offset..end].to_vec());
        }

        if let Some(offset) = self.segment_offset(MOCK_SEGMENT_EPK, mta.address) {
            let epk = self.epk_bytes();
            let end = offset + length;
            if end > epk.len() {
                return Err(format!(
                    "epk read out of range: offset={} length={} epk_len={}",
                    offset,
                    length,
                    epk.len()
                ));
            }
            return Ok(epk[offset..end].to_vec());
        }

        Err(format!("unsupported read at address 0x{:08X}", mta.address))
    }

    pub fn upload(&mut self, length: usize) -> Result<Vec<u8>, String> {
        if let Some(buffer) = self.upload_buffer.as_ref() {
            let start = self.upload_offset;
            let end = (start + length).min(buffer.len());
            let data = buffer[start..end].to_vec();
            self.upload_offset = end;
            if self.upload_offset >= buffer.len() {
                self.upload_buffer = None;
                self.upload_offset = 0;
            }
            return Ok(data);
        }

        let data = self.read_memory(self.current_mta, length)?;
        self.current_mta.address = self.current_mta.address.wrapping_add(length as u32);
        Ok(data)
    }

    pub fn prepare_upload_buffer(&mut self, bytes: Vec<u8>) {
        self.upload_buffer = Some(bytes);
        self.upload_offset = 0;
    }

    pub fn write_memory(&mut self, mta: Mta, data: &[u8]) -> Result<(), String> {
        if mta.extension != 0 {
            return Err(format!(
                "unsupported address extension {} for write to 0x{:08X}",
                mta.extension, mta.address
            ));
        }

        if let Some(offset) = self.segment_offset(MOCK_SEGMENT_PARAMS, mta.address) {
            let page = self
                .get_cal_page(MOCK_SEGMENT_PARAMS)
                .ok_or_else(|| "missing params segment page".to_string())?;
            let page_data = self.params_page_mut(page)?;
            let end = offset + data.len();
            if end > page_data.len() {
                return Err(format!(
                    "params write out of range: offset={} length={} page_len={}",
                    offset,
                    data.len(),
                    page_data.len()
                ));
            }
            page_data[offset..end].copy_from_slice(data);
            return Ok(());
        }

        Err(format!("unsupported write at address 0x{:08X}", mta.address))
    }

    pub fn checksum(&self, mta: Mta, block_size: u32) -> Result<(u8, u32), u32> {
        if block_size > self.fixture.max_checksum_block {
            return Err(self.fixture.max_checksum_block);
        }
        let data = self
            .read_memory(mta, block_size as usize)
            .map_err(|_| self.fixture.max_checksum_block)?;
        let checksum = data
            .iter()
            .fold(0u32, |acc, byte| acc.wrapping_add(*byte as u32));
        Ok((self.fixture.checksum_type, checksum))
    }

    pub fn alloc_daq(&mut self, daq_count: u16) {
        self.daq_lists.clear();
        self.selected_daq_lists.clear();
        self.daq_event_channel.clear();
        for daq_list in 0..daq_count {
            self.daq_lists.insert(daq_list, Vec::new());
            self.daq_event_channel.insert(daq_list, MOCK_EVENT_CHANNEL);
        }
    }

    pub fn free_daq(&mut self) {
        self.daq_lists.clear();
        self.selected_daq_lists.clear();
        self.daq_pointer = None;
        self.daq_running = false;
    }

    pub fn alloc_odt(&mut self, daq_list: u16, odt_count: u8) {
        self.daq_lists
            .insert(daq_list, vec![Vec::new(); odt_count as usize]);
    }

    pub fn alloc_odt_entry(&mut self, daq_list: u16, odt: u8, entry_count: u8) {
        let lists = self
            .daq_lists
            .entry(daq_list)
            .or_insert_with(|| vec![Vec::new(); odt as usize + 1]);
        while lists.len() <= odt as usize {
            lists.push(Vec::new());
        }
        lists[odt as usize] = vec![None; entry_count as usize];
    }

    pub fn set_daq_ptr(&mut self, daq_list: u16, odt: u8, entry: u8) {
        self.daq_pointer = Some(DaqPointer {
            daq_list,
            odt,
            entry,
        });
    }

    pub fn write_daq(&mut self, mta: Mta, size: u8) {
        let Some(pointer) = self.daq_pointer.clone() else {
            return;
        };
        let Some(lists) = self.daq_lists.get_mut(&pointer.daq_list) else {
            return;
        };
        if pointer.odt as usize >= lists.len() {
            return;
        }
        let odt_entries = &mut lists[pointer.odt as usize];
        if pointer.entry as usize >= odt_entries.len() {
            return;
        }
        odt_entries[pointer.entry as usize] = Some(DaqEntry {
            address: mta.address,
            address_ext: mta.extension,
            size,
        });
    }

    pub fn set_daq_list_mode(
        &mut self,
        daq_list: u16,
        _mode: u8,
        event_channel: u16,
        _prescaler: u8,
        _priority: u8,
    ) {
        self.daq_event_channel.insert(daq_list, event_channel);
    }

    pub fn start_stop_daq_list(&mut self, mode: u8, daq_list: u16) {
        match mode {
            0 => {
                self.selected_daq_lists.remove(&daq_list);
            }
            1 | 2 => {
                self.selected_daq_lists.insert(daq_list);
            }
            _ => {}
        }
    }

    pub fn start_stop_synch(&mut self, mode: u8) {
        self.daq_running = mode != 0;
    }

    pub fn download_first(&mut self, total_size: usize, bytes: &[u8]) -> Result<(), String> {
        if total_size <= bytes.len() {
            let target = self.current_mta;
            self.write_memory(target, bytes)?;
            self.current_mta.address = self.current_mta.address.wrapping_add(bytes.len() as u32);
            self.pending_download = None;
            return Ok(());
        }

        self.pending_download = Some(PendingDownload {
            target: self.current_mta,
            total_size,
            data: bytes.to_vec(),
        });
        Ok(())
    }

    pub fn download_next(&mut self, remaining_after: usize, bytes: &[u8]) -> Result<(), String> {
        let Some(pending) = self.pending_download.as_mut() else {
            return Err("DOWNLOAD_NEXT without pending DOWNLOAD".to_string());
        };
        pending.data.extend_from_slice(bytes);
        if remaining_after == 0 || pending.data.len() >= pending.total_size {
            let target = pending.target;
            let data = pending.data.clone();
            self.write_memory(target, &data)?;
            self.current_mta.address = target.address.wrapping_add(data.len() as u32);
            self.pending_download = None;
        }
        Ok(())
    }

    pub fn next_dto_payload(&mut self) -> Option<Vec<u8>> {
        if !self.daq_running {
            return None;
        }
        let daq_list = *self.selected_daq_lists.iter().min()?;
        let (odt_idx, entries) = self
            .daq_lists
            .get(&daq_list)?
            .iter()
            .enumerate()
            .find(|(_, entries)| entries.iter().any(|entry| entry.is_some()))?;
        let entries: Vec<DaqEntry> = entries.iter().flatten().cloned().collect();

        self.tick_measurement_counter();
        self.timestamp_counter = self.timestamp_counter.wrapping_add(1);

        let mut payload = Vec::new();
        payload.push(self.absolute_odt_index(daq_list, odt_idx as u8));
        payload.extend_from_slice(&self.timestamp_counter.to_le_bytes());

        for entry in &entries {
            let bytes = self
                .read_memory(Mta::new(entry.address, entry.address_ext), entry.size as usize)
                .ok()?;
            payload.extend_from_slice(&bytes);
        }

        Some(payload)
    }

    pub fn current_clock(&self) -> u32 {
        self.timestamp_counter
    }

    fn absolute_odt_index(&self, daq_list: u16, odt: u8) -> u8 {
        let mut absolute = 0u8;
        let mut keys: Vec<u16> = self.daq_lists.keys().copied().collect();
        keys.sort_unstable();
        for key in keys {
            if key == daq_list {
                return absolute.wrapping_add(odt);
            }
            absolute = absolute.wrapping_add(
                self.daq_lists
                    .get(&key)
                    .map(|odts| odts.len() as u8)
                    .unwrap_or(0),
            );
        }
        odt
    }

    fn tick_measurement_counter(&mut self) {
        let counter_on = self
            .read_memory(Mta::new(PARAM_COUNTER_ON_ADDR, 0), 1)
            .ok()
            .and_then(|bytes| bytes.first().copied())
            .unwrap_or(1)
            != 0;
        if !counter_on {
            return;
        }

        let max = self
            .read_memory(Mta::new(PARAM_COUNTER_MAX_ADDR, 0), 4)
            .ok()
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap_or([0; 4])))
            .unwrap_or(100);

        if self.measurement_counter >= max {
            self.measurement_counter = 0;
        } else {
            self.measurement_counter = self.measurement_counter.wrapping_add(1);
        }
    }

    fn params_page(&self, page: u8) -> Result<&[u8], String> {
        match page {
            0 => Ok(&self.fixture.params_working_page),
            1 => Ok(&self.fixture.params_reference_page),
            other => Err(format!("unknown params page {}", other)),
        }
    }

    fn params_page_mut(&mut self, page: u8) -> Result<&mut [u8], String> {
        match page {
            0 => Ok(&mut self.fixture.params_working_page),
            1 => Ok(&mut self.fixture.params_reference_page),
            other => Err(format!("unknown params page {}", other)),
        }
    }

    fn epk_bytes(&self) -> Vec<u8> {
        let mut epk = b"mock-xcp-epk".to_vec();
        epk.resize(28, 0);
        epk
    }

    fn segment_offset(&self, segment: u8, address: u32) -> Option<usize> {
        match segment {
            MOCK_SEGMENT_EPK => address.checked_sub(EPK_BASE_ADDR).map(|offset| offset as usize),
            MOCK_SEGMENT_PARAMS => address
                .checked_sub(PARAM_BASE_ADDR)
                .map(|offset| offset as usize),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::support::mock_slave::{MOCK_SEGMENT_PARAMS, PARAM_MATRIX_COLS, PARAM_MATRIX_ROWS};

    #[test]
    fn page_switch_changes_visible_memory() {
        let mut state = MockXcpState::new(MockFixture::default());
        let before = state
            .read_memory(Mta::new(PARAM_COUNTER_MAX_ADDR, 0), 4)
            .expect("read working page");
        assert_eq!(u32::from_le_bytes(before.try_into().unwrap()), 100);

        state.set_cal_page(MOCK_SEGMENT_PARAMS, 1);
        let after = state
            .read_memory(Mta::new(PARAM_COUNTER_MAX_ADDR, 0), 4)
            .expect("read reference page");
        assert_eq!(u32::from_le_bytes(after.try_into().unwrap()), 42);

        assert_eq!(PARAM_MATRIX_ROWS * PARAM_MATRIX_COLS, PARAM_MATRIX_LEN);
    }

    #[test]
    fn checksum_reports_limit_for_large_block() {
        let state = MockXcpState::new(MockFixture::default());
        let err = state
            .checksum(Mta::new(PARAM_MATRIX_ADDR, 0), 32)
            .expect_err("large checksum should report limit");
        assert_eq!(err, state.fixture.max_checksum_block);
    }

    #[test]
    fn daq_configuration_produces_counter_dto() {
        let mut state = MockXcpState::new(MockFixture::default());
        state.alloc_daq(1);
        state.alloc_odt(0, 1);
        state.alloc_odt_entry(0, 0, 1);
        state.set_daq_ptr(0, 0, 0);
        state.write_daq(Mta::new(COUNTER_ADDR, COUNTER_EXT), 4);
        state.start_stop_daq_list(2, 0);
        state.start_stop_synch(1);

        let dto = state.next_dto_payload().expect("dto payload");
        assert_eq!(dto[0], 0);
        assert_eq!(dto.len(), 1 + 4 + 4);
    }

    #[test]
    fn write_memory_updates_scalar_array_and_flag() {
        let mut state = MockXcpState::new(MockFixture::default());
        state
            .write_memory(Mta::new(PARAM_COUNTER_MAX_ADDR, 0), &123u32.to_le_bytes())
            .expect("write scalar");
        state
            .write_memory(Mta::new(PARAM_ARRAY_ADDR, 0), &[4, 5, 6, 7])
            .expect("write array");
        state
            .write_memory(Mta::new(PARAM_COUNTER_ON_ADDR, 0), &[0])
            .expect("write bool");

        let scalar = state.read_memory(Mta::new(PARAM_COUNTER_MAX_ADDR, 0), 4).unwrap();
        let array = state
            .read_memory(Mta::new(PARAM_ARRAY_ADDR, 0), PARAM_ARRAY_LEN)
            .unwrap();
        let flag = state.read_memory(Mta::new(PARAM_COUNTER_ON_ADDR, 0), 1).unwrap();
        assert_eq!(u32::from_le_bytes(scalar.try_into().unwrap()), 123);
        assert_eq!(array, vec![4, 5, 6, 7]);
        assert_eq!(flag, vec![0]);
        assert_eq!(PARAM_BASE_ADDR, PARAM_COUNTER_MAX_ADDR);
    }
}
