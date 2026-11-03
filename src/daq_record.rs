//! Non-interactive DAQ recording CLI subcommand.
//!
//! Records XCP DAQ measurements to MF4 and/or CSV files.
//! Runs until CTRL+C or the user enters "q" + Enter on stdin,
//! then gracefully stops DAQ and saves output files.
//!
//! Usage: `cargo run -- daq-record --config daq_record.config.toml`

use serde::Deserialize;
use tokio::io::AsyncBufReadExt;
use xcp_client::a2l::A2lTransportConfig;
use xcp_client::transport::{
    parse_transport_type, resolve_transport_config as resolve_transport, transport_display,
    TransportConfig, TransportOverrides,
};
use xcp_client::{A2lReader, DaqManager, XcpClient};

/// TOML config file structure
#[derive(Debug, Deserialize)]
pub struct DaqRecordConfig {
    /// Path to the A2L description file (required)
    pub a2l: String,
    pub connection: Option<ConnectionConfig>,
    pub output: Option<OutputConfig>,
    pub daq: Option<DaqConfig>,
    /// Event-based measurement grouping
    pub events: Option<Vec<EventConfig>>,
}

#[derive(Debug, Deserialize)]
pub struct ConnectionConfig {
    /// Transport type: `"udp"` (default), `"tcp"` or `"can"`.
    /// When omitted, an explicit `addr` selects UDP; otherwise the transport
    /// is auto-selected from the A2L file (priority UDP > CAN > TCP).
    #[serde(rename = "type", alias = "transport")]
    pub transport_type: Option<String>,
    /// Slave address (`IP:port`) for UDP/TCP transports.
    /// Falls back to the matching address from the A2L file.
    pub addr: Option<String>,
    /// CAN driver id (e.g. `"pcan"`); `None` uses the default driver.
    #[serde(default)]
    pub driver: Option<String>,
    /// CAN driver channel (e.g. `"PCAN_USBBUS1"`); defaults to `"auto"`.
    #[serde(default)]
    pub channel: Option<String>,
    /// Stable CAN device id (takes precedence over `channel` when set).
    #[serde(default)]
    pub device_id: Option<String>,
    /// Controller index within the CAN device (used with `device_id`).
    #[serde(default)]
    pub controller: Option<u8>,
    /// Command CAN ID (`CAN_ID_MASTER`). Falls back to the A2L `XCP_ON_CAN` block.
    #[serde(default)]
    pub can_id: Option<u32>,
    /// Nominal (arbitration-phase) bit rate in bit/s.
    #[serde(default)]
    pub baudrate: Option<u32>,
    /// CAN-FD data-phase bit rate in bit/s. Presence selects CAN-FD mode.
    #[serde(default)]
    pub data_baudrate: Option<u32>,
    /// Slave response CAN ID (`CAN_ID_SLAVE`).
    #[serde(default)]
    pub slave_id: Option<u32>,
    /// DAQ DTO CAN IDs (`DAQ_LIST_CAN_ID`).
    #[serde(default)]
    pub daq_ids: Option<Vec<u32>>,
    /// Pad command frames to the maximum DLC (`MAX_DLC_REQUIRED`).
    #[serde(default)]
    pub max_dlc_required: Option<bool>,
}

impl ConnectionConfig {
    /// Convert the TOML `[connection]` section into transport overrides.
    fn overrides(&self) -> Result<TransportOverrides, String> {
        Ok(TransportOverrides {
            transport_type: parse_transport_type(self.transport_type.as_deref())?,
            addr: self.addr.clone(),
            driver: self.driver.clone(),
            channel: self.channel.clone(),
            device_id: self.device_id.clone(),
            controller: self.controller,
            can_id: self.can_id,
            baudrate: self.baudrate,
            data_baudrate: self.data_baudrate,
            slave_id: self.slave_id,
            daq_ids: self.daq_ids.clone(),
            max_dlc_required: self.max_dlc_required,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct OutputConfig {
    /// Path to MF4 output file (optional)
    pub mf4: Option<String>,
    /// Path to CSV output file (optional)
    pub csv: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DaqConfig {
    /// Default event channel for measurements without explicit assignment
    pub event_channel: Option<u16>,
}

#[derive(Debug, Deserialize)]
pub struct EventConfig {
    /// Event channel number
    pub channel: u16,
    /// List of measurement names assigned to this event
    pub measurements: Vec<String>,
}

/// Resolve the XCP transport configuration from the config file and the A2L file.
///
/// Thin wrapper around the shared
/// [`resolve_transport_config`](xcp_client::transport::resolve_transport_config)
/// helper: the `[connection]` section becomes transport overrides, and anything
/// not declared falls back to the A2L transport blocks (with UDP > CAN > TCP
/// priority when no type is declared).
fn resolve_transport_config(
    connection: Option<&ConnectionConfig>,
    a2l_configs: &[A2lTransportConfig],
) -> Result<TransportConfig, String> {
    let overrides = connection.map(ConnectionConfig::overrides).transpose()?;
    resolve_transport(overrides.as_ref(), a2l_configs)
}

/// Run the daq-record subcommand
pub async fn run_daq_record(config_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Load config file
    let config_text = std::fs::read_to_string(config_path)
        .map_err(|e| format!("Failed to read config file '{}': {}", config_path, e))?;
    let config: DaqRecordConfig = toml::from_str(&config_text)
        .map_err(|e| format!("Failed to parse config file '{}': {}", config_path, e))?;

    // Resolve output paths
    let csv_path = config.output.as_ref().and_then(|o| o.csv.clone());
    let mf4_path = config.output.as_ref().and_then(|o| o.mf4.clone());
    if csv_path.is_none() && mf4_path.is_none() {
        return Err("No output specified. Set at least one of [output].csv or [output].mf4 in the config file.".into());
    }

    // Check that output files do not already exist
    if let Some(ref p) = csv_path {
        if std::path::Path::new(p).exists() {
            return Err(format!(
                "Output file '{}' already exists. Remove it or choose a different path.",
                p
            )
            .into());
        }
    }
    if let Some(ref p) = mf4_path {
        if std::path::Path::new(p).exists() {
            return Err(format!(
                "Output file '{}' already exists. Remove it or choose a different path.",
                p
            )
            .into());
        }
    }

    if let Some(ref p) = csv_path {
        println!("  CSV output: {}", p);
    }
    if let Some(ref p) = mf4_path {
        println!("  MF4 output: {}", p);
    }

    let a2l_path = &config.a2l;

    let default_event_channel = config
        .daq
        .as_ref()
        .and_then(|d| d.event_channel)
        .unwrap_or(0);

    // Load A2L file
    println!("Loading A2L file: {}", a2l_path);
    let reader = A2lReader::load(a2l_path).map_err(|e| format!("Failed to load A2L: {}", e))?;

    // Resolve transport: [connection] section > A2L transport blocks
    let a2l_transports = reader.get_transport_configs();
    let transport_config =
        resolve_transport_config(config.connection.as_ref(), &a2l_transports)?;

    // Build DAQ manager from A2L
    let mut daq_manager = DaqManager::from_a2l(a2l_path)
        .map_err(|e| format!("Failed to create DAQ manager: {}", e))?;

    // Load events from A2L
    let events = reader.get_events();
    // Distinct event channels (for validation and error messages)
    let mut event_channels: Vec<u16> = events.iter().map(|e| e.channel).collect();
    event_channels.sort_unstable();
    event_channels.dedup();
    if !events.is_empty() {
        println!("  {} DAQ events found in A2L:", events.len());
        for ev in &events {
            // TIME_UNIT is a decade exponent (base 1 ns), so the real cycle
            // is time_cycle * 10^time_unit ns — e.g. cycle=1, unit=7 → 10 ms.
            let cycle = if ev.time_cycle > 0 {
                format!("{:.1}ms", ev.cycle_time_ms())
            } else {
                "n/a".to_string()
            };
            println!("    channel {}: {} ({})", ev.channel, ev.name, cycle);
        }
        daq_manager.set_available_events(events);
    }

    // Validate default event channel
    if !event_channels.is_empty() && !event_channels.contains(&default_event_channel) {
        return Err(format!(
            "Default event_channel {} from config not found in A2L. Available event channels: {:?}",
            default_event_channel, event_channels
        )
        .into());
    }

    let all_count = daq_manager.available_count();
    println!("  {} measurements available from A2L", all_count);

    // Select measurements based on [[events]] config
    daq_manager.deselect_all();

    if let Some(ref event_configs) = config.events {
        if event_configs.is_empty() {
            println!("  No events specified in config — selecting all measurements");
            daq_manager.select_all();
        } else {
            let all_measurements = daq_manager.all_measurements().to_vec();
            for ec in event_configs {
                // Validate event channel
                if !event_channels.is_empty() && !event_channels.contains(&ec.channel) {
                    return Err(format!(
                        "Event channel {} not found in A2L. Available event channels: {:?}",
                        ec.channel, event_channels
                    )
                    .into());
                }
                for name in &ec.measurements {
                    if let Some(idx) = all_measurements.iter().position(|m| &m.name == name) {
                        daq_manager.select_measurement(idx);
                        daq_manager.set_measurement_event(idx, ec.channel);
                    } else {
                        let msg = format!("Measurement '{}' not found in A2L — skipped", name);
                        eprintln!("  ⚠ {}", msg);
                        tracing::warn!("{}", msg);
                    }
                }
            }
        }
    } else {
        println!("  No [[events]] section in config — selecting all measurements");
        daq_manager.select_all();
    }

    let selected_count = daq_manager.selected_count();
    if selected_count == 0 {
        return Err("No measurements selected for recording".into());
    }
    println!("  {} measurements selected for recording", selected_count);

    // Set default event channel
    daq_manager.set_event_channel(default_event_channel);

    // Configure output paths and enable/disable flags
    if let Some(ref csv) = csv_path {
        daq_manager.set_csv_path(csv);
        daq_manager.set_csv_enabled(true);
    } else {
        daq_manager.set_csv_enabled(false);
    }
    if let Some(ref mf4) = mf4_path {
        daq_manager.set_mdf4_path(mf4);
        daq_manager.set_mf4_enabled(true);
    } else {
        daq_manager.set_mf4_enabled(false);
    }

    // Connect to slave
    println!("\nConnecting via {}...", transport_display(&transport_config));
    let mut client = XcpClient::new();
    let slave_id = client
        .connect_slave("daq-record-target", transport_config.clone(), None)
        .await
        .map_err(|e| format!("Failed to connect: {}", e))?;
    println!("  Connected (id: {})", slave_id);

    // Get slave handle
    let xcp_slave = client
        .get_slave(slave_id)
        .ok_or("Failed to get slave handle")?;

    // Set slave address on DAQ manager (display/logging only)
    daq_manager.set_slave_addr(transport_config.addr_string());

    // Start DAQ (configure + start streaming)
    println!("\nStarting DAQ...");
    {
        let guard = xcp_slave.lock().await;
        daq_manager
            .start(&*guard, 0)
            .await
            .map_err(|e| format!("DAQ start failed: {}", e))?;
    }
    println!("  DAQ streaming started");

    // Start recording (file writing)
    let output_desc = match (&csv_path, &mf4_path) {
        (Some(c), Some(m)) => format!("{} + {}", c, m),
        (Some(c), None) => c.clone(),
        (None, Some(m)) => m.clone(),
        _ => unreachable!(),
    };
    println!("Starting recording to: {}", output_desc);
    {
        let guard = xcp_slave.lock().await;
        daq_manager
            .start_recording(&*guard)
            .await
            .map_err(|e| format!("Recording start failed: {}", e))?;
    }
    println!("  Recording... Press CTRL+C to stop.\n");

    // Create channel for stdin quit signal
    let (quit_tx, quit_rx) = tokio::sync::oneshot::channel::<()>();

    // Spawn a background task to monitor stdin for "q"
    tokio::spawn(async move {
        let mut reader = tokio::io::BufReader::new(tokio::io::stdin());
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break, // EOF
                Ok(_) => {
                    if line.trim() == "q" {
                        let _ = quit_tx.send(());
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Wait for either CTRL+C or "q" on stdin
    let quit_reason = tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            "CTRL+C"
        }
        _ = quit_rx => {
            "q"
        }
    };

    println!("\n\n{} received. Shutting down gracefully...", quit_reason);

    // Graceful shutdown: stop recording → stop DAQ → free DAQ → disconnect
    if daq_manager.is_recording() {
        println!("  Stopping recording...");
        if let Err(e) = daq_manager.stop_recording().await {
            eprintln!("  Warning: stop recording failed: {}", e);
        }
    }

    if daq_manager.is_running() {
        println!("  Stopping DAQ...");
        let guard = xcp_slave.lock().await;
        if let Err(e) = daq_manager.stop(&*guard).await {
            eprintln!("  Warning: DAQ stop failed: {}", e);
        }
        drop(guard);
    }

    println!("  Freeing DAQ resources...");
    {
        let guard = xcp_slave.lock().await;
        if let Err(e) = guard.free_daq().await {
            eprintln!("  Warning: FREE_DAQ failed: {}", e);
        }
    }

    println!("  Disconnecting...");
    if let Err(e) = client.disconnect_slave(slave_id).await {
        eprintln!("  Warning: disconnect failed: {}", e);
    }

    // Print summary
    let (duration, csv_size, mf4_size) = daq_manager.recording_stats().await;
    println!("\n--- Recording Summary ---");
    println!("  Duration:  {:.1}s", duration);
    if let Some(ref p) = csv_path {
        println!("  CSV file:  {} ({} bytes)", p, csv_size);
    }
    if let Some(ref p) = mf4_path {
        println!("  MF4 file:  {} ({} bytes)", p, mf4_size);
    }
    println!("\nRecording completed successfully.");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an A2L CAN transport block for tests.
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

    #[test]
    fn test_parse_config_can_connection() {
        let toml_str = r#"
a2l = "ecu.a2l"

[connection]
type = "can"
channel = "PCAN_USBBUS1"
can_id = 0x123
baudrate = 500000
data_baudrate = 2000000
slave_id = 0x124
daq_ids = [0x310, 0x320]
max_dlc_required = true

[output]
mf4 = "recording.mf4"
"#;
        let config: DaqRecordConfig = toml::from_str(toml_str).unwrap();
        let conn = config.connection.unwrap();
        assert_eq!(conn.transport_type.as_deref(), Some("can"));
        assert_eq!(conn.channel.as_deref(), Some("PCAN_USBBUS1"));
        assert_eq!(conn.can_id, Some(0x123));
        assert_eq!(conn.baudrate, Some(500_000));
        assert_eq!(conn.data_baudrate, Some(2_000_000));
        assert_eq!(conn.slave_id, Some(0x124));
        assert_eq!(conn.daq_ids, Some(vec![0x310, 0x320]));
        assert_eq!(conn.max_dlc_required, Some(true));
        assert!(conn.addr.is_none());
    }

    #[test]
    fn test_parse_config_can_hex_ids() {
        // TOML hex integers work for CAN IDs.
        let toml_str = r#"
a2l = "ecu.a2l"

[connection]
type = "can"
can_id = 0x7fb
daq_ids = [0x7fd, 0x7fe]

[output]
csv = "recording.csv"
"#;
        let config: DaqRecordConfig = toml::from_str(toml_str).unwrap();
        let conn = config.connection.unwrap();
        assert_eq!(conn.can_id, Some(0x7fb));
        assert_eq!(conn.daq_ids, Some(vec![0x7fd, 0x7fe]));
    }

    #[test]
    fn test_resolve_transport_udp_from_config() {
        let conn = ConnectionConfig {
            transport_type: None,
            addr: Some("192.168.1.100:5555".into()),
            driver: None,
            channel: None,
            device_id: None,
            controller: None,
            can_id: None,
            baudrate: None,
            data_baudrate: None,
            slave_id: None,
            daq_ids: None,
            max_dlc_required: None,
        };
        let resolved = resolve_transport_config(Some(&conn), &[]).unwrap();
        assert_eq!(
            resolved,
            TransportConfig::Udp {
                addr: "192.168.1.100:5555".into()
            }
        );
    }

    #[test]
    fn test_resolve_transport_tcp_from_config() {
        let conn = ConnectionConfig {
            transport_type: Some("tcp".into()),
            addr: Some("10.0.0.1:6000".into()),
            driver: None,
            channel: None,
            device_id: None,
            controller: None,
            can_id: None,
            baudrate: None,
            data_baudrate: None,
            slave_id: None,
            daq_ids: None,
            max_dlc_required: None,
        };
        let resolved = resolve_transport_config(Some(&conn), &[]).unwrap();
        assert_eq!(
            resolved,
            TransportConfig::Tcp {
                addr: "10.0.0.1:6000".into()
            }
        );
    }

    #[test]
    fn test_resolve_transport_tcp_falls_back_to_a2l() {
        let conn = ConnectionConfig {
            transport_type: Some("tcp".into()),
            addr: None,
            driver: None,
            channel: None,
            device_id: None,
            controller: None,
            can_id: None,
            baudrate: None,
            data_baudrate: None,
            slave_id: None,
            daq_ids: None,
            max_dlc_required: None,
        };
        let a2l = vec![
            A2lTransportConfig::Udp {
                addr: "192.168.0.10:5555".into(),
            },
            A2lTransportConfig::Tcp {
                addr: "192.168.0.10:6001".into(),
            },
        ];
        let resolved = resolve_transport_config(Some(&conn), &a2l).unwrap();
        assert_eq!(
            resolved,
            TransportConfig::Tcp {
                addr: "192.168.0.10:6001".into()
            }
        );
    }

    #[test]
    fn test_resolve_transport_can_full_config() {
        let conn = ConnectionConfig {
            transport_type: Some("can".into()),
            addr: None,
            driver: Some("pcan".into()),
            channel: Some("PCAN_USBBUS1".into()),
            device_id: Some("3".into()),
            controller: Some(1),
            can_id: Some(0x123),
            baudrate: Some(500_000),
            data_baudrate: Some(2_000_000),
            slave_id: Some(0x124),
            daq_ids: Some(vec![0x310]),
            max_dlc_required: Some(true),
        };
        // A2L CAN block present but fully overridden by the config.
        let a2l = vec![a2l_can()];
        let resolved = resolve_transport_config(Some(&conn), &a2l).unwrap();
        assert_eq!(
            resolved,
            TransportConfig::Can {
                driver: Some("pcan".into()),
                channel: "PCAN_USBBUS1".into(),
                device_id: Some("3".into()),
                controller: Some(1),
                can_id: 0x123,
                baudrate: Some(500_000),
                data_baudrate: Some(2_000_000),
                slave_id: Some(0x124),
                daq_ids: vec![0x310],
                max_dlc_required: true,
            }
        );
    }

    #[test]
    fn test_resolve_transport_can_merges_a2l_defaults() {
        // Only the command CAN ID is declared; everything else comes from A2L.
        let conn = ConnectionConfig {
            transport_type: Some("can".into()),
            addr: None,
            driver: None,
            channel: None,
            device_id: None,
            controller: None,
            can_id: Some(0x666),
            baudrate: None,
            data_baudrate: None,
            slave_id: None,
            daq_ids: None,
            max_dlc_required: None,
        };
        let a2l = vec![a2l_can()];
        let resolved = resolve_transport_config(Some(&conn), &a2l).unwrap();
        assert_eq!(
            resolved,
            TransportConfig::Can {
                driver: None,
                channel: "auto".into(),
                device_id: None,
                controller: None,
                can_id: 0x666, // config wins
                baudrate: Some(500_000),
                data_baudrate: None,
                slave_id: Some(0x7fc),
                daq_ids: vec![0x7fd],
                max_dlc_required: true,
            }
        );
    }

    #[test]
    fn test_resolve_transport_can_from_a2l_only() {
        // type = "can" with no CAN fields: everything from the A2L block.
        let conn = ConnectionConfig {
            transport_type: Some("can".into()),
            addr: None,
            driver: None,
            channel: None,
            device_id: None,
            controller: None,
            can_id: None,
            baudrate: None,
            data_baudrate: None,
            slave_id: None,
            daq_ids: None,
            max_dlc_required: None,
        };
        let a2l = vec![a2l_can()];
        let resolved = resolve_transport_config(Some(&conn), &a2l).unwrap();
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
    fn test_resolve_transport_can_requires_can_id() {
        let conn = ConnectionConfig {
            transport_type: Some("can".into()),
            addr: None,
            driver: None,
            channel: None,
            device_id: None,
            controller: None,
            can_id: None,
            baudrate: None,
            data_baudrate: None,
            slave_id: None,
            daq_ids: None,
            max_dlc_required: None,
        };
        let err = resolve_transport_config(Some(&conn), &[]).unwrap_err();
        assert!(
            err.contains("CAN_ID_MASTER"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn test_resolve_transport_auto_prefers_udp_over_can() {
        let a2l = vec![
            a2l_can(),
            A2lTransportConfig::Udp {
                addr: "192.168.0.10:5555".into(),
            },
        ];
        let resolved = resolve_transport_config(None, &a2l).unwrap();
        assert_eq!(
            resolved,
            TransportConfig::Udp {
                addr: "192.168.0.10:5555".into()
            }
        );
    }

    #[test]
    fn test_resolve_transport_auto_falls_back_to_can() {
        // A2L with only a CAN block: auto-select picks CAN.
        let a2l = vec![a2l_can()];
        let resolved = resolve_transport_config(None, &a2l).unwrap();
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
    fn test_resolve_transport_auto_no_transports() {
        let err = resolve_transport_config(None, &[]).unwrap_err();
        assert!(err.contains("No transport"), "unexpected error: {}", err);
    }

    #[test]
    fn test_resolve_transport_unknown_type() {
        let conn = ConnectionConfig {
            transport_type: Some("serial".into()),
            addr: None,
            driver: None,
            channel: None,
            device_id: None,
            controller: None,
            can_id: None,
            baudrate: None,
            data_baudrate: None,
            slave_id: None,
            daq_ids: None,
            max_dlc_required: None,
        };
        let err = resolve_transport_config(Some(&conn), &[]).unwrap_err();
        assert!(err.contains("serial"), "unexpected error: {}", err);
    }

    #[test]
    fn test_resolve_transport_can_fields_without_type_hint() {
        // CAN fields without type = "can" produce a helpful hint.
        let conn = ConnectionConfig {
            transport_type: None,
            addr: None,
            driver: None,
            channel: None,
            device_id: None,
            controller: None,
            can_id: Some(0x123),
            baudrate: None,
            data_baudrate: None,
            slave_id: None,
            daq_ids: None,
            max_dlc_required: None,
        };
        let err = resolve_transport_config(Some(&conn), &[]).unwrap_err();
        assert!(err.contains("type"), "unexpected error: {}", err);
    }

    #[test]
    fn test_parse_transport_type() {
        use xcp_client::transport::TransportType;

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
        let udp = TransportConfig::Udp {
            addr: "127.0.0.1:5555".into(),
        };
        assert_eq!(transport_display(&udp), "UDP 127.0.0.1:5555");

        let can = TransportConfig::Can {
            driver: None,
            channel: "auto".into(),
            device_id: None,
            controller: None,
            can_id: 0x7fb,
            baudrate: Some(500_000),
            data_baudrate: None,
            slave_id: None,
            daq_ids: vec![],
            max_dlc_required: false,
        };
        let label = transport_display(&can);
        assert!(label.contains("CAN"), "label: {}", label);
        assert!(label.contains("0x7FB"), "label: {}", label);

        let canfd = TransportConfig::Can {
            driver: None,
            channel: "auto".into(),
            device_id: None,
            controller: None,
            can_id: 0x7fb,
            baudrate: Some(500_000),
            data_baudrate: Some(2_000_000),
            slave_id: None,
            daq_ids: vec![],
            max_dlc_required: false,
        };
        let label = transport_display(&canfd);
        assert!(label.contains("CAN-FD"), "label: {}", label);
    }

    #[test]
    fn test_parse_config_full() {
        let toml_str = r#"
a2l = "abc.a2l"

[connection]
addr = "192.168.1.100:5555"

[output]
mf4 = "recording.mf4"
csv = "recording.csv"

[daq]
event_channel = 2

[[events]]
channel = 0
measurements = ["speed", "rpm"]

[[events]]
channel = 1
measurements = ["temp"]
"#;
        let config: DaqRecordConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.a2l, "abc.a2l");
        assert_eq!(
            config.connection.unwrap().addr.unwrap(),
            "192.168.1.100:5555"
        );
        let output = config.output.unwrap();
        assert_eq!(output.mf4.unwrap(), "recording.mf4");
        assert_eq!(output.csv.unwrap(), "recording.csv");
        assert_eq!(config.daq.unwrap().event_channel.unwrap(), 2);
        let events = config.events.unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].channel, 0);
        assert_eq!(events[0].measurements, vec!["speed", "rpm"]);
        assert_eq!(events[1].channel, 1);
        assert_eq!(events[1].measurements, vec!["temp"]);
    }

    #[test]
    fn test_parse_config_mf4_only() {
        let toml_str = r#"
a2l = "ecu.a2l"

[connection]
addr = "10.0.0.1:5555"

[output]
mf4 = "data.mf4"
"#;
        let config: DaqRecordConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.a2l, "ecu.a2l");
        let output = config.output.unwrap();
        assert_eq!(output.mf4.unwrap(), "data.mf4");
        assert!(output.csv.is_none());
        assert!(config.daq.is_none());
        assert!(config.events.is_none());
    }

    #[test]
    fn test_parse_config_csv_only() {
        let toml_str = r#"
a2l = "ecu.a2l"

[output]
csv = "data.csv"

[[events]]
channel = 0
measurements = ["speed"]
"#;
        let config: DaqRecordConfig = toml::from_str(toml_str).unwrap();
        let output = config.output.unwrap();
        assert!(output.mf4.is_none());
        assert_eq!(output.csv.unwrap(), "data.csv");
        assert!(config.connection.is_none());
    }

    #[test]
    fn test_parse_config_no_addr() {
        let toml_str = r#"
a2l = "abc.a2l"

[output]
mf4 = "out.mf4"

[daq]
event_channel = 0

[[events]]
channel = 0
measurements = ["speed", "rpm", "accel"]
"#;
        let config: DaqRecordConfig = toml::from_str(toml_str).unwrap();
        assert!(config.connection.is_none());
        let events = config.events.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].measurements.len(), 3);
    }

    #[test]
    fn test_output_file_exists_check() {
        // Verify that existing files are detected
        // Use Cargo.toml as a file that always exists in the project root
        let p = std::path::Path::new("Cargo.toml");
        assert!(p.exists(), "Cargo.toml should exist for this test");

        // A non-existent path should not exist
        let p2 = std::path::Path::new("nonexistent_output_file_12345.mf4");
        assert!(!p2.exists());
    }

    #[test]
    fn test_parse_config_both_outputs() {
        let toml_str = r#"
a2l = "abc.a2l"

[output]
mf4 = "recording.mf4"
csv = "recording.csv"
"#;
        let config: DaqRecordConfig = toml::from_str(toml_str).unwrap();
        let output = config.output.unwrap();
        assert!(output.mf4.is_some());
        assert!(output.csv.is_some());
    }
}
