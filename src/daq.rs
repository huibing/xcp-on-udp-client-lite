//! DAQ (Data Acquisition) Management
//!
//! Implements XCP Dynamic DAQ mechanism:
//! 1. Configure DAQ lists, ODTs, and ODT entries
//! 2. Start DAQ list - slave sends DTO packets automatically
//! 3. Receive and parse DTO packets from slave

use crate::client_v2::XcpClient;
use crate::protocol::Mta;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::io::{Cursor, Read};
use std::fs::OpenOptions;
use std::io::Write as IoWrite;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
/// Measurement definition
#[derive(Debug, Clone)]
pub struct Measurement {
    pub name: String,
    pub address: u32,
    pub address_ext: u8,
    pub size: usize,
}

#[derive(Debug, Clone)]
pub enum IdentificationFieldType {
    AbosulteODT,
    RelatetiveODTBYTE,
    RelatetiveODTWORD,
    RelatetiveODTWORDAlign,
}

#[derive(Debug, Clone, Copy)]
#[repr(u16)]
pub enum TimestampTicks {
    DAQTIMESTAMPUNIT1NS = 0x0,
    DAQTIMESTAMPUNIT10NS = 0x1,
    DAQTIMESTAMPUNIT100NS = 0x2,
    DAQTIMESTAMPUNIT1US = 0x3,
    DAQTIMESTAMPUNIT10US = 0x4,
    DAQTIMESTAMPUNIT100US = 0x5,
    DAQTIMESTAMPUNIT1MS = 0x6,
    DAQTIMESTAMPUNIT10MS = 0x7,
    DAQTIMESTAMPUNIT100MS = 0x8,
    DAQTIMESTAMPUNIT1S = 0x9,
    DAQTIMESTAMPUNIT1PS = 0xa,
    DAQTIMESTAMPUNIT10PS = 0xb,
    DAQTIMESTAMPUNIT100PS = 0xc,
}

impl TimestampTicks {
    /// 获取每个 tick 对应的秒数
    pub fn to_seconds(&self) -> f64 {
        match self {
            TimestampTicks::DAQTIMESTAMPUNIT1PS => 1e-12,
            TimestampTicks::DAQTIMESTAMPUNIT10PS => 1e-11,
            TimestampTicks::DAQTIMESTAMPUNIT100PS => 1e-10,
            TimestampTicks::DAQTIMESTAMPUNIT1NS => 1e-9,
            TimestampTicks::DAQTIMESTAMPUNIT10NS => 1e-8,
            TimestampTicks::DAQTIMESTAMPUNIT100NS => 1e-7,
            TimestampTicks::DAQTIMESTAMPUNIT1US => 1e-6,
            TimestampTicks::DAQTIMESTAMPUNIT10US => 1e-5,
            TimestampTicks::DAQTIMESTAMPUNIT100US => 1e-4,
            TimestampTicks::DAQTIMESTAMPUNIT1MS => 1e-3,
            TimestampTicks::DAQTIMESTAMPUNIT10MS => 1e-2,
            TimestampTicks::DAQTIMESTAMPUNIT100MS => 1e-1,
            TimestampTicks::DAQTIMESTAMPUNIT1S => 1.0,
        }
    }
    
    /// 从 u16 安全转换
    pub fn from_u16(value: u16) -> Option<Self> {
        match value {
            0x0 => Some(TimestampTicks::DAQTIMESTAMPUNIT1NS),
            0x1 => Some(TimestampTicks::DAQTIMESTAMPUNIT10NS),
            0x2 => Some(TimestampTicks::DAQTIMESTAMPUNIT100NS),
            0x3 => Some(TimestampTicks::DAQTIMESTAMPUNIT1US),
            0x4 => Some(TimestampTicks::DAQTIMESTAMPUNIT10US),
            0x5 => Some(TimestampTicks::DAQTIMESTAMPUNIT100US),
            0x6 => Some(TimestampTicks::DAQTIMESTAMPUNIT1MS),
            0x7 => Some(TimestampTicks::DAQTIMESTAMPUNIT10MS),
            0x8 => Some(TimestampTicks::DAQTIMESTAMPUNIT100MS),
            0x9 => Some(TimestampTicks::DAQTIMESTAMPUNIT1S),
            0xa => Some(TimestampTicks::DAQTIMESTAMPUNIT1PS),
            0xb => Some(TimestampTicks::DAQTIMESTAMPUNIT10PS),
            0xc => Some(TimestampTicks::DAQTIMESTAMPUNIT100PS),
            _ => None,
        }
    }
}

impl Measurement {
    pub fn new(name: &str, address: u32, address_ext: u8, size: usize) -> Self {
        Self {
            name: name.to_string(),
            address,
            address_ext,
            size,
        }
    }

    pub fn mta(&self) -> Mta {
        Mta::new(self.address, self.address_ext)
    }
}

/// DAQ data record for CSV output
#[derive(Debug, Clone, Serialize)]
pub struct DaqRecord {
    pub timestamp: String,
    pub counter: Option<u32>,
    pub const_data: Option<u32>,
}

/// Parsed DTO packet data
#[derive(Debug, Clone)]
pub struct DtoData {
    /// DAQ list number (from PID)
    pub daq_list: Option<u16>,
    /// ODT number (implied by sequence in DTO packets)
    pub odt: u8,
    /// Raw data bytes
    pub data: Vec<u8>,
    /// Timestamp
    pub timestamp: Option<u32>,
}

/// DAQ Manager - implements Dynamic DAQ
pub struct DaqManager {
    /// Measurement definitions
    measurements: Vec<Measurement>,
    /// DAQ running flag
    running: Arc<AtomicBool>,
    /// Background task handle
    task_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
    /// CSV output path
    csv_path: PathBuf,
    /// Slave address for DAQ connection
    slave_addr: String,
    /// Number of DAQ lists configured
    daq_list_count: u16,
    /// Event channel for DAQ trigger
    event_channel: u16,
    /// Latest DAQ sample (for UI display)
    latest_sample: Arc<RwLock<Option<DaqRecord>>>,
    /// timestamp mode and size
    timestamp_fixed: bool,
    timestamp_size: Option<u8>,
    indentification_field_type: IdentificationFieldType,
    timestamp_tick: Option<TimestampTicks>,
}

impl DaqManager {
    /// Create a new DAQ manager with measurements from A2L
    pub fn new() -> Self {
        Self {
            measurements: vec![
                Measurement::new("const_data", 0xFFFC, 2, 4),
                Measurement::new("counter", 0xFFF8, 2, 4),
            ],
            running: Arc::new(AtomicBool::new(false)),
            task_handle: Arc::new(RwLock::new(None)),
            csv_path: PathBuf::from("daq_data.csv"),
            slave_addr: "127.0.0.1:5555".to_string(),
            daq_list_count: 1,
            event_channel: 0,
            latest_sample: Arc::new(RwLock::new(None)),
            timestamp_fixed: false,
            timestamp_size: None,
            indentification_field_type: IdentificationFieldType::AbosulteODT,
            timestamp_tick: Some(TimestampTicks::DAQTIMESTAMPUNIT1MS),
        }
    }

    /// Set slave address for DAQ
    pub fn set_slave_addr(&mut self, addr: String) {
        self.slave_addr = addr;
    }

    /// Get measurements
    pub fn measurements(&self) -> &[Measurement] {
        &self.measurements
    }

    /// Get latest sample
    pub async fn get_latest_sample(&self) -> Option<DaqRecord> {
        self.latest_sample.read().await.clone()
    }

    pub async fn get_timestamp_mode(&mut self, client: &XcpClient) -> Result<(), String>{
        tracing::debug!("Getting timestamp mode...");
        // Get timestamp mode from slave through GET_DAQ_RESOLUTION_INFO
        let daqresolution_info = client.get_daq_resolution_info().await.map_err(|e| format!("GET_DAQ_RESOLUTION_INFO failed: {}", e))?;
        let timestamp_mode = daqresolution_info.timestamp_mode;
        self.timestamp_fixed = timestamp_mode & 0b1000 == 0b1000;
        let timestamp_size_bits = timestamp_mode & 0b0111;
        match timestamp_size_bits {
            0 if self.timestamp_fixed => {
                tracing::error!("Timestamp mode is fixed but timestamp size is 0.");
                return Err("Timestamp mode is fixed but timestamp size is not specified".to_string());
            }
            0 => {
                self.timestamp_size = None;
            }
            1 | 2 | 4 => {  
                tracing::debug!("Timestamp size: {}", timestamp_size_bits);
                self.timestamp_size = Some(timestamp_size_bits);
            }
            _ => {
                tracing::error!("Timestamp SIZE NOT ALLOWED: {}", timestamp_size_bits);
                return Err("Timestamp SIZE NOT ALLOWED".to_string())
            }
        }
        let timestamp_tick = daqresolution_info.timestamp_ticks;
        self.timestamp_tick = TimestampTicks::from_u16(timestamp_tick);
        return Ok(());
    }

    pub async fn get_daq_processor_info(&self, client: &XcpClient) -> Result<IdentificationFieldType, String> { 
        tracing::debug!("Getting DAQ processor info...");
        let daq_processor_info = client.get_daq_processor_info().await.map_err(|e| format!("GET_DAQ_PROCESSOR_INFO failed: {}", e))?;
        let daq_key_byte = daq_processor_info.daq_key_byte;
        tracing::info!("DAQ key byte: {}", daq_key_byte);
        let bits = (daq_key_byte & 0b11000000) >> 6;
        match bits {
            0 => {
                tracing::debug!("abosulte odt is used");
                return Ok(IdentificationFieldType::AbosulteODT)
            }
            1  => {
                tracing::debug!("releative odt is used");
                return Ok(IdentificationFieldType::RelatetiveODTBYTE)
            }
            2 => {
                tracing::debug!("releative odt is used");
                return Ok(IdentificationFieldType::RelatetiveODTWORD)
            }
            3 => {
                tracing::debug!("releative odt is used");
                return Ok(IdentificationFieldType::RelatetiveODTWORDAlign)
            }
            _ => {
                tracing::error!("IDENTIFICATION FIELD NOT ALLOWED: {}", bits);
                return Err("IDENTIFICATION FIELD NOT ALLOWED".to_string())
            }
        }
    }

    /// Configure DAQ on the XCP slave using Dynamic DAQ
    ///
    /// Steps:
    /// 1. FREE_DAQ - Release existing DAQ configuration
    /// 2. ALLOC_DAQ - Allocate DAQ lists
    /// 3. ALLOC_ODT - Allocate ODTs in each DAQ list
    /// 4. ALLOC_ODT_ENTRY - Allocate entries in each ODT
    /// 5. SET_DAQ_PTR + WRITE_DAQ - Configure each entry
    /// 6. SET_DAQ_LIST_MODE - Set mode and event channel
    pub async fn configure(&mut self, client: &XcpClient) -> Result<(), String> {
        let identification_field_type = self.get_daq_processor_info(client).await?;
        self.indentification_field_type = identification_field_type;
        self.get_timestamp_mode(client).await?;
        tracing::info!("Configuring Dynamic DAQ...");

        // 1. FREE_DAQ - Release any existing DAQ configuration
        tracing::debug!("FREE_DAQ");
        client.free_daq().await.map_err(|e| format!("FREE_DAQ failed: {}", e))?;

        // 2. ALLOC_DAQ - Allocate DAQ lists (we need 1 list)
        tracing::debug!("ALLOC_DAQ: {} lists", self.daq_list_count);
        client
            .alloc_daq(self.daq_list_count)
            .await
            .map_err(|e| format!("ALLOC_DAQ failed: {}", e))?;

        // For DAQ list 0, configure all measurements
        let daq_list: u16 = 0;

        // 3. ALLOC_ODT - Allocate ODTs (one ODT per measurement for simplicity)
        // Or we can put all measurements in one ODT
        // Option 1: One ODT containing all measurements (more efficient)
        let odt_count = 1;
        tracing::debug!("ALLOC_ODT: DAQ list {}, {} ODTs", daq_list, odt_count);
        client
            .alloc_odt(daq_list, odt_count)
            .await
            .map_err(|e| format!("ALLOC_ODT failed: {}", e))?;

        // 4. ALLOC_ODT_ENTRY - Allocate entries in ODT 0
        let entry_count = self.measurements.len() as u8;
        tracing::debug!("ALLOC_ODT_ENTRY: {} entries", entry_count);
        client
            .alloc_odt_entry(daq_list, 0, entry_count)
            .await
            .map_err(|e| format!("ALLOC_ODT_ENTRY failed: {}", e))?;

        // 5. SET_DAQ_PTR + WRITE_DAQ - Configure each entry
        for (entry_idx, meas) in self.measurements.iter().enumerate() {
            tracing::debug!(
                "WRITE_DAQ: entry {}, addr 0x{:08X}, size {}",
                entry_idx,
                meas.address,
                meas.size
            );

            // Set DAQ pointer to this entry
            client
                .set_daq_ptr(daq_list, 0, entry_idx as u8)
                .await
                .map_err(|e| format!("SET_DAQ_PTR failed: {}", e))?;

            // Write DAQ entry configuration
            client
                .write_daq(0, meas.mta(), meas.size as u8)
                .await
                .map_err(|e| format!("WRITE_DAQ failed: {}", e))?;
        }

        // 6. SET_DAQ_LIST_MODE - Configure DAQ list mode
        // Mode: 0 = alternate (send all ODTs in sequence)
        // Event channel: determines trigger source (0 = default/timer)
        // Prescaler: 1 = no prescaling
        // Priority: 0 = default
        tracing::debug!(
            "SET_DAQ_LIST_MODE: mode=0, daq_list={}, event_channel={}, prescaler=1",
            daq_list,
            self.event_channel
        );
        client
            .set_daq_list_mode(0x10, daq_list, self.event_channel, 1, 0)
            .await
            .map_err(|e| format!("SET_DAQ_LIST_MODE failed: {}", e))?;

        tracing::info!("DAQ configuration complete");
        Ok(())
    }

    /// Start DAQ acquisition
    ///
    /// This starts the DAQ list and begins receiving DTO packets from slave
    pub async fn start(&mut self, client: &XcpClient, _interval_ms: u64) -> Result<(), String> {
        if self.running.load(Ordering::SeqCst) {
            return Err("DAQ already running".to_string());
        }

        // Configure DAQ first
        self.configure(client).await?;

        // Get DTO receiver for DAQ reception (must be done before starting DAQ)
        let dto_rx = client.get_dto_receiver().ok_or("No DTO receiver available")?;

        // Start DAQ list 0
        tracing::info!("Selecte DAQ list 0");
        client
            .start_stop_daq_list(2, 0)  // mode=2 (select DAQ list)
            .await
            .map_err(|e| format!("START_STOP_DAQ_LIST failed: {}", e))?;

        tracing::info!("DAQ selected, starting DAQ...");
        // client.start_stop_synch(0x03).await.map_err(|e| format!("START_STOP_SYNCH failed: {}", e))?;
        client.start_stop_synch(0x01).await.map_err(|e| format!("START_STOP_SYNCH failed: {}", e))?;

        self.running.store(true, Ordering::SeqCst);

        // Create CSV file with header
        let csv_path = self.csv_path.clone();
        {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&csv_path)
                .map_err(|e| format!("Failed to create CSV: {}", e))?;

            writeln!(file, "timestamp,counter,const_data")
                .map_err(|e| format!("Failed to write CSV header: {}", e))?;
        }

        let running = self.running.clone();
        let latest_sample = self.latest_sample.clone();
        let measurements = self.measurements.clone();
        let timestamp_size = self.timestamp_size.unwrap_or(0);
        let iden_field_type = self.indentification_field_type.clone();

        // Spawn DTO reception task
        let handle = tokio::spawn(async move {
            let mut csv_file = OpenOptions::new()
                .append(true)
                .open(&csv_path)
                .expect("Failed to open CSV for appending");

            tracing::info!("DAQ started, waiting for DTO packets...");

            while running.load(Ordering::SeqCst) {
                // Receive DTO packet from channel
                // Lock is acquired for each recv to allow other tasks to access the receiver
                let msg = {
                    let mut rx = dto_rx.lock().await;
                    tokio::time::timeout(
                        std::time::Duration::from_millis(100),
                        rx.recv()
                    ).await
                };

                match msg {
                    Ok(Some(msg)) => {
                        // msg.data already contains complete XCP packet (4-byte header + payload)
                        // Parse single DTO packet
                        if let Some(data) = parse_dto_packet(&msg.data, timestamp_size, &iden_field_type) {
                            {
                                tracing::info!("Received DTO: daq_list={:?}, data_len={}", data.daq_list, data.data.len());
                                // Parse measurement data
                                let mut offset = 0;
                                let mut counter_val: Option<u32> = None;
                                let mut const_data_val: Option<u32> = None;
                                for meas in &measurements {
                                    if offset + meas.size <= data.data.len() {
                                        let bytes: [u8; 4] = if meas.size == 4 {
                                            [
                                                data.data[offset],
                                                data.data[offset + 1],
                                                data.data[offset + 2],
                                                data.data[offset + 3],
                                            ]
                                        } else {
                                            [0, 0, 0, 0]
                                        };
                                        if meas.name == "counter" {
                                            counter_val = Some(u32::from_le_bytes(bytes));
                                        } else if meas.name == "const_data" {
                                            const_data_val = Some(u32::from_le_bytes(bytes));
                                        }
                                    }
                                    offset += meas.size;
                                }

                                // Write to CSV
                                let timestamp: DateTime<Utc> = Utc::now();
                                let record = DaqRecord {
                                    timestamp: timestamp.to_rfc3339(),
                                    counter: counter_val,
                                    const_data: const_data_val,
                                };
                                // Update latest sample for UI
                                {
                                    let mut latest = latest_sample.write().await;
                                    *latest = Some(record.clone());
                                }
                                // Write CSV line
                                let csv_line = format!(
                                    "{},{},{}",
                                    record.timestamp,
                                    record.counter.map(|v| v.to_string()).unwrap_or_default(),
                                    record.const_data.map(|v| v.to_string()).unwrap_or_default()
                                );
                                if let Err(e) = writeln!(csv_file, "{}", csv_line) {
                                    tracing::error!("Failed to write CSV: {}", e);
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        // Channel closed
                        tracing::warn!("DTO channel closed");
                        break;
                    }
                    Err(_) => {
                        // Timeout, continue to check running flag
                        continue;
                    }
                }
            }

            tracing::info!("DAQ reception stopped");
        });

        // Store task handle
        {
            let mut handle_guard = self.task_handle.write().await;
            *handle_guard = Some(handle);
        }

        Ok(())
    }

    /// Stop DAQ acquisition
    pub async fn stop(&self, client: &XcpClient) -> Result<(), String> {
        if !self.running.load(Ordering::SeqCst) {
            return Err("DAQ not running".to_string());
        }

        // First, stop DAQ on slave side to prevent further DTO packets
        tracing::info!("Stopping DAQ list on slave...");
        if let Err(e) = client.start_stop_synch(0x00).await {
            tracing::warn!("Failed to send STOP_SYNCH: {}", e);
        }

        // Small delay to allow any in-flight packets to arrive
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Signal reception task to stop
        self.running.store(false, Ordering::SeqCst);

        // Wait for task to finish
        {
            let mut handle_guard = self.task_handle.write().await;
            if let Some(handle) = handle_guard.take() {
                handle.await.ok();
            }
        }

        tracing::info!("DAQ acquisition stopped. Data saved to {}", self.csv_path.display());
        Ok(())
    }

    /// Check if DAQ is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

impl Default for DaqManager {
    fn default() -> Self {
        Self::new()
    }
}


/// Parse a single DTO packet from slave
///
/// DTO packet format (XCP on Ethernet):
/// - 4 bytes: Transport header [LEN_L, LEN_H, COUNTER_L, COUNTER_H]
/// - 1 byte: PID (DAQ list number | 0xC0)
/// - N bytes: ODT data (including identification field and timestamp)
fn parse_dto_packet(packet: &[u8], timestamp_size: u8, ind_field_type: &IdentificationFieldType) -> Option<DtoData> {
    // Minimum size: 4 (header) + 1 (PID) = 5 bytes
    if packet.len() < 5 {
        tracing::error!("DTO packet too short: {} bytes", packet.len());
        return None;
    }

    let mut cur = Cursor::new(packet);

    // Parse XCP header: [LEN_L, LEN_H, COUNTER_L, COUNTER_H]
    let mut buf = [0u8; 2];
    cur.read_exact(&mut buf).ok()?;
    let len = u16::from_le_bytes(buf) as usize;
    cur.read_exact(&mut buf).ok()?;
    let _counter = u16::from_le_bytes(buf);

    // Parse identification field based on type
    let (daq_list, odt, pid_size) = match ind_field_type {
        IdentificationFieldType::AbosulteODT => {
            let mut odt_buff = [0u8];
            cur.read_exact(&mut odt_buff).ok()?;
            (None, odt_buff[0], 1usize)
        }
        IdentificationFieldType::RelatetiveODTBYTE => {
            let mut odt_buff = [0u8];
            cur.read_exact(&mut odt_buff).ok()?;
            let mut daq_buff = [0u8];
            cur.read_exact(&mut daq_buff).ok()?;
            (Some(daq_buff[0] as u16), odt_buff[0], 2usize)
        }
        IdentificationFieldType::RelatetiveODTWORD => {
            let mut odt_buff = [0u8];
            cur.read_exact(&mut odt_buff).ok()?;
            let mut daq_buff = [0u8; 2];
            cur.read_exact(&mut daq_buff).ok()?;
            (Some(u16::from_le_bytes(daq_buff)), odt_buff[0], 3usize)
        }
        IdentificationFieldType::RelatetiveODTWORDAlign => {
            let mut odt_buff = [0u8; 2]; // FILL byte + ODT
            cur.read_exact(&mut odt_buff).ok()?;
            let mut daq_buff = [0u8; 2];
            cur.read_exact(&mut daq_buff).ok()?;
            (Some(u16::from_le_bytes(daq_buff)), odt_buff[0], 4usize)
        }
    };

    // Parse timestamp if present
    let timestamp = match timestamp_size {
        0 => None,
        1 => {
            let mut timestamp_buf = [0u8];
            cur.read_exact(&mut timestamp_buf).ok()?;
            Some(timestamp_buf[0] as u32)
        }
        2 => {
            let mut timestamp_buf = [0u8; 2];
            cur.read_exact(&mut timestamp_buf).ok()?;
            Some(u16::from_le_bytes(timestamp_buf) as u32)
        }
        4 => {
            let mut timestamp_buf = [0u8; 4];
            cur.read_exact(&mut timestamp_buf).ok()?;
            Some(u32::from_le_bytes(timestamp_buf))
        }
        _ => {
            tracing::error!("Invalid timestamp size: {}", timestamp_size);
            None
        }
    };

    // Calculate and read ODT data
    // len = identification field size + timestamp size + data size
    let data_len = len.saturating_sub(pid_size).saturating_sub(timestamp_size as usize);
    let mut data = vec![0u8; data_len];
    cur.read_exact(&mut data).ok()?;

    Some(DtoData {
        daq_list,
        odt,
        timestamp,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_dto_packet() {
        // Valid DTO packet: header + ODT (0x00) + data (AbosulteODT mode)
        // Note: In AbosulteODT mode, the ODT number is absolute, daq_list is None
        // XCP header: len=5, counter=1, then ODT=0x00, then 4 bytes data
        let packet = [0x05, 0x00, 0x01, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04];
        let result = parse_dto_packet(&packet, 0, &IdentificationFieldType::AbosulteODT);
        assert!(result.is_some());
        let data = result.unwrap();
        assert_eq!(data.daq_list, None); // AbosulteODT doesn't include daq_list
        assert_eq!(data.odt, 0x00);
        assert_eq!(data.data, vec![0x01, 0x02, 0x03, 0x04]);

        // Test with RelativeODT mode - includes DAQ list
        // Format: header + ODT (1 byte) + DAQ list (1 byte) + data
        // XCP header: len=6, counter=2, then ODT=0x01, DAQ=0x01, then 4 bytes data
        let packet = [0x06, 0x00, 0x02, 0x00, 0x01, 0x01, 0xAA, 0xBB, 0xCC, 0xDD];
        let result = parse_dto_packet(&packet, 0, &IdentificationFieldType::RelatetiveODTBYTE);
        assert!(result.is_some());
        let data = result.unwrap();
        assert_eq!(data.daq_list, Some(1));
        assert_eq!(data.odt, 0x01);

        // Packet too short (less than 5 bytes)
        let packet = [0x01, 0x00, 0x01, 0x00];
        let result = parse_dto_packet(&packet, 0, &IdentificationFieldType::AbosulteODT);
        assert!(result.is_none());
    }
}
