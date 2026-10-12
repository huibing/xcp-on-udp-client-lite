//! XCP-on-CAN transport.
//!
//! XCP on CAN maps a single XCP packet into one CAN frame. Unlike XCP on
//! Ethernet (which prefixes each packet with a 4-byte LEN/COUNTER header), CAN
//! carries no transport header: the CAN DLC is the packet length and the XCP
//! packet starts at data byte 0. The CAN identifier selects the object:
//! - Master → Slave commands use `can_id` (`CAN_ID_MASTER`).
//! - Slave → Master responses/DAQ data arrive on other identifiers and are
//!   forwarded to the XCP layer, which classifies packets by PID.
//!
//! This module only implements the transport/protocol concerns (framing, ID
//! filtering, `MAX_DLC_REQUIRED` padding, async queueing). The actual hardware
//! is accessed through the [`CanDevice`] trait; see the [`device`] module for the
//! abstraction and [`pcan`] for the PEAK-System PCAN-Basic driver.
//!
//! Incoming frames are filtered by CAN ID **and** PID so that unrelated
//! traffic on a shared bus cannot pollute the XCP stream:
//! - The master's own transmit ID is always rejected (echo suppression).
//! - CTO-class frames (PID >= 0xFC: RES/ERR/EV/SERV) are accepted only from
//!   `CAN_ID_SLAVE` when it is declared. This prevents random bus frames
//!   whose first byte happens to be >= 0xFC from being mistaken for a command
//!   response (the GET_DAQ_PROCESSOR_INFO `0xFC` parse failure).
//! - DAQ-class frames (PID < 0xFC) are accepted from `CAN_ID_SLAVE` and the
//!   fixed DAQ DTO IDs (`DAQ_LIST_CAN_ID` / `EVENT_CAN_ID_LIST`). When no fixed
//!   DAQ IDs are declared (VARIABLE/INCREMENTAL), all non-master IDs are
//!   accepted so DAQ data is never dropped.

mod device;
mod pcan;

use super::transport::{Transport, TransportError, TransportType};
use async_trait::async_trait;
pub use device::{
    driver_names, drivers, find_driver, list_can_devices, CanDevice, CanDeviceConfig,
    CanDeviceError, CanDeviceInfo, CanDriver, CanFrame,
};
use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Driver used when the config does not specify one.
pub const DEFAULT_CAN_DRIVER: &str = "pcan";

/// Default timeout for XCP commands (T1 = 1000ms).
const DEFAULT_TIMEOUT_MS: u64 = 1000;

/// Configuration for establishing a CAN transport.
///
/// Normally built from the A2L `XCP_ON_CAN` block plus an optional user-selected
/// driver/channel.
#[derive(Debug, Clone, Default)]
pub struct CanTransportConfig {
    /// CAN driver id (e.g. `"pcan"`). `None` uses the default driver.
    pub driver: Option<String>,
    /// Driver channel id/name, or `auto`/empty to let the driver choose.
    pub channel: String,
    /// Stable device id (takes precedence over `channel` when set).
    pub device_id: Option<String>,
    /// Controller index within the device (used with `device_id`).
    pub controller: Option<u8>,
    /// CAN ID used by the master to send commands to the slave (`CAN_ID_MASTER`).
    pub can_id: u32,
    /// Nominal (arbitration-phase) bit rate in bit/s.
    pub baudrate: Option<u32>,
    /// CAN-FD data-phase bit rate in bit/s. `Some` selects CAN-FD mode.
    pub data_baudrate: Option<u32>,
    /// CAN ID the slave uses for responses (`CAN_ID_SLAVE`).
    pub slave_id: Option<u32>,
    /// CAN IDs used for DAQ DTO frames (`DAQ_LIST_CAN_ID`, fixed mapping).
    pub daq_ids: Vec<u32>,
    /// When true, master command frames are padded to the maximum DLC
    /// (8 for classic CAN, 64 for CAN-FD) as required by `MAX_DLC_REQUIRED`.
    pub max_dlc_required: bool,
}

impl CanTransportConfig {
    /// Create a config with a channel and command CAN ID but no bit rate.
    pub fn new(channel: impl Into<String>, can_id: u32) -> Self {
        Self {
            channel: channel.into(),
            can_id,
            ..Default::default()
        }
    }

    /// Set the CAN driver id.
    pub fn with_driver(mut self, driver: impl Into<String>) -> Self {
        self.driver = Some(driver.into());
        self
    }

    /// Set the stable device id (and optional controller index).
    pub fn with_device_id(mut self, device_id: impl Into<String>, controller: Option<u8>) -> Self {
        self.device_id = Some(device_id.into());
        self.controller = controller;
        self
    }

    /// Set the nominal (arbitration-phase) bit rate in bit/s.
    pub fn with_baudrate(mut self, bps: u32) -> Self {
        self.baudrate = Some(bps);
        self
    }

    /// Set the CAN-FD data-phase bit rate in bit/s (selects CAN-FD mode).
    pub fn with_data_baudrate(mut self, bps: u32) -> Self {
        self.data_baudrate = Some(bps);
        self
    }

    /// Set the slave response CAN ID (`CAN_ID_SLAVE`).
    pub fn with_slave_id(mut self, id: u32) -> Self {
        self.slave_id = Some(id);
        self
    }

    /// Set the DAQ DTO CAN IDs.
    pub fn with_daq_ids(mut self, ids: Vec<u32>) -> Self {
        self.daq_ids = ids;
        self
    }

    /// Require command frames to be padded to the maximum DLC.
    pub fn with_max_dlc_required(mut self, required: bool) -> Self {
        self.max_dlc_required = required;
        self
    }
}

/// CAN transport for XCP on CAN, layered on top of a [`CanDevice`].
pub struct CanTransport {
    /// Resolved driver id.
    driver: String,
    /// Resolved channel name.
    channel: String,
    /// CAN ID used for master → slave commands.
    can_id: u32,
    /// True when `can_id` uses a 29-bit identifier.
    extended: bool,
    /// True when CAN-FD framing is used.
    fd: bool,
    /// Pad command frames to the maximum DLC (`MAX_DLC_REQUIRED`).
    max_dlc_required: bool,
    /// Underlying low-level CAN device.
    device: Arc<Mutex<Box<dyn CanDevice>>>,
    /// Command/response timeout.
    timeout_ms: u64,
    /// Frames received by the background reader thread, paired with the
    /// optional hardware reception timestamp (microseconds) from the device.
    rx: Arc<Mutex<VecDeque<(Vec<u8>, Option<u64>)>>>,
    /// Signal to stop the reader thread.
    reader_stop: Arc<AtomicBool>,
    /// Background reader thread handle.
    reader: Mutex<Option<JoinHandle<()>>>,
    /// Connection state.
    connected: AtomicBool,
}

impl CanTransport {
    /// Create a new CAN transport from configuration.
    ///
    /// The driver is chosen from `config.driver` (defaulting to
    /// [`DEFAULT_CAN_DRIVER`]); the channel/bit rate come from the config
    /// (typically derived from the A2L `XCP_ON_CAN` block and/or a user
    /// selection).
    pub async fn connect(config: &CanTransportConfig) -> Result<Self, TransportError> {
        let driver_name = config
            .driver
            .clone()
            .unwrap_or_else(|| DEFAULT_CAN_DRIVER.to_string());
        let driver = find_driver(&driver_name).ok_or_else(|| {
            TransportError::ConnectFailed(format!("CAN driver not found: {}", driver_name))
        })?;

        let device_config = CanDeviceConfig {
            channel: config.channel.clone(),
            device_id: config.device_id.clone(),
            controller: config.controller,
            baudrate: config.baudrate,
            data_baudrate: config.data_baudrate,
        };
        let device = driver.open(&device_config).map_err(map_device_error)?;
        let info = device.info();
        let fd = device.is_fd();
        tracing::info!(
            "CAN transport: driver={}, channel={}, bitrate={}, fd={}",
            driver_name,
            info.channel,
            device.bitrate_description(),
            fd
        );

        let device = Arc::new(Mutex::new(device));
        let rx = Arc::new(Mutex::new(VecDeque::new()));
        let reader_stop = Arc::new(AtomicBool::new(false));
        let filter = FrameFilter::from_config(config);
        tracing::info!(
            "CAN receive filter: master=0x{:X}, {}",
            config.can_id,
            filter.describe()
        );
        if config.slave_id.is_none() {
            tracing::warn!(
                "CAN_ID_SLAVE is not declared; CTO responses cannot be filtered \
                 by CAN ID and may be polluted by unrelated bus traffic"
            );
        }
        let reader = spawn_reader(device.clone(), filter, rx.clone(), reader_stop.clone());

        let can_id = config.can_id;
        Ok(Self {
            driver: driver_name,
            channel: info.channel,
            can_id,
            extended: can_id > 0x7FF,
            fd,
            max_dlc_required: config.max_dlc_required,
            device,
            timeout_ms: DEFAULT_TIMEOUT_MS,
            rx,
            reader_stop,
            reader: Mutex::new(Some(reader)),
            connected: AtomicBool::new(true),
        })
    }

    /// Get the resolved driver id.
    pub fn driver(&self) -> &str {
        &self.driver
    }

    /// Get the resolved channel name.
    pub fn channel(&self) -> &str {
        &self.channel
    }

    /// Get the command CAN ID.
    pub fn can_id(&self) -> u32 {
        self.can_id
    }

    /// Whether CAN-FD framing is active.
    pub fn is_fd(&self) -> bool {
        self.fd
    }

    /// Set the command/response timeout.
    pub fn set_timeout(&mut self, timeout_ms: u64) {
        self.timeout_ms = timeout_ms;
    }

    /// Transmit a normalized XCP payload as a single CAN frame.
    ///
    /// Unlike XCP-on-Ethernet there is no length/counter header: the CAN DLC
    /// carries the length and the XCP packet starts at data byte 0. When
    /// `MAX_DLC_REQUIRED` is set the frame is padded to the maximum DLC.
    fn send_frame(&self, payload: &[u8]) -> Result<(), TransportError> {
        if payload.is_empty() {
            return Err(TransportError::SendFailed(
                "refusing to send empty XCP payload".to_string(),
            ));
        }
        if !self.fd && payload.len() > 8 {
            return Err(TransportError::SendFailed(format!(
                "XCP payload of {} bytes does not fit in a classic CAN frame",
                payload.len()
            )));
        }

        // `MAX_DLC_REQUIRED` requires fixed-size command frames; unused bytes
        // are filled with 0x00 (max DLC is 8 for classic CAN, 64 for CAN-FD).
        let padded = if self.max_dlc_required {
            let target = if self.fd { 64 } else { 8 };
            if payload.len() < target {
                let mut buf = payload.to_vec();
                buf.resize(target, 0x00);
                Some(buf)
            } else {
                None
            }
        } else {
            None
        };
        let payload = padded.as_deref().unwrap_or(payload);

        tracing::trace!(
            "Sending XCP-on-CAN frame: id=0x{:X}, fd={}, len={}, data=[{:02X?}]",
            self.can_id,
            self.fd,
            payload.len(),
            payload
        );

        let frame = CanFrame {
            id: self.can_id,
            extended: self.extended,
            data: payload.to_vec(),
            timestamp_us: None,
        };
        self.device
            .lock()
            .expect("CAN device mutex poisoned")
            .send_frame(&frame)
            .map_err(|e| TransportError::SendFailed(e.to_string()))
    }

    async fn recv_payload(
        &self,
        timeout_ms: Option<u64>,
    ) -> Result<Option<(Vec<u8>, Option<u64>)>, TransportError> {
        let start = Instant::now();
        loop {
            {
                let mut queue = self.rx.lock().unwrap();
                if let Some(item) = queue.pop_front() {
                    return Ok(Some(item));
                }
            }

            if !self.connected.load(Ordering::SeqCst) {
                return Err(TransportError::ConnectionClosed);
            }

            if let Some(timeout) = timeout_ms {
                if start.elapsed() >= Duration::from_millis(timeout) {
                    return Ok(None);
                }
            }

            tokio::time::sleep(Duration::from_micros(500)).await;
        }
    }
}

#[async_trait]
impl Transport for CanTransport {
    async fn send_command_only(&self, command: &[u8]) -> Result<(), TransportError> {
        self.send_frame(command)
    }

    async fn send_command(&self, command: &[u8]) -> Result<Vec<u8>, TransportError> {
        self.send_frame(command)?;
        match self.recv_payload(Some(self.timeout_ms)).await? {
            Some((payload, _)) => Ok(payload),
            None => Err(TransportError::Timeout),
        }
    }

    async fn recv_packet(&self) -> Result<Vec<u8>, TransportError> {
        match self.recv_payload(None).await? {
            Some((payload, _)) => Ok(payload),
            None => Err(TransportError::ConnectionClosed),
        }
    }

    async fn try_recv_packet(&self, timeout_ms: u64) -> Result<Option<Vec<u8>>, TransportError> {
        Ok(self.recv_payload(Some(timeout_ms)).await?.map(|(p, _)| p))
    }

    async fn recv_packet_ts(&self) -> Result<(Vec<u8>, Option<u64>), TransportError> {
        match self.recv_payload(None).await? {
            Some(item) => Ok(item),
            None => Err(TransportError::ConnectionClosed),
        }
    }

    async fn try_recv_packet_ts(
        &self,
        timeout_ms: u64,
    ) -> Result<Option<(Vec<u8>, Option<u64>)>, TransportError> {
        self.recv_payload(Some(timeout_ms)).await
    }

    async fn close(&self) -> Result<(), TransportError> {
        tracing::trace!(
            "Closing CAN transport on {}:{} (id 0x{:X})",
            self.driver,
            self.channel,
            self.can_id
        );

        self.connected.store(false, Ordering::SeqCst);
        self.reader_stop.store(true, Ordering::SeqCst);

        if let Some(handle) = self.reader.lock().unwrap().take() {
            let _ = handle.join();
        }

        if let Ok(mut device) = self.device.lock() {
            let _ = device.close();
        }

        Ok(())
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Can
    }

    fn target_addr(&self) -> String {
        format!("{}:{}:0x{:03X}", self.driver, self.channel, self.can_id)
    }
}

impl Drop for CanTransport {
    fn drop(&mut self) {
        self.reader_stop.store(true, Ordering::SeqCst);
        if let Ok(mut guard) = self.reader.lock() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }
        // Only close the device if close() has not already done it.
        if self.connected.swap(false, Ordering::SeqCst) {
            if let Ok(mut device) = self.device.lock() {
                let _ = device.close();
            }
        }
    }
}

/// Map a driver error into a transport error.
fn map_device_error(error: CanDeviceError) -> TransportError {
    match error {
        CanDeviceError::DriverNotFound(_) | CanDeviceError::UnknownChannel(_) => {
            TransportError::InvalidAddress(error.to_string())
        }
        CanDeviceError::NoChannel(_) | CanDeviceError::Unsupported(_) => {
            TransportError::InvalidAddress(error.to_string())
        }
        CanDeviceError::OpenFailed(_)
        | CanDeviceError::SendFailed(_)
        | CanDeviceError::RecvFailed(_)
        | CanDeviceError::Driver(_) => TransportError::ConnectFailed(error.to_string()),
    }
}

/// Frame acceptance rule for the reader thread.
///
/// Filtering is joint on CAN ID **and** PID so that command responses (CTO)
/// are never polluted by unrelated bus traffic, while DAQ (DTO) stays lenient
/// enough to support VARIABLE/INCREMENTAL DAQ IDs:
/// - The master's own transmit ID is always rejected (echo suppression).
/// - CTO-class frames (PID >= 0xFC: RES/ERR/EV/SERV) are accepted only from
///   `slave_id` when it is known. A random frame whose first byte happens to
///   be >= 0xFC on any other ID is dropped instead of being mistaken for a
///   command response.
/// - DAQ-class frames (PID < 0xFC) are accepted from `slave_id` and the fixed
///   DAQ DTO IDs; when no fixed DAQ IDs are declared, every non-master ID is
///   accepted (VARIABLE DAQ fallback).
struct FrameFilter {
    master_id: u32,
    slave_id: Option<u32>,
    /// DAQ DTO allow-list: `slave_id` ∪ fixed DAQ IDs when fixed DAQ IDs are
    /// declared; `None` when DAQ IDs are unknown (accept all non-master).
    daq_allowed: Option<HashSet<u32>>,
}

impl FrameFilter {
    fn from_config(config: &CanTransportConfig) -> Self {
        // DAQ DTO frames come from the fixed DAQ IDs and/or the slave response
        // ID (DAQ may share it when no separate DAQ ID is declared). When the
        // A2L does not declare fixed DAQ IDs (VARIABLE/INCREMENTAL), keep the
        // DAQ path lenient so dynamically-assigned DAQ traffic is not dropped.
        let daq_allowed = if config.daq_ids.is_empty() {
            None
        } else {
            let mut set: HashSet<u32> = config.daq_ids.iter().copied().collect();
            if let Some(slave_id) = config.slave_id {
                set.insert(slave_id);
            }
            Some(set)
        };

        Self {
            master_id: config.can_id,
            slave_id: config.slave_id,
            daq_allowed,
        }
    }

    /// Decide whether a received frame should enter the XCP stream.
    ///
    /// `pid` is the first data byte (XCP packet ID). CTO-class frames
    /// (PID >= 0xFC) are only accepted from the declared slave response ID, so
    /// unrelated bus traffic can never be mistaken for a command response.
    /// DAQ-class frames (PID < 0xFC) follow the DAQ allow-list (lenient when
    /// DAQ IDs are unknown).
    fn accepts(&self, can_id: u32, pid: u8) -> bool {
        if can_id == self.master_id {
            return false; // echo suppression
        }
        if pid >= 0xFC {
            // CTO (RES/ERR/EV/SERV): only from the slave response ID.
            match self.slave_id {
                Some(slave_id) => can_id == slave_id,
                None => true, // slave ID unknown -> cannot filter (lenient)
            }
        } else {
            // DAQ DTO.
            match &self.daq_allowed {
                Some(set) => set.contains(&can_id),
                None => true, // DAQ IDs unknown -> accept (VARIABLE DAQ)
            }
        }
    }

    fn describe(&self) -> String {
        let cto = match self.slave_id {
            Some(id) => format!("CTO only from 0x{:X}", id),
            None => "CTO accept-all (CAN_ID_SLAVE unknown)".to_string(),
        };
        let dto = match &self.daq_allowed {
            Some(set) => {
                let mut ids: Vec<String> = set.iter().map(|id| format!("0x{:X}", id)).collect();
                ids.sort();
                format!("DAQ strict [{}]", ids.join(", "))
            }
            None => "DAQ accept-all (fixed DAQ IDs unknown)".to_string(),
        };
        format!("{}; {}", cto, dto)
    }
}

/// Start the background thread that drains the CAN device receive queue.
fn spawn_reader(
    device: Arc<Mutex<Box<dyn CanDevice>>>,
    filter: FrameFilter,
    rx: Arc<Mutex<VecDeque<(Vec<u8>, Option<u64>)>>>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("can-reader".to_string())
        .spawn(move || {
            tracing::debug!("CAN reader thread started");
            while !stop.load(Ordering::SeqCst) {
                let mut received_any = false;
                loop {
                    let result = {
                        let device = device.lock().expect("CAN device mutex poisoned");
                        device.recv_frame(None)
                    };

                    match result {
                        Ok(Some(frame)) => {
                            received_any = true;
                            if frame.data.is_empty() {
                                // Error/status frame: ignore.
                                continue;
                            }
                            let pid = frame.data[0];
                            if !filter.accepts(frame.id, pid) {
                                tracing::trace!(
                                    "Ignoring CAN frame id=0x{:X} pid=0x{:02X} (filtered out)",
                                    frame.id, pid
                                );
                                continue;
                            }
                            rx.lock().unwrap().push_back((frame.data, frame.timestamp_us));
                        }
                        Ok(None) => break,
                        Err(error) => {
                            tracing::warn!("CAN receive error: {}", error);
                            break;
                        }
                    }
                }

                // Poll fast when idle; yield briefly when frames are streaming.
                if !received_any {
                    std::thread::sleep(Duration::from_micros(200));
                }
            }
            tracing::debug!("CAN reader thread stopped");
        })
        .expect("failed to spawn CAN reader thread")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_transport_config_builders() {
        let cfg = CanTransportConfig::new("auto", 0x123)
            .with_driver("pcan")
            .with_baudrate(500_000)
            .with_data_baudrate(2_000_000)
            .with_slave_id(0x456)
            .with_daq_ids(vec![0x310, 0x320])
            .with_max_dlc_required(true);
        assert_eq!(cfg.driver.as_deref(), Some("pcan"));
        assert_eq!(cfg.channel, "auto");
        assert_eq!(cfg.can_id, 0x123);
        assert_eq!(cfg.baudrate, Some(500_000));
        assert_eq!(cfg.data_baudrate, Some(2_000_000));
        assert_eq!(cfg.slave_id, Some(0x456));
        assert_eq!(cfg.daq_ids, vec![0x310, 0x320]);
        assert!(cfg.max_dlc_required);

        let cfg = cfg.with_device_id("3", Some(1));
        assert_eq!(cfg.device_id.as_deref(), Some("3"));
        assert_eq!(cfg.controller, Some(1));
    }

    #[test]
    fn test_frame_filter_strict_when_ids_known() {
        let cfg = CanTransportConfig {
            channel: "auto".to_string(),
            can_id: 0x123,
            slave_id: Some(0x456),
            daq_ids: vec![0x310, 0x320],
            ..Default::default()
        };
        let filter = FrameFilter::from_config(&cfg);

        // CTO (RES/ERR/EV/SERV, pid >= 0xFC) only from slave_id.
        assert!(filter.accepts(0x456, 0xFF)); // RES from slave
        assert!(filter.accepts(0x456, 0xFE)); // ERR from slave
        assert!(!filter.accepts(0x310, 0xFF)); // CTO not from DAQ IDs
        assert!(!filter.accepts(0x999, 0xFC)); // unrelated CTO traffic (the bug)
        assert!(!filter.accepts(0x123, 0xFF)); // master echo

        // DAQ (pid < 0xFC) from slave_id and declared DAQ IDs.
        assert!(filter.accepts(0x456, 0x00)); // DAQ on slave id
        assert!(filter.accepts(0x310, 0x10)); // DAQ on declared id
        assert!(filter.accepts(0x320, 0x10));
        assert!(!filter.accepts(0x999, 0x10)); // unrelated DAQ traffic
        assert!(!filter.accepts(0x123, 0x10)); // master echo
    }

    #[test]
    fn test_frame_filter_lenient_without_daq_ids() {
        let cfg = CanTransportConfig {
            channel: "auto".to_string(),
            can_id: 0x123,
            slave_id: Some(0x456),
            daq_ids: Vec::new(),
            ..Default::default()
        };
        let filter = FrameFilter::from_config(&cfg);

        // CTO still strictly from slave_id even when DAQ IDs are unknown:
        // this is the core fix for command-response pollution.
        assert!(filter.accepts(0x456, 0xFF));
        assert!(!filter.accepts(0x999, 0xFC)); // unrelated CTO traffic rejected
        assert!(!filter.accepts(0x123, 0xFF)); // master echo

        // DAQ lenient (VARIABLE DAQ) when DAQ IDs unknown.
        assert!(filter.accepts(0x456, 0x00));
        assert!(filter.accepts(0x999, 0x10)); // unknown DAQ id accepted
        assert!(!filter.accepts(0x123, 0x10)); // master echo
    }

    #[test]
    fn test_frame_filter_slave_id_unknown() {
        // Without CAN_ID_SLAVE we cannot filter CTO by ID (lenient, warned at
        // connect time). DAQ is also lenient. Master echo is still rejected.
        let cfg = CanTransportConfig {
            channel: "auto".to_string(),
            can_id: 0x123,
            slave_id: None,
            daq_ids: Vec::new(),
            ..Default::default()
        };
        let filter = FrameFilter::from_config(&cfg);
        assert!(filter.accepts(0x999, 0xFF));
        assert!(filter.accepts(0x456, 0xFC));
        assert!(!filter.accepts(0x123, 0xFF)); // master echo
        assert!(filter.accepts(0x999, 0x10)); // DAQ lenient
    }

    #[test]
    fn test_frame_filter_rejects_unrelated_cto_traffic_regression() {
        // Reproduces the GET_DAQ_PROCESSOR_INFO failure: a periodic frame on an
        // unrelated CAN ID whose first byte is 0xFC (SERV) was consumed as the
        // command response. With slave_id declared, such frames must be dropped
        // regardless of the DAQ ID configuration.
        let cfg = CanTransportConfig {
            channel: "auto".to_string(),
            can_id: 0x7FF, // master
            slave_id: Some(0x800), // slave response ID
            daq_ids: Vec::new(), // no fixed DAQ IDs
            ..Default::default()
        };
        let filter = FrameFilter::from_config(&cfg);
        // Random 0x123 traffic with a high first byte must NOT enter the stream.
        assert!(!filter.accepts(0x123, 0xFC));
        assert!(!filter.accepts(0x123, 0xFF));
        assert!(!filter.accepts(0x123, 0xFE));
        // The real slave response on slave_id is accepted.
        assert!(filter.accepts(0x800, 0xFF));
        assert!(filter.accepts(0x800, 0xFE));
    }

    #[test]
    fn test_default_driver_is_registered() {
        assert!(find_driver(DEFAULT_CAN_DRIVER).is_some());
    }
}
