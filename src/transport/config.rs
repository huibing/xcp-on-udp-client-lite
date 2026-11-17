//! Transport configuration types.

use serde::{Deserialize, Serialize};

/// Transport type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TransportType {
    Udp,
    Tcp,
    Can,
}

impl TransportType {
    pub fn as_str(&self) -> &'static str {
        match self {
            TransportType::Udp => "UDP",
            TransportType::Tcp => "TCP",
            TransportType::Can => "CAN",
        }
    }
}

/// Transport configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransportConfig {
    Udp {
        addr: String,
    },
    Tcp {
        addr: String,
    },
    Can {
        /// CAN driver id (e.g. `"pcan"`); `None` uses the default driver.
        #[serde(default)]
        driver: Option<String>,
        channel: String,
        /// Stable device id (takes precedence over `channel` when set).
        #[serde(default)]
        device_id: Option<String>,
        /// Controller index within the device (used with `device_id`).
        #[serde(default)]
        controller: Option<u8>,
        can_id: u32,
        /// Nominal (arbitration-phase) bit rate in bit/s, e.g. `500_000`.
        #[serde(default)]
        baudrate: Option<u32>,
        /// CAN-FD data-phase bit rate in bit/s. `Some` selects CAN-FD mode.
        #[serde(default)]
        data_baudrate: Option<u32>,
        /// Slave response CAN ID (`CAN_ID_SLAVE`).
        #[serde(default)]
        slave_id: Option<u32>,
        /// DAQ DTO CAN IDs (`DAQ_LIST_CAN_ID`).
        #[serde(default)]
        daq_ids: Vec<u32>,
        /// Pad command frames to the maximum DLC (`MAX_DLC_REQUIRED`).
        #[serde(default)]
        max_dlc_required: bool,
    },
}

impl TransportConfig {
    pub fn transport_type(&self) -> TransportType {
        match self {
            TransportConfig::Udp { .. } => TransportType::Udp,
            TransportConfig::Tcp { .. } => TransportType::Tcp,
            TransportConfig::Can { .. } => TransportType::Can,
        }
    }

    /// Get display address string
    pub fn addr_string(&self) -> String {
        match self {
            TransportConfig::Udp { addr } => addr.clone(),
            TransportConfig::Tcp { addr } => addr.clone(),
            TransportConfig::Can {
                channel, can_id, ..
            } => {
                format!("{}:{:03X}", channel, can_id)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_transport_type_as_str() {
        assert_eq!(TransportType::Udp.as_str(), "UDP");
        assert_eq!(TransportType::Tcp.as_str(), "TCP");
        assert_eq!(TransportType::Can.as_str(), "CAN");
    }

    #[test]
    fn test_transport_config_udp() {
        let config = TransportConfig::Udp {
            addr: "127.0.0.1:5555".to_string(),
        };
        assert_eq!(config.transport_type(), TransportType::Udp);
        assert_eq!(config.addr_string(), "127.0.0.1:5555");
    }

    #[test]
    fn test_transport_config_can() {
        let config = TransportConfig::Can {
            driver: Some("pcan".to_string()),
            channel: "can0".to_string(),
            device_id: None,
            controller: None,
            can_id: 0x123,
            baudrate: Some(500_000),
            data_baudrate: None,
            slave_id: Some(0x456),
            daq_ids: vec![0x310],
            max_dlc_required: false,
        };
        assert_eq!(config.transport_type(), TransportType::Can);
        assert_eq!(config.addr_string(), "can0:123");
    }

    #[test]
    fn test_transport_config_tcp() {
        let config = TransportConfig::Tcp {
            addr: "127.0.0.1:5555".to_string(),
        };
        assert_eq!(config.transport_type(), TransportType::Tcp);
        assert_eq!(config.addr_string(), "127.0.0.1:5555");
    }

    #[test]
    fn test_transport_type_equality() {
        assert_eq!(TransportType::Udp, TransportType::Udp);
        assert_eq!(TransportType::Tcp, TransportType::Tcp);
        assert_eq!(TransportType::Can, TransportType::Can);
        assert_ne!(TransportType::Udp, TransportType::Tcp);
        assert_ne!(TransportType::Udp, TransportType::Can);
        assert_ne!(TransportType::Tcp, TransportType::Can);
    }

    #[test]
    fn test_transport_config_clone() {
        let config = TransportConfig::Udp {
            addr: "192.168.1.1:5555".to_string(),
        };
        let cloned = config.clone();
        assert_eq!(config.addr_string(), cloned.addr_string());
    }

    #[test]
    fn test_transport_type_hash() {
        let mut set = HashSet::new();
        set.insert(TransportType::Udp);
        set.insert(TransportType::Tcp);
        set.insert(TransportType::Can);
        assert_eq!(set.len(), 3);
    }
}