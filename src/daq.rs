//! DAQ (Data Acquisition) Management
//!
//! Implements XCP Dynamic DAQ mechanism:
//! 1. Configure DAQ lists, ODTs, and ODT entries
//! 2. Start DAQ list - slave sends DTO packets automatically
//! 3. Receive and parse DTO packets from slave

use crate::client::XcpClient;
use crate::protocol::Mta;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write as IoWrite;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

/// DTO (Data Transfer Object) header: PID for DAQ lists
/// PID = DAQ_LIST_NUMBER | 0xC0 (top 2 bits = 11 for DAQ)
const DTO_PID_MASK: u8 = 0xC0;

/// Measurement definition
#[derive(Debug, Clone)]
pub struct Measurement {
    pub name: String,
    pub address: u32,
    pub address_ext: u8,
    pub size: usize,
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
    pub daq_list: u8,
    /// ODT number (implied by sequence in DTO packets)
    pub odt: u8,
    /// Raw data bytes
    pub data: Vec<u8>,
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

    /// Configure DAQ on the XCP slave using Dynamic DAQ
    ///
    /// Steps:
    /// 1. FREE_DAQ - Release existing DAQ configuration
    /// 2. ALLOC_DAQ - Allocate DAQ lists
    /// 3. ALLOC_ODT - Allocate ODTs in each DAQ list
    /// 4. ALLOC_ODT_ENTRY - Allocate entries in each ODT
    /// 5. SET_DAQ_PTR + WRITE_DAQ - Configure each entry
    /// 6. SET_DAQ_LIST_MODE - Set mode and event channel
    pub async fn configure(&self, client: &XcpClient) -> Result<(), String> {
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
    pub async fn start(&self, client: &XcpClient, _interval_ms: u64) -> Result<(), String> {
        if self.running.load(Ordering::SeqCst) {
            return Err("DAQ already running".to_string());
        }

        // Configure DAQ first
        self.configure(client).await?;

        // Get transport for DTO reception (must be done before starting DAQ)
        let transport = client.transport().ok_or("No transport available")?.clone();

        // Start DAQ list 0
        tracing::info!("Starting DAQ list 0");
        client
            .start_stop_daq_list(1, 0) // mode=1 (start), daq_list=0
            .await
            .map_err(|e| format!("START_STOP_DAQ_LIST failed: {}", e))?;

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

        // Spawn DTO reception task
        let handle = tokio::spawn(async move {
            let mut csv_file = OpenOptions::new()
                .append(true)
                .open(&csv_path)
                .expect("Failed to open CSV for appending");

            tracing::info!("DAQ started, waiting for DTO packets...");

            while running.load(Ordering::SeqCst) {
                // Receive DTO packet
                match transport.recv_daq_packet().await {
                    Ok(packet) => {
                        // Parse DTO packet
                        if let Some(data) = parse_dto_packet(&packet) {
                            tracing::trace!("Received DTO: daq_list={}, data_len={}", data.daq_list, data.data.len());

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
                                        counter_val = Some(u32::from_be_bytes(bytes));
                                    } else if meas.name == "const_data" {
                                        const_data_val = Some(u32::from_be_bytes(bytes));
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
                    Err(e) => {
                        tracing::warn!("Failed to receive DTO packet: {}", e);
                        // Small delay to avoid busy loop on error
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
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
    pub async fn stop(&self) -> Result<(), String> {
        if !self.running.load(Ordering::SeqCst) {
            return Err("DAQ not running".to_string());
        }

        // Signal task to stop
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

/// Parse a DTO packet from slave
///
/// DTO packet format (XCP on Ethernet):
/// - 4 bytes: Transport header [LEN_L, LEN_H, COUNTER_L, COUNTER_H]
/// - 1 byte: PID (DAQ list number | 0xC0)
/// - N bytes: ODT data
fn parse_dto_packet(packet: &[u8]) -> Option<DtoData> {
    // Minimum size: 4 (header) + 1 (PID) = 5 bytes
    if packet.len() < 5 {
        tracing::warn!("DTO packet too short: {} bytes", packet.len());
        return None;
    }

    // Skip transport header (4 bytes)
    let pid = packet[4];

    // Check if this is a DAQ DTO (PID should have top 2 bits = 11)
    if pid & DTO_PID_MASK != DTO_PID_MASK {
        tracing::warn!("Invalid DTO PID: 0x{:02X} (expected 0xC0-0xFF)", pid);
        return None;
    }

    // Extract DAQ list number from PID (lower 6 bits)
    let daq_list = pid & 0x3F;

    // ODT number is typically determined by sequence in DTO packets
    // For simple case, assume ODT 0
    let odt = 0;

    // Extract data (skip 4-byte header + 1-byte PID)
    let data = packet[5..].to_vec();

    Some(DtoData {
        daq_list,
        odt,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_dto_packet() {
        // Valid DTO packet: header + PID (0xC0 = DAQ list 0) + data
        let packet = [0x08, 0x00, 0x01, 0x00, 0xC0, 0x01, 0x02, 0x03, 0x04];
        let result = parse_dto_packet(&packet);
        assert!(result.is_some());
        let data = result.unwrap();
        assert_eq!(data.daq_list, 0);
        assert_eq!(data.data, vec![0x01, 0x02, 0x03, 0x04]);

        // DAQ list 1 (PID = 0xC1)
        let packet = [0x08, 0x00, 0x02, 0x00, 0xC1, 0xAA, 0xBB, 0xCC, 0xDD];
        let result = parse_dto_packet(&packet);
        assert!(result.is_some());
        let data = result.unwrap();
        assert_eq!(data.daq_list, 1);

        // Invalid PID (not a DAQ DTO)
        let packet = [0x08, 0x00, 0x03, 0x00, 0xFF, 0x01, 0x02, 0x03, 0x04];
        let result = parse_dto_packet(&packet);
        assert!(result.is_some()); // 0xFF = DAQ list 63

        // Packet too short
        let packet = [0x01, 0x00, 0x01, 0x00];
        let result = parse_dto_packet(&packet);
        assert!(result.is_none());
    }
}
