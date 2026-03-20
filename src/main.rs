//! XCP Client - Terminal UI Application

mod tui;

use std::fs::OpenOptions;
use std::time::Duration;
use tui::app::App;
use tui::event::{Event, EventHandler};
use tui::terminal::Tui;
use xcp_client::{CalibrationManager, DaqManager, XcpClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging to file
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("xcp_client.log")?;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive("debug".parse()?),
        )
        .with_ansi(false)  // Disable ANSI color codes for log file
        .with_writer(move || log_file.try_clone().expect("Failed to clone log file"))
        .try_init()
        .ok();

    // Create application state
    let client = XcpClient::new();
    let cal_manager = CalibrationManager::new();
    let daq_manager = DaqManager::new();

    let mut app = App::new(client, cal_manager, daq_manager);

    // Setup terminal
    let mut tui = Tui::new()?;
    tui.enter()?;

    // Create event handler
    let mut event_handler = EventHandler::new(Duration::from_millis(100));

    // Main loop
    loop {
        // Draw UI
        if let Err(e) = tui.draw(&mut app) {
            eprintln!("Draw error: {}", e);
            break;
        }

        // Handle events
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
            Ok(Event::Resize) => {
                // Terminal resized, will be handled on next draw
            }
            Ok(Event::Error(e)) => {
                app.set_error(format!("Event error: {}", e));
            }
            Err(e) => {
                eprintln!("Event handler error: {}", e);
                break;
            }
        }
    }

    // Cleanup
    tui.exit()?;
    Ok(())
}
