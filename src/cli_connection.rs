//! Shared CLI connection arguments for the non-interactive subcommands.
//!
//! `calibrate` and `verify` used to hard-code UDP, which made them unusable
//! against ECUs that only speak XCP-on-CAN. [`ConnectionArgs`] adds optional
//! transport flags that are merged with the A2L `IF_DATA` transport blocks by
//! [`resolve_transport_config`](xcp_client::transport::resolve_transport_config).

use clap::Args;
use xcp_client::a2l::A2lTransportConfig;
use xcp_client::transport::{
    parse_transport_type, resolve_transport_config, TransportConfig, TransportOverrides,
    TransportType,
};

/// Connection settings shared by the CLI subcommands.
///
/// Every flag is optional. Without any flag the transport is auto-selected from
/// the A2L file (UDP > CAN > TCP), so an ECU described with only an
/// `XCP_ON_CAN` block is reachable out of the box.
#[derive(Args, Debug, Clone, Default)]
pub struct ConnectionArgs {
    /// Transport to use: "udp", "tcp" or "can" (default: auto-detect from A2L)
    #[arg(long, value_name = "TYPE")]
    pub transport: Option<String>,

    /// Slave address for UDP/TCP, e.g. 192.168.1.10:5555 (default: from A2L)
    #[arg(long, value_name = "ADDR")]
    pub addr: Option<String>,

    /// CAN driver id, e.g. "pcan" (default: "pcan")
    #[arg(long, value_name = "ID")]
    pub can_driver: Option<String>,

    /// CAN channel, e.g. "PCAN_USBBUS1" (default: "auto" = first free channel)
    #[arg(long, value_name = "CHANNEL")]
    pub can_channel: Option<String>,

    /// Stable CAN device id (takes precedence over --can-channel)
    #[arg(long, value_name = "ID")]
    pub can_device: Option<String>,

    /// Controller index within the CAN device (used with --can-device)
    #[arg(long, value_name = "N")]
    pub can_controller: Option<u8>,

    /// Command CAN ID / CAN_ID_MASTER, e.g. 0x550 (default: from A2L)
    #[arg(long, value_name = "ID", value_parser = parse_number)]
    pub can_id: Option<u32>,

    /// Nominal CAN bit rate in bit/s, e.g. 500000 (default: from A2L BAUDRATE)
    #[arg(long, value_name = "BPS", value_parser = parse_number)]
    pub baudrate: Option<u32>,

    /// CAN-FD data phase bit rate in bit/s; presence enables CAN-FD
    #[arg(long, value_name = "BPS", value_parser = parse_number)]
    pub data_baudrate: Option<u32>,

    /// Slave response CAN ID / CAN_ID_SLAVE (default: from A2L)
    #[arg(long, value_name = "ID", value_parser = parse_number)]
    pub slave_id: Option<u32>,

    /// DAQ DTO CAN IDs, comma separated, e.g. 0x7fd,0x7fe (default: from A2L)
    #[arg(long, value_name = "IDS", value_delimiter = ',', value_parser = parse_number)]
    pub daq_ids: Option<Vec<u32>>,

    /// Pad command frames to the maximum DLC (MAX_DLC_REQUIRED)
    #[arg(long)]
    pub max_dlc_required: bool,
}

impl ConnectionArgs {
    /// Build the transport overrides described by the flags.
    ///
    /// When no transport is named explicitly, it is inferred from the flags so
    /// that e.g. `--can-id 0x550` works without `--transport can`:
    /// an address selects UDP (backward compatible with the old `--addr`),
    /// CAN settings select CAN.
    pub fn overrides(&self) -> Result<TransportOverrides, String> {
        let mut transport_type = parse_transport_type(self.transport.as_deref())?;

        let overrides = TransportOverrides {
            transport_type,
            addr: self.addr.clone(),
            driver: self.can_driver.clone(),
            channel: self.can_channel.clone(),
            device_id: self.can_device.clone(),
            controller: self.can_controller,
            can_id: self.can_id,
            baudrate: self.baudrate,
            data_baudrate: self.data_baudrate,
            slave_id: self.slave_id,
            daq_ids: self.daq_ids.clone(),
            max_dlc_required: self.max_dlc_required.then_some(true),
        };

        if transport_type.is_none() {
            transport_type = if overrides.addr.is_some() {
                Some(TransportType::Udp)
            } else if overrides.has_can_fields() {
                Some(TransportType::Can)
            } else {
                None
            };
        }

        Ok(TransportOverrides {
            transport_type,
            ..overrides
        })
    }

    /// Resolve the transport configuration against the A2L transport blocks.
    pub fn resolve(&self, a2l_configs: &[A2lTransportConfig]) -> Result<TransportConfig, String> {
        let overrides = self.overrides()?;
        resolve_transport_config(Some(&overrides), a2l_configs)
    }
}

/// Parse a decimal or `0x`-prefixed hexadecimal number (CAN IDs, bit rates).
fn parse_number(value: &str) -> Result<u32, String> {
    let text = value.trim().replace('_', "");
    let (radix, digits) = match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => (16, hex.to_string()),
        None => (10, text),
    };
    u32::from_str_radix(&digits, radix).map_err(|e| format!("invalid number '{}': {}", value, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a2l_can() -> A2lTransportConfig {
        A2lTransportConfig::Can {
            can_id: 0x550,
            baudrate: Some(500_000),
            data_baudrate: None,
            slave_id: Some(0x551),
            daq_ids: Vec::new(),
            max_dlc_required: false,
        }
    }

    #[test]
    fn test_parse_number() {
        assert_eq!(parse_number("500000").unwrap(), 500_000);
        assert_eq!(parse_number("0x550").unwrap(), 0x550);
        assert_eq!(parse_number("0X7FB").unwrap(), 0x7fb);
        assert_eq!(parse_number("0x7_fb").unwrap(), 0x7fb);
        assert!(parse_number("nope").is_err());
    }

    #[test]
    fn test_no_flags_auto_selects_can_from_a2l() {
        // The reported use case: only XCP-on-CAN in the A2L, no flags at all.
        let args = ConnectionArgs::default();
        let resolved = args.resolve(&[a2l_can()]).unwrap();
        assert_eq!(
            resolved,
            TransportConfig::Can {
                driver: None,
                channel: "auto".into(),
                device_id: None,
                controller: None,
                can_id: 0x550,
                baudrate: Some(500_000),
                data_baudrate: None,
                slave_id: Some(0x551),
                daq_ids: vec![],
                max_dlc_required: false,
            }
        );
    }

    #[test]
    fn test_can_id_flag_implies_can() {
        let args = ConnectionArgs {
            can_id: Some(0x123),
            can_channel: Some("PCAN_USBBUS1".into()),
            ..Default::default()
        };
        let resolved = args.resolve(&[a2l_can()]).unwrap();
        match resolved {
            TransportConfig::Can {
                channel,
                can_id,
                baudrate,
                slave_id,
                ..
            } => {
                assert_eq!(channel, "PCAN_USBBUS1");
                assert_eq!(can_id, 0x123);
                assert_eq!(baudrate, Some(500_000)); // A2L fallback
                assert_eq!(slave_id, Some(0x551)); // A2L fallback
            }
            other => panic!("expected CAN, got {:?}", other),
        }
    }

    #[test]
    fn test_addr_flag_implies_udp() {
        let args = ConnectionArgs {
            addr: Some("10.0.0.5:5555".into()),
            ..Default::default()
        };
        let resolved = args.resolve(&[a2l_can()]).unwrap();
        assert_eq!(
            resolved,
            TransportConfig::Udp {
                addr: "10.0.0.5:5555".into()
            }
        );
    }

    #[test]
    fn test_explicit_transport_flag_wins() {
        let args = ConnectionArgs {
            transport: Some("can".into()),
            addr: Some("10.0.0.5:5555".into()),
            ..Default::default()
        };
        let resolved = args.resolve(&[a2l_can()]).unwrap();
        assert!(matches!(resolved, TransportConfig::Can { .. }));
    }

    #[test]
    fn test_unknown_transport_flag() {
        let args = ConnectionArgs {
            transport: Some("serial".into()),
            ..Default::default()
        };
        let err = args.resolve(&[]).unwrap_err();
        assert!(err.contains("serial"), "unexpected error: {}", err);
    }
}
