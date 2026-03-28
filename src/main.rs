//! XCP Client - Terminal UI / Web UI Application

mod tui;

use std::fs::OpenOptions;
use std::time::Duration;
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
        #[arg(short = 'H', long, default_value = "0.0.0.0")]
        host: String,
        /// Port to listen on
        #[arg(short, long, default_value = "8080")]
        port: u16,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Initialize logging to file
    let log_file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open("xcp_client.log")?;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive("debug".parse()?),
        )
        .with_ansi(false)
        .with_writer(move || log_file.try_clone().expect("Failed to clone log file"))
        .try_init()
        .ok();

    match cli.mode {
        Some(Mode::Web { host, port }) => {
            run_web_ui(&host, port).await?;
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
