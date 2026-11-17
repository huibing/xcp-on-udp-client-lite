//! XCP-on-CAN transport.
//!
//! XCP on CAN maps a single XCP packet into one CAN frame. Unlike XCP on Ethernet (which prefixes
//! each packet with a 4-byte LEN/COUNTER header), CAN carries no transport header: the CAN DLC is
//! the packet length and the XCP packet starts at data byte 0. The CAN identifier selects the
//! object:
//! - Master → Slave commands use `can_id` (`CAN_ID_MASTER`).
//! - Slave → Master responses/DAQ data arrive on other identifiers and are forwarded to the XCP
//!   layer, which classifies packets by PID.
//!
//! This module only implements the transport/protocol concerns (framing, ID filtering,
//! `MAX_DLC_REQUIRED` padding, async queueing). Received frames are handed to the consumer in
//! arrival order over a plain FIFO; PID-based classification (CTO vs DTO) is the router's job, not
//! the transport's. The actual hardware is accessed through the [`CanDevice`] trait; see the
//! [`device`] module for the abstraction and [`pcan`] for the PEAK-System PCAN-Basic driver,
//! [`es582`] for ETAS ES582 hardware through the BOA V2 / OCI runtime.
//!
//! Incoming frames are filtered by CAN ID **and** PID so that unrelated traffic on a shared bus
//! cannot pollute the XCP stream (see [`shared::FrameFilter`]).

mod device;
pub mod es582;
mod pcan;
mod receiver;
mod sender;
mod shared;

use super::error::TransportError;
use super::link::TransportLink;
use shared::{spawn_reader, CanShared, FrameFilter};
pub use device::{
    driver_names, drivers, find_driver, list_can_devices, CanDevice, CanDeviceConfig,
    CanDeviceError, CanDeviceInfo, CanDriver, CanFrame,
};
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

/// Drop any cached CAN hardware enumeration (currently the ES582 URI list).
///
/// Call this before an explicit hardware scan so the result reflects the
/// devices attached right now.
pub fn invalidate_hardware_cache() {
    es582::invalidate_uri_cache();
}

/// Driver used when the config does not specify one.
pub const DEFAULT_CAN_DRIVER: &str = "pcan";

/// Configuration for establishing a CAN link.
///
/// Normally built from the A2L `XCP_ON_CAN` block plus an optional user-selected driver/channel.
#[derive(Debug, Clone, Default)]
pub struct CanConfig {
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
    /// When true, master command frames are padded to the maximum DLC (8 for classic CAN, 64 for
    /// CAN-FD) as required by `MAX_DLC_REQUIRED`.
    pub max_dlc_required: bool,
}

impl CanConfig {
    /// Create a config with a channel and command CAN ID but no bit rate.
    pub fn new(channel: impl Into<String>, can_id: u32) -> Self {
        Self {
            channel: channel.into(),
            can_id,
            ..Default::default()
        }
    }

    pub fn with_driver(mut self, driver: impl Into<String>) -> Self {
        self.driver = Some(driver.into());
        self
    }

    pub fn with_device_id(mut self, device_id: impl Into<String>, controller: Option<u8>) -> Self {
        self.device_id = Some(device_id.into());
        self.controller = controller;
        self
    }

    pub fn with_baudrate(mut self, bps: u32) -> Self {
        self.baudrate = Some(bps);
        self
    }

    pub fn with_data_baudrate(mut self, bps: u32) -> Self {
        self.data_baudrate = Some(bps);
        self
    }

    pub fn with_slave_id(mut self, id: u32) -> Self {
        self.slave_id = Some(id);
        self
    }

    pub fn with_daq_ids(mut self, ids: Vec<u32>) -> Self {
        self.daq_ids = ids;
        self
    }

    pub fn with_max_dlc_required(mut self, required: bool) -> Self {
        self.max_dlc_required = required;
        self
    }
}

/// Open a CAN channel and return the two halves of the link.
pub async fn open(config: &CanConfig) -> Result<TransportLink, TransportError> {
    let driver_name = config
        .driver
        .clone()
        .unwrap_or_else(|| DEFAULT_CAN_DRIVER.to_string());
    let addr = format!("{}:{}:0x{:03X}", driver_name, config.channel, config.can_id);

    let driver = find_driver(&driver_name).ok_or_else(|| {
        TransportError::InvalidAddress(format!("CAN driver not found: {driver_name}"))
    })?;

    let filter = FrameFilter::from_config(config);

    // Only the slave's response ID and the DAQ DTO IDs carry XCP data in the slave→master
    // direction. Passing them to the driver lets it program hardware acceptance filters, which
    // matters for devices whose receive path is expensive per frame (e.g. one USB round trip per
    // frame on an ES582): unfiltered periodic bus traffic would otherwise push the response out of
    // the device queue.
    let mut accept_ids: Vec<u32> = Vec::new();
    if let Some(slave_id) = config.slave_id {
        accept_ids.push(slave_id);
    }
    for id in &config.daq_ids {
        if !accept_ids.contains(id) {
            accept_ids.push(*id);
        }
    }

    let device_config = CanDeviceConfig {
        channel: config.channel.clone(),
        device_id: config.device_id.clone(),
        controller: config.controller,
        baudrate: config.baudrate,
        data_baudrate: config.data_baudrate,
        accept_ids,
    };
    let device = driver
        .open(&device_config)
        .map_err(|e| shared::map_open_error(&addr, e))?;
    let info = device.info();
    let fd = device.is_fd();

    tracing::info!(
        "CAN transport: driver={}, channel={}, bitrate={}, fd={}",
        driver_name,
        info.channel,
        device.bitrate_description(),
        fd
    );
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

    let device: Arc<Mutex<Box<dyn CanDevice>>> = Arc::new(Mutex::new(device));
    let (frame_tx, frame_rx) = tokio::sync::mpsc::unbounded_channel();
    let stop = CancellationToken::new();
    let reader = spawn_reader(device.clone(), filter, frame_tx, stop.clone());
    let shared = CanShared::new(device, stop, reader);

    Ok(TransportLink {
        sender: Box::new(sender::CanSender::new(
            shared.clone(),
            config.can_id,
            fd,
            config.max_dlc_required,
            addr.clone(),
        )),
        receiver: Box::new(receiver::CanReceiver::new(shared, frame_rx, addr)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_config_builders() {
        let cfg = CanConfig::new("auto", 0x123)
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
        let cfg = CanConfig {
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
    fn test_frame_filter_daq_defaults_to_slave_id_without_daq_ids() {
        // No DAQ_LIST_CAN_ID declared: per the XCP-on-CAN spec, DAQ packets share CAN_ID_SLAVE.
        // Unrelated bus traffic must NOT be accepted as DAQ data — otherwise periodic CAN
        // messages whose first data byte happens to be a valid PID (e.g. 0x00) corrupt DAQ
        // recordings with garbage/zero samples.
        let cfg = CanConfig {
            channel: "auto".to_string(),
            can_id: 0x550,
            slave_id: Some(0x551),
            daq_ids: Vec::new(),
            ..Default::default()
        };
        let filter = FrameFilter::from_config(&cfg);

        assert!(filter.accepts(0x551, 0x00)); // real DAQ DTO on CAN_ID_SLAVE
        assert!(filter.accepts(0x551, 0x60));
        assert!(!filter.accepts(0x310, 0x00)); // ECU application CAN message
        assert!(!filter.accepts(0x999, 0x10)); // unrelated bus traffic
        assert!(!filter.accepts(0x123, 0x10)); // master echo

        assert!(filter.accepts(0x551, 0xFF));
        assert!(!filter.accepts(0x999, 0xFC));
    }

    #[test]
    fn test_frame_filter_slave_id_unknown() {
        // Without CAN_ID_SLAVE we cannot filter CTO by ID (lenient, warned at connect time).
        // DAQ is also lenient. Master echo is still rejected.
        let cfg = CanConfig {
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
        // Reproduces the GET_DAQ_PROCESSOR_INFO failure: a periodic frame on an unrelated CAN ID
        // whose first byte is 0xFC (SERV) was consumed as the command response.
        let cfg = CanConfig {
            channel: "auto".to_string(),
            can_id: 0x7FF,
            slave_id: Some(0x800),
            daq_ids: Vec::new(),
            ..Default::default()
        };
        let filter = FrameFilter::from_config(&cfg);
        assert!(!filter.accepts(0x123, 0xFC));
        assert!(!filter.accepts(0x123, 0xFF));
        assert!(!filter.accepts(0x123, 0xFE));
        assert!(filter.accepts(0x800, 0xFF));
        assert!(filter.accepts(0x800, 0xFE));
    }

    #[test]
    fn test_default_driver_is_registered() {
        assert!(find_driver(DEFAULT_CAN_DRIVER).is_some());
    }
}