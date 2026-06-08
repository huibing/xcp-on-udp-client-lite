//! Non-interactive DAQ recording CLI subcommand.
//!
//! Records XCP DAQ measurements to MF4 and/or CSV files.
//! Runs until CTRL+C, then gracefully stops DAQ and saves output files.
//!
//! Usage: `cargo run -- daq-record --config daq_record.config.toml`

use serde::Deserialize;
use xcp_client::transport::TransportConfig;
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
    pub addr: Option<String>,
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

    // Resolve slave address: config > A2L
    let slave_addr = config
        .connection
        .as_ref()
        .and_then(|c| c.addr.clone())
        .or_else(|| reader.get_slave_addresses().into_iter().next())
        .ok_or("No slave address found in config or A2L file")?;

    // Build DAQ manager from A2L
    let mut daq_manager = DaqManager::from_a2l(a2l_path)
        .map_err(|e| format!("Failed to create DAQ manager: {}", e))?;

    // Load events from A2L
    let events = reader.get_events();
    let event_channels: Vec<u16> = events.iter().map(|e| e.channel).collect();
    if !events.is_empty() {
        println!("  {} DAQ events found in A2L:", events.len());
        for ev in &events {
            println!(
                "    channel {}: {} ({}ms cycle)",
                ev.channel, ev.name, ev.time_cycle
            );
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
    println!("\nConnecting to slave at {}...", slave_addr);
    let mut client = XcpClient::new();
    let transport_config = TransportConfig::Udp {
        addr: slave_addr.clone(),
    };
    let slave_id = client
        .connect_slave("daq-record-target", transport_config, None)
        .await
        .map_err(|e| format!("Failed to connect: {}", e))?;
    println!("  Connected (id: {})", slave_id);

    // Get slave handle
    let xcp_slave = client
        .get_slave(slave_id)
        .ok_or("Failed to get slave handle")?;

    // Set slave address on DAQ manager
    daq_manager.set_slave_addr(slave_addr.clone());

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

    // Wait for CTRL+C
    tokio::signal::ctrl_c()
        .await
        .map_err(|e| format!("Failed to listen for CTRL+C: {}", e))?;

    println!("\n\nCTRL+C received. Shutting down gracefully...");

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
