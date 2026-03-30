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

use crate::calibration::{CalibrationManager, CalibrationValue};
use crate::daq::DaqManager;
use crate::slave::SlaveId;
use crate::transport::TransportConfig;
use crate::web::auth::{clear_session, try_create_session, verify_credentials};
use crate::web::state::{AppState, DeviceState, SlaveState};
use crate::web::templates;

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
                (
                    AppendHeaders([(SET_COOKIE, cookie)]),
                    Redirect::to("/"),
                )
                    .into_response()
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

    Html(templates::page("Dashboard", &content, status.as_deref(), error.as_deref()))
}

/// Connect page (GET /connect)
pub async fn connect_page(State(state): State<AppState>) -> Html<String> {
    // Get all devices
    let devices = state.all_devices().await;

    // Build device list for template
    let device_list: Vec<serde_json::Value> = devices
        .iter()
        .map(|d| {
            json!({
                "id": d.id,
                "name": d.name,
                "address": d.address,
                "a2l_name": d.original_filename,
                "connected": d.connected,
            })
        })
        .collect();

    // Calculate next ECU name
    let next_ecu_num = devices.len() + 1;
    let default_name = format!("ECU{}", next_ecu_num);

    let status = state.take_status().await;
    let error = state.take_error().await;

    let content = templates::connect_page_devices(&device_list, &default_name);
    Html(templates::page("Connect", &content, status.as_deref(), error.as_deref()))
}

/// Connect API (POST /api/connect)
pub async fn connect_api(
    State(state): State<AppState>,
    Form(form): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    let address = form
        .get("custom")
        .filter(|s| !s.is_empty())
        .or_else(|| form.get("address"))
        .cloned()
        .unwrap_or_else(|| "127.0.0.1:5555".to_string());

    let slave_name = form
        .get("name")
        .cloned()
        .unwrap_or_else(|| format!("Slave {}", address));

    let mut client = state.client.lock().await;

    let transport_config = TransportConfig::Udp { addr: address.clone() };

    match client.connect_slave(&slave_name, transport_config).await {
        Ok(slave_id) => {
            // Get the slave info from the connected slave
            if let Some(xcp_slave) = client.get_slave(slave_id) {
                let xcp_slave_guard = xcp_slave.lock().await;
                let info = xcp_slave_guard.info.clone();
                let connect_info = xcp_slave_guard.info.connect_info.clone();
                drop(xcp_slave_guard);

                // Create slave state
                let slave_state = SlaveState::new(slave_id, info);
                state.add_slave_state(slave_state).await;
                state.set_current_slave(Some(slave_id)).await;

                if let Some(ref ci) = connect_info {
                    state
                        .set_status(format!(
                            "Connected to {} (max CTO={}, max DTO={})",
                            slave_name, ci.max_cto, ci.max_dto
                        ))
                        .await;
                } else {
                    state.set_status(format!("Connected to {}", slave_name)).await;
                }
            }
        }
        Err(e) => {
            state.set_error(format!("Connection failed: {}", e)).await;
        }
    }

    Redirect::to("/")
}

/// Disconnect API (GET /api/disconnect)
/// Disconnects current slave
pub async fn disconnect_api(State(state): State<AppState>) -> impl IntoResponse {
    let current_id = *state.current_slave_id.read().await;

    if let Some(id) = current_id {
        let mut client = state.client.lock().await;

        if let Err(e) = client.disconnect_slave(id).await {
            state.set_error(format!("Disconnect error: {}", e)).await;
        } else {
            state.remove_slave_state(id).await;

            // Select another slave if available
            let remaining = state.all_slaves().await;
            if remaining.is_empty() {
                state.set_current_slave(None).await;
                state.set_status("All slaves disconnected".to_string()).await;
            } else {
                state.set_current_slave(Some(remaining[0].id)).await;
                state.set_status(format!("Disconnected, now using {}", remaining[0].display_name())).await;
            }
        }
    } else {
        state.set_error("No slave selected".to_string()).await;
    }

    Redirect::to("/")
}

/// Disconnect specific slave API (GET /api/disconnect/:id)
pub async fn disconnect_slave_api(
    State(state): State<AppState>,
    Path(id_str): Path<String>,
) -> impl IntoResponse {
    if let Ok(id) = uuid::Uuid::parse_str(&id_str) {
        let slave_id = SlaveId(id);
        let mut client = state.client.lock().await;

        if let Err(e) = client.disconnect_slave(slave_id).await {
            state.set_error(format!("Disconnect error: {}", e)).await;
        } else {
            state.remove_slave_state(slave_id).await;
            state.set_status("Slave disconnected".to_string()).await;
        }
    } else {
        state.set_error("Invalid slave ID".to_string()).await;
    }

    Redirect::to("/")
}

/// Select slave API (GET /api/slave/select/:id)
pub async fn select_slave_api(
    State(state): State<AppState>,
    Path(id_str): Path<String>,
) -> impl IntoResponse {
    if let Ok(id) = uuid::Uuid::parse_str(&id_str) {
        let slave_id = SlaveId(id);
        if state.get_slave(slave_id).await.is_some() {
            state.set_current_slave(Some(slave_id)).await;
            if let Some(slave) = state.get_slave(slave_id).await {
                state.set_status(format!("Selected: {}", slave.display_name())).await;
            }
        } else {
            state.set_error("Slave not found".to_string()).await;
        }
    } else {
        state.set_error("Invalid slave ID".to_string()).await;
    }

    Redirect::to("/")
}

// ============== Device Management Routes ==============

/// Connect all devices API (POST /api/devices/connect-all)
pub async fn connect_all_devices(State(state): State<AppState>) -> impl IntoResponse {
    let devices = state.all_devices().await;
    let mut client = state.client.lock().await;
    let mut connected_count = 0;
    let mut failed_devices: Vec<String> = Vec::new();

    for device in &devices {
        if device.connected {
            continue; // Already connected
        }

        let transport_config = TransportConfig::Udp {
            addr: device.address.clone(),
        };

        match client.connect_slave(&device.name, transport_config).await {
            Ok(slave_id) => {
                // Get slave info from XcpClient
                if let Some(xcp_slave) = client.get_slave(slave_id) {
                    let guard = xcp_slave.lock().await;
                    let info = guard.info.clone();
                    drop(guard);

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
                    };
                    state.add_slave_state(slave_state).await;

                    // Update device connection status
                    state.update_device_connection(&device.id, true, Some(slave_id)).await;

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
pub async fn connect_device(State(state): State<AppState>, Json(payload): Json<serde_json::Value>) -> impl IntoResponse {
    let device_id = payload.get("device_id")
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

    if device.connected {
        return Json(serde_json::json!({
            "success": false,
            "error": "Device already connected"
        }));
    }

    let transport_config = TransportConfig::Udp {
        addr: device.address.clone(),
    };

    let mut client = state.client.lock().await;

    match client.connect_slave(&device.name, transport_config).await {
        Ok(slave_id) => {
            // Get slave info from XcpClient
            let info = if let Some(xcp_slave) = client.get_slave(slave_id) {
                let guard = xcp_slave.lock().await;
                guard.info.clone()
            } else {
                return Json(serde_json::json!({
                    "success": false,
                    "error": "Failed to get slave info after connection",
                }));
            };

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
            };
            state.add_slave_state(slave_state).await;

            // Update device connection status
            state.update_device_connection(&device.id, true, Some(slave_id)).await;

            // If this is the first connection, set it as current
            if state.current_slave().await.is_none() {
                state.set_current_slave(Some(slave_id)).await;
            }

            Json(serde_json::json!({
                "success": true,
                "message": format!("Connected to {}", device.name),
            }))
        }
        Err(e) => {
            Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to connect: {}", e),
            }))
        }
    }
}

/// Disconnect device API (POST /api/devices/disconnect)
pub async fn disconnect_device(State(state): State<AppState>, Json(payload): Json<serde_json::Value>) -> impl IntoResponse {
    let device_id = payload.get("device_id")
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
    state.update_device_connection(&device.id, false, None).await;

    Json(serde_json::json!({
        "success": true,
        "message": format!("Disconnected from {}", device.name),
    }))
}

/// Remove device API (POST /api/devices/remove)
pub async fn remove_device(State(state): State<AppState>, Json(payload): Json<serde_json::Value>) -> impl IntoResponse {
    let device_id = payload.get("device_id")
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

/// Slaves list page (GET /slaves)
pub async fn slaves_page(State(state): State<AppState>) -> Html<String> {
    let slaves = state.all_slaves().await;
    let current_id = *state.current_slave_id.read().await;

    let status = state.take_status().await;
    let error = state.take_error().await;

    // Convert SlaveState to tuple format for template
    let mut slaves_data = Vec::new();
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

    let content = templates::slaves_page(&slaves_data, current_id);

    Html(templates::page("Slaves", &content, status.as_deref(), error.as_deref()))
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
    Html(templates::page("Calibration", &content, status.as_deref(), error.as_deref()))
}

/// Get available calibration variables (GET /api/cal/available)
pub async fn cal_available(State(state): State<AppState>) -> impl IntoResponse {
    let mut variables: Vec<serde_json::Value> = Vec::new();

    // First check connected slaves
    let slaves = state.all_slaves().await;
    for slave in &slaves {
        let cal_manager = slave.cal_manager.read().await;
        for (i, p) in cal_manager.params().iter().enumerate() {
            variables.push(json!({
                "device_id": slave.id.to_string(),
                "device_name": slave.display_name(),
                "idx": i,
                "name": p.name,
                "type": p.data_type.type_name(),
                "address_hex": format!("{:08X}", p.address),
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

        let cal_mgr = device.cal_manager.read().await;
        for (i, p) in cal_mgr.params().iter().enumerate() {
            variables.push(json!({
                "device_id": device.id.clone(),
                "device_name": device.name.clone(),
                "idx": i,
                "name": p.name,
                "type": p.data_type.type_name(),
                "address_hex": format!("{:08X}", p.address),
                "connected": device.connected,
            }));
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
                    slave.cal_values.write().await.insert(param.name.clone(), value.clone());
                    return Json(json!({
                        "success": true,
                        "name": param.name,
                        "value": value_str,
                        "device_name": slave.display_name()
                    }));
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
    use crate::calibration::{CalibrationType, CalibrationValue};
    use crate::protocol::Mta;

    let mta = Mta::new(param.address, param.address_ext);
    let size = param.data_type.size();

    tracing::debug!("Reading {} bytes from address 0x{:08X} (ext: {})", size, mta.address, mta.extension);

    // Check for zero size (invalid matrix_dim issue)
    if size == 0 {
        return Err(format!(
            "Invalid parameter size (0 bytes). Check A2L matrix_dim for '{}'",
            param.name
        ));
    }

    // Use slave's actual maxCTO from CONNECT response (not A2L's MAX_CTO)
    // This ensures we use the correct chunk size for UPLOAD operations
    let data = slave.read_large_data(mta, size, None).await.map_err(|e| e.to_string())?;

    tracing::debug!("Read {} bytes, expected {}", data.len(), size);

    // Verify data length
    if data.len() != size {
        return Err(format!(
            "Data length mismatch: expected {} bytes, got {}",
            size, data.len()
        ));
    }

    // Parse based on type
    let value = match &param.data_type {
        // Scalar types
        CalibrationType::U8 => CalibrationValue::U8(data[0]),
        CalibrationType::U16 => {
            CalibrationValue::U16(u16::from_le_bytes([data[0], data[1]]))
        }
        CalibrationType::U32 => {
            CalibrationValue::U32(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
        }
        CalibrationType::U64 => {
            CalibrationValue::U64(u64::from_le_bytes([data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7]]))
        }
        CalibrationType::I8 => CalibrationValue::I8(data[0] as i8),
        CalibrationType::I16 => {
            CalibrationValue::I16(i16::from_le_bytes([data[0], data[1]]))
        }
        CalibrationType::I32 => {
            CalibrationValue::I32(i32::from_le_bytes([data[0], data[1], data[2], data[3]]))
        }
        CalibrationType::I64 => {
            CalibrationValue::I64(i64::from_le_bytes([data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7]]))
        }
        CalibrationType::F32 => {
            CalibrationValue::F32(f32::from_le_bytes([data[0], data[1], data[2], data[3]]))
        }
        CalibrationType::F64 => {
            CalibrationValue::F64(f64::from_le_bytes([data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7]]))
        }
        CalibrationType::Bool => CalibrationValue::Bool(data[0] != 0),

        // Array types
        CalibrationType::U8Array(_len) => {
            CalibrationValue::U8Array(data.clone())
        }
        CalibrationType::U16Array(_len) => {
            let vals: Vec<u16> = data.chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            CalibrationValue::U16Array(vals)
        }
        CalibrationType::U32Array(_len) => {
            let vals: Vec<u32> = data.chunks_exact(4)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            CalibrationValue::U32Array(vals)
        }
        CalibrationType::U64Array(_len) => {
            let vals: Vec<u64> = data.chunks_exact(8)
                .map(|c| u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
                .collect();
            CalibrationValue::U64Array(vals)
        }
        CalibrationType::I8Array(_len) => {
            CalibrationValue::I8Array(data.iter().map(|&v| v as i8).collect())
        }
        CalibrationType::I16Array(_len) => {
            let vals: Vec<i16> = data.chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect();
            CalibrationValue::I16Array(vals)
        }
        CalibrationType::I32Array(_len) => {
            let vals: Vec<i32> = data.chunks_exact(4)
                .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            CalibrationValue::I32Array(vals)
        }
        CalibrationType::I64Array(_len) => {
            let vals: Vec<i64> = data.chunks_exact(8)
                .map(|c| i64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
                .collect();
            CalibrationValue::I64Array(vals)
        }
        CalibrationType::F32Array(_len) => {
            let vals: Vec<f32> = data.chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            CalibrationValue::F32Array(vals)
        }
        CalibrationType::F64Array(_len) => {
            let vals: Vec<f64> = data.chunks_exact(8)
                .map(|c| f64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
                .collect();
            CalibrationValue::F64Array(vals)
        }
        CalibrationType::BoolArray(_len) => {
            CalibrationValue::BoolArray(data.iter().map(|&v| v != 0).collect())
        }

        // Matrix types - flatten to 2D vectors
        CalibrationType::U8Matrix(_rows, cols) => {
            let matrix: Vec<Vec<u8>> = data.chunks_exact(*cols)
                .map(|c| c.to_vec())
                .collect();
            CalibrationValue::U8Matrix(matrix)
        }
        CalibrationType::U16Matrix(_rows, cols) => {
            let matrix: Vec<Vec<u16>> = data.chunks_exact(cols * 2)
                .map(|row| {
                    row.chunks_exact(2)
                        .map(|c| u16::from_le_bytes([c[0], c[1]]))
                        .collect()
                })
                .collect();
            CalibrationValue::U16Matrix(matrix)
        }
        CalibrationType::U32Matrix(_rows, cols) => {
            let matrix: Vec<Vec<u32>> = data.chunks_exact(cols * 4)
                .map(|row| {
                    row.chunks_exact(4)
                        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                        .collect()
                })
                .collect();
            CalibrationValue::U32Matrix(matrix)
        }
        CalibrationType::U64Matrix(_rows, cols) => {
            let matrix: Vec<Vec<u64>> = data.chunks_exact(cols * 8)
                .map(|row| {
                    row.chunks_exact(8)
                        .map(|c| u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
                        .collect()
                })
                .collect();
            CalibrationValue::U64Matrix(matrix)
        }
        CalibrationType::I8Matrix(_rows, cols) => {
            let matrix: Vec<Vec<i8>> = data.chunks_exact(*cols)
                .map(|row| row.iter().map(|&v| v as i8).collect())
                .collect();
            CalibrationValue::I8Matrix(matrix)
        }
        CalibrationType::I16Matrix(_rows, cols) => {
            let matrix: Vec<Vec<i16>> = data.chunks_exact(cols * 2)
                .map(|row| {
                    row.chunks_exact(2)
                        .map(|c| i16::from_le_bytes([c[0], c[1]]))
                        .collect()
                })
                .collect();
            CalibrationValue::I16Matrix(matrix)
        }
        CalibrationType::I32Matrix(_rows, cols) => {
            let matrix: Vec<Vec<i32>> = data.chunks_exact(cols * 4)
                .map(|row| {
                    row.chunks_exact(4)
                        .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                        .collect()
                })
                .collect();
            CalibrationValue::I32Matrix(matrix)
        }
        CalibrationType::I64Matrix(_rows, cols) => {
            let matrix: Vec<Vec<i64>> = data.chunks_exact(cols * 8)
                .map(|row| {
                    row.chunks_exact(8)
                        .map(|c| i64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
                        .collect()
                })
                .collect();
            CalibrationValue::I64Matrix(matrix)
        }
        CalibrationType::F32Matrix(_rows, cols) => {
            let matrix: Vec<Vec<f32>> = data.chunks_exact(cols * 4)
                .map(|row| {
                    row.chunks_exact(4)
                        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                        .collect()
                })
                .collect();
            CalibrationValue::F32Matrix(matrix)
        }
        CalibrationType::F64Matrix(_rows, cols) => {
            let matrix: Vec<Vec<f64>> = data.chunks_exact(cols * 8)
                .map(|row| {
                    row.chunks_exact(8)
                        .map(|c| f64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
                        .collect()
                })
                .collect();
            CalibrationValue::F64Matrix(matrix)
        }
        CalibrationType::BoolMatrix(_rows, cols) => {
            let matrix: Vec<Vec<bool>> = data.chunks_exact(*cols)
                .map(|row| row.iter().map(|&v| v != 0).collect())
                .collect();
            CalibrationValue::BoolMatrix(matrix)
        }
    };

    Ok(value)
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
        let content = templates::cal_write_page(
            &device_id_str,
            idx,
            &param.name,
            current.as_deref(),
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
        state.set_error("Device not found or not connected".to_string()).await;
        return Redirect::to("/calibration");
    };

    if !slave.is_connected() {
        state.set_error("Not connected to XCP slave".to_string()).await;
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
                    }.await;

                    match result {
                        Ok(_) => {
                            state
                                .set_status(format!("[{}] Wrote {} = {}", slave.display_name(), param.name, value_str))
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

/// Helper function to write calibration to a slave
async fn write_calibration_to_slave(
    slave: &crate::slave::XcpSlave,
    param: &crate::calibration::CalibrationParam,
    value: &CalibrationValue,
) -> Result<(), String> {
    use crate::calibration::CalibrationType;
    use crate::protocol::Mta;

    let mta = Mta::new(param.address, param.address_ext);

    // Convert value to bytes
    let data: Vec<u8> = match (value, &param.data_type) {
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
        _ => return Err(format!("Value type mismatch for {:?}", param.data_type)),
    };

    // Write data using SHORT_DOWNLOAD for small writes
    if data.len() <= 255 {
        slave.write_memory(mta, &data).await.map_err(|e| e.to_string())
    } else {
        // Use set_mta + download for larger writes
        slave.write_u8_array(mta, &data).await.map_err(|e| e.to_string())
    }
}

/// DAQ page (GET /daq)
pub async fn daq_page(State(state): State<AppState>) -> Html<String> {
    let status = state.take_status().await;
    let error = state.take_error().await;

    // Render empty DAQ page - variables loaded via API
    let content = templates::daq_page(
        &[],  // measurements
        &[],  // selected_indices
        false, // daq_running
        &[],   // latest_values
        &[],   // slaves
        None,  // current_slave_id
    );
    Html(templates::page("DAQ", &content, status.as_deref(), error.as_deref()))
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

    let client = state.client.lock().await;
    let mut daq_manager = slave.daq_manager.write().await;

    // Use the client to start DAQ (DAQ manager needs XcpClient)
    match daq_manager.start(&*client, 1000).await {
        Ok(_) => {
            *slave.daq_running.write().await = true;
            state.set_status(format!("DAQ started on {}", slave_name)).await;
            Json(json!({ "success": true }))
        }
        Err(e) => {
            Json(json!({
                "success": false,
                "error": format!("DAQ start failed: {}", e)
            }))
        }
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

    let client = state.client.lock().await;
    let daq_manager = slave.daq_manager.write().await;

    match daq_manager.stop(&*client).await {
        Ok(_) => {
            *slave.daq_running.write().await = false;
            state.set_status(format!("DAQ stopped on {}", slave_name)).await;
            Json(json!({ "success": true }))
        }
        Err(e) => {
            Json(json!({
                "success": false,
                "error": format!("DAQ stop failed: {}", e)
            }))
        }
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

    let running = if let Some(ref slave) = current_slave {
        *slave.daq_running.read().await
    } else {
        false
    };

    Json(json!({
        "running": running
    }))
}

/// DAQ toggle measurement (POST /api/daq/toggle)
pub async fn daq_toggle(
    State(state): State<AppState>,
    Form(form): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    // Support both slave_id and device_id parameters
    let device_id_str = form.get("device_id").or_else(|| form.get("slave_id")).cloned();
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
        state.set_status(format!("DAQ data cleared for {}", slave.display_name())).await;
    } else {
        // Clear all slaves
        let slaves = state.all_slaves().await;
        for slave in slaves {
            let mut daq_manager = slave.daq_manager.write().await;
            daq_manager.clear_samples();
        }
        state.set_status("DAQ data cleared for all slaves".to_string()).await;
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

    // Get values from all running slaves
    let slaves = state.all_slaves().await;
    for slave in slaves {
        if *slave.daq_running.read().await {
            let daq_manager = slave.daq_manager.read().await;
            if let Some(r) = daq_manager.get_latest_sample().await {
                for (k, v) in r.values {
                    let device_id = slave.id.to_string();

                    // Filter by selected variables if any
                    if filter_set.is_empty() || filter_set.contains(&(device_id.clone(), k.clone())) {
                        values.push(json!({
                            "device_id": device_id,
                            "var_name": k,
                            "display_name": slave.display_name(),
                            "value": format!("{:.6}", v)
                        }));
                    }
                }
            }
        }
    }

    Json(json!({
        "success": true,
        "values": values
    }))
}

/// A2L page (GET /a2l)
pub async fn a2l_page(State(state): State<AppState>) -> Html<String> {
    let devices = state.all_devices().await;

    // Get path from current device or first device
    let (path, cal_count, meas_count) = if let Some(current_device) = state.current_device().await {
        let cal_mgr = current_device.cal_manager.read().await;
        let daq_mgr = current_device.daq_manager.read().await;
        (current_device.a2l_path.clone(), cal_mgr.params().len(), daq_mgr.available_count())
    } else if let Some(first_device) = devices.first() {
        let cal_mgr = first_device.cal_manager.read().await;
        let daq_mgr = first_device.daq_manager.read().await;
        (first_device.a2l_path.clone(), cal_mgr.params().len(), daq_mgr.available_count())
    } else {
        (String::new(), 0, 0)
    };

    // Get slave addresses from devices
    let slave_addresses: Vec<String> = devices.iter().map(|d| d.address.clone()).collect();

    // Build current device info for template
    let current_device_info = state.current_device().await.map(|d| {
        (d.id.clone(), d.name.clone())
    });

    let status = state.take_status().await;
    let error = state.take_error().await;

    let content = templates::a2l_page(&path, current_device_info.as_ref(), &slave_addresses, cal_count, meas_count);
    Html(templates::page("A2L", &content, status.as_deref(), error.as_deref()))
}

/// A2L load API (POST /api/a2l/load)
pub async fn a2l_load(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let mut file_data = None;
    let mut file_name = None;

    // Process multipart form data
    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        let name = field.name().unwrap_or("").to_string();

        if name == "a2l_file" {
            // Get the filename and file content
            if let Some(filename) = field.file_name() {
                file_name = Some(filename.to_string());
                // Read the file content
                match field.bytes().await {
                    Ok(bytes) => {
                        file_data = Some(bytes);
                    }
                    Err(e) => {
                        return axum::Json(serde_json::json!({
                            "success": false,
                            "error": format!("Failed to read file: {}", e)
                        }));
                    }
                }
            }
        }
    }

    // Save uploaded file to temp location and load it
    let temp_path = if let (Some(name), Some(data)) = (&file_name, &file_data) {
        // Create temp directory if not exists
        let temp_dir = std::env::temp_dir().join("xcp_client");
        if let Err(e) = tokio::fs::create_dir_all(&temp_dir).await {
            return axum::Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to create temp dir: {}", e)
            }));
        }

        // Generate unique filename to avoid collisions
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let safe_name = name.replace(['/', '\\', ':'], "_");
        let temp_file = temp_dir.join(format!("{}_{}", timestamp, safe_name));

        // Write file content
        if let Err(e) = tokio::fs::write(&temp_file, data).await {
            return axum::Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to save temp file: {}", e)
            }));
        }

        Some(temp_file)
    } else {
        None
    };

    let path = temp_path;

    if let Some(path) = path {
        use crate::A2lReader;

        // Parse A2L file to get device address
        let (reader, addresses) = match tokio::task::spawn_blocking({
            let path = path.clone();
            move || {
                // Load A2L and get addresses without using ? in closure
                let reader_result = A2lReader::load(&path);
                match reader_result {
                    Ok(reader) => {
                        let addrs = reader.get_slave_addresses();
                        Ok((reader, addrs))
                    }
                    Err(e) => Err(format!("A2L parse error: {:?}", e)),
                }
            }
        }).await {
            Ok(Ok(result)) => result,
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

        // Get device address from A2L
        let device_addr = addresses.first().cloned().unwrap_or_else(|| "127.0.0.1:5555".to_string());

        // Check for duplicate IP address
        if state.address_exists(&device_addr).await {
            return axum::Json(serde_json::json!({
                "success": false,
                "error": format!(
                    "Device with address {} already exists. Each device must have a unique IP address.",
                    device_addr
                )
            }));
        }

        // Get A2L data
        let characteristics = reader.get_characteristics();
        let measurements = reader.get_measurements();

        // Get MAX_CTO from A2L (default to 255 if not specified)
        let max_cto = reader.get_max_cto().unwrap_or(255);

        // Store counts before consuming vectors
        let cal_count = characteristics.len();
        let meas_count = measurements.len();

        // Generate device ID and name
        let device_id = uuid::Uuid::new_v4().to_string();

        // Use original filename from user upload
        let original_filename = file_name.clone().unwrap_or_else(|| "unknown.a2l".to_string());

        // Get current device count for naming (synchronous approximation)
        let device_count = state.devices.read().await.len();
        let device_name = std::path::Path::new(&original_filename)
            .file_stem()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("Device{}", device_count + 1));

        // Create calibration and DAQ managers for this device
        let mut cal_manager = CalibrationManager::new();
        for ch in characteristics {
            if let Some(param) = convert_characteristic(ch) {
                cal_manager.add_param(param);
            }
        }

        let mut daq_manager = DaqManager::new();
        for m in measurements {
            if let Some(meas) = convert_measurement(m) {
                daq_manager.add_measurement(meas);
            }
        }

        // Create and add device
        let device = DeviceState {
            id: device_id.clone(),
            name: device_name,
            address: device_addr.clone(),
            a2l_path: path.to_string_lossy().to_string(),
            original_filename: original_filename.clone(),
            max_cto,
            connected: false,
            slave_id: None,
            cal_manager: Arc::new(RwLock::new(cal_manager)),
            daq_manager: Arc::new(RwLock::new(daq_manager)),
            cal_values: Arc::new(RwLock::new(HashMap::new())),
            daq_running: Arc::new(RwLock::new(false)),
        };

        state.add_device(device).await;

        state.set_status(format!(
            "Added device {} with address {} ({} cal params, {} measurements)",
            original_filename, device_addr, cal_count, meas_count
        )).await;

        return axum::Json(serde_json::json!({
            "success": true,
            "message": format!(
                "Added device {} with address {}",
                original_filename, device_addr
            ),
            "path": path,
            "cal_count": cal_count,
            "meas_count": meas_count,
            "device_id": device_id,
            "device_address": device_addr,
        }));
    }

    axum::Json(serde_json::json!({
        "success": false,
        "error": "No file provided"
    }))
}

/// Convert A2L characteristic to calibration parameter
fn convert_characteristic(
    ch: crate::a2l_reader::A2lCharacteristic,
) -> Option<crate::calibration::CalibrationParam> {
    use crate::calibration::{CalibrationParam, CalibrationType};

    let data_type = match ch.data_type {
        crate::a2l_reader::A2lDataType::U8 => CalibrationType::U8,
        crate::a2l_reader::A2lDataType::U16 => CalibrationType::U16,
        crate::a2l_reader::A2lDataType::U32 => CalibrationType::U32,
        crate::a2l_reader::A2lDataType::U64 => CalibrationType::U64,
        crate::a2l_reader::A2lDataType::I8 => CalibrationType::I8,
        crate::a2l_reader::A2lDataType::I16 => CalibrationType::I16,
        crate::a2l_reader::A2lDataType::I32 => CalibrationType::I32,
        crate::a2l_reader::A2lDataType::I64 => CalibrationType::I64,
        crate::a2l_reader::A2lDataType::F32 => CalibrationType::F32,
        crate::a2l_reader::A2lDataType::F64 => CalibrationType::F64,
        crate::a2l_reader::A2lDataType::Bool => CalibrationType::Bool,
        crate::a2l_reader::A2lDataType::U8Array(n) => CalibrationType::U8Array(n),
        crate::a2l_reader::A2lDataType::U16Array(n) => CalibrationType::U16Array(n),
        crate::a2l_reader::A2lDataType::U32Array(n) => CalibrationType::U32Array(n),
        crate::a2l_reader::A2lDataType::U64Array(n) => CalibrationType::U64Array(n),
        crate::a2l_reader::A2lDataType::I8Array(n) => CalibrationType::I8Array(n),
        crate::a2l_reader::A2lDataType::I16Array(n) => CalibrationType::I16Array(n),
        crate::a2l_reader::A2lDataType::I32Array(n) => CalibrationType::I32Array(n),
        crate::a2l_reader::A2lDataType::I64Array(n) => CalibrationType::I64Array(n),
        crate::a2l_reader::A2lDataType::F32Array(n) => CalibrationType::F32Array(n),
        crate::a2l_reader::A2lDataType::F64Array(n) => CalibrationType::F64Array(n),
        crate::a2l_reader::A2lDataType::BoolArray(n) => CalibrationType::BoolArray(n),
        crate::a2l_reader::A2lDataType::U8Matrix(r, c) => CalibrationType::U8Matrix(r, c),
        crate::a2l_reader::A2lDataType::U16Matrix(r, c) => CalibrationType::U16Matrix(r, c),
        crate::a2l_reader::A2lDataType::U32Matrix(r, c) => CalibrationType::U32Matrix(r, c),
        crate::a2l_reader::A2lDataType::U64Matrix(r, c) => CalibrationType::U64Matrix(r, c),
        crate::a2l_reader::A2lDataType::I8Matrix(r, c) => CalibrationType::I8Matrix(r, c),
        crate::a2l_reader::A2lDataType::I16Matrix(r, c) => CalibrationType::I16Matrix(r, c),
        crate::a2l_reader::A2lDataType::I32Matrix(r, c) => CalibrationType::I32Matrix(r, c),
        crate::a2l_reader::A2lDataType::I64Matrix(r, c) => CalibrationType::I64Matrix(r, c),
        crate::a2l_reader::A2lDataType::F32Matrix(r, c) => CalibrationType::F32Matrix(r, c),
        crate::a2l_reader::A2lDataType::F64Matrix(r, c) => CalibrationType::F64Matrix(r, c),
        crate::a2l_reader::A2lDataType::BoolMatrix(r, c) => CalibrationType::BoolMatrix(r, c),
    };

    let mut param = CalibrationParam::new(&ch.name, ch.address, ch.address_ext, data_type)
        .with_description(&ch.description)
        .with_range(ch.min, ch.max);

    if let Some(unit) = ch.unit {
        param = param.with_unit(&unit);
    }

    Some(param)
}

/// Convert A2L measurement to DAQ measurement
fn convert_measurement(
    m: crate::a2l_reader::A2lMeasurement,
) -> Option<crate::daq::Measurement> {
    Some(crate::daq::Measurement::with_type(
        &m.name,
        m.address,
        m.address_ext,
        m.data_type,
        m.unit,
        crate::daq::CompuMethod::Identical,
    ))
}
