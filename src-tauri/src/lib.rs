// Tauri app entry point and command registration.

mod commands;
mod state;

use state::TauriAppState;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use commands::log_console::{ConsoleLogLayer, ConsoleLogState};

/// Initialise file-based tracing for the desktop app.
/// Logs go to `logs/` (relative to the working directory) as `xcp_desktop_<timestamp>.log`,
/// consistent with the CLI binary.  Also captures logs to an in-memory buffer for the
/// in-app console viewer.
fn init_logging() -> Arc<ConsoleLogState> {
    use std::fs::OpenOptions;
    use std::path::Path;

    let console_state = Arc::new(ConsoleLogState::new());
    let console_layer = ConsoleLogLayer::new(console_state.clone());

    let filter = tracing_subscriber::EnvFilter::from_default_env()
        .add_directive("debug".parse().unwrap())
        .add_directive("a2lfile=error".parse().unwrap());

    let logs_dir = Path::new("logs");
    let _ = std::fs::create_dir_all(logs_dir);

    let now = chrono::Local::now();
    let filename = format!("xcp_desktop_{}.log", now.format("%Y%m%d_%H%M%S"));
    let log_path = logs_dir.join(filename);

    match OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&log_path)
    {
        Ok(log_file) => {
            let file_layer = tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(move || log_file.try_clone().expect("clone log fd"));

            let _ = tracing_subscriber::registry()
                .with(filter)
                .with(file_layer)
                .with(console_layer)
                .try_init();
            tracing::info!("Tauri desktop logging started → {}", log_path.display());
        }
        Err(e) => {
            let _ = tracing_subscriber::registry()
                .with(filter)
                .with(console_layer)
                .try_init();
            tracing::warn!("Failed to create log file: {}", e);
        }
    }

    console_state
}

pub fn run() {
    let console_state = init_logging();

    let app_state = TauriAppState::new();
    let packet_log = app_state.packet_log.clone();
    let app_state_arc = Arc::new(RwLock::new(app_state));

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .manage(console_state)
        .manage(app_state_arc)
        .manage(packet_log)
        .manage(commands::mf4_viewer::Mf4FileCache::new())
        .invoke_handler(tauri::generate_handler![
            // Console log viewer
            commands::log_console::console_logs_get,
            commands::log_console::console_logs_clear,
            // Device management
            commands::device::list_devices,
            commands::device::list_can_devices,
            commands::device::add_device,
            commands::device::ensure_devices_registered,
            commands::device::connect_device,
            commands::device::disconnect_device,
            commands::device::remove_device,
            commands::device::rename_device,
            commands::device::connect_all_devices,
            // A2L
            commands::a2l::load_a2l,
            // Calibration
            commands::calibration::cal_available,
            commands::calibration::cal_read,
            commands::calibration::cal_read_all,
            commands::calibration::cal_write,
            commands::calibration::cal_write_element,
            commands::calibration::cal_write_axis_point,
            commands::calibration::cal_value,
            commands::calibration::cal_detail,
            commands::calibration::get_page_mode,
            commands::calibration::switch_page_mode,
            // DAQ
            commands::daq::daq_available,
            commands::daq::daq_start,
            commands::daq::daq_stop,
            commands::daq::daq_status,
            commands::daq::daq_toggle,
            commands::daq::daq_set_selection,
            commands::daq::daq_clear,
            commands::daq::daq_data,
            commands::daq::daq_events,
            commands::daq::daq_set_event,
            commands::daq::daq_set_measurement_events,
            commands::daq::daq_start_recording,
            commands::daq::daq_stop_recording,
            commands::daq::daq_download_csv,
            // Checksum
            commands::checksum::hex_load,
            commands::checksum::working_hex_load,
            commands::checksum::checksum_verify,
            // Compare
            commands::compare::compare_sessions,
            commands::compare::compare_create_session,
            commands::compare::compare_rename_session,
            commands::compare::compare_delete_session,
            commands::compare::compare_load_source,
            commands::compare::compare_clear_source,
            commands::compare::compare_variables,
            commands::compare::compare_select_variables,
            commands::compare::compare_result,
            // MF4 Viewer
            commands::mf4_viewer::mf4_list_record_files,
            commands::mf4_viewer::mf4_open_file,
            commands::mf4_viewer::mf4_get_channel_data,
            commands::mf4_viewer::mf4_get_channel_time,
            commands::mf4_viewer::mf4_save_config,
            commands::mf4_viewer::mf4_load_config,
            // Experiment
            commands::experiment::experiment_variables,
            commands::experiment::experiment_events,
            commands::experiment::experiment_daq_meta,
            commands::experiment::experiment_set_event,
            commands::experiment::experiment_start,
            commands::experiment::experiment_stop,
            commands::experiment::experiment_status,
            commands::experiment::experiment_pagemode,
            commands::experiment::experiment_rename_slave,
            commands::experiment::experiment_checksum,
            commands::experiment::experiment_cal_changed,
            commands::experiment::experiment_dcm_export,
            commands::experiment::experiment_cal_write,
            commands::experiment::experiment_cal_original,
            commands::experiment::experiment_record_start,
            commands::experiment::experiment_record_stop,
            commands::experiment::write_text_file,
            commands::experiment::read_text_file,
            commands::experiment::open_folder,
            // Upload
            commands::upload::upload_get_segments,
            commands::upload::upload_memory,
            // XCP Packet Browser
            commands::xcp_packet::xcp_packet_get,
            commands::xcp_packet::xcp_packet_clear,
            commands::xcp_packet::xcp_packet_set_enabled,
        ])
        .run(tauri::generate_context!())
        .expect("error while running xcp-client-desktop");
}
