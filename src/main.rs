//! XCP Client - Terminal UI / Web UI Application

mod tui;
mod calibrate;
mod verify;
mod daq_record;

use std::fs::{self, OpenOptions};
use std::path::Path;
use std::time::Duration;
use chrono::Local;
use clap::{Parser, Subcommand};
use tui::app::App;
use tui::event::{Event, EventHandler};
use tui::terminal::Tui;
use xcp_client::{CalibrationManager, DaqManager, XcpClient};

/// XCP Client CLI
#[derive(Parser)]
#[command(name = "xcp-client")]
#[command(about = "XCP Master Client for calibration and DAQ")]
struct Cli {
    /// Mode selection
    #[command(subcommand)]
    mode: Option<Mode>,
}

#[derive(Subcommand)]
enum Mode {
    /// Run Terminal UI (default)
    Tui,
    /// Run Web UI
    Web {
        /// Host address to bind to
        #[arg(short = 'H', long, default_value = "localhost")]
        host: String,
        /// Port to listen on
        #[arg(short, long, default_value = "8080")]
        port: u16,
    },
    /// Calibrate ECU parameters non-interactively
    Calibrate {
        /// Path to A2L description file
        #[arg(long)]
        a2l: String,
        /// Variable name to write (single-var mode)
        #[arg(long)]
        var: Option<String>,
        /// Value to write (single-var mode)
        #[arg(long)]
        value: Option<String>,
        /// Path to DCM file with calibration values (DCM mode)
        #[arg(long)]
        dcm: Option<String>,
        /// Slave address override (default: from A2L)
        #[arg(long)]
        addr: Option<String>,
    },
    /// Verify ECU flash memory via BUILD_CHECKSUM
    Verify {
        /// Path to A2L description file
        #[arg(long)]
        a2l: String,
        /// Path to Intel HEX file
        #[arg(long)]
        hex: String,
        /// Slave address override (default: from A2L)
        #[arg(long)]
        addr: Option<String>,
        /// Max block size for BUILD_CHECKSUM (default: 16 KiB)
        #[arg(long)]
        max_block_size: Option<u32>,
    },
    /// Record DAQ measurements to MF4 and/or CSV files.
    ///
    /// Runs until CTRL+C, then gracefully stops DAQ and saves output files.
    /// All settings (A2L path, output files, measurements) are in the config.
    ///
    /// Config file format (TOML):
    ///
    ///   a2l = "path/to/description.a2l"
    ///
    ///   [connection]
    ///   addr = "192.168.1.100:5555"   # optional, falls back to A2L
    ///
    ///   [output]
    ///   mf4 = "recording.mf4"        # optional (at least one required)
    ///   csv = "recording.csv"         # optional (at least one required)
    ///
    ///   [daq]
    ///   event_channel = 0             # default event channel
    ///
    ///   [[events]]
    ///   channel = 0
    ///   measurements = ["speed", "rpm", "accel"]
    ///
    ///   [[events]]
    ///   channel = 1
    ///   measurements = ["coolant_temp", "oil_temp"]
    #[command(verbatim_doc_comment)]
    DaqRecord {
        /// Path to TOML config file
        #[arg(long)]
        config: String,
    },
}

/// Derive a short mode label from the CLI arguments for log naming.
fn mode_label(mode: &Option<Mode>) -> &'static str {
    match mode {
        Some(Mode::Web { .. }) => "web",
        Some(Mode::Calibrate { .. }) => "calibrate",
        Some(Mode::Verify { .. }) => "verify",
        Some(Mode::DaqRecord { .. }) => "daq_record",
        Some(Mode::Tui) | None => "tui",
    }
}

/// Setup logging to a file in the logs directory.
/// The log filename includes the subcommand so users can identify which
/// CLI mode produced each log file.
fn setup_logging(mode: &str) -> Result<std::fs::File, Box<dyn std::error::Error>> {
    // Create logs directory if it doesn't exist
    let logs_dir = Path::new("logs");
    if !logs_dir.exists() {
        fs::create_dir_all(logs_dir)?;
    }

    // Generate log filename: xcp_client_<mode>_<timestamp>.log
    let now = Local::now();
    let log_filename = format!("xcp_client_{}_{}.log", mode, now.format("%Y%m%d_%H%M%S"));
    let log_path = logs_dir.join(log_filename);

    // Create log file
    let log_file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&log_path)?;

    tracing::info!("Log file created: {}", log_path.display());

    Ok(log_file)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let mode = mode_label(&cli.mode);

    // Initialize logging to file (filename includes mode)
    let log_file = setup_logging(mode)?;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("debug".parse()?)
                .add_directive("a2lfile=error".parse()?),  // Suppress a2lfile warnings
        )
        .with_ansi(false)
        .with_writer(move || log_file.try_clone().expect("Failed to clone log file"))
        .try_init()
        .ok();

    tracing::info!("XCP Client started — mode: {}, args: {:?}", mode, std::env::args().collect::<Vec<_>>());

    match cli.mode {
        Some(Mode::Web { host, port }) => {
            run_web_ui(&host, port).await?;
        }
        Some(Mode::Calibrate { a2l, var, value, dcm, addr }) => {
            calibrate::run_calibrate(&a2l, var, value, dcm, addr).await?;
        }
        Some(Mode::Verify { a2l, hex, addr, max_block_size }) => {
            verify::run_verify(&a2l, &hex, addr, max_block_size).await?;
        }
        Some(Mode::DaqRecord { config }) => {
            daq_record::run_daq_record(&config).await?;
        }
        _ => {
            run_tui().await?;
        }
    }

    Ok(())
}

/// Run Terminal UI
async fn run_tui() -> Result<(), Box<dyn std::error::Error>> {
    let a2l_path = "hello_xcp.a2l".to_string();

    let client = XcpClient::new();
    let cal_manager = CalibrationManager::new();
    let daq_manager = DaqManager::new();

    let mut app = App::new(client, cal_manager, daq_manager, a2l_path);

    let mut tui = Tui::new()?;
    tui.enter()?;

    let mut event_handler = EventHandler::new(Duration::from_millis(100));

    loop {
        if let Err(e) = tui.draw(&mut app) {
            eprintln!("Draw error: {}", e);
            break;
        }

        match event_handler.next().await {
            Ok(Event::Key(key)) => {
                match app.handle_key(key).await {
                    Ok(should_continue) => {
                        if !should_continue {
                            break;
                        }
                    }
                    Err(e) => {
                        app.set_error(format!("Error: {}", e));
                    }
                }
            }
            Ok(Event::Tick) => {
                app.on_tick().await;
            }
            Ok(Event::Resize) => {}
            Ok(Event::Error(e)) => {
                app.set_error(format!("Event error: {}", e));
            }
            Err(e) => {
                eprintln!("Event handler error: {}", e);
                break;
            }
        }
    }

    tui.exit()?;
    Ok(())
}

/// Run Web UI
async fn run_web_ui(host: &str, port: u16) -> Result<(), Box<dyn std::error::Error>> {
    use xcp_client::web::{AppState, WebServer};

    let a2l_path = "hello_xcp.a2l".to_string();

    let client = XcpClient::new();
    let cal_manager = CalibrationManager::new();
    let daq_manager = DaqManager::new();

    let state = AppState::new(client, cal_manager, daq_manager, a2l_path);
    let server = WebServer::new_with_host(state, host, port);

    println!("Starting XCP Web Client...");
    println!("Binding to: http://{}:{}/login", host, port);
    println!("Default credentials: admin / admin");

    server.run().await?;
    Ok(())
}
