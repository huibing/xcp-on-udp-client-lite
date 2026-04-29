// Tauri app entry point and command registration.

mod commands;
mod state;

use state::TauriAppState;
use std::sync::Arc;
use tokio::sync::RwLock;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .manage(Arc::new(RwLock::new(TauriAppState::new())))
        .invoke_handler(tauri::generate_handler![
            // Device management
            commands::device::list_devices,
            commands::device::add_device,
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
            // Experiment
            commands::experiment::experiment_variables,
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running xcp-client-desktop");
}
