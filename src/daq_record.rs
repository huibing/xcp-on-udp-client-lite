//! Non-interactive DAQ recording CLI subcommand.
//!
//! Records XCP DAQ measurements to MF4 and/or CSV files for one or more slaves.
//! Runs until CTRL+C or the user enters "q" + Enter on stdin, then gracefully
//! stops DAQ and saves the output files.
//!
//! A config file either describes a single slave with the top-level `a2l`,
//! `[connection]`, `[output]`, `[daq]` and `[[events]]` sections, or several
//! slaves with one `[[slaves]]` table each (own A2L, transport, output files,
//! DAQ defaults and event groups). In the multi-slave form a top-level
//! `[output]` is still allowed and acts as the shared default for slaves that
//! do not declare their own; `{name}` / `{index}` placeholders expand to the
//! slave label and 0-based index.
//!
//! Usage: `cargo run -- daq-record --config daq_record.config.toml`

use serde::Deserialize;
use xcp_client::a2l::A2lTransportConfig;
use xcp_client::transport::{transport_display, TransportConfig};
use xcp_client::{A2lReader, DaqManager, SlaveId, XcpClient};

pub use crate::cli_connection::ConnectionConfig;
use crate::cli_slave::SlaveNamer;

/// TOML config file structure.
///
/// The top-level `a2l`/`connection`/`daq`/`events` fields are the single-slave
/// shorthand; `output` may also be used as a shared default for `[[slaves]]`.
#[derive(Debug, Deserialize)]
pub struct DaqRecordConfig {
    /// Path to the A2L description file (single-slave shorthand)
    pub a2l: Option<String>,
    pub connection: Option<ConnectionConfig>,
    pub output: Option<OutputConfig>,
    pub daq: Option<DaqConfig>,
    /// Event-based measurement grouping (single-slave shorthand)
    pub events: Option<Vec<EventConfig>>,
    /// One entry per slave to record (multi-slave form)
    #[serde(default)]
    pub slaves: Vec<SlaveConfig>,
}

/// One `[[slaves]]` entry.
#[derive(Debug, Deserialize)]
pub struct SlaveConfig {
    /// Label used in log lines and `{name}` output placeholders.
    /// Defaults to the A2L file stem.
    pub name: Option<String>,
    /// Path to the A2L description file (required)
    pub a2l: String,
    pub connection: Option<ConnectionConfig>,
    /// Output files; falls back to the top-level `[output]` when omitted
    pub output: Option<OutputConfig>,
    pub daq: Option<DaqConfig>,
    /// Event-based measurement grouping
    pub events: Option<Vec<EventConfig>>,
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

/// A slave config after shorthand expansion, label resolution and output
/// placeholder expansion.
#[derive(Debug)]
struct ResolvedSlave {
    label: String,
    a2l: String,
    connection: Option<ConnectionConfig>,
    csv: Option<String>,
    mf4: Option<String>,
    daq: Option<DaqConfig>,
    events: Option<Vec<EventConfig>>,
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
    crate::cli_connection::resolve_connection_config(connection, a2l_configs)
}

/// Expand `{name}` / `{index}` placeholders in an output path.
///
/// `{name}` is the slave label with path-hostile characters replaced by `_`;
/// `{index}` is the 0-based position of the slave in the config.
fn expand_output_path(path: &str, label: &str, index: usize) -> String {
    path.replace("{name}", &sanitize_file_component(label))
        .replace("{index}", &index.to_string())
}

/// Make a slave label safe to embed into a file name.
fn sanitize_file_component(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect()
}

/// Expand the single-slave shorthand and validate all output paths.
fn resolve_slaves(config: DaqRecordConfig) -> Result<Vec<ResolvedSlave>, String> {
    let DaqRecordConfig {
        a2l,
        connection,
        output,
        daq,
        events,
        slaves,
    } = config;

    if slaves.is_empty() {
        let a2l = a2l.ok_or(
            "No slave configured: add `a2l = \"description.a2l\"` or one or more \
             [[slaves]] sections to the config file",
        )?;
        let label = crate::cli_slave::default_slave_name(&a2l, 0);
        let (csv, mf4) = resolve_output(output.as_ref(), &label, 0)?;
        let resolved = vec![ResolvedSlave {
            label,
            a2l,
            connection,
            csv,
            mf4,
            daq,
            events,
        }];
        validate_output_paths(&resolved)?;
        return Ok(resolved);
    }

    if a2l.is_some() || connection.is_some() || daq.is_some() || events.is_some() {
        return Err(
            "Top-level `a2l`/`[connection]`/`[daq]`/`[[events]]` cannot be combined with \
             [[slaves]]; move those settings into the [[slaves]] entries. A top-level \
             [output] is allowed and shared by all slaves."
                .to_string(),
        );
    }

    let mut namer = SlaveNamer::new();
    let mut resolved = Vec::with_capacity(slaves.len());
    for (index, slave) in slaves.into_iter().enumerate() {
        let label = namer.label(slave.name.as_deref(), &slave.a2l, index);
        let slave_output = slave.output.as_ref().or(output.as_ref());
        let (csv, mf4) = resolve_output(slave_output, &label, index)?;
        resolved.push(ResolvedSlave {
            label,
            a2l: slave.a2l,
            connection: slave.connection,
            csv,
            mf4,
            daq: slave.daq,
            events: slave.events,
        });
    }
    validate_output_paths(&resolved)?;
    Ok(resolved)
}

/// Resolve the CSV/MF4 paths for one slave (at least one is required).
fn resolve_output(
    output: Option<&OutputConfig>,
    label: &str,
    index: usize,
) -> Result<(Option<String>, Option<String>), String> {
    let Some(output) = output else {
        return Err(format!(
            "[{}] no [output] configured: set [output].csv and/or [output].mf4 \
             (a top-level [output] is shared by all slaves)",
            label
        ));
    };
    let csv = output
        .csv
        .as_deref()
        .map(|path| expand_output_path(path, label, index));
    let mf4 = output
        .mf4
        .as_deref()
        .map(|path| expand_output_path(path, label, index));
    if csv.is_none() && mf4.is_none() {
        return Err(format!(
            "[{}] [output] needs at least one of `csv` or `mf4`",
            label
        ));
    }
    Ok((csv, mf4))
}

/// Fail before connecting when two slaves write the same file or a file exists.
fn validate_output_paths(slaves: &[ResolvedSlave]) -> Result<(), String> {
    let mut seen: std::collections::HashMap<String, &str> = std::collections::HashMap::new();
    for slave in slaves {
        for path in [&slave.csv, &slave.mf4].into_iter().flatten() {
            // Windows paths are case-insensitive, so compare case-insensitively.
            if let Some(previous) = seen.insert(path.to_lowercase(), &slave.label) {
                return Err(format!(
                    "Output file '{}' is used more than once (by '{}' and '{}'). \
                     Use distinct paths or the {{name}}/{{index}} placeholders.",
                    path, previous, slave.label
                ));
            }
            if std::path::Path::new(path).exists() {
                return Err(format!(
                    "Output file '{}' already exists. Remove it or choose a different path.",
                    path
                ));
            }
        }
    }
    Ok(())
}

/// One connected slave with a running recording session.
struct RecordSession {
    label: String,
    client: XcpClient,
    slave_id: SlaveId,
    daq: DaqManager,
    csv_path: Option<String>,
    mf4_path: Option<String>,
}

/// Run the daq-record subcommand
pub async fn run_daq_record(config_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config_text = std::fs::read_to_string(config_path)
        .map_err(|e| format!("Failed to read config file '{}': {}", config_path, e))?;
    let config: DaqRecordConfig = toml::from_str(&config_text)
        .map_err(|e| format!("Failed to parse config file '{}': {}", config_path, e))?;

    let slave_configs = resolve_slaves(config)?;
    println!("Recording {} slave(s):", slave_configs.len());
    for slave in &slave_configs {
        println!("  [{}] A2L: {}", slave.label, slave.a2l);
        if let Some(ref path) = slave.csv {
            println!("  [{}] CSV output: {}", slave.label, path);
        }
        if let Some(ref path) = slave.mf4 {
            println!("  [{}] MF4 output: {}", slave.label, path);
        }
    }

    // Start every slave; roll back already started ones when one fails.
    let mut sessions: Vec<RecordSession> = Vec::with_capacity(slave_configs.len());
    for slave in slave_configs {
        match open_session(slave).await {
            Ok(session) => sessions.push(session),
            Err(error) => {
                shutdown(&mut sessions).await;
                return Err(error.into());
            }
        }
    }

    println!(
        "\nRecording {} slave(s). Press CTRL+C to stop.\n",
        sessions.len()
    );

    // Quit channel for the "q" + Enter shortcut.
    //
    // The extra sender clone keeps the channel open when the stdin thread ends
    // (e.g. piped stdin at EOF), so EOF is not mistaken for a quit request.
    let (quit_tx, mut quit_rx) = tokio::sync::mpsc::channel::<()>(1);
    let _quit_keepalive = quit_tx.clone();

    // A plain OS thread, deliberately *not* `tokio::io::stdin()`: a pending
    // tokio stdin read lives in the blocking pool, and `Runtime::drop` waits
    // for blocking tasks indefinitely — the process would hang after shutdown
    // until another CTRL+C killed it. A std thread blocked on stdin is simply
    // terminated when the process exits.
    {
        let quit_tx = quit_tx.clone();
        std::thread::Builder::new()
            .name("daq-record-stdin".to_string())
            .spawn(move || {
                let stdin = std::io::stdin();
                let mut line = String::new();
                loop {
                    line.clear();
                    match stdin.read_line(&mut line) {
                        Ok(0) => break, // EOF
                        Ok(_) => {
                            if line.trim() == "q" {
                                let _ = quit_tx.blocking_send(());
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            })
            .ok();
    }

    // Wait for either CTRL+C or "q" on stdin.
    //
    // The CTRL+C listener stays armed for the whole run (including the graceful
    // shutdown) so a second press cannot kill the process mid-shutdown on
    // Windows — see `cli_signal` for details.
    let mut ctrl_c_rx = crate::cli_signal::ctrl_c_channel();
    let quit_reason = tokio::select! {
        _ = ctrl_c_rx.changed() => {
            "CTRL+C"
        }
        _ = quit_rx.recv() => {
            "q"
        }
    };

    println!("\n\n{} received. Shutting down gracefully...", quit_reason);
    shutdown(&mut sessions).await;

    // Print summary
    println!("\n--- Recording Summary ---");
    for session in &sessions {
        let (duration, csv_size, mf4_size) = session.daq.recording_stats().await;
        println!("  [{}] Duration: {:.1}s", session.label, duration);
        if let Some(ref path) = session.csv_path {
            println!(
                "  [{}] CSV file: {} ({} bytes)",
                session.label, path, csv_size
            );
        }
        if let Some(ref path) = session.mf4_path {
            println!(
                "  [{}] MF4 file: {} ({} bytes)",
                session.label, path, mf4_size
            );
        }
    }
    println!("\nRecording completed successfully.");

    Ok(())
}

/// Load the A2L, select measurements, connect, then start DAQ and recording.
async fn open_session(slave: ResolvedSlave) -> Result<RecordSession, String> {
    let label = slave.label.clone();

    // Load A2L file
    let reader = A2lReader::load(&slave.a2l)
        .map_err(|e| format!("[{}] failed to load A2L '{}': {}", label, slave.a2l, e))?;

    // Resolve transport: [connection] section > A2L transport blocks
    let transport =
        resolve_transport_config(slave.connection.as_ref(), &reader.get_transport_configs())
            .map_err(|e| format!("[{}] {}", label, e))?;

    // Build DAQ manager from A2L
    let mut daq = DaqManager::from_a2l(&slave.a2l)
        .map_err(|e| format!("[{}] failed to create DAQ manager: {}", label, e))?;

    // Load events from A2L
    let events = reader.get_events();
    let mut event_channels: Vec<u16> = events.iter().map(|e| e.channel).collect();
    event_channels.sort_unstable();
    event_channels.dedup();
    if !events.is_empty() {
        daq.set_available_events(events);
    }

    let default_event_channel = slave
        .daq
        .as_ref()
        .and_then(|d| d.event_channel)
        .unwrap_or(0);
    if !event_channels.is_empty() && !event_channels.contains(&default_event_channel) {
        return Err(format!(
            "[{}] default event_channel {} from config not found in A2L. Available: {:?}",
            label, default_event_channel, event_channels
        ));
    }

    // Select measurements based on [[events]] config
    daq.deselect_all();
    match slave.events.as_ref() {
        Some(event_configs) if !event_configs.is_empty() => {
            for event in event_configs {
                if !event_channels.is_empty() && !event_channels.contains(&event.channel) {
                    return Err(format!(
                        "[{}] event channel {} not found in A2L. Available: {:?}",
                        label, event.channel, event_channels
                    ));
                }
                for name in &event.measurements {
                    // Exact name, or every element of an expanded array/matrix.
                    if daq.select_by_name(name, event.channel) == 0 {
                        let message = format!("Measurement '{}' not found in A2L — skipped", name);
                        eprintln!("  [{}] ⚠ {}", label, message);
                        tracing::warn!("[{}] {}", label, message);
                    }
                }
            }
        }
        Some(_) => daq.select_all(),
        None => daq.select_all(),
    }

    if daq.selected_count() == 0 {
        return Err(format!(
            "[{}] no measurements selected for recording — check the [[events]] measurements",
            label
        ));
    }

    daq.set_event_channel(default_event_channel);

    // Configure output paths and enable/disable flags
    if let Some(ref csv) = slave.csv {
        daq.set_csv_path(csv);
        daq.set_csv_enabled(true);
    } else {
        daq.set_csv_enabled(false);
    }
    if let Some(ref mf4) = slave.mf4 {
        daq.set_mdf4_path(mf4);
        daq.set_mf4_enabled(true);
    } else {
        daq.set_mf4_enabled(false);
    }

    println!(
        "[{}] {} measurement(s) selected, connecting via {}...",
        label,
        daq.selected_count(),
        transport_display(&transport)
    );

    // Connect to slave
    let mut client = XcpClient::new();
    let slave_id = client
        .connect_slave(label.clone(), transport.clone(), None)
        .await
        .map_err(|e| format!("[{}] connection failed: {}", label, e))?;

    // Set slave address on DAQ manager (display/logging only)
    daq.set_slave_addr(transport.addr_string());

    // Start DAQ (configure + start streaming)
    let start_result = match client.get_slave(slave_id) {
        Some(xcp_slave) => {
            let guard = xcp_slave.lock().await;
            daq.start(&guard, 0).await
        }
        None => Err("failed to get slave handle".to_string()),
    };
    if let Err(error) = start_result {
        let _ = client.disconnect_slave(slave_id).await;
        return Err(format!("[{}] DAQ start failed: {}", label, error));
    }

    // Start recording (file writing)
    let recording_result = match client.get_slave(slave_id) {
        Some(xcp_slave) => {
            let guard = xcp_slave.lock().await;
            daq.start_recording(&guard).await
        }
        None => Err("failed to get slave handle".to_string()),
    };
    if let Err(error) = recording_result {
        // Roll back the DAQ session we just started.
        if let Some(xcp_slave) = client.get_slave(slave_id) {
            let guard = xcp_slave.lock().await;
            let _ = daq.stop(&guard).await;
            let _ = guard.free_daq().await;
        }
        let _ = client.disconnect_slave(slave_id).await;
        return Err(format!("[{}] recording start failed: {}", label, error));
    }

    println!(
        "[{}] recording ({})",
        label,
        describe_outputs(&slave.csv, &slave.mf4)
    );

    Ok(RecordSession {
        label,
        client,
        slave_id,
        daq,
        csv_path: slave.csv,
        mf4_path: slave.mf4,
    })
}

/// Human readable output description, e.g. `CSV + MF4`.
fn describe_outputs(csv: &Option<String>, mf4: &Option<String>) -> String {
    match (csv, mf4) {
        (Some(_), Some(_)) => "CSV + MF4".to_string(),
        (Some(_), None) => "CSV".to_string(),
        (None, Some(_)) => "MF4".to_string(),
        (None, None) => "no output".to_string(),
    }
}

/// Graceful shutdown: stop recording → stop DAQ → free DAQ → disconnect.
async fn shutdown(sessions: &mut [RecordSession]) {
    for session in sessions.iter_mut() {
        if session.daq.is_recording() {
            println!("  [{}] stopping recording...", session.label);
            if let Err(e) = session.daq.stop_recording().await {
                eprintln!(
                    "  [{}] warning: stop recording failed: {}",
                    session.label, e
                );
            }
        }

        if let Some(xcp_slave) = session.client.get_slave(session.slave_id) {
            let guard = xcp_slave.lock().await;
            if session.daq.is_running() {
                println!("  [{}] stopping DAQ...", session.label);
                if let Err(e) = session.daq.stop(&guard).await {
                    eprintln!("  [{}] warning: DAQ stop failed: {}", session.label, e);
                }
            }
            println!("  [{}] freeing DAQ resources...", session.label);
            if let Err(e) = guard.free_daq().await {
                eprintln!("  [{}] warning: FREE_DAQ failed: {}", session.label, e);
            }
        }

        println!("  [{}] disconnecting...", session.label);
        if let Err(e) = session.client.disconnect_slave(session.slave_id).await {
            eprintln!("  [{}] warning: disconnect failed: {}", session.label, e);
        }
    }
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
        assert!(err.contains("CAN_ID_MASTER"), "unexpected error: {}", err);
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
        use xcp_client::transport::{parse_transport_type, TransportType};
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
        assert_eq!(config.a2l.as_deref(), Some("abc.a2l"));
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
        assert_eq!(config.a2l.as_deref(), Some("ecu.a2l"));
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

    // ==================== multi-slave ====================

    #[test]
    fn test_parse_multi_slave_config() {
        let toml_str = r#"
[output]
mf4 = "rec_{name}.mf4"

[[slaves]]
name = "inverter"
a2l = "pms-a.a2l"

[slaves.connection]
type = "can"
channel = "PCAN_USBBUS1"
can_id = 0x7fb
daq_ids = [0x7fd]

[slaves.output]
csv = "inv.csv"

[slaves.daq]
event_channel = 0

[[slaves.events]]
channel = 0
measurements = ["A", "B"]

[[slaves]]
a2l = "charger.a2l"

[slaves.connection]
addr = "192.168.1.20:5555"

[[slaves.events]]
channel = 2
measurements = ["V"]
"#;
        let config: DaqRecordConfig = toml::from_str(toml_str).unwrap();
        assert!(config.a2l.is_none());
        assert_eq!(config.slaves.len(), 2);
        assert_eq!(
            config.output.as_ref().unwrap().mf4.as_deref(),
            Some("rec_{name}.mf4")
        );

        let resolved = resolve_slaves(config).unwrap();
        assert_eq!(resolved.len(), 2);

        // First slave: its own [slaves.output] replaces the shared one.
        assert_eq!(resolved[0].label, "inverter");
        assert_eq!(resolved[0].csv.as_deref(), Some("inv.csv"));
        assert!(resolved[0].mf4.is_none());
        assert_eq!(resolved[0].daq.as_ref().unwrap().event_channel, Some(0));
        assert_eq!(resolved[0].events.as_ref().unwrap().len(), 1);
        let conn = resolved[0].connection.as_ref().unwrap();
        assert_eq!(conn.transport_type.as_deref(), Some("can"));
        assert_eq!(conn.can_id, Some(0x7fb));

        // Second slave: no explicit name → A2L stem; shared outputs only.
        assert_eq!(resolved[1].label, "charger");
        assert!(resolved[1].csv.is_none());
        assert_eq!(resolved[1].mf4.as_deref(), Some("rec_charger.mf4"));
        assert_eq!(
            resolved[1].connection.as_ref().unwrap().addr.as_deref(),
            Some("192.168.1.20:5555")
        );
    }

    #[test]
    fn test_multi_slave_shared_output_index_placeholder() {
        let toml_str = r#"
[output]
csv = "rec_{index}_{name}.csv"

[[slaves]]
a2l = "a/ecu.a2l"

[[slaves]]
a2l = "b/ecu.a2l"
"#;
        let config: DaqRecordConfig = toml::from_str(toml_str).unwrap();
        let resolved = resolve_slaves(config).unwrap();
        assert_eq!(resolved[0].label, "ecu");
        assert_eq!(resolved[0].csv.as_deref(), Some("rec_0_ecu.csv"));
        assert_eq!(resolved[1].label, "ecu#2");
        assert_eq!(resolved[1].csv.as_deref(), Some("rec_1_ecu#2.csv"));
    }

    #[test]
    fn test_multi_slave_rejects_top_level_shorthand() {
        let toml_str = r#"
a2l = "top.a2l"

[[slaves]]
a2l = "ecu.a2l"

[slaves.output]
csv = "rec.csv"
"#;
        let config: DaqRecordConfig = toml::from_str(toml_str).unwrap();
        let err = resolve_slaves(config).unwrap_err();
        assert!(err.contains("cannot be combined"), "error: {}", err);
    }

    #[test]
    fn test_multi_slave_allows_shared_top_level_output() {
        let toml_str = r#"
[output]
csv = "rec_{name}.csv"

[[slaves]]
a2l = "ecu.a2l"
"#;
        let config: DaqRecordConfig = toml::from_str(toml_str).unwrap();
        let resolved = resolve_slaves(config).unwrap();
        assert_eq!(resolved[0].csv.as_deref(), Some("rec_ecu.csv"));
    }

    #[test]
    fn test_multi_slave_requires_output() {
        let toml_str = r#"
[[slaves]]
a2l = "ecu.a2l"
"#;
        let config: DaqRecordConfig = toml::from_str(toml_str).unwrap();
        let err = resolve_slaves(config).unwrap_err();
        assert!(err.contains("[output]"), "error: {}", err);
        assert!(err.contains("ecu"), "error: {}", err);
    }

    #[test]
    fn test_multi_slave_rejects_duplicate_outputs() {
        let toml_str = r#"
[[slaves]]
name = "a"
a2l = "a.a2l"

[slaves.output]
csv = "recording.csv"

[[slaves]]
name = "b"
a2l = "b.a2l"

[slaves.output]
csv = "RECORDING.CSV"
"#;
        let config: DaqRecordConfig = toml::from_str(toml_str).unwrap();
        let err = resolve_slaves(config).unwrap_err();
        assert!(err.contains("more than once"), "error: {}", err);
        // Windows paths are case-insensitive, so the check is too.
        assert!(err.contains("RECORDING.CSV"), "error: {}", err);
    }

    #[test]
    fn test_multi_slave_rejects_existing_output() {
        // Cargo.toml always exists in the project root.
        let toml_str = r#"
[[slaves]]
a2l = "ecu.a2l"

[slaves.output]
csv = "Cargo.toml"
"#;
        let config: DaqRecordConfig = toml::from_str(toml_str).unwrap();
        let err = resolve_slaves(config).unwrap_err();
        assert!(err.contains("already exists"), "error: {}", err);
    }

    #[test]
    fn test_expand_output_path() {
        assert_eq!(
            expand_output_path("rec_{name}_{index}.mf4", "inverter", 2),
            "rec_inverter_2.mf4"
        );
        assert_eq!(expand_output_path("plain.csv", "x", 0), "plain.csv");
        // Placeholders are optional and may repeat.
        assert_eq!(expand_output_path("{name}-{name}.csv", "a", 0), "a-a.csv");
    }

    #[test]
    fn test_sanitize_file_component() {
        assert_eq!(sanitize_file_component("inverter"), "inverter");
        assert_eq!(sanitize_file_component(r"a/b\c:d"), "a_b_c_d");
        assert_eq!(sanitize_file_component("a*b?c"), "a_b_c");
        assert_eq!(sanitize_file_component("ecu#2"), "ecu#2");
    }

    #[test]
    fn test_resolve_slaves_requires_a2l() {
        let config: DaqRecordConfig = toml::from_str("[output]\ncsv = \"rec.csv\"\n").unwrap();
        let err = resolve_slaves(config).unwrap_err();
        assert!(err.contains("No slave configured"), "error: {}", err);
    }

    #[test]
    fn test_describe_outputs() {
        assert_eq!(
            describe_outputs(&Some("a.csv".into()), &Some("a.mf4".into())),
            "CSV + MF4"
        );
        assert_eq!(describe_outputs(&Some("a.csv".into()), &None), "CSV");
        assert_eq!(describe_outputs(&None, &Some("a.mf4".into())), "MF4");
        assert_eq!(describe_outputs(&None, &None), "no output");
    }
}
