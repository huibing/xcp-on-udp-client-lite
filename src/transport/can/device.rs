//! CAN device driver abstraction.
//!
//! A [`CanDevice`] is a low-level, driver-specific CAN controller handle. It
//! knows how to open a channel, transmit/receive raw CAN frames and close the
//! channel — nothing about XCP. The XCP-on-CAN transport ([`super::CanTransport`])
//! is built on top of it and owns the protocol-level concerns (one XCP packet
//! per frame, PID-based receive, `MAX_DLC_REQUIRED` padding, async queueing).
//!
//! New hardware support is added by implementing [`CanDriver`] + [`CanDevice`]
//! (see [`super::pcan`] for the PEAK-System PCAN-Basic implementation) and
//! registering the driver in [`drivers`].

use std::time::Duration;

/// One raw CAN frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanFrame {
    /// 11-bit or 29-bit CAN identifier.
    pub id: u32,
    /// True when `id` is a 29-bit extended identifier.
    pub extended: bool,
    /// Data bytes (0..=8 for classic CAN, 0..=64 for CAN-FD).
    pub data: Vec<u8>,
    /// Hardware reception timestamp in microseconds since the device clock
    /// epoch, when the driver provides one (e.g. PCAN USB frames). `None` for
    /// drivers/transports without hardware timestamping.
    pub timestamp_us: Option<u64>,
}

impl CanFrame {
    /// Create a frame, deriving the extended flag from the identifier.
    pub fn new(id: u32, data: impl Into<Vec<u8>>) -> Self {
        Self {
            id,
            extended: id > 0x7FF,
            data: data.into(),
            timestamp_us: None,
        }
    }
}

/// Describes a CAN channel that can be opened.
///
/// A driver may expose several channels per physical device, so the identity is
/// split into a **device level** (`device_id`, `name`, `device_type`) that stays
/// stable across re-plug/reboot, and a **channel level** (`controller`,
/// `channel`) that identifies the specific controller on that device.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CanDeviceInfo {
    /// Driver id, e.g. `"pcan"`.
    pub driver: String,
    /// Stable device identifier within the driver (PCAN: the device number).
    pub device_id: String,
    /// Human-readable display name of the device.
    pub name: String,
    /// Hardware family, e.g. `"PCAN-USB"`.
    pub device_type: String,
    /// Controller index within the device (0-based).
    pub controller: u8,
    /// Driver-specific channel id, e.g. `"PCAN_USBBUS1"` or `"0x51"`.
    pub channel: String,
    /// Whether the channel is currently free to open.
    pub available: bool,
    /// Whether the hardware reports CAN-FD capability.
    pub fd_capable: bool,
}

impl CanDeviceInfo {
    /// Stable selection token passed from the UI: `driver:device_id:controller`.
    pub fn selection_value(&self) -> String {
        format!("{}:{}:{}", self.driver, self.device_id, self.controller)
    }

    /// Display label combining device and channel information.
    pub fn display_label(&self) -> String {
        let bus = if self.channel.is_empty() {
            format!("ctrl{}", self.controller)
        } else {
            self.channel.clone()
        };
        format!("{} · {} (device {})", self.name, bus, self.device_id)
    }
}

/// Parameters used to open a channel.
#[derive(Debug, Clone, Default)]
pub struct CanDeviceConfig {
    /// Driver channel id/name, or `"auto"`/empty to let the driver choose.
    pub channel: String,
    /// Stable device id (take precedence over `channel` when set).
    pub device_id: Option<String>,
    /// Controller index within the device (used with `device_id`).
    pub controller: Option<u8>,
    /// Nominal (arbitration-phase) bit rate in bit/s.
    pub baudrate: Option<u32>,
    /// CAN-FD data-phase bit rate in bit/s; `Some` selects CAN-FD mode.
    pub data_baudrate: Option<u32>,
}

/// Errors returned by CAN device drivers.
#[derive(Debug, thiserror::Error)]
pub enum CanDeviceError {
    #[error("CAN driver not found: {0}")]
    DriverNotFound(String),

    #[error("no CAN channel available: {0}")]
    NoChannel(String),

    #[error("unknown CAN channel: {0}")]
    UnknownChannel(String),

    #[error("unsupported operation: {0}")]
    Unsupported(String),

    #[error("failed to open CAN channel: {0}")]
    OpenFailed(String),

    #[error("failed to send CAN frame: {0}")]
    SendFailed(String),

    #[error("failed to receive CAN frame: {0}")]
    RecvFailed(String),

    #[error("CAN driver error: {0}")]
    Driver(String),
}

/// A low-level CAN controller handle.
pub trait CanDevice: Send + Sync {
    /// Transmit one frame.
    fn send_frame(&self, frame: &CanFrame) -> Result<(), CanDeviceError>;

    /// Receive one frame.
    ///
    /// `timeout = None` performs a non-blocking poll and returns `Ok(None)` when
    /// no frame is queued. `Some(t)` waits up to `t` for a frame.
    fn recv_frame(&self, timeout: Option<Duration>) -> Result<Option<CanFrame>, CanDeviceError>;

    /// Release the channel.
    fn close(&mut self) -> Result<(), CanDeviceError>;

    /// Channel information.
    fn info(&self) -> CanDeviceInfo;

    /// Whether the channel is operating in CAN-FD mode.
    fn is_fd(&self) -> bool;

    /// Human-readable bit rate description (for logs/UI).
    fn bitrate_description(&self) -> String {
        "unknown".to_string()
    }
}

/// A CAN driver: enumerates channels and opens them.
pub trait CanDriver: Send + Sync {
    /// Short driver id, e.g. `"pcan"`.
    fn name(&self) -> &'static str;

    /// Human-readable description.
    fn description(&self) -> &'static str;

    /// Enumerate the channels known to this driver.
    fn list_channels(&self) -> Vec<CanDeviceInfo>;

    /// Open a channel.
    fn open(&self, config: &CanDeviceConfig) -> Result<Box<dyn CanDevice>, CanDeviceError>;

    /// Pick a default channel when the requested channel is empty/`"auto"`.
    fn default_channel(&self) -> Option<CanDeviceInfo> {
        self.list_channels().into_iter().find(|c| c.available)
    }
}

/// All compiled-in CAN drivers, in priority order.
pub fn drivers() -> Vec<Box<dyn CanDriver>> {
    vec![Box::new(super::pcan::PcanDriver::new())]
}

/// Names of all compiled-in CAN drivers.
pub fn driver_names() -> Vec<&'static str> {
    drivers().iter().map(|d| d.name()).collect()
}

/// Find a driver by name (case-insensitive).
pub fn find_driver(name: &str) -> Option<Box<dyn CanDriver>> {
    drivers()
        .into_iter()
        .find(|d| d.name().eq_ignore_ascii_case(name))
}

/// Enumerate every channel across every compiled-in driver.
pub fn list_can_devices() -> Vec<CanDeviceInfo> {
    let mut devices = Vec::new();
    for driver in drivers() {
        devices.extend(driver.list_channels());
    }
    devices
}

/// Decode a CAN-FD DLC code into the number of data bytes.
pub fn dlc_to_len(dlc: u8) -> usize {
    match dlc {
        0..=8 => dlc as usize,
        9 => 12,
        10 => 16,
        11 => 20,
        12 => 24,
        13 => 32,
        14 => 48,
        _ => 64,
    }
}

/// Encode a data byte count into the smallest CAN-FD DLC code that fits.
pub fn len_to_dlc(len: usize) -> u8 {
    match len {
        0..=8 => len as u8,
        9..=12 => 9,
        13..=16 => 10,
        17..=20 => 11,
        21..=24 => 12,
        25..=32 => 13,
        33..=48 => 14,
        _ => 15,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dlc_len_roundtrip_classic() {
        for len in 0..=8usize {
            assert_eq!(dlc_to_len(len_to_dlc(len)), len);
        }
    }

    #[test]
    fn test_dlc_to_len_fd_sizes() {
        assert_eq!(dlc_to_len(9), 12);
        assert_eq!(dlc_to_len(10), 16);
        assert_eq!(dlc_to_len(11), 20);
        assert_eq!(dlc_to_len(12), 24);
        assert_eq!(dlc_to_len(13), 32);
        assert_eq!(dlc_to_len(14), 48);
        assert_eq!(dlc_to_len(15), 64);
    }

    #[test]
    fn test_len_to_dlc_buckets() {
        assert_eq!(len_to_dlc(0), 0);
        assert_eq!(len_to_dlc(8), 8);
        assert_eq!(len_to_dlc(9), 9);
        assert_eq!(len_to_dlc(12), 9);
        assert_eq!(len_to_dlc(13), 10);
        assert_eq!(len_to_dlc(33), 14);
        assert_eq!(len_to_dlc(64), 15);
        assert_eq!(len_to_dlc(100), 15);
    }

    #[test]
    fn test_can_frame_derives_extended() {
        assert!(!CanFrame::new(0x123, vec![1, 2]).extended);
        assert!(CanFrame::new(0x1FFFFFFF, vec![1, 2]).extended);
    }

    #[test]
    fn test_selection_value_and_label() {
        let info = CanDeviceInfo {
            driver: "pcan".to_string(),
            device_id: "3".to_string(),
            name: "PCAN-USB".to_string(),
            device_type: "PCAN-USB".to_string(),
            controller: 1,
            channel: "PCAN_USBBUS2".to_string(),
            available: true,
            fd_capable: false,
        };
        assert_eq!(info.selection_value(), "pcan:3:1");
        assert!(info.display_label().contains("PCAN_USBBUS2"));
        assert!(info.display_label().contains("device 3"));
    }
}
