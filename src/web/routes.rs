//! HTTP Routes
//!
//! All route handlers for the web interface.
//! Multi-slave support: routes now handle multiple XCP slave connections.

use axum::{
    extract::{Form, Json, Multipart, Path, Query, State},
    http::header::SET_COOKIE,
    response::{AppendHeaders, Html, IntoResponse, Redirect},
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[allow(unused)]
use serde_json::json;

use crate::a2l::A2lTransportConfig;
use crate::calibration::{CalibrationManager, CalibrationParam, CalibrationType, CalibrationValue};
use crate::daq::DaqManager;
use crate::slave::SlaveId;
use crate::transport::TransportType;
use crate::web::auth::{clear_session, try_create_session, verify_credentials};
use crate::web::state::{AppState, DeviceState, SlaveState};
use crate::web::templates;

/// Helper function to serialize calibration parameter info including axis data
fn serialize_cal_param(
    p: &CalibrationParam,
    device_id: &str,
    device_name: &str,
    idx: usize,
    connected: bool,
) -> serde_json::Value {
    p.to_json(device_id, device_name, idx, connected)
}

fn parse_transport_type(value: Option<&str>) -> Result<Option<TransportType>, String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    if value.eq_ignore_ascii_case("auto") {
        return Ok(None);
    }

    if value.eq_ignore_ascii_case("udp") {
        return Ok(Some(TransportType::Udp));
    }

    if value.eq_ignore_ascii_case("can") {
        return Ok(Some(TransportType::Can));
    }

    if value.eq_ignore_ascii_case("tcp") {
        return Ok(Some(TransportType::Tcp));
    }

    Err(format!("Unsupported transport protocol '{}'", value))
}

fn transport_option_value(transport_type: TransportType) -> &'static str {
    match transport_type {
        TransportType::Udp => "udp",
        TransportType::Can => "can",
        TransportType::Tcp => "tcp",
    }
}

fn build_transport_options(device: &DeviceState) -> Vec<serde_json::Value> {
    let mut options = vec![json!({
        "value": "",
        "label": "Auto (UDP -> CAN -> TCP)",
    })];

    for config in &device.transport_configs {
        options.push(json!({
            "value": transport_option_value(config.transport_type()),
            "label": config.display_label(),
        }));
    }

    options
}

fn display_device_address(config: &A2lTransportConfig) -> String {
    match config {
        A2lTransportConfig::Udp { addr } | A2lTransportConfig::Tcp { addr } => addr.clone(),
        A2lTransportConfig::Can { can_id } => format!("CAN 0x{:X}", can_id),
    }
}

/// Login page (GET /login)
pub async fn login_page(Query(params): Query<HashMap<String, String>>) -> Html<String> {
    let error = params.get("error").map(|s| s.as_str());
    let engine = crate::web::templates::TemplateEngine::new();
    Html(engine.login_page(error))
}

/// Login API (POST /api/login)
pub async fn login_api(
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Form(form): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    let username = form.get("username").cloned().unwrap_or_default();
    let password = form.get("password").cloned().unwrap_or_default();

    if verify_credentials(&username, &password) {
        let client_ip = addr.ip().to_string();

        match try_create_session(&client_ip) {
            Ok(token) => {
                let cookie = format!("session={}; Path=/; HttpOnly; SameSite=Strict", token);
                (AppendHeaders([(SET_COOKIE, cookie)]), Redirect::to("/?fresh=1")).into_response()
            }
            Err(error_msg) => {
                // Another user is already connected
                Redirect::to(&format!("/login?error={}", urlencoding::encode(&error_msg)))
                    .into_response()
            }
        }
    } else {
        Redirect::to("/login?error=Invalid+credentials").into_response()
    }
}

/// Logout (GET /logout)
pub async fn logout() -> impl IntoResponse {
    clear_session();
    let cookie = "session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0";
    (
        AppendHeaders([(SET_COOKIE, cookie)]),
        Redirect::to("/login"),
    )
}

/// Dashboard page (GET /)
pub async fn dashboard(State(state): State<AppState>) -> Html<String> {
    let slaves = state.all_slaves().await;
    let current_slave = state.current_slave().await;
    let connected_count = state.connected_count().await;
    let total_cal = state.total_cal_count().await;
    let total_meas = state.total_meas_count().await;

    // Check if any DAQ is running
    let mut daq_running = false;
    for slave in &slaves {
        if *slave.daq_running.read().await {
            daq_running = true;
            break;
        }
    }

    // Convert SlaveState to tuple format for template
    let mut slaves_data: Vec<(String, String, bool, usize, usize)> = Vec::new();
    for s in &slaves {
        let cal_count = s.cal_manager.read().await.params().len();
        let meas_count = s.daq_manager.read().await.available_count();
        slaves_data.push((
            s.id.to_string(),
            s.display_name(),
            s.is_connected(),
            cal_count,
            meas_count,
        ));
    }

    // Convert current slave to tuple format
    let current_data: Option<(String, String)> = if let Some(ref cs) = current_slave {
        Some((cs.id.to_string(), cs.display_name()))
    } else {
        None
    };

    let status = state.take_status().await;
    let error = state.take_error().await;

    let content = templates::dashboard_page(
        &slaves_data,
        current_data.as_ref(),
        connected_count,
        total_cal,
        total_meas,
        daq_running,
    );

    Html(templates::page(
        "Dashboard",
        &content,
        status.as_deref(),
        error.as_deref(),
    ))
}

// ============== Device Management Routes ==============

/// Connect all devices API (POST /api/devices/connect-all)
pub async fn connect_all_devices(
    State(state): State<AppState>,
    payload: Option<Json<serde_json::Value>>,
) -> impl IntoResponse {
    let devices = state.all_devices().await;
    let mut client = state.client.lock().await;
    let mut connected_count = 0;
    let mut failed_devices: Vec<String> = Vec::new();
    let transport_overrides = payload
        .as_ref()
        .and_then(|Json(value)| value.get("transport_protocols"))
        .and_then(|value| value.as_object())
        .cloned()
        .unwrap_or_default();

    for device in &devices {
        if device.connected {
            continue; // Already connected
        }

        let requested_transport = match parse_transport_type(
            transport_overrides
                .get(&device.id)
                .and_then(|value| value.as_str()),
        ) {
            Ok(transport) => transport,
            Err(error) => {
                failed_devices.push(format!("{}: {}", device.name, error));
                continue;
            }
        };
        let resolved_transport = match device.resolve_transport_config(requested_transport) {
            Ok(config) => config,
            Err(error) => {
                failed_devices.push(format!("{}: {}", device.name, error));
                continue;
            }
        };
        let transport_config = resolved_transport.to_transport_config();

        match client.connect_slave(&device.name, transport_config).await {
            Ok(slave_id) => {
                // Copy pre-parsed memory segments to XcpSlave (no re-parsing)
                if !device.a2l_path.is_empty() {
                    if let Some(xcp_slave) = client.get_slave(slave_id) {
                        let mut xcp_slave_guard = xcp_slave.lock().await;
                        let segments = device
                            .cal_manager
                            .read()
                            .await
                            .get_memory_segments()
                            .to_vec();
                        xcp_slave_guard.set_memory_segments(
                            segments,
                            Some(std::path::PathBuf::from(&device.a2l_path)),
                        );
                        if let Err(e) = xcp_slave_guard.update_segment_page_state().await {
                            tracing::warn!(
                                "Failed to query page state for {}: {:?}",
                                device.name,
                                e
                            );
                        }
                    }
                }

                // Get slave info from XcpClient
                if let Some(xcp_slave) = client.get_slave(slave_id) {
                    let guard = xcp_slave.lock().await;
                    let info = guard.info.clone();

                    // Query live DAQ events from slave; takes precedence over A2L events
                    let live_events_opt = if let Ok(daq_info) = guard.get_daq_processor_info().await
                    {
                        if daq_info.max_event_channel > 0 {
                            guard.get_daq_events(daq_info.max_event_channel).await.ok()
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    drop(guard);

                    if let Some(live_events) = live_events_opt {
                        if !live_events.is_empty() {
                            let mut daq_mgr = device.daq_manager.write().await;
                            daq_mgr.set_available_events(live_events);
                            tracing::info!(
                                "Loaded {} live DAQ events from slave '{}'",
                                daq_mgr.get_available_events().len(),
                                device.name
                            );
                        }
                    }

                    // Create SlaveState using device's cal_manager and daq_manager
                    let slave_state = SlaveState {
                        id: slave_id,
                        info,
                        max_cto: device.max_cto,
                        cal_manager: device.cal_manager.clone(),
                        daq_manager: device.daq_manager.clone(),
                        cal_values: device.cal_values.clone(),
                        daq_running: device.daq_running.clone(),
                        a2l_path: Arc::new(RwLock::new(device.a2l_path.clone())),
                        cal_original_values: Arc::new(RwLock::new(HashMap::new())),
                    };
                    state.add_slave_state(slave_state).await;

                    // Update device connection status
                    state
                        .update_device_connection(&device.id, true, Some(slave_id))
                        .await;

                    // If this is the first connection, set it as current
                    if state.current_slave().await.is_none() {
                        state.set_current_slave(Some(slave_id)).await;
                    }

                    connected_count += 1;
                } else {
                    failed_devices.push(format!("{}: failed to get slave info", device.name));
                }
            }
            Err(e) => {
                failed_devices.push(format!("{}: {}", device.name, e));
            }
        }
    }

    let message = if connected_count > 0 {
        if failed_devices.is_empty() {
            format!("Connected to {} device(s)", connected_count)
        } else {
            format!(
                "Connected to {} device(s). Failed: {}",
                connected_count,
                failed_devices.join(", ")
            )
        }
    } else if failed_devices.is_empty() {
        "All devices already connected or no devices available".to_string()
    } else {
        format!("Failed to connect: {}", failed_devices.join(", "))
    };

    Json(serde_json::json!({
        "success": failed_devices.is_empty() || connected_count > 0,
        "message": message,
        "connected": connected_count,
    }))
}

/// Connect single device API (POST /api/devices/connect)
pub async fn connect_device(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let device_id = payload
        .get("device_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let requested_transport = match parse_transport_type(
        payload
            .get("transport_protocol")
            .and_then(|value| value.as_str()),
    ) {
        Ok(transport) => transport,
        Err(error) => {
            return Json(serde_json::json!({
                "success": false,
                "error": error,
            }));
        }
    };

    let device = match state.get_device(device_id).await {
        Some(d) => d,
        None => {
            return Json(serde_json::json!({
                "success": false,
                "error": "Device not found"
            }));
        }
    };

    if device.connected {
        return Json(serde_json::json!({
            "success": false,
            "error": "Device already connected"
        }));
    }

    let resolved_transport = match device.resolve_transport_config(requested_transport) {
        Ok(config) => config,
        Err(error) => {
            return Json(serde_json::json!({
                "success": false,
                "error": error,
            }));
        }
    };
    let transport_config = resolved_transport.to_transport_config();

    let mut client = state.client.lock().await;

    match client.connect_slave(&device.name, transport_config).await {
        Ok(slave_id) => {
            // Copy pre-parsed memory segments to XcpSlave (no re-parsing)
            if !device.a2l_path.is_empty() {
                if let Some(xcp_slave) = client.get_slave(slave_id) {
                    let mut xcp_slave_guard = xcp_slave.lock().await;
                    let segments = device
                        .cal_manager
                        .read()
                        .await
                        .get_memory_segments()
                        .to_vec();
                    xcp_slave_guard.set_memory_segments(
                        segments,
                        Some(std::path::PathBuf::from(&device.a2l_path)),
                    );
                    if let Err(e) = xcp_slave_guard.update_segment_page_state().await {
                        tracing::warn!("Failed to query page state for {}: {:?}", device.name, e);
                    }
                }
            }

            // Get slave info from XcpClient and query live DAQ events
            let xcp_slave_arc = match client.get_slave(slave_id) {
                Some(s) => s,
                None => {
                    return Json(serde_json::json!({
                        "success": false,
                        "error": "Failed to get slave info after connection",
                    }))
                }
            };
            let info = xcp_slave_arc.lock().await.info.clone();

            // Query live DAQ events from slave; takes precedence over A2L events
            {
                let slave_guard = xcp_slave_arc.lock().await;
                let live_events_opt =
                    if let Ok(daq_info) = slave_guard.get_daq_processor_info().await {
                        if daq_info.max_event_channel > 0 {
                            slave_guard
                                .get_daq_events(daq_info.max_event_channel)
                                .await
                                .ok()
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                drop(slave_guard);
                if let Some(live_events) = live_events_opt {
                    if !live_events.is_empty() {
                        let mut daq_mgr = device.daq_manager.write().await;
                        daq_mgr.set_available_events(live_events);
                        tracing::info!(
                            "Loaded {} live DAQ events from slave '{}'",
                            daq_mgr.get_available_events().len(),
                            device.name
                        );
                    }
                }
            }

            // Create SlaveState using device's cal_manager and daq_manager
            let slave_state = SlaveState {
                id: slave_id,
                info: info.clone(),
                max_cto: device.max_cto,
                cal_manager: device.cal_manager.clone(),
                daq_manager: device.daq_manager.clone(),
                cal_values: device.cal_values.clone(),
                daq_running: device.daq_running.clone(),
                a2l_path: Arc::new(RwLock::new(device.a2l_path.clone())),
                cal_original_values: Arc::new(RwLock::new(HashMap::new())),
            };
            state.add_slave_state(slave_state).await;

            // Update device connection status
            state
                .update_device_connection(&device.id, true, Some(slave_id))
                .await;

            // If this is the first connection, set it as current
            if state.current_slave().await.is_none() {
                state.set_current_slave(Some(slave_id)).await;
            }

            Json(serde_json::json!({
                "success": true,
                "message": format!("Connected to {} via {}", device.name, display_device_address(&resolved_transport)),
            }))
        }
        Err(e) => Json(serde_json::json!({
            "success": false,
            "error": format!("Failed to connect: {}", e),
        })),
    }
}

/// Disconnect device API (POST /api/devices/disconnect)
pub async fn disconnect_device(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let device_id = payload
        .get("device_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let device = match state.get_device(device_id).await {
        Some(d) => d,
        None => {
            return Json(serde_json::json!({
                "success": false,
                "error": "Device not found"
            }));
        }
    };

    if !device.connected {
        return Json(serde_json::json!({
            "success": false,
            "error": "Device not connected"
        }));
    }

    if let Some(slave_id) = device.slave_id {
        let mut client = state.client.lock().await;
        if let Err(e) = client.disconnect_slave(slave_id).await {
            return Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to disconnect: {}", e),
            }));
        }
        // Remove SlaveState
        state.remove_slave_state(slave_id).await;

        // Clear current_slave if it was this device
        let current_id = *state.current_slave_id.read().await;
        if current_id == Some(slave_id) {
            // Find another connected slave or set to None
            let remaining = state.all_slaves().await;
            if remaining.is_empty() {
                state.set_current_slave(None).await;
            } else {
                state.set_current_slave(Some(remaining[0].id)).await;
            }
        }
    }

    // Update device connection status
    state
        .update_device_connection(&device.id, false, None)
        .await;

    Json(serde_json::json!({
        "success": true,
        "message": format!("Disconnected from {}", device.name),
    }))
}

/// Remove device API (POST /api/devices/remove)
pub async fn remove_device(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let device_id = payload
        .get("device_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // First disconnect if connected
    let device = match state.get_device(device_id).await {
        Some(d) => d,
        None => {
            return Json(serde_json::json!({
                "success": false,
                "error": "Device not found"
            }));
        }
    };

    if device.connected {
        if let Some(slave_id) = device.slave_id {
            let mut client = state.client.lock().await;
            let _ = client.disconnect_slave(slave_id).await;
            // Remove SlaveState
            state.remove_slave_state(slave_id).await;
        }
    }

    // Remove device
    state.remove_device(device_id).await;

    Json(serde_json::json!({
        "success": true,
        "message": "Device removed",
    }))
}

/// Add new device entry by name (POST /api/devices/add)
pub async fn add_device_entry(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let name = payload
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    if name.is_empty() {
        return Json(serde_json::json!({
            "success": false,
            "error": "Name cannot be empty"
        }));
    }

    let device_id = uuid::Uuid::new_v4().to_string();
    let device = crate::web::state::DeviceState::new_empty(device_id.clone(), name.clone());
    state.add_device(device).await;

    Json(serde_json::json!({
        "success": true,
        "device_id": device_id,
        "message": format!("Added slave '{}'", name),
    }))
}

/// Rename a device entry (POST /api/devices/rename)
pub async fn rename_device(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let device_id = payload
        .get("device_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let new_name = payload
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    if new_name.is_empty() {
        return Json(serde_json::json!({
            "success": false,
            "error": "Name cannot be empty"
        }));
    }

    if state.get_device(device_id).await.is_none() {
        return Json(serde_json::json!({
            "success": false,
            "error": "Device not found"
        }));
    }

    state.update_device_name(device_id, &new_name).await;

    Json(serde_json::json!({
        "success": true,
        "message": format!("Renamed to '{}'", new_name),
    }))
}

/// List all devices as JSON (GET /api/devices/list)
pub async fn list_devices(State(state): State<AppState>) -> impl IntoResponse {
    let devices = state.all_devices().await;
    let mut device_list: Vec<serde_json::Value> = Vec::new();

    for d in &devices {
        let hex_filename = d.hex_filename.read().await.clone();
        let checksum_report = d.checksum_report.read().await;
        let checksum_status = if let Some(ref report) = *checksum_report {
            if report.all_passed() {
                "passed"
            } else {
                "failed"
            }
        } else {
            "not_run"
        };
        drop(checksum_report);
        let cal_count = d.cal_manager.read().await.params().len();
        let meas_count = d.daq_manager.read().await.available_count();

        device_list.push(serde_json::json!({
            "id": d.id,
            "name": d.name,
            "address": d.address,
            "a2l_loaded": !d.a2l_path.is_empty(),
            "a2l_name": d.original_filename,
            "hex_loaded": hex_filename.is_some(),
            "hex_name": hex_filename,
            "connected": d.connected,
            "checksum_status": checksum_status,
            "cal_count": cal_count,
            "meas_count": meas_count,
            "transport_options": build_transport_options(&d),
        }));
    }

    Json(serde_json::json!({
        "success": true,
        "devices": device_list,
    }))
}

/// Slave Manage page (GET /slaves)
pub async fn slaves_page(State(state): State<AppState>) -> impl IntoResponse {
    // Guard: cannot change slave config while DAQ is running
    if state.is_any_daq_running().await {
        state
            .set_error(
                "Cannot manage slaves while measurement is running. Stop DAQ first.".to_string(),
            )
            .await;
        return Redirect::to("/").into_response();
    }

    let status = state.take_status().await;
    let error = state.take_error().await;

    let content = templates::slave_manage_page();
    Html(templates::page(
        "Slaves",
        &content,
        status.as_deref(),
        error.as_deref(),
    ))
    .into_response()
}

/// Calibration page (GET /calibration)
pub async fn calibration_page(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Html<String> {
    // Get status/error from URL parameters (from cal_read redirect)
    let url_status = params.get("status").cloned();
    let url_error = params.get("error").cloned();

    // If URL has status/error, use it; otherwise use state
    let status = if url_status.is_some() {
        url_status
    } else {
        state.take_status().await
    };
    let error = if url_error.is_some() {
        url_error
    } else {
        state.take_error().await
    };

    // Render empty calibration page - variables loaded via API
    let content = templates::calibration_page(&[], 0);
    Html(templates::page(
        "Calibration",
        &content,
        status.as_deref(),
        error.as_deref(),
    ))
}

/// Get available calibration variables (GET /api/cal/available)
pub async fn cal_available(State(state): State<AppState>) -> impl IntoResponse {
    let mut variables: Vec<serde_json::Value> = Vec::new();

    // First check connected slaves
    let slaves = state.all_slaves().await;
    for slave in &slaves {
        let cal_manager = slave.cal_manager.read().await;
        for (i, p) in cal_manager.params().iter().enumerate() {
            variables.push(serialize_cal_param(
                p,
                &slave.id.to_string(),
                &slave.display_name(),
                i,
                slave.is_connected(),
            ));
        }
    }

    // Also check devices (for offline A2L data)
    let devices = state.all_devices().await;
    for device in devices {
        // Skip if this device's slave is already in the list
        if let Some(slave_id) = device.slave_id {
            if slaves.iter().any(|s| s.id == slave_id) {
                continue;
            }
        }

        let cal_mgr = device.cal_manager.read().await;
        for (i, p) in cal_mgr.params().iter().enumerate() {
            variables.push(serialize_cal_param(
                p,
                &device.id,
                &device.name,
                i,
                device.connected,
            ));
        }
    }

    Json(json!({
        "success": true,
        "variables": variables,
    }))
}

/// Read calibration parameter (GET /api/cal/read/:device_id/:idx)
pub async fn cal_read(
    State(state): State<AppState>,
    Path((device_id_str, idx)): Path<(String, usize)>,
) -> impl IntoResponse {
    // Try to parse as UUID (could be slave_id or device_id)
    let uuid = match uuid::Uuid::parse_str(&device_id_str) {
        Ok(id) => id,
        Err(_) => {
            return Json(json!({
                "success": false,
                "error": "Invalid device ID"
            }));
        }
    };

    // First try to find as SlaveId
    let slave_id = SlaveId(uuid);
    let slave = state.get_slave(slave_id).await;

    // If not found as slave, try to find device and check if connected
    let Some(slave) = slave else {
        // Check if it's a device ID
        if let Some(device) = state.get_device(&device_id_str).await {
            if !device.connected {
                return Json(json!({
                    "success": false,
                    "error": "Device not connected. Please connect first."
                }));
            }
            // Device is connected but slave state not found - internal error
            return Json(json!({
                "success": false,
                "error": "Device connected but slave state missing. Try reconnecting."
            }));
        }
        return Json(json!({
            "success": false,
            "error": "Device not found"
        }));
    };

    if !slave.is_connected() {
        return Json(json!({
            "success": false,
            "error": "Not connected to XCP slave"
        }));
    }

    let client = state.client.lock().await;
    let cal_manager = slave.cal_manager.read().await;

    if let Some(param) = cal_manager.params().get(idx) {
        let param = param.clone();
        drop(cal_manager); // Release lock before async operation

        // Check if we're in working page mode and have cached value
        let page_mode = if let Some(xcp_slave) = client.get_slave(slave_id) {
            let xcp_slave_guard = xcp_slave.lock().await;
            xcp_slave_guard.get_current_page_mode()
        } else {
            crate::slave::PageMode::Reference
        };

        // In working page mode, try to get value from cache first
        if page_mode == crate::slave::PageMode::Working {
            let cal_values = slave.cal_values.read().await;
            if let Some(cached_value) = cal_values.get(&param.name) {
                let value_str = cached_value.format();
                tracing::info!(
                    "Cache hit for '{}' in working page mode: {}",
                    param.name,
                    value_str
                );

                // Build response using parameter's to_json for base metadata
                let mut base = param.to_json("", &slave.display_name(), 0, true);
                if let serde_json::Value::Object(ref mut map) = base {
                    map.remove("device_id");
                    map.remove("idx");
                    map.insert("success".to_string(), serde_json::json!(true));
                    map.insert("value".to_string(), serde_json::json!(value_str));
                    map.insert("cached".to_string(), serde_json::json!(true));
                    // Add enum info for scalar values
                    let is_enum = param.is_enum();
                    let enum_options: Vec<serde_json::Value> = param
                        .enum_options()
                        .map(|opts| {
                            opts.iter()
                                .map(|opt| {
                                    json!({
                                        "raw": opt.raw_value,
                                        "text": opt.text_value,
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    map.insert("is_enum".to_string(), serde_json::json!(is_enum));
                    map.insert("enum_options".to_string(), serde_json::json!(enum_options));
                }
                return Json(base);
            }
        }

        tracing::info!(
            "Reading calibration param '{}' at address 0x{:08X} (ext: {}), type: {:?}",
            param.name,
            param.address,
            param.address_ext,
            param.data_type
        );

        // Get the XcpSlave handle for this slave
        if let Some(xcp_slave) = client.get_slave(slave_id) {
            // Read using the slave's built-in methods
            // Uses slave's actual maxCTO from CONNECT response for proper chunking
            let result = {
                let xcp_slave_guard = xcp_slave.lock().await;
                read_calibration_from_slave(&xcp_slave_guard, &param).await
            };

            match result {
                Ok(value) => {
                    let value_str = value.format();
                    tracing::info!("Successfully read {} = {}", param.name, value_str);
                    slave
                        .cal_values
                        .write()
                        .await
                        .insert(param.name.clone(), value.clone());

                    // Build response using parameter's to_json for base metadata
                    let mut base = param.to_json("", &slave.display_name(), 0, true);
                    if let serde_json::Value::Object(ref mut map) = base {
                        map.remove("device_id");
                        map.remove("idx");
                        map.insert("success".to_string(), serde_json::json!(true));
                        map.insert("value".to_string(), serde_json::json!(value_str));
                        // Add enum info for scalar values
                        let is_enum = param.is_enum();
                        let enum_options: Vec<serde_json::Value> = param
                            .enum_options()
                            .map(|opts| {
                                opts.iter()
                                    .map(|opt| {
                                        json!({
                                            "raw": opt.raw_value,
                                            "text": opt.text_value,
                                        })
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        map.insert("is_enum".to_string(), serde_json::json!(is_enum));
                        map.insert("enum_options".to_string(), serde_json::json!(enum_options));
                    }
                    return Json(base);
                }
                Err(e) => {
                    tracing::error!("Failed to read {}: {}", param.name, e);
                    return Json(json!({
                        "success": false,
                        "error": format!("Failed to read {}: {}", param.name, e)
                    }));
                }
            }
        } else {
            tracing::error!("XcpSlave not found in client for slave_id: {:?}", slave_id);
            return Json(json!({
                "success": false,
                "error": "Slave not available in client"
            }));
        }
    } else {
        tracing::error!("Calibration parameter index {} not found", idx);
        return Json(json!({
            "success": false,
            "error": format!("Parameter index {} not found", idx)
        }));
    }
}

/// Helper function to read calibration from a slave
async fn read_calibration_from_slave(
    slave: &crate::slave::XcpSlave,
    param: &crate::calibration::CalibrationParam,
) -> Result<CalibrationValue, String> {
    use crate::protocol::Mta;

    let mta = Mta::new(param.address, param.address_ext);
    let size = param.data_type.size();

    tracing::trace!(
        "Reading {} bytes from address 0x{:08X} (ext: {})",
        size,
        mta.address,
        mta.extension
    );

    // Check for zero size (invalid matrix_dim issue)
    if size == 0 {
        return Err(format!(
            "Invalid parameter size (0 bytes). Check A2L matrix_dim for '{}'",
            param.name
        ));
    }

    // Use slave's actual maxCTO from CONNECT response (not A2L's MAX_CTO)
    // This ensures we use the correct chunk size for UPLOAD operations
    let data = slave
        .read_large_data(mta, size, None)
        .await
        .map_err(|e| e.to_string())?;

    tracing::trace!("Read {} bytes, expected {}", data.len(), size);

    // Verify data length
    if data.len() != size {
        return Err(format!(
            "Data length mismatch: expected {} bytes, got {}",
            size,
            data.len()
        ));
    }

    bytes_to_calibration_value(&data, &param.data_type)
}

/// Convert raw bytes to a CalibrationValue based on the expected data type.
/// Shared by the XCP-slave path and the HEX-file path.
fn bytes_to_calibration_value(
    data: &[u8],
    cal_type: &CalibrationType,
) -> Result<CalibrationValue, String> {
    use crate::calibration::CalibrationValue;
    let value = match cal_type {
        // Scalar types
        CalibrationType::U8 => CalibrationValue::U8(data[0]),
        CalibrationType::U16 => CalibrationValue::U16(u16::from_le_bytes([data[0], data[1]])),
        CalibrationType::U32 => {
            CalibrationValue::U32(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
        }
        CalibrationType::U64 => CalibrationValue::U64(u64::from_le_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ])),
        CalibrationType::I8 => CalibrationValue::I8(data[0] as i8),
        CalibrationType::I16 => CalibrationValue::I16(i16::from_le_bytes([data[0], data[1]])),
        CalibrationType::I32 => {
            CalibrationValue::I32(i32::from_le_bytes([data[0], data[1], data[2], data[3]]))
        }
        CalibrationType::I64 => CalibrationValue::I64(i64::from_le_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ])),
        CalibrationType::F32 => {
            CalibrationValue::F32(f32::from_le_bytes([data[0], data[1], data[2], data[3]]))
        }
        CalibrationType::F64 => CalibrationValue::F64(f64::from_le_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ])),
        CalibrationType::Bool => CalibrationValue::Bool(data[0] != 0),

        // Array types
        CalibrationType::U8Array(_) => CalibrationValue::U8Array(data.to_vec()),
        CalibrationType::U16Array(_) => CalibrationValue::U16Array(
            data.chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect(),
        ),
        CalibrationType::U32Array(_) => CalibrationValue::U32Array(
            data.chunks_exact(4)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect(),
        ),
        CalibrationType::U64Array(_) => CalibrationValue::U64Array(
            data.chunks_exact(8)
                .map(|c| u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
                .collect(),
        ),
        CalibrationType::I8Array(_) => {
            CalibrationValue::I8Array(data.iter().map(|&v| v as i8).collect())
        }
        CalibrationType::I16Array(_) => CalibrationValue::I16Array(
            data.chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect(),
        ),
        CalibrationType::I32Array(_) => CalibrationValue::I32Array(
            data.chunks_exact(4)
                .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect(),
        ),
        CalibrationType::I64Array(_) => CalibrationValue::I64Array(
            data.chunks_exact(8)
                .map(|c| i64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
                .collect(),
        ),
        CalibrationType::F32Array(_) => CalibrationValue::F32Array(
            data.chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect(),
        ),
        CalibrationType::F64Array(_) => CalibrationValue::F64Array(
            data.chunks_exact(8)
                .map(|c| f64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
                .collect(),
        ),
        CalibrationType::BoolArray(_) => {
            CalibrationValue::BoolArray(data.iter().map(|&v| v != 0).collect())
        }

        // Matrix types - row-major
        CalibrationType::U8Matrix(_, cols) => CalibrationValue::U8Matrix(
            data.chunks_exact(*cols).map(|c| c.to_vec()).collect(),
        ),
        CalibrationType::U16Matrix(_, cols) => CalibrationValue::U16Matrix(
            data.chunks_exact(cols * 2)
                .map(|row| {
                    row.chunks_exact(2)
                        .map(|c| u16::from_le_bytes([c[0], c[1]]))
                        .collect()
                })
                .collect(),
        ),
        CalibrationType::U32Matrix(_, cols) => CalibrationValue::U32Matrix(
            data.chunks_exact(cols * 4)
                .map(|row| {
                    row.chunks_exact(4)
                        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                        .collect()
                })
                .collect(),
        ),
        CalibrationType::U64Matrix(_, cols) => CalibrationValue::U64Matrix(
            data.chunks_exact(cols * 8)
                .map(|row| {
                    row.chunks_exact(8)
                        .map(|c| {
                            u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]])
                        })
                        .collect()
                })
                .collect(),
        ),
        CalibrationType::I8Matrix(_, cols) => CalibrationValue::I8Matrix(
            data.chunks_exact(*cols)
                .map(|row| row.iter().map(|&v| v as i8).collect())
                .collect(),
        ),
        CalibrationType::I16Matrix(_, cols) => CalibrationValue::I16Matrix(
            data.chunks_exact(cols * 2)
                .map(|row| {
                    row.chunks_exact(2)
                        .map(|c| i16::from_le_bytes([c[0], c[1]]))
                        .collect()
                })
                .collect(),
        ),
        CalibrationType::I32Matrix(_, cols) => CalibrationValue::I32Matrix(
            data.chunks_exact(cols * 4)
                .map(|row| {
                    row.chunks_exact(4)
                        .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                        .collect()
                })
                .collect(),
        ),
        CalibrationType::I64Matrix(_, cols) => CalibrationValue::I64Matrix(
            data.chunks_exact(cols * 8)
                .map(|row| {
                    row.chunks_exact(8)
                        .map(|c| {
                            i64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]])
                        })
                        .collect()
                })
                .collect(),
        ),
        CalibrationType::F32Matrix(_, cols) => CalibrationValue::F32Matrix(
            data.chunks_exact(cols * 4)
                .map(|row| {
                    row.chunks_exact(4)
                        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                        .collect()
                })
                .collect(),
        ),
        CalibrationType::F64Matrix(_, cols) => CalibrationValue::F64Matrix(
            data.chunks_exact(cols * 8)
                .map(|row| {
                    row.chunks_exact(8)
                        .map(|c| {
                            f64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]])
                        })
                        .collect()
                })
                .collect(),
        ),
        CalibrationType::BoolMatrix(_, cols) => CalibrationValue::BoolMatrix(
            data.chunks_exact(*cols)
                .map(|row| row.iter().map(|&v| v != 0).collect())
                .collect(),
        ),
    };
    Ok(value)
}

/// Read a calibration parameter value from a HEX image.
fn read_calibration_from_hex(
    hex: &crate::hex::HexImage,
    param: &CalibrationParam,
) -> Result<CalibrationValue, String> {
    let size = param.data_type.size();
    if size == 0 {
        return Err(format!(
            "Invalid parameter size (0 bytes) for '{}'",
            param.name
        ));
    }
    let data = hex.extract_range(param.address, size as u32, 0xFF);
    bytes_to_calibration_value(&data, &param.data_type)
}

/// Read axis values from a HEX image (mirrors read_axis_values_from_slave).
fn read_axis_values_from_hex(
    hex: &crate::hex::HexImage,
    param: &CalibrationParam,
) -> Vec<Vec<f64>> {
    use crate::a2l::AxisPointSource;

    let mut result = Vec::new();
    for axis in &param.axis_info {
        if let Some(values) = axis.get_static_values() {
            result.push(values);
            continue;
        }
        if let AxisPointSource::Memory {
            address,
            data_type,
            num_points,
            ..
        } = &axis.point_source
        {
            let element_size = match data_type {
                crate::a2l::A2lDataType::U8
                | crate::a2l::A2lDataType::I8
                | crate::a2l::A2lDataType::Bool => 1,
                crate::a2l::A2lDataType::U16 | crate::a2l::A2lDataType::I16 => 2,
                crate::a2l::A2lDataType::U32
                | crate::a2l::A2lDataType::I32
                | crate::a2l::A2lDataType::F32 => 4,
                crate::a2l::A2lDataType::U64
                | crate::a2l::A2lDataType::I64
                | crate::a2l::A2lDataType::F64 => 8,
                _ => 4,
            };
            let total_size = (element_size * num_points) as u32;
            let data = hex.extract_range(*address, total_size, 0xFF);
            let values = parse_axis_values(&data, data_type, *num_points);
            result.push(values);
        } else {
            result.push((0..axis.num_points).map(|i| i as f64).collect());
        }
    }
    result
}

/// Write calibration page (GET /cal/write/:device_id/:idx)
pub async fn cal_write_page(
    State(state): State<AppState>,
    Path((device_id_str, idx)): Path<(String, usize)>,
) -> Html<String> {
    let uuid = match uuid::Uuid::parse_str(&device_id_str) {
        Ok(id) => id,
        Err(_) => {
            return Html(templates::page(
                "Error",
                r#"<p>Invalid device ID</p><a href="/calibration">Back</a>"#,
                None,
                Some("Invalid device ID format"),
            ));
        }
    };

    let slave_id = SlaveId(uuid);
    let Some(slave) = state.get_slave(slave_id).await else {
        return Html(templates::page(
            "Error",
            r#"<p>Device not found or not connected</p><a href="/calibration">Back</a>"#,
            None,
            Some("Device not found"),
        ));
    };

    let cal_manager = slave.cal_manager.read().await;
    let cal_values = slave.cal_values.read().await;

    if let Some(param) = cal_manager.params().get(idx) {
        let current = cal_values.get(&param.name).map(|v| v.format());
        let is_enum = param.is_enum();
        let enum_options: Vec<(i64, String)> = param
            .enum_options()
            .map(|opts| {
                opts.iter()
                    .map(|opt| (opt.raw_value, opt.text_value.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let content = templates::cal_write_page(
            &device_id_str,
            idx,
            &param.name,
            current.as_deref(),
            is_enum,
            &enum_options,
        );
        Html(templates::page("Write Calibration", &content, None, None))
    } else {
        Html(templates::page(
            "Error",
            r#"<p>Parameter not found</p><a href="/calibration">Back</a>"#,
            None,
            Some("Invalid parameter index"),
        ))
    }
}

/// Write calibration API (POST /api/cal/write/:device_id/:idx)
pub async fn cal_write_api(
    State(state): State<AppState>,
    Path((device_id_str, idx)): Path<(String, usize)>,
    Form(form): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    let uuid = match uuid::Uuid::parse_str(&device_id_str) {
        Ok(id) => id,
        Err(_) => {
            state.set_error("Invalid device ID".to_string()).await;
            return Redirect::to("/calibration");
        }
    };

    let slave_id = SlaveId(uuid);
    let Some(slave) = state.get_slave(slave_id).await else {
        state
            .set_error("Device not found or not connected".to_string())
            .await;
        return Redirect::to("/calibration");
    };

    if !slave.is_connected() {
        state
            .set_error("Not connected to XCP slave".to_string())
            .await;
        return Redirect::to("/calibration");
    }

    let value_str = form.get("value").cloned().unwrap_or_default();

    let cal_manager = slave.cal_manager.read().await;

    if let Some(param) = cal_manager.params().get(idx) {
        let param = param.clone();
        drop(cal_manager);

        let cal_manager = CalibrationManager::new();

        match cal_manager.parse_value(&param, &value_str) {
            Ok(value) => {
                let client = state.client.lock().await;

                // Get the XcpSlave handle for this slave
                if let Some(xcp_slave) = client.get_slave(slave_id) {
                    // Write using the slave's built-in methods
                    let result = async {
                        let xcp_slave_guard = xcp_slave.lock().await;
                        write_calibration_to_slave(&xcp_slave_guard, &param, &value).await
                    }
                    .await;

                    match result {
                        Ok(_) => {
                            // Update cache after successful write
                            slave
                                .cal_values
                                .write()
                                .await
                                .insert(param.name.clone(), value.clone());
                            state
                                .set_status(format!(
                                    "[{}] Wrote {} = {}",
                                    slave.display_name(),
                                    param.name,
                                    value_str
                                ))
                                .await;
                        }
                        Err(e) => {
                            state
                                .set_error(format!("Failed to write {}: {}", param.name, e))
                                .await;
                        }
                    }
                } else {
                    state.set_error("Slave not available".to_string()).await;
                }
            }
            Err(e) => {
                state
                    .set_error(format!("Invalid value for {}: {}", param.name, e))
                    .await;
            }
        }
    }

    Redirect::to("/calibration")
}

/// JSON calibration write API (POST /api/experiment/cal/write/:device_id/:idx)
/// Used by the experiment page which expects JSON responses.
pub async fn cal_write_json_api(
    State(state): State<AppState>,
    Path((device_id_str, idx)): Path<(String, usize)>,
    Form(form): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    let uuid = match uuid::Uuid::parse_str(&device_id_str) {
        Ok(id) => id,
        Err(_) => {
            return Json(json!({ "success": false, "error": "Invalid device ID" }));
        }
    };

    let slave_id = SlaveId(uuid);
    let Some(slave) = state.get_slave(slave_id).await else {
        return Json(json!({ "success": false, "error": "Device not found or not connected" }));
    };

    if !slave.is_connected() {
        return Json(json!({ "success": false, "error": "Not connected to XCP slave" }));
    }

    let value_str = form.get("value").cloned().unwrap_or_default();
    let cal_manager = slave.cal_manager.read().await;

    let Some(param) = cal_manager.params().get(idx).cloned() else {
        return Json(json!({ "success": false, "error": "Parameter not found" }));
    };
    drop(cal_manager);

    let parse_mgr = CalibrationManager::new();
    let value = match parse_mgr.parse_value(&param, &value_str) {
        Ok(v) => v,
        Err(e) => {
            return Json(
                json!({ "success": false, "error": format!("Invalid value: {}", e) }),
            );
        }
    };

    let client = state.client.lock().await;
    if let Some(xcp_slave) = client.get_slave(slave_id) {
        let xcp_slave_guard = xcp_slave.lock().await;
        match write_calibration_to_slave(&xcp_slave_guard, &param, &value).await {
            Ok(_) => {
                slave
                    .cal_values
                    .write()
                    .await
                    .insert(param.name.clone(), value);
                Json(json!({ "success": true, "message": format!("Wrote {} = {}", param.name, value_str) }))
            }
            Err(e) => {
                Json(json!({ "success": false, "error": format!("Write failed: {}", e) }))
            }
        }
    } else {
        Json(json!({ "success": false, "error": "Slave not available" }))
    }
}

/// Get or capture the original (reference) calibration value for change tracking
/// (GET /api/experiment/cal/original/:device_id/:idx)
///
/// On first call for a given parameter, reads the value from the slave and stores it.
/// Subsequent calls return the stored original without re-reading.
pub async fn cal_original_api(
    State(state): State<AppState>,
    Path((device_id_str, idx)): Path<(String, usize)>,
) -> impl IntoResponse {
    let uuid = match uuid::Uuid::parse_str(&device_id_str) {
        Ok(id) => id,
        Err(_) => return Json(json!({ "success": false, "error": "Invalid device ID" })),
    };

    let slave_id = SlaveId(uuid);
    let slave = state.get_slave(slave_id).await;
    let is_connected = slave.as_ref().map_or(false, |s| s.is_connected());

    if !is_connected {
        // Not connected — use HEX as the reference/original value
        let devices = state.devices.read().await;
        let device = devices.iter().find(|d| {
            d.id == device_id_str
                || d.slave_id.map(|s| s.to_string()) == Some(device_id_str.to_string())
        });
        let Some(device) = device else {
            return Json(json!({ "success": false, "error": "Device not found" }));
        };
        let cal_manager = device.cal_manager.read().await;
        let Some(param) = cal_manager.params().get(idx).cloned() else {
            return Json(json!({ "success": false, "error": "Parameter not found" }));
        };
        drop(cal_manager);
        let hex_guard = device.hex_image.read().await;
        let Some(ref hex) = *hex_guard else {
            return Json(json!({ "success": false, "no_hex": true }));
        };
        return match read_calibration_from_hex(hex, &param) {
            Ok(value) => Json(json!({ "success": true, "value": value.format_scalar_str() })),
            Err(e) => {
                Json(json!({ "success": false, "error": format!("HEX read failed: {}", e) }))
            }
        };
    }

    let slave = slave.unwrap();

    // Return cached original if already stored
    {
        let originals = slave.cal_original_values.read().await;
        if let Some(orig) = originals.get(&idx) {
            return Json(json!({ "success": true, "value": orig.clone() }));
        }
    }

    // Not yet stored — read current value from slave and remember it as the original
    let cal_manager = slave.cal_manager.read().await;
    let Some(param) = cal_manager.params().get(idx).cloned() else {
        return Json(json!({ "success": false, "error": "Parameter not found" }));
    };
    drop(cal_manager);

    let client = state.client.lock().await;
    let Some(xcp_slave) = client.get_slave(slave_id) else {
        return Json(json!({ "success": false, "error": "Slave not available" }));
    };
    let xcp_slave_guard = xcp_slave.lock().await;
    match read_calibration_from_slave(&xcp_slave_guard, &param).await {
        Ok(value) => {
            let formatted = value.format_scalar_str();
            drop(xcp_slave_guard);
            slave
                .cal_original_values
                .write()
                .await
                .insert(idx, formatted.clone());
            Json(json!({ "success": true, "value": formatted }))
        }
        Err(e) => Json(json!({ "success": false, "error": format!("Read failed: {}", e) })),
    }
}

/// Helper function to write calibration to a slave
async fn write_calibration_to_slave(
    slave: &crate::slave::XcpSlave,
    param: &crate::calibration::CalibrationParam,
    value: &CalibrationValue,
) -> Result<(), String> {
    use crate::calibration::CalibrationType;
    use crate::protocol::Mta;

    tracing::info!(
        "write_calibration_to_slave: param='{}' type={:?} address=0x{:08X}",
        param.name,
        param.data_type,
        param.address
    );

    let mta = Mta::new(param.address, param.address_ext);

    // Convert value to bytes
    let data: Vec<u8> = match (value, &param.data_type) {
        // Scalar types
        (CalibrationValue::U8(v), CalibrationType::U8) => vec![*v],
        (CalibrationValue::U16(v), CalibrationType::U16) => v.to_le_bytes().to_vec(),
        (CalibrationValue::U32(v), CalibrationType::U32) => v.to_le_bytes().to_vec(),
        (CalibrationValue::U64(v), CalibrationType::U64) => v.to_le_bytes().to_vec(),
        (CalibrationValue::I8(v), CalibrationType::I8) => vec![*v as u8],
        (CalibrationValue::I16(v), CalibrationType::I16) => v.to_le_bytes().to_vec(),
        (CalibrationValue::I32(v), CalibrationType::I32) => v.to_le_bytes().to_vec(),
        (CalibrationValue::I64(v), CalibrationType::I64) => v.to_le_bytes().to_vec(),
        (CalibrationValue::F32(v), CalibrationType::F32) => v.to_le_bytes().to_vec(),
        (CalibrationValue::F64(v), CalibrationType::F64) => v.to_le_bytes().to_vec(),
        (CalibrationValue::Bool(v), CalibrationType::Bool) => vec![if *v { 1 } else { 0 }],
        // 1D Array types
        (CalibrationValue::U8Array(arr), CalibrationType::U8Array(_)) => {
            arr.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::U16Array(arr), CalibrationType::U16Array(_)) => {
            arr.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::U32Array(arr), CalibrationType::U32Array(_)) => {
            arr.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::U64Array(arr), CalibrationType::U64Array(_)) => {
            arr.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::I8Array(arr), CalibrationType::I8Array(_)) => {
            arr.iter().map(|v| *v as u8).collect()
        }
        (CalibrationValue::I16Array(arr), CalibrationType::I16Array(_)) => {
            arr.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::I32Array(arr), CalibrationType::I32Array(_)) => {
            arr.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::I64Array(arr), CalibrationType::I64Array(_)) => {
            arr.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::F32Array(arr), CalibrationType::F32Array(_)) => {
            arr.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::F64Array(arr), CalibrationType::F64Array(_)) => {
            arr.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        (CalibrationValue::BoolArray(arr), CalibrationType::BoolArray(_)) => {
            arr.iter().map(|v| if *v { 1u8 } else { 0u8 }).collect()
        }
        // 2D Matrix types - flatten row by row
        (CalibrationValue::U8Matrix(mat), CalibrationType::U8Matrix(_, _)) => {
            mat.iter().flat_map(|row| row.iter().copied()).collect()
        }
        (CalibrationValue::U16Matrix(mat), CalibrationType::U16Matrix(_, _)) => mat
            .iter()
            .flat_map(|row| row.iter().flat_map(|v| v.to_le_bytes()))
            .collect(),
        (CalibrationValue::U32Matrix(mat), CalibrationType::U32Matrix(_, _)) => mat
            .iter()
            .flat_map(|row| row.iter().flat_map(|v| v.to_le_bytes()))
            .collect(),
        (CalibrationValue::U64Matrix(mat), CalibrationType::U64Matrix(_, _)) => mat
            .iter()
            .flat_map(|row| row.iter().flat_map(|v| v.to_le_bytes()))
            .collect(),
        (CalibrationValue::I8Matrix(mat), CalibrationType::I8Matrix(_, _)) => mat
            .iter()
            .flat_map(|row| row.iter().map(|v| *v as u8))
            .collect(),
        (CalibrationValue::I16Matrix(mat), CalibrationType::I16Matrix(_, _)) => mat
            .iter()
            .flat_map(|row| row.iter().flat_map(|v| v.to_le_bytes()))
            .collect(),
        (CalibrationValue::I32Matrix(mat), CalibrationType::I32Matrix(_, _)) => mat
            .iter()
            .flat_map(|row| row.iter().flat_map(|v| v.to_le_bytes()))
            .collect(),
        (CalibrationValue::I64Matrix(mat), CalibrationType::I64Matrix(_, _)) => mat
            .iter()
            .flat_map(|row| row.iter().flat_map(|v| v.to_le_bytes()))
            .collect(),
        (CalibrationValue::F32Matrix(mat), CalibrationType::F32Matrix(_, _)) => mat
            .iter()
            .flat_map(|row| row.iter().flat_map(|v| v.to_le_bytes()))
            .collect(),
        (CalibrationValue::F64Matrix(mat), CalibrationType::F64Matrix(_, _)) => mat
            .iter()
            .flat_map(|row| row.iter().flat_map(|v| v.to_le_bytes()))
            .collect(),
        (CalibrationValue::BoolMatrix(mat), CalibrationType::BoolMatrix(_, _)) => mat
            .iter()
            .flat_map(|row| row.iter().map(|v| if *v { 1u8 } else { 0u8 }))
            .collect(),
        _ => return Err(format!("Value type mismatch for {:?}", param.data_type)),
    };

    tracing::info!(
        "write_calibration_to_slave: writing {} bytes to 0x{:08X} (ext={})",
        data.len(),
        mta.address,
        mta.extension
    );

    // Write data using sequential DOWNLOAD commands (no DOWNLOAD_NEXT)
    // This is more compatible with slaves that don't support block transfer
    if data.len() <= 255 {
        tracing::info!("write_calibration_to_slave: using write_memory_sequential (small data)");
        slave
            .write_memory_sequential(mta, &data)
            .await
            .map_err(|e| e.to_string())
    } else {
        tracing::info!("write_calibration_to_slave: using write_u8_array_sequential (large data)");
        // Use sequential download for larger writes
        slave
            .write_u8_array_sequential(mta, &data)
            .await
            .map_err(|e| e.to_string())
    }
}

/// Get page mode for all connected devices (GET /api/cal/pagemode)
pub async fn get_page_mode(State(state): State<AppState>) -> impl IntoResponse {
    let mut devices: Vec<serde_json::Value> = Vec::new();

    // Get page mode from connected slaves
    let client = state.client.lock().await;
    let slaves = state.all_slaves().await;

    for slave_state in &slaves {
        if !slave_state.is_connected() {
            continue;
        }

        // Get the page mode from XcpSlave if available
        let page_mode = if let Some(xcp_slave) = client.get_slave(slave_state.id) {
            let xcp_slave_guard = xcp_slave.lock().await;
            xcp_slave_guard.get_current_page_mode()
        } else {
            crate::slave::PageMode::Reference
        };

        devices.push(json!({
            "device_id": slave_state.id.to_string(),
            "device_name": slave_state.display_name(),
            "page_mode": match page_mode {
                crate::slave::PageMode::Working => "working",
                crate::slave::PageMode::Reference => "reference",
            },
            "page_mode_description": page_mode.description(),
        }));
    }

    Json(json!({
        "success": true,
        "devices": devices,
    }))
}

/// Switch page mode for a device (POST /api/cal/pagemode/:device_id)
pub async fn switch_page_mode(
    State(state): State<AppState>,
    Path(device_id_str): Path<String>,
    Json(payload): Json<HashMap<String, String>>,
) -> impl IntoResponse {
    // Get the mode from payload
    let mode_str = payload
        .get("mode")
        .cloned()
        .unwrap_or_else(|| "reference".to_string());
    let mode = match mode_str.as_str() {
        "working" => crate::slave::PageMode::Working,
        "reference" => crate::slave::PageMode::Reference,
        _ => {
            return Json(json!({
                "success": false,
                "error": "Invalid mode. Use 'working' or 'reference'."
            }));
        }
    };

    // Try to parse as UUID
    let uuid = match uuid::Uuid::parse_str(&device_id_str) {
        Ok(id) => id,
        Err(_) => {
            return Json(json!({
                "success": false,
                "error": "Invalid device ID"
            }));
        }
    };

    let slave_id = SlaveId(uuid);

    // Get the XcpSlave and switch pages
    let client = state.client.lock().await;
    if let Some(xcp_slave) = client.get_slave(slave_id) {
        let mut xcp_slave_guard = xcp_slave.lock().await;

        // Check if connected
        if !xcp_slave_guard.info.connected {
            return Json(json!({
                "success": false,
                "error": "Device not connected"
            }));
        }

        // Switch pages
        match xcp_slave_guard.switch_all_pages(mode).await {
            Ok(segments_switched) => {
                let new_mode = xcp_slave_guard.get_current_page_mode();
                Json(json!({
                    "success": true,
                    "message": format!("Switched {} segment(s) to {} mode", segments_switched.len(), new_mode.description()),
                    "page_mode": match new_mode {
                        crate::slave::PageMode::Working => "working",
                        crate::slave::PageMode::Reference => "reference",
                    },
                    "page_mode_description": new_mode.description(),
                    "segments_switched": segments_switched.len(),
                }))
            }
            Err(e) => Json(json!({
                "success": false,
                "error": e.to_string()
            })),
        }
    } else {
        Json(json!({
            "success": false,
            "error": "Device not found"
        }))
    }
}

/// DAQ page (GET /daq)
pub async fn daq_page(State(state): State<AppState>) -> Html<String> {
    let status = state.take_status().await;
    let error = state.take_error().await;

    // Render empty DAQ page - variables loaded via API
    let content = templates::daq_page(
        &[],   // measurements
        &[],   // selected_indices
        false, // daq_running
        &[],   // latest_values
        &[],   // slaves
        None,  // current_slave_id
    );
    Html(templates::page(
        "DAQ",
        &content,
        status.as_deref(),
        error.as_deref(),
    ))
}

/// Get available DAQ measurements (GET /api/daq/available)
pub async fn daq_available(State(state): State<AppState>) -> impl IntoResponse {
    let mut variables: Vec<serde_json::Value> = Vec::new();

    // First check connected slaves
    let slaves = state.all_slaves().await;
    for slave in &slaves {
        let daq_manager = slave.daq_manager.read().await;
        for (i, m) in daq_manager.all_measurements().iter().enumerate() {
            variables.push(json!({
                "device_id": slave.id.to_string(),
                "device_name": slave.display_name(),
                "idx": i,
                "name": m.name,
                "type": format!("{:?}", m.data_type),
                "address_hex": format!("{:08X}", m.address),
                "connected": slave.is_connected(),
            }));
        }
    }

    // Also check devices (for offline A2L data)
    let devices = state.all_devices().await;
    for device in devices {
        // Skip if this device's slave is already in the list
        if let Some(slave_id) = device.slave_id {
            if slaves.iter().any(|s| s.id == slave_id) {
                continue;
            }
        }

        let daq_mgr = device.daq_manager.read().await;
        for (i, m) in daq_mgr.all_measurements().iter().enumerate() {
            variables.push(json!({
                "device_id": device.id.clone(),
                "device_name": device.name.clone(),
                "idx": i,
                "name": m.name,
                "type": format!("{:?}", m.data_type),
                "address_hex": format!("{:08X}", m.address),
                "connected": device.connected,
            }));
        }
    }

    Json(json!({
        "success": true,
        "variables": variables,
    }))
}

/// DAQ start API (GET /api/daq/start)
pub async fn daq_start(State(state): State<AppState>) -> impl IntoResponse {
    let current_slave = state.current_slave().await;

    let Some(slave) = current_slave else {
        return Json(json!({
            "success": false,
            "error": "No slave selected"
        }));
    };

    if !slave.is_connected() {
        return Json(json!({
            "success": false,
            "error": "Not connected to XCP slave"
        }));
    }

    // Clone what we need before the async block
    let slave_name = slave.display_name();
    let slave_id = slave.id;

    let client = state.client.lock().await;
    let mut daq_manager = slave.daq_manager.write().await;

    // Get the actual XcpSlave from client
    let xcp_slave = match client.get_slave(slave_id) {
        Some(s) => s,
        None => {
            return Json(json!({
                "success": false,
                "error": "Slave not found in client"
            }));
        }
    };
    let xcp_slave_guard = xcp_slave.lock().await;

    // Use the XcpSlave to start DAQ (DAQ manager needs XcpSlave)
    match daq_manager.start(&*xcp_slave_guard, 1000).await {
        Ok(_) => {
            *slave.daq_running.write().await = true;
            state
                .set_status(format!("DAQ started on {}", slave_name))
                .await;
            Json(json!({ "success": true }))
        }
        Err(e) => Json(json!({
            "success": false,
            "error": format!("DAQ start failed: {}", e)
        })),
    }
}

/// DAQ stop API (GET /api/daq/stop)
pub async fn daq_stop(State(state): State<AppState>) -> impl IntoResponse {
    let current_slave = state.current_slave().await;

    let Some(slave) = current_slave else {
        return Json(json!({
            "success": false,
            "error": "No slave selected"
        }));
    };

    // Clone what we need before the async block
    let slave_name = slave.display_name();
    let slave_id = slave.id;

    let client = state.client.lock().await;
    let daq_manager = slave.daq_manager.write().await;

    // Get the actual XcpSlave from client
    let xcp_slave = match client.get_slave(slave_id) {
        Some(s) => s,
        None => {
            return Json(json!({
                "success": false,
                "error": "Slave not found in client"
            }));
        }
    };
    let xcp_slave_guard = xcp_slave.lock().await;

    match daq_manager.stop(&*xcp_slave_guard).await {
        Ok(_) => {
            *slave.daq_running.write().await = false;
            state
                .set_status(format!("DAQ stopped on {}", slave_name))
                .await;
            Json(json!({ "success": true }))
        }
        Err(e) => Json(json!({
            "success": false,
            "error": format!("DAQ stop failed: {}", e)
        })),
    }
}

/// DAQ start recording (POST /api/daq/record/start)
///
/// Sends GET_DAQ_CLOCK to all slaves with running DAQ, creates CSV/MF4 files,
/// and begins file recording. Timestamps start from 0.
pub async fn daq_start_recording(State(state): State<AppState>) -> impl IntoResponse {
    let slaves = state.all_slaves().await;
    let client = state.client.lock().await;
    let mut errors = Vec::new();
    let mut started_count = 0;

    for slave in &slaves {
        if !*slave.daq_running.read().await || !slave.is_connected() {
            continue;
        }

        let xcp_slave = match client.get_slave(slave.id) {
            Some(s) => s,
            None => continue,
        };
        let xcp_slave_guard = xcp_slave.lock().await;
        let mut daq_manager = slave.daq_manager.write().await;

        match daq_manager.start_recording(&*xcp_slave_guard).await {
            Ok(_) => {
                started_count += 1;
                tracing::info!("Recording started on slave {}", slave.display_name());
            }
            Err(e) => {
                errors.push(format!("{}: {}", slave.display_name(), e));
            }
        }
    }

    if !errors.is_empty() {
        Json(json!({
            "success": false,
            "error": errors.join("; "),
            "started": started_count
        }))
    } else if started_count == 0 {
        Json(json!({
            "success": false,
            "error": "No running DAQ found to record"
        }))
    } else {
        state
            .set_status(format!("Recording started on {} slave(s)", started_count))
            .await;
        Json(json!({ "success": true, "started": started_count }))
    }
}

/// DAQ stop recording (POST /api/daq/record/stop)
///
/// Stops file recording on all slaves. DAQ streaming continues for live display.
/// Returns success so the frontend can show the download popup.
pub async fn daq_stop_recording(State(state): State<AppState>) -> impl IntoResponse {
    let slaves = state.all_slaves().await;
    let mut errors = Vec::new();
    let mut stopped_count = 0;

    for slave in &slaves {
        let daq_manager = slave.daq_manager.read().await;
        if !daq_manager.is_recording() {
            continue;
        }
        drop(daq_manager);

        let daq_manager = slave.daq_manager.read().await;
        match daq_manager.stop_recording().await {
            Ok(_) => {
                stopped_count += 1;
                tracing::info!("Recording stopped on slave {}", slave.display_name());
            }
            Err(e) => {
                errors.push(format!("{}: {}", slave.display_name(), e));
            }
        }
    }

    if !errors.is_empty() {
        Json(json!({
            "success": false,
            "error": errors.join("; "),
            "stopped": stopped_count
        }))
    } else {
        state
            .set_status(format!("Recording stopped on {} slave(s)", stopped_count))
            .await;
        Json(json!({ "success": true, "stopped": stopped_count }))
    }
}

/// DAQ set selected measurements (POST /api/daq/select)
pub async fn daq_set_selection(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let selections: Vec<(String, usize, bool)> = payload
        .get("selections")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    let device_id = v.get("device_id").and_then(|d| d.as_str()).unwrap_or("");
                    let idx = v.get("idx").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                    let selected = v.get("selected").and_then(|s| s.as_bool()).unwrap_or(false);
                    if !device_id.is_empty() {
                        Some((device_id.to_string(), idx, selected))
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    for (device_id, idx, selected) in selections {
        // Try to parse as UUID first (for connected slaves)
        if let Ok(uuid) = uuid::Uuid::parse_str(&device_id) {
            let slave_id = SlaveId(uuid);
            if let Some(slave) = state.get_slave(slave_id).await {
                let mut daq_manager = slave.daq_manager.write().await;
                let is_currently_selected = daq_manager.selected_indices().contains(&idx);

                if selected && !is_currently_selected {
                    daq_manager.select_measurement(idx);
                } else if !selected && is_currently_selected {
                    daq_manager.deselect_measurement(idx);
                }
            }
        } else {
            // Not a UUID, try to find by device ID
            if let Some(device) = state.get_device(&device_id).await {
                let mut daq_manager = device.daq_manager.write().await;
                let is_currently_selected = daq_manager.selected_indices().contains(&idx);

                if selected && !is_currently_selected {
                    daq_manager.select_measurement(idx);
                } else if !selected && is_currently_selected {
                    daq_manager.deselect_measurement(idx);
                }
            }
        }
    }

    Json(json!({ "success": true }))
}

/// DAQ status API (GET /api/daq/status)
pub async fn daq_status(State(state): State<AppState>) -> impl IntoResponse {
    let current_slave = state.current_slave().await;

    let (running, recording, rec_duration, csv_size, mf4_size) =
        if let Some(ref slave) = current_slave {
            let daq = slave.daq_manager.read().await;
            let is_rec = daq.is_recording();
            let (dur, csv, mf4) = if is_rec {
                daq.recording_stats().await
            } else {
                (0.0, 0, 0)
            };
            (*slave.daq_running.read().await, is_rec, dur, csv, mf4)
        } else {
            (false, false, 0.0, 0, 0)
        };

    Json(json!({
        "running": running,
        "recording": recording,
        "rec_duration_s": rec_duration,
        "csv_size": csv_size,
        "mf4_size": mf4_size
    }))
}

/// DAQ toggle measurement (POST /api/daq/toggle)
pub async fn daq_toggle(
    State(state): State<AppState>,
    Form(form): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    // Support both slave_id and device_id parameters
    let device_id_str = form
        .get("device_id")
        .or_else(|| form.get("slave_id"))
        .cloned();
    let idx_str = form.get("idx").cloned();
    let selected = form.get("selected").map(|s| s == "true");

    if let (Some(device_id_str), Some(idx_str)) = (device_id_str, idx_str) {
        if let Ok(idx) = idx_str.parse::<usize>() {
            // Try to parse as UUID first (for connected slaves)
            if let Ok(uuid) = uuid::Uuid::parse_str(&device_id_str) {
                let slave_id = SlaveId(uuid);
                if let Some(slave) = state.get_slave(slave_id).await {
                    let mut daq_manager = slave.daq_manager.write().await;
                    let is_selected = daq_manager.selected_indices().contains(&idx);

                    if let Some(should_select) = selected {
                        if should_select && !is_selected {
                            daq_manager.select_measurement(idx);
                        } else if !should_select && is_selected {
                            daq_manager.deselect_measurement(idx);
                        }
                    } else {
                        if is_selected {
                            daq_manager.deselect_measurement(idx);
                        } else {
                            daq_manager.select_measurement(idx);
                        }
                    }
                }
            } else {
                // Not a UUID, try to find by device ID (for unconnected devices)
                if let Some(device) = state.get_device(&device_id_str).await {
                    let mut daq_manager = device.daq_manager.write().await;
                    let is_selected = daq_manager.selected_indices().contains(&idx);

                    if let Some(should_select) = selected {
                        if should_select && !is_selected {
                            daq_manager.select_measurement(idx);
                        } else if !should_select && is_selected {
                            daq_manager.deselect_measurement(idx);
                        }
                    } else {
                        if is_selected {
                            daq_manager.deselect_measurement(idx);
                        } else {
                            daq_manager.select_measurement(idx);
                        }
                    }
                }
            }
        }
    }

    Json(json!({ "success": true }))
}

/// DAQ clear data (GET /api/daq/clear)
pub async fn daq_clear(State(state): State<AppState>) -> impl IntoResponse {
    let current_slave = state.current_slave().await;

    if let Some(slave) = current_slave {
        let mut daq_manager = slave.daq_manager.write().await;
        daq_manager.clear_samples();
        state
            .set_status(format!("DAQ data cleared for {}", slave.display_name()))
            .await;
    } else {
        // Clear all slaves
        let slaves = state.all_slaves().await;
        for slave in slaves {
            let mut daq_manager = slave.daq_manager.write().await;
            daq_manager.clear_samples();
        }
        state
            .set_status("DAQ data cleared for all slaves".to_string())
            .await;
    }

    Json(json!({ "success": true }))
}

/// DAQ data API (POST /api/daq/data) - for AJAX updates with variable filter
pub async fn daq_data(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    // Get filter variables from payload
    let filter_vars: Vec<(String, String)> = payload
        .get("variables")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    let device_id = v.get("device_id").and_then(|d| d.as_str()).unwrap_or("");
                    let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    if !device_id.is_empty() && !name.is_empty() {
                        Some((device_id.to_string(), name.to_string()))
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let filter_set: std::collections::HashSet<(String, String)> = filter_vars.into_iter().collect();

    let mut values: Vec<serde_json::Value> = Vec::new();
    let mut seen_device_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Check DeviceState DaqManagers first (experiment page starts DAQ here)
    let devices = state.all_devices().await;
    for device in &devices {
        if !*device.daq_running.read().await {
            continue;
        }
        let device_id = device
            .slave_id
            .map(|s| s.to_string())
            .unwrap_or_else(|| device.id.clone());

        let daq_manager = device.daq_manager.read().await;
        if let Some(r) = daq_manager.get_latest_sample().await {
            seen_device_ids.insert(device_id.clone());
            let device_name = device.display_name_or_custom().await;
            for (k, v) in &r.values {
                if filter_set.is_empty()
                    || filter_set.contains(&(device_id.clone(), k.clone()))
                {
                    let updated_at =
                        r.signal_timestamps.get(k).copied().unwrap_or(r.timestamp_s);
                    values.push(json!({
                        "device_id": device_id,
                        "var_name": k,
                        "display_name": device_name,
                        "value": format!("{:.6}", v),
                        "updated_at": format!("{:.6}", updated_at)
                    }));
                }
            }
        }
    }

    // Also check SlaveState DaqManagers (DAQ page starts DAQ here)
    let slaves = state.all_slaves().await;
    for slave in slaves {
        let slave_device_id = slave.id.to_string();
        if seen_device_ids.contains(&slave_device_id) {
            continue; // Already got data from DeviceState
        }
        if *slave.daq_running.read().await {
            let daq_manager = slave.daq_manager.read().await;
            if let Some(r) = daq_manager.get_latest_sample().await {
                for (k, v) in &r.values {
                    let device_id = slave_device_id.clone();
                    if filter_set.is_empty()
                        || filter_set.contains(&(device_id.clone(), k.clone()))
                    {
                        let updated_at =
                            r.signal_timestamps.get(k).copied().unwrap_or(r.timestamp_s);
                        values.push(json!({
                            "device_id": device_id,
                            "var_name": k,
                            "display_name": slave.display_name(),
                            "value": format!("{:.6}", v),
                            "updated_at": format!("{:.6}", updated_at)
                        }));
                    }
                }
            }
        }
    }

    // Compute the latest elapsed timestamp
    let mut latest_ts: f64 = 0.0;
    for device in &devices {
        if *device.daq_running.read().await {
            let dm = device.daq_manager.read().await;
            if let Some(r) = dm.get_latest_sample().await {
                if r.timestamp_s > latest_ts {
                    latest_ts = r.timestamp_s;
                }
            }
        }
    }
    {
        let slaves2 = state.all_slaves().await;
        for s in slaves2 {
            let sid = s.id.to_string();
            if seen_device_ids.contains(&sid) {
                continue;
            }
            if *s.daq_running.read().await {
                let dm = s.daq_manager.read().await;
                if let Some(r) = dm.get_latest_sample().await {
                    if r.timestamp_s > latest_ts {
                        latest_ts = r.timestamp_s;
                    }
                }
            }
        }
    }

    Json(json!({
        "success": true,
        "values": values,
        "timestamp_s": format!("{:.6}", latest_ts)
    }))
}

/// A2L load API (POST /api/a2l/load)
/// Loads an A2L file for an existing device (identified by device_id form field).
pub async fn a2l_load(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let mut file_data: Option<bytes::Bytes> = None;
    let mut file_name: Option<String> = None;
    let mut device_id: Option<String> = None;

    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "device_id" => {
                if let Ok(text) = field.text().await {
                    device_id = Some(text);
                }
            }
            "a2l_file" => {
                if let Some(filename) = field.file_name() {
                    file_name = Some(filename.to_string());
                }
                match field.bytes().await {
                    Ok(bytes) => file_data = Some(bytes),
                    Err(e) => {
                        return axum::Json(serde_json::json!({
                            "success": false,
                            "error": format!("Failed to read file: {}", e)
                        }));
                    }
                }
            }
            _ => {}
        }
    }

    let device_id = match device_id {
        Some(id) if !id.trim().is_empty() => id,
        _ => {
            return axum::Json(serde_json::json!({
                "success": false,
                "error": "device_id is required"
            }));
        }
    };

    // Verify device exists
    if state.get_device(&device_id).await.is_none() {
        return axum::Json(serde_json::json!({
            "success": false,
            "error": "Device not found"
        }));
    }

    // Save uploaded file to temp location
    let temp_path = if let (Some(name), Some(data)) = (&file_name, &file_data) {
        let temp_dir = std::env::temp_dir().join("xcp_client");
        if let Err(e) = tokio::fs::create_dir_all(&temp_dir).await {
            return axum::Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to create temp dir: {}", e)
            }));
        }
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let safe_name = name.replace(['/', '\\', ':'], "_");
        let temp_file = temp_dir.join(format!("{}_{}", timestamp, safe_name));
        if let Err(e) = tokio::fs::write(&temp_file, data).await {
            return axum::Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to save temp file: {}", e)
            }));
        }
        temp_file
    } else {
        return axum::Json(serde_json::json!({
            "success": false,
            "error": "No A2L file uploaded"
        }));
    };

    use crate::A2lReader;
    let reader = match tokio::task::spawn_blocking({
        let path = temp_path.clone();
        move || A2lReader::load(&path).map_err(|e| format!("A2L parse error: {:?}", e))
    })
    .await
    {
        Ok(Ok(reader)) => reader,
        Ok(Err(e)) => {
            return axum::Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to parse A2L: {}", e)
            }));
        }
        Err(e) => {
            return axum::Json(serde_json::json!({
                "success": false,
                "error": format!("Task error: {}", e)
            }));
        }
    };

    let transport_configs = reader.get_transport_configs();
    let device_addr = transport_configs
        .first()
        .map(display_device_address)
        .or_else(|| reader.get_slave_addresses().first().cloned())
        .unwrap_or_else(|| "127.0.0.1:5555".to_string());

    // Check for duplicate address excluding this device
    {
        let devices = state.devices.read().await;
        let dup = devices
            .iter()
            .filter(|d| d.id != device_id)
            .any(|d| d.address == device_addr);
        if dup {
            return axum::Json(serde_json::json!({
                "success": false,
                "error": format!(
                    "Another device already uses address {}. Each device must have a unique address.",
                    device_addr
                )
            }));
        }
    }

    let characteristics = reader.get_characteristics();
    let measurements = reader.get_measurements();
    let a2l_events = reader.get_events();
    let max_cto = reader.get_max_cto().unwrap_or(255);
    let cal_count = characteristics.len();
    let meas_count = measurements.len();
    let original_filename = file_name.clone().unwrap_or_else(|| "unknown.a2l".to_string());

    let mut cal_manager = CalibrationManager::new();
    for ch in characteristics {
        if let Some(param) = convert_characteristic(ch, &reader) {
            cal_manager.add_param(param);
        }
    }
    let memory_segments = reader.get_memory_segments();
    cal_manager.set_memory_segments(memory_segments);

    let mut daq_manager = DaqManager::new();
    for m in measurements {
        if let Some(meas) = convert_measurement(m, &reader) {
            daq_manager.add_measurement(meas);
        }
    }
    if !a2l_events.is_empty() {
        daq_manager.set_available_events(a2l_events);
    }

    // Update the existing device
    let slave_id_opt = {
        let mut devices = state.devices.write().await;
        let device = match devices.iter_mut().find(|d| d.id == device_id) {
            Some(d) => d,
            None => {
                return axum::Json(serde_json::json!({
                    "success": false,
                    "error": "Device not found (race condition)"
                }));
            }
        };
        device.address = device_addr.clone();
        device.a2l_path = temp_path.to_string_lossy().to_string();
        device.original_filename = original_filename.clone();
        device.max_cto = max_cto;
        device.transport_configs = transport_configs.clone();
        *device.cal_manager.write().await = cal_manager;
        *device.daq_manager.write().await = daq_manager;
        device.slave_id
    };

    // If connected, sync cal/daq managers to SlaveState
    if let Some(slave_id) = slave_id_opt {
        if let Some(device) = state.get_device(&device_id).await {
            let mut slave_states = state.slave_states.write().await;
            if let Some(slave_state) = slave_states.get_mut(&slave_id) {
                slave_state.cal_manager = device.cal_manager.clone();
                slave_state.daq_manager = device.daq_manager.clone();
            }
        }
    }

    axum::Json(serde_json::json!({
        "success": true,
        "message": format!("Loaded A2L: {} characteristics, {} measurements", cal_count, meas_count),
        "cal_count": cal_count,
        "meas_count": meas_count,
        "address": device_addr,
    }))
}

/// Get available DAQ events (GET /api/daq/events)
pub async fn daq_events(State(state): State<AppState>) -> impl IntoResponse {
    let get_events_from_daq =
        |daq: &DaqManager| -> (Vec<serde_json::Value>, u16, serde_json::Value) {
            let events = daq
                .get_available_events()
                .iter()
                .map(|e| {
                    json!({
                        "channel": e.channel,
                        "name": e.name,
                        "short_name": e.short_name,
                        "time_cycle": e.time_cycle,
                        "time_unit": e.time_unit,
                        "max_daq_list": e.max_daq_list,
                        "priority": e.priority,
                        "display_label": e.display_label(),
                        "cycle_time_ms": e.cycle_time_ms(),
                    })
                })
                .collect();
            // Build per-measurement event map: { "measurement_idx": event_channel, ... }
            let meas_events: serde_json::Map<String, serde_json::Value> = daq
                .get_measurement_events()
                .iter()
                .map(|(idx, ch)| (idx.to_string(), json!(*ch)))
                .collect();
            (events, daq.get_event_channel(), json!(meas_events))
        };

    if let Some(slave) = state.current_slave().await {
        let daq = slave.daq_manager.read().await;
        let (events, selected, meas_events) = get_events_from_daq(&daq);
        return Json(
            json!({ "success": true, "events": events, "selected_channel": selected, "measurement_events": meas_events }),
        );
    }
    if let Some(device) = state.current_device().await {
        let daq = device.daq_manager.read().await;
        let (events, selected, meas_events) = get_events_from_daq(&daq);
        return Json(
            json!({ "success": true, "events": events, "selected_channel": selected, "measurement_events": meas_events }),
        );
    }
    Json(
        json!({ "success": true, "events": [], "selected_channel": 0u16, "measurement_events": {} }),
    )
}

/// Set default DAQ event channel (POST /api/daq/event)
pub async fn daq_set_event(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let channel = match payload.get("channel").and_then(|c| c.as_u64()) {
        Some(c) => c as u16,
        None => return Json(json!({ "success": false, "error": "Missing channel field" })),
    };

    if let Some(slave) = state.current_slave().await {
        let mut daq = slave.daq_manager.write().await;
        daq.set_event_channel(channel);
        return Json(json!({ "success": true, "channel": channel }));
    }
    if let Some(device) = state.current_device().await {
        let mut daq = device.daq_manager.write().await;
        daq.set_event_channel(channel);
        return Json(json!({ "success": true, "channel": channel }));
    }
    Json(json!({ "success": false, "error": "No device connected" }))
}

/// Set per-measurement event channels (POST /api/daq/measurement-events)
/// Body: { "assignments": [ { "measurement_idx": 0, "channel": 2 }, ... ] }
pub async fn daq_set_measurement_events(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let assignments: Vec<(usize, u16)> = payload
        .get("assignments")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    let idx = v.get("measurement_idx").and_then(|i| i.as_u64())? as usize;
                    let channel = v.get("channel").and_then(|c| c.as_u64())? as u16;
                    Some((idx, channel))
                })
                .collect()
        })
        .unwrap_or_default();

    if assignments.is_empty() {
        return Json(json!({ "success": false, "error": "No valid assignments provided" }));
    }

    let apply = |daq: &mut DaqManager| {
        daq.set_measurement_events_batch(&assignments);
    };

    if let Some(slave) = state.current_slave().await {
        let mut daq = slave.daq_manager.write().await;
        apply(&mut daq);
        return Json(json!({ "success": true }));
    }
    if let Some(device) = state.current_device().await {
        let mut daq = device.daq_manager.write().await;
        apply(&mut daq);
        return Json(json!({ "success": true }));
    }
    Json(json!({ "success": false, "error": "No device selected" }))
}

/// Download DAQ CSV file
pub async fn daq_download_csv(State(_state): State<AppState>) -> impl IntoResponse {
    serve_file_download("daq_data.csv", "text/csv", "daq_data.csv").await
}

/// Download DAQ MDF4 file
pub async fn daq_download_mf4(State(_state): State<AppState>) -> impl IntoResponse {
    serve_file_download("daq_data.mf4", "application/octet-stream", "daq_data.mf4").await
}

/// Serve a file for download with proper headers
async fn serve_file_download(
    path: &str,
    content_type: &str,
    filename: &str,
) -> axum::response::Response {
    use axum::body::Body;
    use axum::http::{header, StatusCode};

    match tokio::fs::read(path).await {
        Ok(data) => {
            let headers = [
                (header::CONTENT_TYPE, content_type.to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{}\"", filename),
                ),
            ];
            (StatusCode::OK, headers, Body::from(data)).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "File not found").into_response(),
    }
}

/// Convert A2L characteristic to calibration parameter
fn convert_characteristic(
    ch: crate::a2l::A2lCharacteristic,
    reader: &crate::a2l::A2lReader,
) -> Option<crate::calibration::CalibrationParam> {
    use crate::calibration::{CalibrationParam, CalibrationType};

    let data_type = match ch.data_type {
        crate::a2l::A2lDataType::U8 => CalibrationType::U8,
        crate::a2l::A2lDataType::U16 => CalibrationType::U16,
        crate::a2l::A2lDataType::U32 => CalibrationType::U32,
        crate::a2l::A2lDataType::U64 => CalibrationType::U64,
        crate::a2l::A2lDataType::I8 => CalibrationType::I8,
        crate::a2l::A2lDataType::I16 => CalibrationType::I16,
        crate::a2l::A2lDataType::I32 => CalibrationType::I32,
        crate::a2l::A2lDataType::I64 => CalibrationType::I64,
        crate::a2l::A2lDataType::F32 => CalibrationType::F32,
        crate::a2l::A2lDataType::F64 => CalibrationType::F64,
        crate::a2l::A2lDataType::Bool => CalibrationType::Bool,
        crate::a2l::A2lDataType::U8Array(n) => CalibrationType::U8Array(n),
        crate::a2l::A2lDataType::U16Array(n) => CalibrationType::U16Array(n),
        crate::a2l::A2lDataType::U32Array(n) => CalibrationType::U32Array(n),
        crate::a2l::A2lDataType::U64Array(n) => CalibrationType::U64Array(n),
        crate::a2l::A2lDataType::I8Array(n) => CalibrationType::I8Array(n),
        crate::a2l::A2lDataType::I16Array(n) => CalibrationType::I16Array(n),
        crate::a2l::A2lDataType::I32Array(n) => CalibrationType::I32Array(n),
        crate::a2l::A2lDataType::I64Array(n) => CalibrationType::I64Array(n),
        crate::a2l::A2lDataType::F32Array(n) => CalibrationType::F32Array(n),
        crate::a2l::A2lDataType::F64Array(n) => CalibrationType::F64Array(n),
        crate::a2l::A2lDataType::BoolArray(n) => CalibrationType::BoolArray(n),
        crate::a2l::A2lDataType::U8Matrix(r, c) => CalibrationType::U8Matrix(r, c),
        crate::a2l::A2lDataType::U16Matrix(r, c) => CalibrationType::U16Matrix(r, c),
        crate::a2l::A2lDataType::U32Matrix(r, c) => CalibrationType::U32Matrix(r, c),
        crate::a2l::A2lDataType::U64Matrix(r, c) => CalibrationType::U64Matrix(r, c),
        crate::a2l::A2lDataType::I8Matrix(r, c) => CalibrationType::I8Matrix(r, c),
        crate::a2l::A2lDataType::I16Matrix(r, c) => CalibrationType::I16Matrix(r, c),
        crate::a2l::A2lDataType::I32Matrix(r, c) => CalibrationType::I32Matrix(r, c),
        crate::a2l::A2lDataType::I64Matrix(r, c) => CalibrationType::I64Matrix(r, c),
        crate::a2l::A2lDataType::F32Matrix(r, c) => CalibrationType::F32Matrix(r, c),
        crate::a2l::A2lDataType::F64Matrix(r, c) => CalibrationType::F64Matrix(r, c),
        crate::a2l::A2lDataType::BoolMatrix(r, c) => CalibrationType::BoolMatrix(r, c),
    };

    let mut param = CalibrationParam::new(&ch.name, ch.address, ch.address_ext, data_type)
        .with_description(&ch.description)
        .with_range(ch.min, ch.max);

    if let Some(unit) = ch.unit {
        param = param.with_unit(&unit);
    }

    // Set characteristic type, axis info, and storage order
    param.characteristic_type = ch.characteristic_type;
    param.axis_info = ch.axis_info;
    param.storage_order = ch.storage_order;

    // Resolve COMPU_METHOD for value conversion (enum, linear, etc.)
    param.compu_method = reader.get_compu_method(&ch.conversion_name);

    Some(param)
}

/// Convert A2L measurement to DAQ measurement
fn convert_measurement(m: crate::a2l::A2lMeasurement, reader: &crate::a2l::A2lReader) -> Option<crate::daq::Measurement> {
    Some(crate::daq::Measurement::with_type(
        &m.name,
        m.address,
        m.address_ext,
        m.data_type,
        m.unit,
        reader.get_compu_method(&m.conversion_name),
    ))
}

/// Build a cal_value_api response from a HEX image when the slave is not connected.
/// Returns `{"success": false, "no_hex": true}` when no HEX is available.
async fn cal_value_from_hex_or_empty(
    state: &AppState,
    device_id_str: &str,
    idx: usize,
) -> Json<serde_json::Value> {
    let devices = state.devices.read().await;
    let device = devices.iter().find(|d| {
        d.id == device_id_str
            || d.slave_id.map(|s| s.to_string()) == Some(device_id_str.to_string())
    });
    let Some(device) = device else {
        return Json(json!({"success": false, "error": "Device not found"}));
    };

    let cal_manager = device.cal_manager.read().await;
    let Some(param) = cal_manager.params().get(idx).cloned() else {
        return Json(json!({
            "success": false,
            "error": format!("Parameter index {} not found", idx)
        }));
    };
    drop(cal_manager);

    let hex_guard = device.hex_image.read().await;
    let Some(ref hex) = *hex_guard else {
        // No HEX loaded — frontend will show "—"
        return Json(json!({"success": false, "no_hex": true}));
    };

    match read_calibration_from_hex(hex, &param) {
        Ok(value) => {
            let (rows, cols) = match &param.data_type {
                CalibrationType::U8Array(n)
                | CalibrationType::I8Array(n)
                | CalibrationType::U16Array(n)
                | CalibrationType::I16Array(n)
                | CalibrationType::U32Array(n)
                | CalibrationType::I32Array(n)
                | CalibrationType::U64Array(n)
                | CalibrationType::I64Array(n)
                | CalibrationType::F32Array(n)
                | CalibrationType::F64Array(n)
                | CalibrationType::BoolArray(n) => (1, *n),
                CalibrationType::U8Matrix(r, c)
                | CalibrationType::I8Matrix(r, c)
                | CalibrationType::U16Matrix(r, c)
                | CalibrationType::I16Matrix(r, c)
                | CalibrationType::U32Matrix(r, c)
                | CalibrationType::I32Matrix(r, c)
                | CalibrationType::U64Matrix(r, c)
                | CalibrationType::I64Matrix(r, c)
                | CalibrationType::F32Matrix(r, c)
                | CalibrationType::F64Matrix(r, c)
                | CalibrationType::BoolMatrix(r, c) => (*r, *c),
                _ => (1, 1),
            };
            let is_1d = matches!(
                param.data_type,
                CalibrationType::U8Array(_)
                    | CalibrationType::I8Array(_)
                    | CalibrationType::U16Array(_)
                    | CalibrationType::I16Array(_)
                    | CalibrationType::U32Array(_)
                    | CalibrationType::I32Array(_)
                    | CalibrationType::U64Array(_)
                    | CalibrationType::I64Array(_)
                    | CalibrationType::F32Array(_)
                    | CalibrationType::F64Array(_)
                    | CalibrationType::BoolArray(_)
            );
            let value_json = calibration_value_to_json(&value, is_1d);
            let axis_info: Vec<serde_json::Value> =
                param.axis_info.iter().map(|a| a.to_json()).collect();
            let axis_values = read_axis_values_from_hex(hex, &param);
            let is_enum = param.is_enum();
            let enum_options: Vec<serde_json::Value> = param
                .enum_options()
                .map(|opts| {
                    opts.iter()
                        .map(|opt| json!({"raw": opt.raw_value, "text": opt.text_value}))
                        .collect()
                })
                .unwrap_or_default();

            Json(json!({
                "success": true,
                "source": "hex",
                "name": param.name,
                "type": param.data_type.type_name(),
                "value": value_json,
                "rows": rows,
                "cols": cols,
                "is_1d": is_1d,
                "is_working_page": false,
                "axis_info": axis_info,
                "axis_values": axis_values,
                "characteristic_type": param.characteristic_type.as_str(),
                "is_enum": is_enum,
                "enum_options": enum_options,
            }))
        }
        Err(e) => Json(json!({
            "success": false,
            "error": format!("HEX read failed: {}", e)
        })),
    }
}

/// Get full calibration parameter value (GET /api/cal/value/:device_id/:idx)
/// Returns complete value with axis info and dimensions for matrix editor
pub async fn cal_value_api(
    State(state): State<AppState>,
    Path((device_id_str, idx)): Path<(String, usize)>,
) -> impl IntoResponse {
    // Try to parse as UUID (could be slave_id or device_id)
    let uuid = match uuid::Uuid::parse_str(&device_id_str) {
        Ok(id) => id,
        Err(_) => {
            return Json(json!({
                "success": false,
                "error": "Invalid device ID"
            }));
        }
    };

    let slave_id = SlaveId(uuid);
    let slave = state.get_slave(slave_id).await;
    let is_connected = slave.as_ref().map_or(false, |s| s.is_connected());

    if !is_connected {
        // Slave not connected — try to read from HEX image, or return empty if none loaded
        return cal_value_from_hex_or_empty(&state, &device_id_str, idx).await;
    }

    let slave = slave.unwrap();

    let client = state.client.lock().await;
    let cal_manager = slave.cal_manager.read().await;

    if let Some(param) = cal_manager.params().get(idx) {
        let param = param.clone();
        drop(cal_manager);

        // Get page mode
        let page_mode = if let Some(xcp_slave) = client.get_slave(slave_id) {
            let xcp_slave_guard = xcp_slave.lock().await;
            xcp_slave_guard.get_current_page_mode()
        } else {
            crate::slave::PageMode::Reference
        };
        let is_working_page = page_mode == crate::slave::PageMode::Working;

        // Read the value
        if let Some(xcp_slave) = client.get_slave(slave_id) {
            let xcp_slave_guard = xcp_slave.lock().await;
            match read_calibration_from_slave(&xcp_slave_guard, &param).await {
                Ok(value) => {
                    // Get dimensions
                    let (rows, cols) = match &param.data_type {
                        CalibrationType::U8Array(n)
                        | CalibrationType::I8Array(n)
                        | CalibrationType::U16Array(n)
                        | CalibrationType::I16Array(n)
                        | CalibrationType::U32Array(n)
                        | CalibrationType::I32Array(n)
                        | CalibrationType::U64Array(n)
                        | CalibrationType::I64Array(n)
                        | CalibrationType::F32Array(n)
                        | CalibrationType::F64Array(n)
                        | CalibrationType::BoolArray(n) => (1, *n),
                        CalibrationType::U8Matrix(r, c)
                        | CalibrationType::I8Matrix(r, c)
                        | CalibrationType::U16Matrix(r, c)
                        | CalibrationType::I16Matrix(r, c)
                        | CalibrationType::U32Matrix(r, c)
                        | CalibrationType::I32Matrix(r, c)
                        | CalibrationType::U64Matrix(r, c)
                        | CalibrationType::I64Matrix(r, c)
                        | CalibrationType::F32Matrix(r, c)
                        | CalibrationType::F64Matrix(r, c)
                        | CalibrationType::BoolMatrix(r, c) => (*r, *c),
                        _ => (1, 1),
                    };

                    // Check if it's a 1D or 2D type
                    let is_1d = matches!(
                        param.data_type,
                        CalibrationType::U8Array(_)
                            | CalibrationType::I8Array(_)
                            | CalibrationType::U16Array(_)
                            | CalibrationType::I16Array(_)
                            | CalibrationType::U32Array(_)
                            | CalibrationType::I32Array(_)
                            | CalibrationType::U64Array(_)
                            | CalibrationType::I64Array(_)
                            | CalibrationType::F32Array(_)
                            | CalibrationType::F64Array(_)
                            | CalibrationType::BoolArray(_)
                    );

                    // Convert value to JSON array format
                    let value_json = calibration_value_to_json(&value, is_1d);

                    // Get axis info
                    let axis_info: Vec<serde_json::Value> =
                        param.axis_info.iter().map(|axis| axis.to_json()).collect();

                    // Read axis values (from static source or XCP memory)
                    let axis_values = read_axis_values_from_slave(&xcp_slave_guard, &param).await;

                    tracing::info!(
                        "cal_value_api: param '{}' type={}, characteristic_type={:?}, axis_info count={}, axis_values count={}",
                        param.name,
                        param.data_type.type_name(),
                        param.characteristic_type,
                        param.axis_info.len(),
                        axis_values.len()
                    );

                    // Check if this is an enum type and get options
                    let is_enum = param.is_enum();
                    tracing::info!(
                        "cal_value_api: param '{}' is_enum={}, enum_options_count={}",
                        param.name,
                        is_enum,
                        param.enum_count()
                    );
                    let enum_options: Vec<serde_json::Value> = param
                        .enum_options()
                        .map(|opts| {
                            opts.iter()
                                .map(|opt| {
                                    json!({
                                        "raw": opt.raw_value,
                                        "text": opt.text_value,
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default();

                    Json(json!({
                        "success": true,
                        "name": param.name,
                        "type": param.data_type.type_name(),
                        "value": value_json,
                        "rows": rows,
                        "cols": cols,
                        "is_1d": is_1d,
                        "is_working_page": is_working_page,
                        "axis_info": axis_info,
                        "axis_values": axis_values,
                        "characteristic_type": param.characteristic_type.as_str(),
                        "is_enum": is_enum,
                        "enum_options": enum_options,
                    }))
                }
                Err(e) => Json(json!({
                    "success": false,
                    "error": format!("Failed to read: {}", e)
                })),
            }
        } else {
            Json(json!({
                "success": false,
                "error": "Slave not available in client"
            }))
        }
    } else {
        Json(json!({
            "success": false,
            "error": format!("Parameter index {} not found", idx)
        }))
    }
}

/// Read axis values from XCP slave for axes that require memory read
async fn read_axis_values_from_slave(
    xcp_slave: &crate::slave::XcpSlave,
    param: &CalibrationParam,
) -> Vec<Vec<f64>> {
    use crate::a2l::AxisPointSource;
    use crate::protocol::Mta;

    let mut result = Vec::new();

    for axis in &param.axis_info {
        // First try static values
        if let Some(values) = axis.get_static_values() {
            result.push(values);
            continue;
        }

        // If requires memory read, read from XCP
        if let AxisPointSource::Memory {
            address,
            address_ext,
            data_type,
            num_points,
        } = &axis.point_source
        {
            // Calculate element size (size of single axis point)
            let element_size = match data_type {
                crate::a2l::A2lDataType::U8
                | crate::a2l::A2lDataType::I8
                | crate::a2l::A2lDataType::Bool => 1,
                crate::a2l::A2lDataType::U16 | crate::a2l::A2lDataType::I16 => 2,
                crate::a2l::A2lDataType::U32
                | crate::a2l::A2lDataType::I32
                | crate::a2l::A2lDataType::F32 => 4,
                crate::a2l::A2lDataType::U64
                | crate::a2l::A2lDataType::I64
                | crate::a2l::A2lDataType::F64 => 8,
                _ => 4, // Default to 4 bytes
            };
            let total_size = element_size * num_points;

            let mta = Mta::new(*address, *address_ext);
            match xcp_slave.read_large_data(mta, total_size, None).await {
                Ok(data) => {
                    let values = parse_axis_values(&data, data_type, *num_points);
                    result.push(values);
                }
                Err(e) => {
                    tracing::warn!("Failed to read axis values for '{}': {}", axis.name, e);
                    // Fall back to index-based values
                    result.push((0..*num_points).map(|i| i as f64).collect());
                }
            }
        } else {
            // Should not happen, but fallback to indices
            result.push((0..axis.num_points).map(|i| i as f64).collect());
        }
    }

    result
}

/// Parse axis values from raw bytes based on data type
fn parse_axis_values(
    data: &[u8],
    data_type: &crate::a2l::A2lDataType,
    num_points: usize,
) -> Vec<f64> {
    use crate::a2l::A2lDataType;

    let mut values = Vec::with_capacity(num_points);
    // Calculate element size
    let element_size = match data_type {
        A2lDataType::U8 | A2lDataType::I8 | A2lDataType::Bool => 1,
        A2lDataType::U16 | A2lDataType::I16 => 2,
        A2lDataType::U32 | A2lDataType::I32 | A2lDataType::F32 => 4,
        A2lDataType::U64 | A2lDataType::I64 | A2lDataType::F64 => 8,
        _ => 4,
    };

    for i in 0..num_points {
        let offset = i * element_size;
        if offset + element_size > data.len() {
            values.push(i as f64);
            continue;
        }

        let slice = &data[offset..offset + element_size];
        let val = match data_type {
            A2lDataType::U8 | A2lDataType::U8Array(_) => slice[0] as f64,
            A2lDataType::I8 | A2lDataType::I8Array(_) => slice[0] as i8 as f64,
            A2lDataType::U16 | A2lDataType::U16Array(_) => {
                u16::from_le_bytes([slice[0], slice[1]]) as f64
            }
            A2lDataType::I16 | A2lDataType::I16Array(_) => {
                i16::from_le_bytes([slice[0], slice[1]]) as f64
            }
            A2lDataType::U32 | A2lDataType::U32Array(_) => {
                u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]) as f64
            }
            A2lDataType::I32 | A2lDataType::I32Array(_) => {
                i32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]) as f64
            }
            A2lDataType::F32 | A2lDataType::F32Array(_) => {
                f32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]) as f64
            }
            A2lDataType::U64 | A2lDataType::U64Array(_) => u64::from_le_bytes([
                slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
            ]) as f64,
            A2lDataType::I64 | A2lDataType::I64Array(_) => i64::from_le_bytes([
                slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
            ]) as f64,
            A2lDataType::F64 | A2lDataType::F64Array(_) => f64::from_le_bytes([
                slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
            ]),
            _ => i as f64,
        };
        values.push(val);
    }

    values
}

/// Convert CalibrationValue to JSON array format
fn calibration_value_to_json(value: &CalibrationValue, is_1d: bool) -> serde_json::Value {
    match value {
        // Scalar
        CalibrationValue::U8(v) => json!([*v]),
        CalibrationValue::U16(v) => json!([*v]),
        CalibrationValue::U32(v) => json!([*v]),
        CalibrationValue::U64(v) => json!([*v]),
        CalibrationValue::I8(v) => json!([*v]),
        CalibrationValue::I16(v) => json!([*v]),
        CalibrationValue::I32(v) => json!([*v]),
        CalibrationValue::I64(v) => json!([*v]),
        CalibrationValue::F32(v) => json!([format!("{:.6}", v)]),
        CalibrationValue::F64(v) => json!([format!("{:.6}", v)]),
        CalibrationValue::Bool(v) => json!([*v]),
        // Arrays (1D)
        CalibrationValue::U8Array(arr) => {
            if is_1d {
                json!(arr.iter().map(|v| json!(*v)).collect::<Vec<_>>())
            } else {
                json!([arr.iter().map(|v| json!(*v)).collect::<Vec<_>>()])
            }
        }
        CalibrationValue::U16Array(arr) => {
            if is_1d {
                json!(arr.iter().map(|v| json!(*v)).collect::<Vec<_>>())
            } else {
                json!([arr.iter().map(|v| json!(*v)).collect::<Vec<_>>()])
            }
        }
        CalibrationValue::U32Array(arr) => {
            if is_1d {
                json!(arr.iter().map(|v| json!(*v)).collect::<Vec<_>>())
            } else {
                json!([arr.iter().map(|v| json!(*v)).collect::<Vec<_>>()])
            }
        }
        CalibrationValue::U64Array(arr) => {
            if is_1d {
                json!(arr.iter().map(|v| json!(*v)).collect::<Vec<_>>())
            } else {
                json!([arr.iter().map(|v| json!(*v)).collect::<Vec<_>>()])
            }
        }
        CalibrationValue::I8Array(arr) => {
            if is_1d {
                json!(arr.iter().map(|v| json!(*v)).collect::<Vec<_>>())
            } else {
                json!([arr.iter().map(|v| json!(*v)).collect::<Vec<_>>()])
            }
        }
        CalibrationValue::I16Array(arr) => {
            if is_1d {
                json!(arr.iter().map(|v| json!(*v)).collect::<Vec<_>>())
            } else {
                json!([arr.iter().map(|v| json!(*v)).collect::<Vec<_>>()])
            }
        }
        CalibrationValue::I32Array(arr) => {
            if is_1d {
                json!(arr.iter().map(|v| json!(*v)).collect::<Vec<_>>())
            } else {
                json!([arr.iter().map(|v| json!(*v)).collect::<Vec<_>>()])
            }
        }
        CalibrationValue::I64Array(arr) => {
            if is_1d {
                json!(arr.iter().map(|v| json!(*v)).collect::<Vec<_>>())
            } else {
                json!([arr.iter().map(|v| json!(*v)).collect::<Vec<_>>()])
            }
        }
        CalibrationValue::F32Array(arr) => {
            if is_1d {
                json!(arr
                    .iter()
                    .map(|v| json!(format!("{:.6}", v)))
                    .collect::<Vec<_>>())
            } else {
                json!([arr
                    .iter()
                    .map(|v| json!(format!("{:.6}", v)))
                    .collect::<Vec<_>>()])
            }
        }
        CalibrationValue::F64Array(arr) => {
            if is_1d {
                json!(arr
                    .iter()
                    .map(|v| json!(format!("{:.6}", v)))
                    .collect::<Vec<_>>())
            } else {
                json!([arr
                    .iter()
                    .map(|v| json!(format!("{:.6}", v)))
                    .collect::<Vec<_>>()])
            }
        }
        CalibrationValue::BoolArray(arr) => {
            if is_1d {
                json!(arr.iter().map(|v| json!(*v)).collect::<Vec<_>>())
            } else {
                json!([arr.iter().map(|v| json!(*v)).collect::<Vec<_>>()])
            }
        }
        // Matrices (2D)
        CalibrationValue::U8Matrix(mat) => {
            json!(mat
                .iter()
                .map(|row| row.iter().map(|v| json!(*v)).collect::<Vec<_>>())
                .collect::<Vec<_>>())
        }
        CalibrationValue::U16Matrix(mat) => {
            json!(mat
                .iter()
                .map(|row| row.iter().map(|v| json!(*v)).collect::<Vec<_>>())
                .collect::<Vec<_>>())
        }
        CalibrationValue::U32Matrix(mat) => {
            json!(mat
                .iter()
                .map(|row| row.iter().map(|v| json!(*v)).collect::<Vec<_>>())
                .collect::<Vec<_>>())
        }
        CalibrationValue::U64Matrix(mat) => {
            json!(mat
                .iter()
                .map(|row| row.iter().map(|v| json!(*v)).collect::<Vec<_>>())
                .collect::<Vec<_>>())
        }
        CalibrationValue::I8Matrix(mat) => {
            json!(mat
                .iter()
                .map(|row| row.iter().map(|v| json!(*v)).collect::<Vec<_>>())
                .collect::<Vec<_>>())
        }
        CalibrationValue::I16Matrix(mat) => {
            json!(mat
                .iter()
                .map(|row| row.iter().map(|v| json!(*v)).collect::<Vec<_>>())
                .collect::<Vec<_>>())
        }
        CalibrationValue::I32Matrix(mat) => {
            json!(mat
                .iter()
                .map(|row| row.iter().map(|v| json!(*v)).collect::<Vec<_>>())
                .collect::<Vec<_>>())
        }
        CalibrationValue::I64Matrix(mat) => {
            json!(mat
                .iter()
                .map(|row| row.iter().map(|v| json!(*v)).collect::<Vec<_>>())
                .collect::<Vec<_>>())
        }
        CalibrationValue::F32Matrix(mat) => {
            json!(mat
                .iter()
                .map(|row| row
                    .iter()
                    .map(|v| json!(format!("{:.6}", v)))
                    .collect::<Vec<_>>())
                .collect::<Vec<_>>())
        }
        CalibrationValue::F64Matrix(mat) => {
            json!(mat
                .iter()
                .map(|row| row
                    .iter()
                    .map(|v| json!(format!("{:.6}", v)))
                    .collect::<Vec<_>>())
                .collect::<Vec<_>>())
        }
        CalibrationValue::BoolMatrix(mat) => {
            json!(mat
                .iter()
                .map(|row| row.iter().map(|v| json!(*v)).collect::<Vec<_>>())
                .collect::<Vec<_>>())
        }
    }
}

/// Write single element of calibration parameter (POST /api/cal/write-element/:device_id/:idx)
/// Body: { "row": 0, "col": 1, "value": 5.0 }
pub async fn cal_write_element_api(
    State(state): State<AppState>,
    Path((device_id_str, idx)): Path<(String, usize)>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    // Parse device ID
    let uuid = match uuid::Uuid::parse_str(&device_id_str) {
        Ok(id) => id,
        Err(_) => {
            return Json(json!({
                "success": false,
                "error": "Invalid device ID"
            }));
        }
    };

    let slave_id = SlaveId(uuid);
    let slave = match state.get_slave(slave_id).await {
        Some(s) => s,
        None => {
            return Json(json!({
                "success": false,
                "error": "Device not found or not connected"
            }));
        }
    };

    if !slave.is_connected() {
        return Json(json!({
            "success": false,
            "error": "Not connected to XCP slave"
        }));
    }

    // Get row and col from payload
    let row = payload.get("row").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let col = payload.get("col").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let value_str = payload.get("value").and_then(|v| v.as_str()).unwrap_or("0");

    let client = state.client.lock().await;
    let cal_manager = slave.cal_manager.read().await;

    if let Some(param) = cal_manager.params().get(idx) {
        let param = param.clone();
        drop(cal_manager);

        // Check page mode
        let page_mode = if let Some(xcp_slave) = client.get_slave(slave_id) {
            let xcp_slave_guard = xcp_slave.lock().await;
            xcp_slave_guard.get_current_page_mode()
        } else {
            crate::slave::PageMode::Reference
        };

        if page_mode != crate::slave::PageMode::Working {
            return Json(json!({
                "success": false,
                "error": "Cannot write in Reference page mode. Switch to Working page first."
            }));
        }

        // Read current value
        if let Some(xcp_slave) = client.get_slave(slave_id) {
            let mut current_value = {
                let xcp_slave_guard = xcp_slave.lock().await;
                match read_calibration_from_slave(&xcp_slave_guard, &param).await {
                    Ok(v) => v,
                    Err(e) => {
                        return Json(json!({
                            "success": false,
                            "error": format!("Failed to read current value: {}", e)
                        }));
                    }
                }
            };

            // Convert enum text to raw value if needed
            let value_to_write = if param.is_enum() {
                match param.parse_display(value_str) {
                    Ok(raw) => raw.to_string(),
                    Err(e) => {
                        return Json(json!({
                            "success": false,
                            "error": format!("Invalid enum value '{}': {}", value_str, e)
                        }));
                    }
                }
            } else {
                value_str.to_string()
            };

            // Modify the element at (row, col)
            match modify_element(
                &mut current_value,
                &param.data_type,
                row,
                col,
                &value_to_write,
            ) {
                Ok(_) => {
                    // Write back the entire value
                    let xcp_slave_guard = xcp_slave.lock().await;
                    match write_calibration_to_slave(&xcp_slave_guard, &param, &current_value).await
                    {
                        Ok(_) => {
                            // Update cache
                            slave
                                .cal_values
                                .write()
                                .await
                                .insert(param.name.clone(), current_value.clone());
                            Json(json!({
                                "success": true,
                                "message": format!("Updated element [{}, {}] = {}", row, col, value_str)
                            }))
                        }
                        Err(e) => Json(json!({
                            "success": false,
                            "error": format!("Failed to write: {}", e)
                        })),
                    }
                }
                Err(e) => Json(json!({
                    "success": false,
                    "error": e
                })),
            }
        } else {
            Json(json!({
                "success": false,
                "error": "Slave not available in client"
            }))
        }
    } else {
        Json(json!({
            "success": false,
            "error": format!("Parameter index {} not found", idx)
        }))
    }
}

/// Modify a single element in a CalibrationValue
fn modify_element(
    value: &mut CalibrationValue,
    data_type: &CalibrationType,
    row: usize,
    col: usize,
    value_str: &str,
) -> Result<(), String> {
    match (value, data_type) {
        // 1D Arrays - treat as row 0
        (CalibrationValue::U8Array(arr), CalibrationType::U8Array(_)) => {
            if col < arr.len() {
                arr[col] = value_str.parse().map_err(|_| "Invalid U8 value")?;
                Ok(())
            } else {
                Err(format!("Index {} out of bounds", col))
            }
        }
        (CalibrationValue::U16Array(arr), CalibrationType::U16Array(_)) => {
            if col < arr.len() {
                arr[col] = value_str.parse().map_err(|_| "Invalid U16 value")?;
                Ok(())
            } else {
                Err(format!("Index {} out of bounds", col))
            }
        }
        (CalibrationValue::U32Array(arr), CalibrationType::U32Array(_)) => {
            if col < arr.len() {
                arr[col] = value_str.parse().map_err(|_| "Invalid U32 value")?;
                Ok(())
            } else {
                Err(format!("Index {} out of bounds", col))
            }
        }
        (CalibrationValue::U64Array(arr), CalibrationType::U64Array(_)) => {
            if col < arr.len() {
                arr[col] = value_str.parse().map_err(|_| "Invalid U64 value")?;
                Ok(())
            } else {
                Err(format!("Index {} out of bounds", col))
            }
        }
        (CalibrationValue::I8Array(arr), CalibrationType::I8Array(_)) => {
            if col < arr.len() {
                arr[col] = value_str.parse().map_err(|_| "Invalid I8 value")?;
                Ok(())
            } else {
                Err(format!("Index {} out of bounds", col))
            }
        }
        (CalibrationValue::I16Array(arr), CalibrationType::I16Array(_)) => {
            if col < arr.len() {
                arr[col] = value_str.parse().map_err(|_| "Invalid I16 value")?;
                Ok(())
            } else {
                Err(format!("Index {} out of bounds", col))
            }
        }
        (CalibrationValue::I32Array(arr), CalibrationType::I32Array(_)) => {
            if col < arr.len() {
                arr[col] = value_str.parse().map_err(|_| "Invalid I32 value")?;
                Ok(())
            } else {
                Err(format!("Index {} out of bounds", col))
            }
        }
        (CalibrationValue::I64Array(arr), CalibrationType::I64Array(_)) => {
            if col < arr.len() {
                arr[col] = value_str.parse().map_err(|_| "Invalid I64 value")?;
                Ok(())
            } else {
                Err(format!("Index {} out of bounds", col))
            }
        }
        (CalibrationValue::F32Array(arr), CalibrationType::F32Array(_)) => {
            if col < arr.len() {
                arr[col] = value_str.parse().map_err(|_| "Invalid F32 value")?;
                Ok(())
            } else {
                Err(format!("Index {} out of bounds", col))
            }
        }
        (CalibrationValue::F64Array(arr), CalibrationType::F64Array(_)) => {
            if col < arr.len() {
                arr[col] = value_str.parse().map_err(|_| "Invalid F64 value")?;
                Ok(())
            } else {
                Err(format!("Index {} out of bounds", col))
            }
        }
        (CalibrationValue::BoolArray(arr), CalibrationType::BoolArray(_)) => {
            if col < arr.len() {
                arr[col] = value_str
                    .parse()
                    .map_err(|_| "Invalid BOOL value (true/false)")?;
                Ok(())
            } else {
                Err(format!("Index {} out of bounds", col))
            }
        }
        // 2D Matrices
        (CalibrationValue::U8Matrix(mat), CalibrationType::U8Matrix(_, _)) => {
            if row < mat.len() && col < mat[row].len() {
                mat[row][col] = value_str.parse().map_err(|_| "Invalid U8 value")?;
                Ok(())
            } else {
                Err(format!("Index [{}, {}] out of bounds", row, col))
            }
        }
        (CalibrationValue::U16Matrix(mat), CalibrationType::U16Matrix(_, _)) => {
            if row < mat.len() && col < mat[row].len() {
                mat[row][col] = value_str.parse().map_err(|_| "Invalid U16 value")?;
                Ok(())
            } else {
                Err(format!("Index [{}, {}] out of bounds", row, col))
            }
        }
        (CalibrationValue::U32Matrix(mat), CalibrationType::U32Matrix(_, _)) => {
            if row < mat.len() && col < mat[row].len() {
                mat[row][col] = value_str.parse().map_err(|_| "Invalid U32 value")?;
                Ok(())
            } else {
                Err(format!("Index [{}, {}] out of bounds", row, col))
            }
        }
        (CalibrationValue::U64Matrix(mat), CalibrationType::U64Matrix(_, _)) => {
            if row < mat.len() && col < mat[row].len() {
                mat[row][col] = value_str.parse().map_err(|_| "Invalid U64 value")?;
                Ok(())
            } else {
                Err(format!("Index [{}, {}] out of bounds", row, col))
            }
        }
        (CalibrationValue::I8Matrix(mat), CalibrationType::I8Matrix(_, _)) => {
            if row < mat.len() && col < mat[row].len() {
                mat[row][col] = value_str.parse().map_err(|_| "Invalid I8 value")?;
                Ok(())
            } else {
                Err(format!("Index [{}, {}] out of bounds", row, col))
            }
        }
        (CalibrationValue::I16Matrix(mat), CalibrationType::I16Matrix(_, _)) => {
            if row < mat.len() && col < mat[row].len() {
                mat[row][col] = value_str.parse().map_err(|_| "Invalid I16 value")?;
                Ok(())
            } else {
                Err(format!("Index [{}, {}] out of bounds", row, col))
            }
        }
        (CalibrationValue::I32Matrix(mat), CalibrationType::I32Matrix(_, _)) => {
            if row < mat.len() && col < mat[row].len() {
                mat[row][col] = value_str.parse().map_err(|_| "Invalid I32 value")?;
                Ok(())
            } else {
                Err(format!("Index [{}, {}] out of bounds", row, col))
            }
        }
        (CalibrationValue::I64Matrix(mat), CalibrationType::I64Matrix(_, _)) => {
            if row < mat.len() && col < mat[row].len() {
                mat[row][col] = value_str.parse().map_err(|_| "Invalid I64 value")?;
                Ok(())
            } else {
                Err(format!("Index [{}, {}] out of bounds", row, col))
            }
        }
        (CalibrationValue::F32Matrix(mat), CalibrationType::F32Matrix(_, _)) => {
            if row < mat.len() && col < mat[row].len() {
                mat[row][col] = value_str.parse().map_err(|_| "Invalid F32 value")?;
                Ok(())
            } else {
                Err(format!("Index [{}, {}] out of bounds", row, col))
            }
        }
        (CalibrationValue::F64Matrix(mat), CalibrationType::F64Matrix(_, _)) => {
            if row < mat.len() && col < mat[row].len() {
                mat[row][col] = value_str.parse().map_err(|_| "Invalid F64 value")?;
                Ok(())
            } else {
                Err(format!("Index [{}, {}] out of bounds", row, col))
            }
        }
        (CalibrationValue::BoolMatrix(mat), CalibrationType::BoolMatrix(_, _)) => {
            if row < mat.len() && col < mat[row].len() {
                mat[row][col] = value_str
                    .parse()
                    .map_err(|_| "Invalid BOOL value (true/false)")?;
                Ok(())
            } else {
                Err(format!("Index [{}, {}] out of bounds", row, col))
            }
        }
        // Scalar types - row and col are ignored, just replace the value
        (CalibrationValue::U8(v), CalibrationType::U8) => {
            *v = value_str.parse().map_err(|_| "Invalid U8 value")?;
            Ok(())
        }
        (CalibrationValue::U16(v), CalibrationType::U16) => {
            *v = value_str.parse().map_err(|_| "Invalid U16 value")?;
            Ok(())
        }
        (CalibrationValue::U32(v), CalibrationType::U32) => {
            *v = value_str.parse().map_err(|_| "Invalid U32 value")?;
            Ok(())
        }
        (CalibrationValue::U64(v), CalibrationType::U64) => {
            *v = value_str.parse().map_err(|_| "Invalid U64 value")?;
            Ok(())
        }
        (CalibrationValue::I8(v), CalibrationType::I8) => {
            *v = value_str.parse().map_err(|_| "Invalid I8 value")?;
            Ok(())
        }
        (CalibrationValue::I16(v), CalibrationType::I16) => {
            *v = value_str.parse().map_err(|_| "Invalid I16 value")?;
            Ok(())
        }
        (CalibrationValue::I32(v), CalibrationType::I32) => {
            *v = value_str.parse().map_err(|_| "Invalid I32 value")?;
            Ok(())
        }
        (CalibrationValue::I64(v), CalibrationType::I64) => {
            *v = value_str.parse().map_err(|_| "Invalid I64 value")?;
            Ok(())
        }
        (CalibrationValue::F32(v), CalibrationType::F32) => {
            *v = value_str.parse().map_err(|_| "Invalid F32 value")?;
            Ok(())
        }
        (CalibrationValue::F64(v), CalibrationType::F64) => {
            *v = value_str.parse().map_err(|_| "Invalid F64 value")?;
            Ok(())
        }
        (CalibrationValue::Bool(v), CalibrationType::Bool) => {
            *v = value_str
                .parse()
                .map_err(|_| "Invalid BOOL value (true/false)")?;
            Ok(())
        }
        _ => Err("Type mismatch or unsupported type".to_string()),
    }
}

/// Get calibration parameter details (GET /api/cal/detail/:device_id/:idx)
/// Returns full parameter information for info dialog
pub async fn cal_detail_api(
    State(state): State<AppState>,
    Path((device_id_str, idx)): Path<(String, usize)>,
) -> impl IntoResponse {
    // Try to parse as UUID
    let uuid = match uuid::Uuid::parse_str(&device_id_str) {
        Ok(id) => id,
        Err(_) => {
            return Json(json!({
                "success": false,
                "error": "Invalid device ID"
            }));
        }
    };

    let slave_id = SlaveId(uuid);

    // First try to find as SlaveId
    if let Some(slave) = state.get_slave(slave_id).await {
        let cal_manager = slave.cal_manager.read().await;
        if let Some(param) = cal_manager.params().get(idx) {
            let param = param.clone();
            drop(cal_manager);

            // Build detail JSON with axis values from XCP if needed
            let mut detail = param.to_detail_json(
                &slave.id.to_string(),
                &slave.display_name(),
                idx,
                slave.is_connected(),
            );

            // Read axis values from XCP if connected and has memory-based axes
            if slave.is_connected() && param.has_axis() {
                let client = state.client.lock().await;
                if let Some(xcp_slave) = client.get_slave(slave_id) {
                    let xcp_slave_guard = xcp_slave.lock().await;
                    let axis_values = read_axis_values_from_slave(&xcp_slave_guard, &param).await;
                    if let serde_json::Value::Object(ref mut map) = detail {
                        map.insert("axis_values".to_string(), serde_json::json!(axis_values));
                    }
                }
            }

            return Json(json!({
                "success": true,
                "detail": detail,
                "var_type": "calibration"
            }));
        }
    }

    // Try to find as device ID
    if let Some(device) = state.get_device(&device_id_str).await {
        let cal_mgr = device.cal_manager.read().await;
        if let Some(param) = cal_mgr.params().get(idx) {
            return Json(json!({
                "success": true,
                "detail": param.to_detail_json(
                    &device.id,
                    &device.name,
                    idx,
                    device.connected
                ),
                "var_type": "calibration"
            }));
        }
    }

    Json(json!({
        "success": false,
        "error": "Parameter not found"
    }))
}

/// Get DAQ measurement details (GET /api/daq/detail/:device_id/:idx)
/// Returns full measurement information for info dialog
pub async fn daq_detail_api(
    State(state): State<AppState>,
    Path((device_id_str, idx)): Path<(String, usize)>,
) -> impl IntoResponse {
    // Try to parse as UUID
    let uuid = match uuid::Uuid::parse_str(&device_id_str) {
        Ok(id) => id,
        Err(_) => {
            return Json(json!({
                "success": false,
                "error": "Invalid device ID"
            }));
        }
    };

    let slave_id = SlaveId(uuid);

    // First try to find as SlaveId
    if let Some(slave) = state.get_slave(slave_id).await {
        let daq_manager = slave.daq_manager.read().await;
        if let Some(meas) = daq_manager.all_measurements().get(idx) {
            return Json(json!({
                "success": true,
                "detail": {
                    "device_id": slave.id.to_string(),
                    "device_name": slave.display_name(),
                    "idx": idx,
                    "name": meas.name,
                    "type": meas.data_type.type_name(),
                    "address": format!("0x{:08X}", meas.address),
                    "address_ext": meas.address_ext,
                    "unit": meas.unit.as_deref().unwrap_or("N/A"),
                    "data_size": format!("{} bytes", meas.size),
                },
                "var_type": "measurement"
            }));
        }
    }

    // Try to find as device ID
    if let Some(device) = state.get_device(&device_id_str).await {
        let daq_mgr = device.daq_manager.read().await;
        if let Some(meas) = daq_mgr.all_measurements().get(idx) {
            return Json(json!({
                "success": true,
                "detail": {
                    "device_id": device.id,
                    "device_name": device.name,
                    "idx": idx,
                    "name": meas.name,
                    "type": meas.data_type.type_name(),
                    "address": format!("0x{:08X}", meas.address),
                    "address_ext": meas.address_ext,
                    "unit": meas.unit.as_deref().unwrap_or("N/A"),
                    "data_size": format!("{} bytes", meas.size),
                },
                "var_type": "measurement"
            }));
        }
    }

    Json(json!({
        "success": false,
        "error": "Measurement not found"
    }))
}

// ---------------------------------------------------------------------------
// Checksum verification routes
// ---------------------------------------------------------------------------

/// Checksum page (GET /checksum) — redirects to /slaves
pub async fn checksum_page(
    State(_state): State<AppState>,
    Query(_params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    Redirect::to("/slaves")
}

/// HEX file upload API (POST /api/hex/load/:device_id)
pub async fn hex_load(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let mut file_data = None;
    let mut file_name = None;

    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        let name = field.name().unwrap_or("").to_string();

        if name == "hex_file" {
            if let Some(filename) = field.file_name() {
                file_name = Some(filename.to_string());
                match field.bytes().await {
                    Ok(bytes) => {
                        file_data = Some(bytes);
                    }
                    Err(e) => {
                        return Json(json!({
                            "success": false,
                            "error": format!("Failed to read file: {}", e)
                        }));
                    }
                }
            }
        }
    }

    let (fname, fdata) = match (file_name, file_data) {
        (Some(n), Some(d)) => (n, d),
        _ => {
            return Json(json!({
                "success": false,
                "error": "No HEX file uploaded"
            }));
        }
    };

    // Parse the HEX data
    let hex_str = match std::str::from_utf8(&fdata) {
        Ok(s) => s.to_string(),
        Err(e) => {
            return Json(json!({
                "success": false,
                "error": format!("Invalid UTF-8 in HEX file: {}", e)
            }));
        }
    };

    let hex_image =
        match tokio::task::spawn_blocking(move || crate::hex::HexImage::from_str(&hex_str)).await {
            Ok(Ok(image)) => image,
            Ok(Err(e)) => {
                return Json(json!({
                    "success": false,
                    "error": format!("HEX parse error: {}", e)
                }));
            }
            Err(e) => {
                return Json(json!({
                    "success": false,
                    "error": format!("Task error: {}", e)
                }));
            }
        };

    let byte_count = hex_image.total_bytes();

    // Store in device state
    if let Some(device) = state.get_device(&device_id).await {
        *device.hex_image.write().await = Some(hex_image);
        *device.hex_filename.write().await = Some(fname.clone());
        // Clear previous report when new HEX is loaded
        *device.checksum_report.write().await = None;
    } else {
        return Json(json!({
            "success": false,
            "error": "Device not found"
        }));
    }

    Json(json!({
        "success": true,
        "message": format!("{} loaded ({} bytes of data)", fname, byte_count)
    }))
}

/// Checksum verification API (POST /api/checksum/verify/:device_id)
pub async fn checksum_verify(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
) -> impl IntoResponse {
    // Get device
    let device = match state.get_device(&device_id).await {
        Some(d) => d,
        None => {
            return Json(json!({
                "success": false,
                "error": "Device not found"
            }));
        }
    };

    // Check connected
    if !device.connected {
        return Json(json!({
            "success": false,
            "error": "Device is not connected"
        }));
    }

    // Check HEX image loaded
    let hex_image = device.hex_image.read().await.clone();
    let hex_image = match hex_image {
        Some(img) => img,
        None => {
            return Json(json!({
                "success": false,
                "error": "No HEX file loaded for this device"
            }));
        }
    };

    // Get memory segments from A2L
    let segments = {
        let a2l_path = device.a2l_path.clone();
        if a2l_path.is_empty() {
            return Json(json!({
                "success": false,
                "error": "No A2L file loaded for this device"
            }));
        }

        use crate::A2lReader;
        match A2lReader::load(&a2l_path) {
            Ok(reader) => reader.get_memory_segments(),
            Err(e) => {
                return Json(json!({
                    "success": false,
                    "error": format!("Failed to load A2L: {:?}", e)
                }));
            }
        }
    };

    if segments.is_empty() {
        return Json(json!({
            "success": false,
            "error": "No memory segments found in A2L file"
        }));
    }

    // Get slave ID and run verification
    let slave_id = match device.slave_id {
        Some(id) => id,
        None => {
            return Json(json!({
                "success": false,
                "error": "Device has no slave ID"
            }));
        }
    };

    let report = {
        let client = state.client.lock().await;
        let slave_arc = match client.get_slave(slave_id) {
            Some(s) => s,
            None => {
                return Json(json!({
                    "success": false,
                    "error": "Slave not found in client"
                }));
            }
        };
        drop(client); // Release client lock before locking slave

        let mut slave = slave_arc.lock().await;
        match crate::checksum::verify_segments(&mut slave, &hex_image, &segments, None).await {
            Ok(report) => report,
            Err(e) => {
                return Json(json!({
                    "success": false,
                    "error": format!("Verification error: {}", e)
                }));
            }
        }
    };

    // Store report
    *device.checksum_report.write().await = Some(report.clone());

    let summary = report.summary();
    let all_passed = report.all_passed();

    if all_passed {
        state.set_status(summary).await;
    } else {
        state.set_error(summary).await;
    }

    Json(json!({
        "success": true,
        "all_passed": all_passed,
        "summary": report.summary(),
        "total": report.total_segments(),
        "passed": report.passed(),
        "failed": report.failed(),
    }))
}

// ============ Experiment Page Routes ============

/// Experiment page (GET /experiment)
pub async fn experiment_page(State(state): State<AppState>) -> Html<String> {
    let status = state.take_status().await;
    let error = state.take_error().await;

    let content = templates::experiment_page();
    Html(templates::page(
        "Experiment",
        &content,
        status.as_deref(),
        error.as_deref(),
    ))
}

/// Serve experiment help page (GET /experiment/help)
pub async fn experiment_help_page(State(state): State<AppState>) -> Html<String> {
    let status = state.take_status().await;
    let error = state.take_error().await;

    let content = templates::experiment_help_page();
    Html(templates::page(
        "Experiment Help",
        &content,
        status.as_deref(),
        error.as_deref(),
    ))
}

/// Get all variables (measurements + calibration) from all devices (GET /api/experiment/variables)
pub async fn experiment_variables(State(state): State<AppState>) -> impl IntoResponse {
    let mut measurements: Vec<serde_json::Value> = Vec::new();
    let mut calibrations: Vec<serde_json::Value> = Vec::new();

    let devices = state.all_devices().await;

    for device in &devices {
        let device_name = device.display_name_or_custom().await;
        let device_id = if let Some(slave_id) = device.slave_id {
            slave_id.to_string()
        } else {
            device.id.clone()
        };
        let connected = device.connected;

        // Get measurements
        let daq_mgr = device.daq_manager.read().await;
        for (i, m) in daq_mgr.all_measurements().iter().enumerate() {
            let meas_enum_options: Vec<serde_json::Value> = m
                .compu_method
                .enum_options()
                .map(|opts| {
                    opts.iter()
                        .map(|opt| json!({"raw": opt.raw_value, "text": opt.text_value}))
                        .collect()
                })
                .unwrap_or_default();
            measurements.push(json!({
                "device_id": device_id,
                "device_name": device_name,
                "idx": i,
                "name": m.name,
                "type": format!("{:?}", m.data_type),
                "address_hex": format!("{:08X}", m.address),
                "connected": connected,
                "unit": m.unit.as_deref().unwrap_or(""),
                "is_enum": m.compu_method.is_enum(),
                "enum_options": meas_enum_options,
            }));
        }

        // Get calibration parameters
        let cal_mgr = device.cal_manager.read().await;
        for (i, p) in cal_mgr.params().iter().enumerate() {
            calibrations.push(serialize_cal_param(
                p,
                &device_id,
                &device_name,
                i,
                connected,
            ));
        }
    }

    Json(json!({
        "success": true,
        "measurements": measurements,
        "calibrations": calibrations,
    }))
}

/// Start DAQ on specified slaves (POST /api/experiment/start)
pub async fn experiment_start(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    // payload: { "selections": { "device_id": [idx1, idx2, ...], ... } }
    let selections = match payload.get("selections").and_then(|v| v.as_object()) {
        Some(s) => s,
        None => {
            return Json(json!({
                "success": false,
                "error": "Missing 'selections' object"
            }));
        }
    };

    let client = state.client.lock().await;
    let mut errors = Vec::new();
    let mut started_count = 0;

    for (device_id_str, indices_val) in selections {
        let indices: Vec<usize> = match indices_val.as_array() {
            Some(arr) => arr
                .iter()
                .filter_map(|v| v.as_u64().map(|n| n as usize))
                .collect(),
            None => continue,
        };

        if indices.is_empty() {
            continue;
        }

        // Find the device
        let device = match state.get_device(device_id_str).await {
            Some(d) => d,
            None => {
                // Try as slave ID
                if let Ok(uuid) = uuid::Uuid::parse_str(device_id_str) {
                    let slave_id = SlaveId(uuid);
                    let devices = state.all_devices().await;
                    match devices.iter().find(|d| d.slave_id == Some(slave_id)) {
                        Some(d) => d.clone(),
                        None => {
                            errors.push(format!("Device {} not found", device_id_str));
                            continue;
                        }
                    }
                } else {
                    errors.push(format!("Device {} not found", device_id_str));
                    continue;
                }
            }
        };

        if !device.connected {
            errors.push(format!("{}: not connected", device.name));
            continue;
        }

        let slave_id = match device.slave_id {
            Some(id) => id,
            None => {
                errors.push(format!("{}: no slave ID", device.name));
                continue;
            }
        };

        // Set measurement selections
        {
            let mut daq_manager = device.daq_manager.write().await;
            daq_manager.deselect_all();
            for idx in &indices {
                daq_manager.select_measurement(*idx);
            }
        }

        // Start DAQ
        let xcp_slave = match client.get_slave(slave_id) {
            Some(s) => s,
            None => {
                errors.push(format!("{}: slave not found in client", device.name));
                continue;
            }
        };
        let xcp_slave_guard = xcp_slave.lock().await;
        let mut daq_manager = device.daq_manager.write().await;

        match daq_manager.start(&*xcp_slave_guard, 1000).await {
            Ok(_) => {
                *device.daq_running.write().await = true;
                if let Some(slave) = state.get_slave(slave_id).await {
                    *slave.daq_running.write().await = true;
                }
                started_count += 1;
                tracing::info!("Experiment DAQ started on {}", device.name);
            }
            Err(e) => {
                errors.push(format!("{}: {}", device.name, e));
            }
        }
    }

    if started_count > 0 {
        state.set_experiment_active(true).await;
    }

    if !errors.is_empty() {
        Json(json!({
            "success": started_count > 0,
            "started": started_count,
            "errors": errors
        }))
    } else if started_count == 0 {
        Json(json!({
            "success": false,
            "error": "No devices to start DAQ on"
        }))
    } else {
        Json(json!({ "success": true, "started": started_count }))
    }
}

/// Stop DAQ on all slaves (POST /api/experiment/stop)
pub async fn experiment_stop(State(state): State<AppState>) -> impl IntoResponse {
    let devices = state.all_devices().await;
    let client = state.client.lock().await;
    let mut errors = Vec::new();
    let mut stopped_count = 0;

    for device in &devices {
        if !*device.daq_running.read().await || !device.connected {
            continue;
        }

        let slave_id = match device.slave_id {
            Some(id) => id,
            None => continue,
        };

        // Stop recording first if active
        {
            let daq_manager = device.daq_manager.read().await;
            if daq_manager.is_recording() {
                if let Err(e) = daq_manager.stop_recording().await {
                    tracing::warn!("Failed to stop recording on {}: {}", device.name, e);
                }
            }
        }

        // Stop DAQ
        let xcp_slave = match client.get_slave(slave_id) {
            Some(s) => s,
            None => continue,
        };
        let xcp_slave_guard = xcp_slave.lock().await;
        let daq_manager = device.daq_manager.read().await;

        match daq_manager.stop(&*xcp_slave_guard).await {
            Ok(_) => {
                *device.daq_running.write().await = false;
                if let Some(slave) = state.get_slave(slave_id).await {
                    *slave.daq_running.write().await = false;
                }
                stopped_count += 1;
                tracing::info!("Experiment DAQ stopped on {}", device.name);
            }
            Err(e) => {
                errors.push(format!("{}: {}", device.name, e));
            }
        }
    }

    state.set_experiment_active(false).await;

    if !errors.is_empty() {
        Json(json!({
            "success": stopped_count > 0,
            "stopped": stopped_count,
            "errors": errors
        }))
    } else {
        Json(json!({ "success": true, "stopped": stopped_count }))
    }
}

/// Start MF4 recording (POST /api/experiment/record/start)
pub async fn experiment_record_start(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let title = payload
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("experiment");
    let dir = payload
        .get("dir")
        .and_then(|v| v.as_str())
        .unwrap_or("records");

    // Generate MF4 filename: {title}_{YYYYMMDD}_{HHMMSS}.mf4
    let now = chrono::Local::now();
    let timestamp = now.format("%Y%m%d_%H%M%S").to_string();
    let safe_title = title
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect::<String>();

    // Ensure output directory exists
    let dir_path = std::path::Path::new(dir);
    if !dir_path.exists() {
        if let Err(e) = std::fs::create_dir_all(dir_path) {
            return Json(json!({
                "success": false,
                "error": format!("Failed to create output directory: {}", e)
            }));
        }
    }

    let devices = state.all_devices().await;
    let client = state.client.lock().await;
    let mut errors = Vec::new();
    let mut started_count = 0;

    for device in &devices {
        if !*device.daq_running.read().await || !device.connected {
            continue;
        }

        let slave_id = match device.slave_id {
            Some(id) => id,
            None => continue,
        };

        let xcp_slave = match client.get_slave(slave_id) {
            Some(s) => s,
            None => continue,
        };
        let xcp_slave_guard = xcp_slave.lock().await;
        let mut daq_manager = device.daq_manager.write().await;

        // Set MF4 path with experiment title and device name
        let device_name = device.display_name_or_custom().await;
        let safe_device = device_name
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect::<String>();
        let mf4_filename = if devices.iter().filter(|d| d.connected).count() > 1 {
            format!("{}_{}_{}.mf4", safe_title, safe_device, timestamp)
        } else {
            format!("{}_{}.mf4", safe_title, timestamp)
        };
        let mf4_path = dir_path.join(&mf4_filename);
        daq_manager.set_mdf4_path(&mf4_path);

        // Also set CSV path alongside
        let csv_path = mf4_path.with_extension("csv");
        daq_manager.set_csv_path(&csv_path);

        match daq_manager.start_recording(&*xcp_slave_guard).await {
            Ok(_) => {
                started_count += 1;
                tracing::info!(
                    "Experiment recording started on {} → {}",
                    device_name,
                    mf4_path.display()
                );
            }
            Err(e) => {
                errors.push(format!("{}: {}", device_name, e));
            }
        }
    }

    if !errors.is_empty() {
        Json(json!({
            "success": started_count > 0,
            "started": started_count,
            "errors": errors
        }))
    } else if started_count == 0 {
        Json(json!({
            "success": false,
            "error": "No running DAQ found to record"
        }))
    } else {
        Json(json!({ "success": true, "started": started_count }))
    }
}

/// Stop MF4 recording (POST /api/experiment/record/stop)
pub async fn experiment_record_stop(State(state): State<AppState>) -> impl IntoResponse {
    let devices = state.all_devices().await;
    let mut errors = Vec::new();
    let mut stopped_count = 0;

    for device in &devices {
        let daq_manager = device.daq_manager.read().await;
        if !daq_manager.is_recording() {
            continue;
        }

        match daq_manager.stop_recording().await {
            Ok(_) => {
                stopped_count += 1;
                tracing::info!(
                    "Experiment recording stopped on {}",
                    device.display_name_or_custom().await
                );
            }
            Err(e) => {
                errors.push(format!(
                    "{}: {}",
                    device.display_name_or_custom().await,
                    e
                ));
            }
        }
    }

    if !errors.is_empty() {
        Json(json!({
            "success": stopped_count > 0,
            "stopped": stopped_count,
            "errors": errors
        }))
    } else {
        Json(json!({ "success": true, "stopped": stopped_count }))
    }
}

/// Get experiment status (GET /api/experiment/status)
pub async fn experiment_status(State(state): State<AppState>) -> impl IntoResponse {
    let devices = state.all_devices().await;
    let experiment_active = state.is_experiment_active().await;
    let mut daq_running_count = 0;
    let mut recording_count = 0;
    let mut device_statuses: Vec<serde_json::Value> = Vec::new();

    for device in &devices {
        let daq_running = *device.daq_running.read().await;
        let daq_mgr = device.daq_manager.read().await;
        let is_recording = daq_mgr.is_recording();
        let page_mode = *device.page_mode.read().await;
        let device_name = device.display_name_or_custom().await;
        let device_id = if let Some(slave_id) = device.slave_id {
            slave_id.to_string()
        } else {
            device.id.clone()
        };

        if daq_running {
            daq_running_count += 1;
        }
        if is_recording {
            recording_count += 1;
        }

        device_statuses.push(json!({
            "device_id": device_id,
            "device_name": device_name,
            "connected": device.connected,
            "daq_running": daq_running,
            "recording": is_recording,
            "page_mode": match page_mode {
                crate::slave::PageMode::Working => "working",
                crate::slave::PageMode::Reference => "reference",
            },
            "a2l_file": device.original_filename,
        }));
    }

    Json(json!({
        "success": true,
        "experiment_active": experiment_active,
        "daq_running_count": daq_running_count,
        "recording_count": recording_count,
        "devices": device_statuses,
    }))
}

/// Switch page mode on all slaves (POST /api/experiment/pagemode)
pub async fn experiment_pagemode(
    State(state): State<AppState>,
    Json(payload): Json<HashMap<String, String>>,
) -> impl IntoResponse {
    let mode_str = payload
        .get("mode")
        .cloned()
        .unwrap_or_else(|| "reference".to_string());
    let mode = match mode_str.as_str() {
        "working" => crate::slave::PageMode::Working,
        "reference" => crate::slave::PageMode::Reference,
        _ => {
            return Json(json!({
                "success": false,
                "error": "Invalid mode. Use 'working' or 'reference'."
            }));
        }
    };

    let devices = state.all_devices().await;
    let client = state.client.lock().await;
    let mut results: Vec<serde_json::Value> = Vec::new();
    let mut success_count = 0;
    let mut fail_count = 0;

    for device in &devices {
        if !device.connected {
            continue;
        }

        let slave_id = match device.slave_id {
            Some(id) => id,
            None => continue,
        };

        let xcp_slave = match client.get_slave(slave_id) {
            Some(s) => s,
            None => continue,
        };
        let mut xcp_slave_guard = xcp_slave.lock().await;

        match xcp_slave_guard.switch_all_pages(mode).await {
            Ok(_segments) => {
                *device.page_mode.write().await = mode;
                success_count += 1;
                results.push(json!({
                    "device_id": slave_id.to_string(),
                    "device_name": device.name,
                    "success": true,
                    "page_mode": mode_str,
                }));
            }
            Err(e) => {
                fail_count += 1;
                results.push(json!({
                    "device_id": slave_id.to_string(),
                    "device_name": device.name,
                    "success": false,
                    "error": e.to_string(),
                    "read_only": true,
                }));
            }
        }
    }

    Json(json!({
        "success": fail_count == 0,
        "switched": success_count,
        "failed": fail_count,
        "results": results,
        "mode": mode_str,
    }))
}

/// Rename a slave (POST /api/experiment/rename-slave/:device_id)
pub async fn experiment_rename_slave(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    Json(payload): Json<HashMap<String, String>>,
) -> impl IntoResponse {
    let new_name = match payload.get("name") {
        Some(name) if !name.trim().is_empty() => name.trim().to_string(),
        _ => {
            return Json(json!({
                "success": false,
                "error": "Name is required"
            }));
        }
    };

    let mut devices = state.devices.write().await;
    let found = devices.iter_mut().find(|d| {
        d.id == device_id || d.slave_id.map(|s| s.to_string()) == Some(device_id.clone())
    });

    match found {
        Some(device) => {
            *device.custom_name.write().await = Some(new_name.clone());
            Json(json!({
                "success": true,
                "name": new_name
            }))
        }
        None => Json(json!({
            "success": false,
            "error": "Device not found"
        })),
    }
}

/// Verify checksum on all connected slaves (POST /api/experiment/checksum)
pub async fn experiment_checksum(State(state): State<AppState>) -> impl IntoResponse {
    let devices = state.all_devices().await;
    let mut results: Vec<serde_json::Value> = Vec::new();

    for device in &devices {
        if !device.connected {
            continue;
        }

        let dev_id = device
            .slave_id
            .map(|s| s.to_string())
            .unwrap_or_else(|| device.id.clone());
        let dev_name = device.display_name_or_custom().await;

        let hex_image = device.hex_image.read().await.clone();
        let hex_image = match hex_image {
            Some(img) => img,
            None => {
                results.push(json!({
                    "device_id": dev_id,
                    "device_name": dev_name,
                    "success": false,
                    "error": "No HEX file loaded"
                }));
                continue;
            }
        };

        let slave_id = match device.slave_id {
            Some(id) => id,
            None => {
                results.push(json!({
                    "device_id": dev_id,
                    "device_name": dev_name,
                    "success": false,
                    "error": "No slave ID"
                }));
                continue;
            }
        };

        // Get memory segments from A2L
        let segments = {
            let a2l_path = device.a2l_path.clone();
            if a2l_path.is_empty() {
                results.push(json!({
                    "device_id": dev_id,
                    "device_name": dev_name,
                    "success": false,
                    "error": "No A2L file loaded"
                }));
                continue;
            }

            use crate::A2lReader;
            match A2lReader::load(&a2l_path) {
                Ok(reader) => reader.get_memory_segments(),
                Err(e) => {
                    results.push(json!({
                        "device_id": dev_id,
                        "device_name": dev_name,
                        "success": false,
                        "error": format!("Failed to load A2L: {:?}", e)
                    }));
                    continue;
                }
            }
        };

        if segments.is_empty() {
            results.push(json!({
                "device_id": dev_id,
                "device_name": dev_name,
                "success": false,
                "error": "No memory segments found"
            }));
            continue;
        }

        let report = {
            let client = state.client.lock().await;
            let slave_arc = match client.get_slave(slave_id) {
                Some(s) => s,
                None => {
                    results.push(json!({
                        "device_id": dev_id,
                        "device_name": dev_name,
                        "success": false,
                        "error": "Slave not found"
                    }));
                    continue;
                }
            };
            drop(client);

            let mut slave_guard = slave_arc.lock().await;
            crate::checksum::verify_segments(&mut slave_guard, &hex_image, &segments, None).await
        };

        match report {
            Ok(report) => {
                let passed = report.all_passed();
                *device.checksum_report.write().await = Some(report.clone());
                results.push(json!({
                    "device_id": dev_id,
                    "device_name": dev_name,
                    "success": true,
                    "passed": passed,
                    "segments": report.results.iter().map(|s| {
                        let mut seg = json!({
                            "name": s.name,
                            "address": format!("0x{:08X}", s.address),
                            "size": s.size,
                            "algorithm": s.algorithm,
                            "master_checksum": format!("0x{:08X}", s.master_checksum),
                            "slave_checksum": format!("0x{:08X}", s.slave_checksum),
                            "match": s.passed,
                            "status": format!("{}", s.status),
                        });
                        if let Some(chunks) = &s.chunks {
                            seg["chunks"] = json!(chunks.iter().map(|c| json!({
                                "offset": format!("0x{:08X}", c.offset),
                                "size": c.size,
                                "master_checksum": format!("0x{:08X}", c.master_checksum),
                                "slave_checksum": format!("0x{:08X}", c.slave_checksum),
                                "match": c.passed,
                            })).collect::<Vec<_>>());
                        }
                        seg
                    }).collect::<Vec<_>>(),
                }));
            }
            Err(e) => {
                results.push(json!({
                    "device_id": dev_id,
                    "device_name": dev_name,
                    "success": false,
                    "error": format!("{}", e),
                }));
            }
        }
    }

    let all_passed = results.iter().all(|r| {
        r.get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
            && r.get("passed")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
    });

    Json(json!({
        "success": true,
        "all_passed": all_passed,
        "results": results,
    }))
}

/// Return all calibration variables that have changed from their original values
/// GET /api/experiment/cal/changed
pub async fn experiment_cal_changed(State(state): State<AppState>) -> impl IntoResponse {
    let devices = state.all_devices().await;
    let mut device_results: Vec<serde_json::Value> = Vec::new();

    for device in &devices {
        if !device.connected {
            continue;
        }

        let slave_id = match device.slave_id {
            Some(id) => id,
            None => continue,
        };

        let Some(slave_state) = state.get_slave(slave_id).await else {
            continue;
        };

        let original_values = slave_state.cal_original_values.read().await;
        let cal_values = slave_state.cal_values.read().await;
        let cal_manager = slave_state.cal_manager.read().await;
        let params = cal_manager.params();

        let mut indices: Vec<usize> = original_values.keys().cloned().collect();
        indices.sort();

        let mut variables: Vec<serde_json::Value> = Vec::new();

        for idx in indices {
            let original_value = match original_values.get(&idx) {
                Some(v) => v.clone(),
                None => continue,
            };

            let param = match params.get(idx) {
                Some(p) => p,
                None => continue,
            };

            let current_value = cal_values
                .get(&param.name)
                .map(|v| v.format_scalar_str())
                .unwrap_or_default();

            // Skip scalars whose value is unchanged
            if !current_value.is_empty() && current_value == original_value {
                continue;
            }

            variables.push(json!({
                "idx": idx,
                "name": param.name,
                "description": param.description,
                "unit": param.unit.as_deref().unwrap_or(""),
                "type_name": param.data_type.type_name(),
                "characteristic_type": param.characteristic_type.as_str(),
                "original_value": original_value,
                "current_value": current_value,
            }));
        }

        let dev_id = device
            .slave_id
            .map(|s| s.to_string())
            .unwrap_or_else(|| device.id.clone());
        let dev_name = device.display_name_or_custom().await;

        device_results.push(json!({
            "device_id": dev_id,
            "device_name": dev_name,
            "variables": variables,
        }));
    }

    Json(json!({
        "success": true,
        "devices": device_results,
    }))
}

/// Generate and download a DCM (Konservierung Format 2.0) file
/// POST /api/experiment/dcm/export
pub async fn experiment_dcm_export(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> axum::response::Response {
    use axum::body::Body;
    use axum::http::{header, StatusCode};

    let title = payload
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("export")
        .to_string();

    let variables = match payload.get("variables").and_then(|v| v.as_array()) {
        Some(v) => v.clone(),
        None => {
            return (StatusCode::BAD_REQUEST, "Missing 'variables' field").into_response();
        }
    };

    let mut entries: Vec<(CalibrationParam, CalibrationValue)> = Vec::new();

    for var in &variables {
        let device_id_str = match var.get("device_id").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => continue,
        };
        let idx = match var.get("idx").and_then(|v| v.as_u64()) {
            Some(i) => i as usize,
            None => continue,
        };

        let uuid = match uuid::Uuid::parse_str(device_id_str) {
            Ok(id) => id,
            Err(_) => continue,
        };

        let slave_id = SlaveId(uuid);
        let Some(slave_state) = state.get_slave(slave_id).await else {
            continue;
        };

        let cal_manager = slave_state.cal_manager.read().await;
        let param = match cal_manager.params().get(idx).cloned() {
            Some(p) => p,
            None => continue,
        };
        drop(cal_manager);

        let cal_values = slave_state.cal_values.read().await;
        let value = match cal_values.get(&param.name).cloned() {
            Some(v) => v,
            None => continue,
        };
        drop(cal_values);

        entries.push((param, value));
    }

    let dcm_content = generate_dcm(&title, &entries);

    let now = chrono::Local::now();
    let timestamp = now.format("%Y%m%d_%H%M%S").to_string();
    let safe_title = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    let filename = format!("{}_{}.dcm", safe_title, timestamp);

    let headers = [
        (
            header::CONTENT_TYPE,
            "application/octet-stream".to_string(),
        ),
        (
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename),
        ),
    ];

    (StatusCode::OK, headers, Body::from(dcm_content.into_bytes())).into_response()
}

// ── DCM helpers ──────────────────────────────────────────────────────────────

/// Convert a scalar `CalibrationValue` to f64. Returns `None` for non-scalar variants.
fn cal_value_to_dcm_f64(value: &CalibrationValue) -> Option<f64> {
    match value {
        CalibrationValue::U8(v) => Some(*v as f64),
        CalibrationValue::U16(v) => Some(*v as f64),
        CalibrationValue::U32(v) => Some(*v as f64),
        CalibrationValue::U64(v) => Some(*v as f64),
        CalibrationValue::I8(v) => Some(*v as f64),
        CalibrationValue::I16(v) => Some(*v as f64),
        CalibrationValue::I32(v) => Some(*v as f64),
        CalibrationValue::I64(v) => Some(*v as f64),
        CalibrationValue::F32(v) => Some(*v as f64),
        CalibrationValue::F64(v) => Some(*v),
        CalibrationValue::Bool(v) => Some(if *v { 1.0 } else { 0.0 }),
        _ => None,
    }
}

/// Flatten a 1-D `CalibrationValue` into a `Vec<f64>`.
/// Returns `None` for scalar or matrix variants.
fn cal_value_to_dcm_f64_array(value: &CalibrationValue) -> Option<Vec<f64>> {
    match value {
        CalibrationValue::U8Array(a) => Some(a.iter().map(|v| *v as f64).collect()),
        CalibrationValue::U16Array(a) => Some(a.iter().map(|v| *v as f64).collect()),
        CalibrationValue::U32Array(a) => Some(a.iter().map(|v| *v as f64).collect()),
        CalibrationValue::U64Array(a) => Some(a.iter().map(|v| *v as f64).collect()),
        CalibrationValue::I8Array(a) => Some(a.iter().map(|v| *v as f64).collect()),
        CalibrationValue::I16Array(a) => Some(a.iter().map(|v| *v as f64).collect()),
        CalibrationValue::I32Array(a) => Some(a.iter().map(|v| *v as f64).collect()),
        CalibrationValue::I64Array(a) => Some(a.iter().map(|v| *v as f64).collect()),
        CalibrationValue::F32Array(a) => Some(a.iter().map(|v| *v as f64).collect()),
        CalibrationValue::F64Array(a) => Some(a.clone()),
        CalibrationValue::BoolArray(a) => Some(a.iter().map(|v| if *v { 1.0 } else { 0.0 }).collect()),
        _ => None,
    }
}

/// Convert a 2-D `CalibrationValue` into a row-major `Vec<Vec<f64>>`.
/// Returns `None` for non-matrix variants.
fn cal_value_to_dcm_f64_matrix(value: &CalibrationValue) -> Option<Vec<Vec<f64>>> {
    macro_rules! to_f64_matrix {
        ($m:expr) => {
            Some($m.iter().map(|r| r.iter().map(|v| *v as f64).collect()).collect())
        };
    }
    match value {
        CalibrationValue::U8Matrix(m) => to_f64_matrix!(m),
        CalibrationValue::U16Matrix(m) => to_f64_matrix!(m),
        CalibrationValue::U32Matrix(m) => to_f64_matrix!(m),
        CalibrationValue::U64Matrix(m) => to_f64_matrix!(m),
        CalibrationValue::I8Matrix(m) => to_f64_matrix!(m),
        CalibrationValue::I16Matrix(m) => to_f64_matrix!(m),
        CalibrationValue::I32Matrix(m) => to_f64_matrix!(m),
        CalibrationValue::I64Matrix(m) => to_f64_matrix!(m),
        CalibrationValue::F32Matrix(m) => Some(m.iter().map(|r| r.iter().map(|v| *v as f64).collect()).collect()),
        CalibrationValue::F64Matrix(m) => Some(m.clone()),
        CalibrationValue::BoolMatrix(m) => Some(m.iter().map(|r| r.iter().map(|v| if *v { 1.0 } else { 0.0 }).collect()).collect()),
        _ => None,
    }
}

/// Generate the full text of a DCM (Konservierung Format 2.0) file using the `dcm_utils` crate.
fn generate_dcm(_title: &str, entries: &[(CalibrationParam, CalibrationValue)]) -> String {
    use crate::a2l::CharacteristicType;
    use dcm_utils::block::Block;
    use dcm_utils::blocks::{FESTWERT, FESTWERTEBLOCK, GRUPPENKENNFELD, GRUPPENKENNLINIE};
    use dcm_utils::{write_dcm_data, DcmData};

    let mut data = DcmData::from_blocks(Default::default());

    for (param, value) in entries {
        let name = param.name.clone();
        let desc = param.description.clone();
        let unit = param.unit.clone().unwrap_or_default();

        let block_opt: Option<Block> = match param.characteristic_type {
            CharacteristicType::Value => cal_value_to_dcm_f64(value).map(|v| {
                Block::Constant(FESTWERT::from_f64(name.clone(), v, desc.clone(), unit.clone()))
            }),

            CharacteristicType::Curve => cal_value_to_dcm_f64_array(value).map(|values| {
                let n = values.len();
                let axis_info = param.axis_info.first();
                let axis = axis_info
                    .and_then(|a| a.get_static_values())
                    .unwrap_or_else(|| (0..n).map(|i| i as f64).collect());
                let axis_name = axis_info.map(|a| a.name.as_str()).unwrap_or("").to_string();
                let unit_x = axis_info
                    .and_then(|a| a.unit.as_deref())
                    .unwrap_or("")
                    .to_string();
                Block::Table(GRUPPENKENNLINIE::from_f64(
                    &name, &values, &desc, &unit, &unit_x, &axis_name, &axis,
                ))
            }),

            CharacteristicType::Map => cal_value_to_dcm_f64_matrix(value).map(|matrix| {
                let rows = matrix.len();
                let cols = matrix.first().map(|r| r.len()).unwrap_or(0);
                let x_axis_info = param.axis_info.first();
                let y_axis_info = param.axis_info.get(1);
                let x_axis = x_axis_info
                    .and_then(|a| a.get_static_values())
                    .unwrap_or_else(|| (0..cols).map(|i| i as f64).collect());
                let y_axis = y_axis_info
                    .and_then(|a| a.get_static_values())
                    .unwrap_or_else(|| (0..rows).map(|i| i as f64).collect());
                let x_name = x_axis_info.map(|a| a.name.as_str()).unwrap_or("").to_string();
                let y_name = y_axis_info.map(|a| a.name.as_str()).unwrap_or("").to_string();
                let unit_x = x_axis_info
                    .and_then(|a| a.unit.as_deref())
                    .unwrap_or("")
                    .to_string();
                let unit_y = y_axis_info
                    .and_then(|a| a.unit.as_deref())
                    .unwrap_or("")
                    .to_string();
                Block::Map(GRUPPENKENNFELD::from_f64(
                    &name, matrix, x_axis, y_axis, &x_name, &y_name, &desc, &unit, &unit_x,
                    &unit_y,
                ))
            }),

            CharacteristicType::ValBlk => {
                let flat = cal_value_to_dcm_f64_array(value).or_else(|| {
                    cal_value_to_dcm_f64_matrix(value)
                        .map(|m| m.into_iter().flatten().collect())
                });
                flat.map(|values| {
                    Block::ConstantBlock(FESTWERTEBLOCK::from_f64(
                        name.clone(),
                        values,
                        desc.clone(),
                        unit.clone(),
                    ))
                })
            }
        };

        if let Some(block) = block_opt {
            data.insert_block(block);
        }
    }

    // dcm_utils writes ./output/data.json in debug builds; pre-create the directory.
    #[cfg(debug_assertions)]
    let _ = std::fs::create_dir_all("./output");

    // Write to a temp file and read it back as a String
    let temp_path = std::env::temp_dir().join(format!("{}.dcm", uuid::Uuid::new_v4()));
    write_dcm_data(&data, &temp_path);
    let content = std::fs::read_to_string(&temp_path).unwrap_or_else(|e| {
        tracing::warn!("Failed to read generated DCM temp file: {}", e);
        String::new()
    });
    let _ = std::fs::remove_file(&temp_path);
    content
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a2l::CharacteristicType;
    use crate::calibration::{CalibrationParam, CalibrationValue, CalibrationType};

    // ── cal_value_to_dcm_f64 ─────────────────────────────────────────────────

    #[test]
    fn test_dcm_f64_scalar_types() {
        assert_eq!(cal_value_to_dcm_f64(&CalibrationValue::U8(42)), Some(42.0));
        assert_eq!(cal_value_to_dcm_f64(&CalibrationValue::I16(-100)), Some(-100.0));
        assert_eq!(cal_value_to_dcm_f64(&CalibrationValue::F32(1.5)), Some(1.5));
        assert_eq!(cal_value_to_dcm_f64(&CalibrationValue::F64(3.14)), Some(3.14));
        assert_eq!(cal_value_to_dcm_f64(&CalibrationValue::Bool(true)), Some(1.0));
        assert_eq!(cal_value_to_dcm_f64(&CalibrationValue::Bool(false)), Some(0.0));
    }

    #[test]
    fn test_dcm_f64_returns_none_for_non_scalars() {
        assert_eq!(
            cal_value_to_dcm_f64(&CalibrationValue::U8Array(vec![1, 2, 3])),
            None
        );
        assert_eq!(
            cal_value_to_dcm_f64(&CalibrationValue::F64Matrix(vec![vec![1.0, 2.0]])),
            None
        );
    }

    // ── cal_value_to_dcm_f64_array ───────────────────────────────────────────

    #[test]
    fn test_dcm_f64_array_conversions() {
        assert_eq!(
            cal_value_to_dcm_f64_array(&CalibrationValue::U8Array(vec![1, 2, 3])),
            Some(vec![1.0, 2.0, 3.0])
        );
        assert_eq!(
            cal_value_to_dcm_f64_array(&CalibrationValue::F32Array(vec![1.5, 2.5])),
            Some(vec![1.5, 2.5])
        );
        assert_eq!(
            cal_value_to_dcm_f64_array(&CalibrationValue::BoolArray(vec![true, false])),
            Some(vec![1.0, 0.0])
        );
    }

    #[test]
    fn test_dcm_f64_array_returns_none_for_scalars_and_matrices() {
        assert_eq!(cal_value_to_dcm_f64_array(&CalibrationValue::U8(5)), None);
        assert_eq!(
            cal_value_to_dcm_f64_array(&CalibrationValue::U16Matrix(vec![vec![1, 2], vec![3, 4]])),
            None
        );
    }

    // ── cal_value_to_dcm_f64_matrix ──────────────────────────────────────────

    #[test]
    fn test_dcm_f64_matrix_conversions() {
        let m = vec![vec![1u8, 2], vec![3, 4]];
        let result = cal_value_to_dcm_f64_matrix(&CalibrationValue::U8Matrix(m));
        assert_eq!(result, Some(vec![vec![1.0, 2.0], vec![3.0, 4.0]]));

        let fm = vec![vec![1.5f64, 2.5]];
        let result = cal_value_to_dcm_f64_matrix(&CalibrationValue::F64Matrix(fm.clone()));
        assert_eq!(result, Some(fm));

        let bm = vec![vec![true, false]];
        let result = cal_value_to_dcm_f64_matrix(&CalibrationValue::BoolMatrix(bm));
        assert_eq!(result, Some(vec![vec![1.0, 0.0]]));
    }

    #[test]
    fn test_dcm_f64_matrix_returns_none_for_non_matrices() {
        assert_eq!(cal_value_to_dcm_f64_matrix(&CalibrationValue::F64(1.0)), None);
        assert_eq!(
            cal_value_to_dcm_f64_matrix(&CalibrationValue::F64Array(vec![1.0, 2.0])),
            None
        );
    }

    // ── generate_dcm ─────────────────────────────────────────────────────────

    fn scalar_param(name: &str) -> CalibrationParam {
        CalibrationParam::new(name, 0x1000, 0, CalibrationType::F64)
            .with_description("test scalar")
            .with_unit("rpm")
    }

    fn array_param(name: &str, n: usize) -> CalibrationParam {
        let mut p =
            CalibrationParam::new(name, 0x2000, 0, CalibrationType::U8Array(n))
                .with_description("test array")
                .with_unit("km/h");
        p.characteristic_type = CharacteristicType::ValBlk;
        p
    }

    fn curve_param(name: &str, n: usize) -> CalibrationParam {
        let mut p = CalibrationParam::new(name, 0x3000, 0, CalibrationType::F64)
            .with_description("test curve")
            .with_unit("°C");
        p.characteristic_type = CharacteristicType::Curve;
        // No axis_info → synthetic integer axis used
        let _ = n;
        p
    }

    fn map_param(name: &str) -> CalibrationParam {
        let mut p = CalibrationParam::new(name, 0x4000, 0, CalibrationType::F64)
            .with_description("test map")
            .with_unit("bar");
        p.characteristic_type = CharacteristicType::Map;
        p
    }

    #[test]
    fn test_generate_dcm_scalar() {
        let entries = vec![(scalar_param("RPM_LIMIT"), CalibrationValue::F64(5500.0))];
        let dcm = generate_dcm("my_test", &entries);

        assert!(dcm.contains("KONSERVIERUNG_FORMAT 2.0"), "missing header");
        assert!(dcm.contains("FESTWERT RPM_LIMIT"), "missing FESTWERT block");
        assert!(dcm.contains("test scalar"), "missing description");
        assert!(dcm.contains("rpm"), "missing unit");
        assert!(dcm.contains("5500"), "missing value");
        assert!(dcm.contains("END"), "missing END keyword");
    }

    #[test]
    fn test_generate_dcm_val_blk() {
        let entries = vec![(
            array_param("LOOKUP_TBL", 4),
            CalibrationValue::F64Array(vec![1.0, 2.0, 3.0, 4.0]),
        )];
        let dcm = generate_dcm("test", &entries);

        assert!(dcm.contains("FESTWERTEBLOCK LOOKUP_TBL"), "missing FESTWERTEBLOCK");
        assert!(dcm.contains("km/h"), "missing unit");
    }

    #[test]
    fn test_generate_dcm_curve() {
        let entries = vec![(
            curve_param("TEMP_CURVE", 3),
            CalibrationValue::F64Array(vec![10.0, 20.0, 30.0]),
        )];
        let dcm = generate_dcm("test", &entries);

        assert!(dcm.contains("GRUPPENKENNLINIE TEMP_CURVE"), "missing GRUPPENKENNLINIE");
        assert!(dcm.contains("ST/X"), "missing X axis");
        assert!(dcm.contains("°C"), "missing unit");
    }

    #[test]
    fn test_generate_dcm_map() {
        let entries = vec![(
            map_param("PRESSURE_MAP"),
            CalibrationValue::F64Matrix(vec![vec![1.0, 2.0], vec![3.0, 4.0]]),
        )];
        let dcm = generate_dcm("test", &entries);

        assert!(dcm.contains("GRUPPENKENNFELD PRESSURE_MAP"), "missing GRUPPENKENNFELD");
        assert!(dcm.contains("ST/X"), "missing X axis");
        assert!(dcm.contains("ST/Y"), "missing Y axis");
        assert!(dcm.contains("bar"), "missing unit");
    }

    #[test]
    fn test_generate_dcm_empty() {
        let dcm = generate_dcm("empty_export", &[]);
        assert!(dcm.contains("KONSERVIERUNG_FORMAT 2.0"), "header always present");
    }

    #[test]
    fn test_generate_dcm_skips_type_mismatch() {
        // Scalar param with an array value → block should be skipped (no FESTWERT emitted)
        let entries = vec![(
            scalar_param("BROKEN"),
            CalibrationValue::F64Array(vec![1.0, 2.0]),
        )];
        let dcm = generate_dcm("test", &entries);
        assert!(!dcm.contains("FESTWERT BROKEN"), "should skip mismatched types");
    }
}
