//! Transport selection from caller settings + A2L transport blocks.
//!
//! Both the `daq-record` config file and the `calibrate` / `verify` CLI flags
//! describe a connection as a set of *optional* overrides: whatever is not
//! declared falls back to the matching `IF_DATA` transport block in the A2L
//! file (`XCP_ON_UDP_IP`, `XCP_ON_TCP_IP`, `XCP_ON_CAN`). This module turns
//! such overrides into a concrete [`TransportConfig`].
//!
//! Resolution rules:
//! - An explicitly declared transport type is used, and undeclared fields fall
//!   back to the matching A2L block.
//! - Without a declared type, an explicit `addr` selects UDP (backward
//!   compatible with the original CLI), and CAN-only fields produce a hint to
//!   declare the transport type.
//! - With no overrides at all, the transport is auto-selected from the A2L file
//!   with UDP > CAN > TCP priority.

use super::transport::{TransportConfig, TransportType};
use crate::a2l::A2lTransportConfig;

/// Transport settings supplied by the caller (CLI flags or a config file).
///
/// Every field is optional: `None` means "not declared" and falls back to the
/// matching A2L transport block.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TransportOverrides {
    /// Explicitly requested transport. `None` means "infer".
    pub transport_type: Option<TransportType>,
    /// Slave address (`IP:port`) for UDP/TCP transports.
    pub addr: Option<String>,
    /// CAN driver id (e.g. `"pcan"`); `None` uses the default driver.
    pub driver: Option<String>,
    /// CAN driver channel (e.g. `"PCAN_USBBUS1"`); `None` uses `"auto"`.
    pub channel: Option<String>,
    /// Stable CAN device id (takes precedence over `channel` when set).
    pub device_id: Option<String>,
    /// Controller index within the CAN device (used with `device_id`).
    pub controller: Option<u8>,
    /// Command CAN ID (`CAN_ID_MASTER`).
    pub can_id: Option<u32>,
    /// Nominal (arbitration-phase) bit rate in bit/s.
    pub baudrate: Option<u32>,
    /// CAN-FD data-phase bit rate in bit/s; presence selects CAN-FD mode.
    pub data_baudrate: Option<u32>,
    /// Slave response CAN ID (`CAN_ID_SLAVE`).
    pub slave_id: Option<u32>,
    /// DAQ DTO CAN IDs (`DAQ_LIST_CAN_ID`).
    pub daq_ids: Option<Vec<u32>>,
    /// Pad command frames to the maximum DLC (`MAX_DLC_REQUIRED`).
    pub max_dlc_required: Option<bool>,
}

impl TransportOverrides {
    /// True when any CAN-specific field is declared.
    pub fn has_can_fields(&self) -> bool {
        self.driver.is_some()
            || self.channel.is_some()
            || self.device_id.is_some()
            || self.controller.is_some()
            || self.can_id.is_some()
            || self.baudrate.is_some()
            || self.data_baudrate.is_some()
            || self.slave_id.is_some()
            || self.daq_ids.is_some()
            || self.max_dlc_required.is_some()
    }

    /// True when no setting at all is declared.
    pub fn is_empty(&self) -> bool {
        self.transport_type.is_none() && self.addr.is_none() && !self.has_can_fields()
    }
}

/// Parse a transport type name (`"udp"`, `"tcp"`, `"can"`, case-insensitive).
/// `None` means "not declared".
pub fn parse_transport_type(value: Option<&str>) -> Result<Option<TransportType>, String> {
    match value.map(str::to_ascii_lowercase).as_deref() {
        None => Ok(None),
        Some("udp") => Ok(Some(TransportType::Udp)),
        Some("tcp") => Ok(Some(TransportType::Tcp)),
        Some("can") => Ok(Some(TransportType::Can)),
        Some(other) => Err(format!(
            "Unknown transport type '{}': expected \"udp\", \"tcp\" or \"can\"",
            other
        )),
    }
}

/// Resolve the XCP transport configuration from caller overrides and the A2L
/// transport blocks discovered in the description file.
pub fn resolve_transport_config(
    overrides: Option<&TransportOverrides>,
    a2l_configs: &[A2lTransportConfig],
) -> Result<TransportConfig, String> {
    let Some(ovr) = overrides else {
        return auto_select_transport(a2l_configs);
    };

    match ovr.transport_type {
        Some(TransportType::Udp) => {
            let addr = ovr
                .addr
                .clone()
                .or_else(|| find_ip_addr(a2l_configs, TransportType::Udp))
                .ok_or(
                    "UDP transport requires an address (--addr or [connection].addr) \
                     or an XCP_ON_UDP_IP block in the A2L file"
                        .to_string(),
                )?;
            Ok(TransportConfig::Udp { addr })
        }
        Some(TransportType::Tcp) => {
            let addr = ovr
                .addr
                .clone()
                .or_else(|| find_ip_addr(a2l_configs, TransportType::Tcp))
                .ok_or(
                    "TCP transport requires an address (--addr or [connection].addr) \
                     or an XCP_ON_TCP_IP block in the A2L file"
                        .to_string(),
                )?;
            Ok(TransportConfig::Tcp { addr })
        }
        Some(TransportType::Can) => build_can_transport_config(ovr, a2l_configs),
        None => {
            // No type declared: an explicit addr selects UDP (backward compatible).
            if let Some(addr) = ovr.addr.clone() {
                return Ok(TransportConfig::Udp { addr });
            }
            if ovr.has_can_fields() {
                return Err(
                    "CAN settings were given but no transport type — declare `can` \
                     (CLI: --transport can, config: type = \"can\")"
                        .to_string(),
                );
            }
            auto_select_transport(a2l_configs)
        }
    }
}

/// Auto-select a transport from the A2L transport blocks (UDP > CAN > TCP).
fn auto_select_transport(a2l_configs: &[A2lTransportConfig]) -> Result<TransportConfig, String> {
    for ty in [TransportType::Udp, TransportType::Can, TransportType::Tcp] {
        if let Some(config) = a2l_configs.iter().find(|c| c.transport_type() == ty) {
            return Ok(config.to_transport_config());
        }
    }
    Err(
        "No transport found in the connection settings or the A2L file: \
         pass a transport type/address (CLI: --transport, --addr or CAN settings) \
         or add an XCP_ON_UDP_IP / XCP_ON_CAN / XCP_ON_TCP_IP block to the A2L"
            .to_string(),
    )
}

/// Find an IP address for the given transport type in the A2L transport blocks.
fn find_ip_addr(a2l_configs: &[A2lTransportConfig], ty: TransportType) -> Option<String> {
    a2l_configs.iter().find_map(|config| match config {
        A2lTransportConfig::Udp { addr } if ty == TransportType::Udp => Some(addr.clone()),
        A2lTransportConfig::Tcp { addr } if ty == TransportType::Tcp => Some(addr.clone()),
        _ => None,
    })
}

/// Build a CAN transport config, merging the overrides over the A2L
/// `XCP_ON_CAN` block (override values take precedence).
fn build_can_transport_config(
    ovr: &TransportOverrides,
    a2l_configs: &[A2lTransportConfig],
) -> Result<TransportConfig, String> {
    // Fields from the A2L XCP_ON_CAN block, used as fallbacks.
    let (a2l_can_id, a2l_baudrate, a2l_data_baudrate, a2l_slave_id, a2l_daq_ids, a2l_max_dlc) =
        match a2l_configs
            .iter()
            .find(|config| matches!(config, A2lTransportConfig::Can { .. }))
        {
            Some(A2lTransportConfig::Can {
                can_id,
                baudrate,
                data_baudrate,
                slave_id,
                daq_ids,
                max_dlc_required,
            }) => (
                Some(*can_id),
                *baudrate,
                *data_baudrate,
                *slave_id,
                daq_ids.clone(),
                *max_dlc_required,
            ),
            _ => (None, None, None, None, Vec::new(), false),
        };

    let can_id = ovr.can_id.or(a2l_can_id).ok_or(
        "CAN transport requires a command CAN ID (CAN_ID_MASTER) or an XCP_ON_CAN \
         block in the A2L file",
    )?;

    Ok(TransportConfig::Can {
        driver: ovr.driver.clone(),
        channel: ovr.channel.clone().unwrap_or_else(|| "auto".to_string()),
        device_id: ovr.device_id.clone(),
        controller: ovr.controller,
        can_id,
        baudrate: ovr.baudrate.or(a2l_baudrate),
        data_baudrate: ovr.data_baudrate.or(a2l_data_baudrate),
        slave_id: ovr.slave_id.or(a2l_slave_id),
        daq_ids: ovr.daq_ids.clone().unwrap_or(a2l_daq_ids),
        max_dlc_required: ovr.max_dlc_required.unwrap_or(a2l_max_dlc),
    })
}

/// Human-readable one-line description of a transport configuration.
pub fn transport_display(config: &TransportConfig) -> String {
    match config {
        TransportConfig::Udp { addr } => format!("UDP {}", addr),
        TransportConfig::Tcp { addr } => format!("TCP {}", addr),
        TransportConfig::Can {
            channel,
            can_id,
            baudrate,
            data_baudrate,
            slave_id,
            ..
        } => {
            let mode = if data_baudrate.is_some() {
                "CAN-FD"
            } else {
                "CAN"
            };
            let rate = match (baudrate, data_baudrate) {
                (Some(nom), Some(data)) => format!(" {}/{} bit/s", nom, data),
                (Some(nom), None) => format!(" {} bit/s", nom),
                _ => String::new(),
            };
            let resp = match slave_id {
                Some(id) => format!(" (response 0x{:X})", id),
                None => String::new(),
            };
            format!(
                "{} channel {} ID 0x{:X}{}{}",
                mode, channel, can_id, resp, rate
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a2l_can() -> A2lTransportConfig {
        A2lTransportConfig::Can {
            can_id: 0x7fb,
            baudrate: Some(500_000),
            data_baudrate: None,
            slave_id: Some(0x7fc),
            daq_ids: vec![0x7fd],
            max_dlc_required: true,
        }
    }

    fn a2l_udp() -> A2lTransportConfig {
        A2lTransportConfig::Udp {
            addr: "192.168.0.10:5555".into(),
        }
    }

    #[test]
    fn test_overrides_empty() {
        let ovr = TransportOverrides::default();
        assert!(ovr.is_empty());
        assert!(!ovr.has_can_fields());
    }

    #[test]
    fn test_can_only_a2l_auto_selects_can() {
        // The reported problem: an ECU that only speaks XCP-on-CAN must work
        // without any transport flags.
        let resolved = resolve_transport_config(None, &[a2l_can()]).unwrap();
        assert_eq!(
            resolved,
            TransportConfig::Can {
                driver: None,
                channel: "auto".into(),
                device_id: None,
                controller: None,
                can_id: 0x7fb,
                baudrate: Some(500_000),
                data_baudrate: None,
                slave_id: Some(0x7fc),
                daq_ids: vec![0x7fd],
                max_dlc_required: true,
            }
        );
    }

    #[test]
    fn test_explicit_can_merges_a2l_defaults() {
        let ovr = TransportOverrides {
            transport_type: Some(TransportType::Can),
            channel: Some("PCAN_USBBUS1".into()),
            can_id: Some(0x550),
            ..Default::default()
        };
        let resolved = resolve_transport_config(Some(&ovr), &[a2l_can()]).unwrap();
        assert_eq!(
            resolved,
            TransportConfig::Can {
                driver: None,
                channel: "PCAN_USBBUS1".into(),
                device_id: None,
                controller: None,
                can_id: 0x550,           // override wins
                baudrate: Some(500_000), // from A2L
                data_baudrate: None,
                slave_id: Some(0x7fc),  // from A2L
                daq_ids: vec![0x7fd],   // from A2L
                max_dlc_required: true, // from A2L
            }
        );
    }

    #[test]
    fn test_can_overrides_replace_a2l_rates_and_ids() {
        let ovr = TransportOverrides {
            transport_type: Some(TransportType::Can),
            can_id: Some(0x123),
            baudrate: Some(1_000_000),
            data_baudrate: Some(4_000_000),
            slave_id: Some(0x124),
            daq_ids: Some(vec![0x310, 0x320]),
            max_dlc_required: Some(false),
            ..Default::default()
        };
        let resolved = resolve_transport_config(Some(&ovr), &[a2l_can()]).unwrap();
        assert_eq!(
            resolved,
            TransportConfig::Can {
                driver: None,
                channel: "auto".into(),
                device_id: None,
                controller: None,
                can_id: 0x123,
                baudrate: Some(1_000_000),
                data_baudrate: Some(4_000_000),
                slave_id: Some(0x124),
                daq_ids: vec![0x310, 0x320],
                max_dlc_required: false,
            }
        );
    }

    #[test]
    fn test_explicit_can_without_any_can_id_fails() {
        let ovr = TransportOverrides {
            transport_type: Some(TransportType::Can),
            ..Default::default()
        };
        let err = resolve_transport_config(Some(&ovr), &[]).unwrap_err();
        assert!(err.contains("CAN_ID_MASTER"), "unexpected error: {}", err);
    }

    #[test]
    fn test_addr_only_still_selects_udp() {
        // Backward compatibility for `calibrate --addr 1.2.3.4:5555`.
        let ovr = TransportOverrides {
            addr: Some("1.2.3.4:5555".into()),
            ..Default::default()
        };
        let resolved = resolve_transport_config(Some(&ovr), &[a2l_can()]).unwrap();
        assert_eq!(
            resolved,
            TransportConfig::Udp {
                addr: "1.2.3.4:5555".into()
            }
        );
    }

    #[test]
    fn test_can_fields_without_type_hint() {
        let ovr = TransportOverrides {
            can_id: Some(0x123),
            ..Default::default()
        };
        let err = resolve_transport_config(Some(&ovr), &[]).unwrap_err();
        assert!(err.contains("transport"), "unexpected error: {}", err);
    }

    #[test]
    fn test_auto_select_prefers_udp_over_can() {
        let resolved = resolve_transport_config(None, &[a2l_can(), a2l_udp()]).unwrap();
        assert_eq!(
            resolved,
            TransportConfig::Udp {
                addr: "192.168.0.10:5555".into()
            }
        );
    }

    #[test]
    fn test_no_transport_available() {
        let err = resolve_transport_config(None, &[]).unwrap_err();
        assert!(err.contains("No transport"), "unexpected error: {}", err);
    }

    #[test]
    fn test_udp_without_addr_or_a2l_block() {
        let ovr = TransportOverrides {
            transport_type: Some(TransportType::Udp),
            ..Default::default()
        };
        let err = resolve_transport_config(Some(&ovr), &[a2l_can()]).unwrap_err();
        assert!(err.contains("UDP"), "unexpected error: {}", err);
    }

    #[test]
    fn test_parse_transport_type() {
        assert_eq!(parse_transport_type(None).unwrap(), None);
        assert_eq!(
            parse_transport_type(Some("udp")).unwrap(),
            Some(TransportType::Udp)
        );
        assert_eq!(
            parse_transport_type(Some("TCP")).unwrap(),
            Some(TransportType::Tcp)
        );
        assert_eq!(
            parse_transport_type(Some("Can")).unwrap(),
            Some(TransportType::Can)
        );
        assert!(parse_transport_type(Some("xyz")).is_err());
    }

    #[test]
    fn test_transport_display() {
        assert_eq!(
            transport_display(&TransportConfig::Udp {
                addr: "127.0.0.1:5555".into()
            }),
            "UDP 127.0.0.1:5555"
        );

        let can = TransportConfig::Can {
            driver: None,
            channel: "auto".into(),
            device_id: None,
            controller: None,
            can_id: 0x7fb,
            baudrate: Some(500_000),
            data_baudrate: None,
            slave_id: Some(0x7fc),
            daq_ids: vec![],
            max_dlc_required: false,
        };
        let label = transport_display(&can);
        assert!(label.contains("CAN"), "label: {}", label);
        assert!(label.contains("0x7FB"), "label: {}", label);
        assert!(label.contains("0x7FC"), "label: {}", label);

        let canfd = TransportConfig::Can {
            driver: None,
            channel: "auto".into(),
            device_id: None,
            controller: None,
            can_id: 0x7fb,
            baudrate: Some(500_000),
            data_baudrate: Some(2_000_000),
            slave_id: Some(0x7fc),
            daq_ids: vec![],
            max_dlc_required: false,
        };
        assert!(transport_display(&canfd).contains("CAN-FD"));
    }
}
