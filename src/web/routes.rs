//! HTTP Routes
//!
//! All route handlers for the web interface.
//! Multi-slave support: routes now handle multiple XCP slave connections.

use axum::{
    extract::{Form, Path, Query, State},
    http::header::SET_COOKIE,
    response::{AppendHeaders, Html, IntoResponse, Redirect},
};
use std::collections::HashMap;

use crate::calibration::{CalibrationManager, CalibrationValue};
use crate::slave::{SlaveId, SlaveInfo};
use crate::transport::TransportConfig;
use crate::web::auth::{clear_session, try_create_session, verify_credentials};
use crate::web::state::{AppState, SlaveState};
use crate::web::templates;

/// Login page (GET /login)
pub async fn login_page(Query(params): Query<HashMap<String, String>>) -> Html<String> {
    let error = params.get("error").map(|s| s.as_str());
    Html(templates::login_page(error))
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
    // Get unique addresses from all loaded A2L files
    let slaves = state.all_slaves().await;
    let mut slave_addrs: Vec<String> = slaves
        .iter()
        .filter_map(|s| {
            if s.is_connected() {
                Some(s.info.addr_string())
            } else {
                None
            }
        })
        .collect();

    // Add default addresses
    slave_addrs.push("127.0.0.1:5555".to_string());
    slave_addrs.push("192.168.1.100:5555".to_string());
    slave_addrs.dedup();

    Html(templates::connect_page(&slave_addrs))
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
    let search = params.get("search").cloned().unwrap_or_default();
    let slave_filter = params.get("slave").cloned();

    let current_slave = state.current_slave().await;

    let mut all_params: Vec<(String, usize, String, String, u32, Option<String>)> = Vec::new();

    if let Some(slave) = current_slave {
        // Show calibration for current slave
        let cal_manager = slave.cal_manager.read().await;
        let cal_values = slave.cal_values.read().await;

        for (i, p) in cal_manager.params().iter().enumerate() {
            if search.is_empty() || p.name.to_lowercase().contains(&search.to_lowercase()) {
                let value = cal_values.get(&p.name).map(|v| v.format());
                all_params.push((
                    slave.display_name(),
                    i,
                    p.name.clone(),
                    p.data_type.type_name(),
                    p.address,
                    value,
                ));
            }
        }
    } else {
        // Show calibration from all slaves
        let slaves = state.all_slaves().await;
        for slave in slaves {
            let cal_manager = slave.cal_manager.read().await;
            let cal_values = slave.cal_values.read().await;

            for (i, p) in cal_manager.params().iter().enumerate() {
                let matches_search = search.is_empty()
                    || p.name.to_lowercase().contains(&search.to_lowercase());
                let matches_slave = slave_filter.as_ref().map_or(true, |s| {
                    slave.display_name().to_lowercase().contains(&s.to_lowercase())
                });

                if matches_search && matches_slave {
                    let value = cal_values.get(&p.name).map(|v| v.format());
                    all_params.push((
                        slave.display_name(),
                        i,
                        p.name.clone(),
                        p.data_type.type_name(),
                        p.address,
                        value,
                    ));
                }
            }
        }
    }

    Html(templates::calibration_page(&all_params, 0))
}

/// Read calibration parameter (GET /api/cal/read/:slave_id/:idx)
pub async fn cal_read(
    State(state): State<AppState>,
    Path((slave_id_str, idx)): Path<(String, usize)>,
) -> impl IntoResponse {
    let slave_id = match uuid::Uuid::parse_str(&slave_id_str) {
        Ok(id) => SlaveId(id),
        Err(_) => {
            state.set_error("Invalid slave ID".to_string()).await;
            return Redirect::to("/calibration");
        }
    };

    let Some(slave) = state.get_slave(slave_id).await else {
        state.set_error("Slave not found".to_string()).await;
        return Redirect::to("/calibration");
    };

    if !slave.is_connected() {
        state.set_error("Not connected to XCP slave".to_string()).await;
        return Redirect::to("/calibration");
    }

    let client = state.client.lock().await;
    let cal_manager = slave.cal_manager.read().await;

    if let Some(param) = cal_manager.params().get(idx) {
        let param = param.clone();
        drop(cal_manager); // Release lock before async operation

        // Get the XcpSlave handle for this slave
        if let Some(xcp_slave) = client.get_slave(slave_id) {
            // Read using the slave's built-in methods
            let result = {
                let xcp_slave_guard = xcp_slave.lock().await;
                read_calibration_from_slave(&xcp_slave_guard, &param).await
            };

            match result {
                Ok(value) => {
                    let value_str = value.format();
                    slave.cal_values.write().await.insert(param.name.clone(), value);
                    state.set_status(format!("[{}] {} = {}", slave.display_name(), param.name, value_str)).await;
                }
                Err(e) => {
                    state
                        .set_error(format!("Failed to read {}: {}", param.name, e))
                        .await;
                }
            }
        } else {
            state.set_error("Slave not available".to_string()).await;
        }
    }

    Redirect::to("/calibration")
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

    // Read data from slave
    let data = if size <= 255 {
        slave.read_memory(mta, size as u8).await.map_err(|e| e.to_string())?
    } else {
        slave.read_u8_array(mta, size).await.map_err(|e| e.to_string())?
    };

    // Parse based on type
    let value = match &param.data_type {
        CalibrationType::U8 => CalibrationValue::U8(data.first().copied().unwrap_or(0)),
        CalibrationType::U16 => {
            CalibrationValue::U16(u16::from_le_bytes([data[0], data[1]]))
        }
        CalibrationType::U32 => {
            CalibrationValue::U32(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
        }
        CalibrationType::U64 => {
            CalibrationValue::U64(u64::from_le_bytes([data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7]]))
        }
        CalibrationType::I8 => CalibrationValue::I8(data.first().copied().unwrap_or(0) as i8),
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
        CalibrationType::Bool => CalibrationValue::Bool(data.first().copied().unwrap_or(0) != 0),
        _ => return Err(format!("Array/Matrix types not yet supported ({} bytes)", data.len())),
    };

    Ok(value)
}

/// Write calibration page (GET /cal/write/:slave_id/:idx)
pub async fn cal_write_page(
    State(state): State<AppState>,
    Path((slave_id_str, idx)): Path<(String, usize)>,
) -> Html<String> {
    let slave_id = match uuid::Uuid::parse_str(&slave_id_str) {
        Ok(id) => SlaveId(id),
        Err(_) => {
            return Html(templates::page(
                "Error",
                r#"<p>Invalid slave ID</p><a href="/calibration">Back</a>"#,
                None,
                Some("Invalid slave ID format"),
            ));
        }
    };

    let Some(slave) = state.get_slave(slave_id).await else {
        return Html(templates::page(
            "Error",
            r#"<p>Slave not found</p><a href="/calibration">Back</a>"#,
            None,
            Some("Slave not found"),
        ));
    };

    let cal_manager = slave.cal_manager.read().await;
    let cal_values = slave.cal_values.read().await;

    if let Some(param) = cal_manager.params().get(idx) {
        let current = cal_values.get(&param.name).map(|v| v.format());
        Html(templates::cal_write_page(
            &slave_id_str,
            idx,
            &param.name,
            current.as_deref(),
        ))
    } else {
        Html(templates::page(
            "Error",
            r#"<p>Parameter not found</p><a href="/calibration">Back</a>"#,
            None,
            Some("Invalid parameter index"),
        ))
    }
}

/// Write calibration API (POST /api/cal/write/:slave_id/:idx)
pub async fn cal_write_api(
    State(state): State<AppState>,
    Path((slave_id_str, idx)): Path<(String, usize)>,
    Form(form): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    let slave_id = match uuid::Uuid::parse_str(&slave_id_str) {
        Ok(id) => SlaveId(id),
        Err(_) => {
            state.set_error("Invalid slave ID".to_string()).await;
            return Redirect::to("/calibration");
        }
    };

    let Some(slave) = state.get_slave(slave_id).await else {
        state.set_error("Slave not found".to_string()).await;
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
    let current_slave = state.current_slave().await;
    let current_slave_id = current_slave.as_ref().map(|s| s.id.to_string());

    // Use current slave's DAQ manager, or aggregate from all if no current
    let (measurements, selected_indices, daq_running, latest_values) = if let Some(ref slave) = current_slave {
        let daq_manager = slave.daq_manager.read().await;
        let daq_running = *slave.daq_running.read().await;

        let all_meas = daq_manager.all_measurements();
        let selected = daq_manager.selected_indices();

        let measurements: Vec<_> = all_meas
            .iter()
            .enumerate()
            .map(|(i, m)| (slave.id.to_string(), i, m.name.clone(), selected.contains(&i)))
            .collect();

        // Get latest DAQ values
        let latest_values = if daq_running {
            daq_manager
                .get_latest_sample()
                .await
                .map(|r| {
                    r.values
                        .into_iter()
                        .map(|(k, v)| (format!("[{}] {}", slave.display_name(), k), v))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let selected_indices: Vec<_> = selected.iter().copied().collect();

        (measurements, selected_indices, daq_running, latest_values)
    } else {
        // Aggregate from all slaves
        let slaves = state.all_slaves().await;
        let mut all_measurements = Vec::new();
        let mut all_selected = Vec::new();
        let mut any_running = false;
        let mut all_values = Vec::new();

        for slave in slaves {
            let daq_manager = slave.daq_manager.read().await;
            let daq_running = *slave.daq_running.read().await;
            any_running |= daq_running;

            let selected = daq_manager.selected_indices();
            let offset = all_measurements.len();

            for (i, m) in daq_manager.all_measurements().iter().enumerate() {
                all_measurements.push((slave.id.to_string(), i + offset, m.name.clone(), selected.contains(&i)));
            }

            all_selected.extend(selected.iter().map(|&i| i + offset));

            if daq_running {
                if let Some(r) = daq_manager.get_latest_sample().await {
                    for (k, v) in r.values {
                        all_values.push((format!("[{}] {}", slave.display_name(), k), v));
                    }
                }
            }
        }

        (all_measurements, all_selected, any_running, all_values)
    };

    let slaves = state.all_slaves().await;

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

    Html(templates::daq_page(
        &measurements,
        &selected_indices,
        daq_running,
        &latest_values,
        &slaves_data,
        current_slave_id.as_deref(),
    ))
}

/// DAQ start API (GET /api/daq/start)
pub async fn daq_start(State(state): State<AppState>) -> impl IntoResponse {
    let current_slave = state.current_slave().await;

    let Some(slave) = current_slave else {
        state.set_error("No slave selected".to_string()).await;
        return Redirect::to("/daq");
    };

    if !slave.is_connected() {
        state.set_error("Not connected to XCP slave".to_string()).await;
        return Redirect::to("/daq");
    }

    // Clone what we need before the async block
    let slave_name = slave.display_name();
    let slave_id = slave.id;

    let client = state.client.lock().await;
    let mut daq_manager = slave.daq_manager.write().await;

    // Use the client to start DAQ (DAQ manager needs XcpClient)
    match daq_manager.start(&*client, 1000).await {
        Ok(_) => {
            *slave.daq_running.write().await = true;
            state.set_status(format!("DAQ started on {}", slave_name)).await;
        }
        Err(e) => {
            state.set_error(format!("DAQ start failed: {}", e)).await;
        }
    }

    Redirect::to("/daq")
}

/// DAQ stop API (GET /api/daq/stop)
pub async fn daq_stop(State(state): State<AppState>) -> impl IntoResponse {
    let current_slave = state.current_slave().await;

    let Some(slave) = current_slave else {
        state.set_error("No slave selected".to_string()).await;
        return Redirect::to("/daq");
    };

    // Clone what we need before the async block
    let slave_name = slave.display_name();

    let client = state.client.lock().await;
    let mut daq_manager = slave.daq_manager.write().await;

    match daq_manager.stop(&*client).await {
        Ok(_) => {
            *slave.daq_running.write().await = false;
            state.set_status(format!("DAQ stopped on {}", slave_name)).await;
        }
        Err(e) => {
            state.set_error(format!("DAQ stop failed: {}", e)).await;
        }
    }

    Redirect::to("/daq")
}

/// DAQ toggle measurement (POST /api/daq/toggle)
pub async fn daq_toggle(
    State(state): State<AppState>,
    Form(form): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    let slave_id_str = form.get("slave_id").cloned();
    let idx_str = form.get("idx").cloned();

    if let (Some(slave_id_str), Some(idx_str)) = (slave_id_str, idx_str) {
        if let Ok(idx) = idx_str.parse::<usize>() {
            if let Ok(id) = uuid::Uuid::parse_str(&slave_id_str) {
                let slave_id = SlaveId(id);
                if let Some(slave) = state.get_slave(slave_id).await {
                    let mut daq_manager = slave.daq_manager.write().await;
                    let is_selected = daq_manager.selected_indices().contains(&idx);

                    if is_selected {
                        daq_manager.deselect_measurement(idx);
                    } else {
                        daq_manager.select_measurement(idx);
                    }
                }
            }
        }
    }

    Redirect::to("/daq")
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

    Redirect::to("/daq")
}

/// DAQ data partial (GET /api/daq/data) - for AJAX updates
pub async fn daq_data(State(state): State<AppState>) -> Html<String> {
    let current_slave = state.current_slave().await;

    let values = if let Some(slave) = current_slave {
        let daq_manager = slave.daq_manager.read().await;
        daq_manager
            .get_latest_sample()
            .await
            .map(|r| {
                r.values
                    .into_iter()
                    .map(|(k, v)| (format!("[{}] {}", slave.display_name(), k), v))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    } else {
        // Aggregate from all running slaves
        let slaves = state.all_slaves().await;
        let mut all_values = Vec::new();

        for slave in slaves {
            if *slave.daq_running.read().await {
                let daq_manager = slave.daq_manager.read().await;
                if let Some(r) = daq_manager.get_latest_sample().await {
                    for (k, v) in r.values {
                        all_values.push((format!("[{}] {}", slave.display_name(), k), v));
                    }
                }
            }
        }

        all_values
    };

    Html(templates::daq_data_partial(&values))
}

/// A2L page (GET /a2l)
pub async fn a2l_page(State(state): State<AppState>) -> Html<String> {
    let current_slave = state.current_slave().await;
    let path = if let Some(slave) = current_slave {
        slave.a2l_path.read().await.clone()
    } else {
        String::new()
    };
    Html(templates::a2l_page(&path))
}

/// A2L load API (POST /api/a2l/load)
pub async fn a2l_load(
    State(state): State<AppState>,
    Form(form): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    let path = form.get("path").cloned();
    let slave_id_str = form.get("slave_id").cloned();

    if let Some(path) = path {
        use crate::A2lReader;

        // Determine which slave to load A2L for
        let target_slave = if let Some(id_str) = slave_id_str {
            uuid::Uuid::parse_str(&id_str).ok().map(SlaveId)
        } else {
            *state.current_slave_id.read().await
        };

        match target_slave {
            Some(slave_id) => {
                // Load for specific slave
                let mut client = state.client.lock().await;

                match client.load_a2l_for_slave(slave_id, &path).await {
                    Ok(_) => {
                        // Update the slave state with loaded data
                        if let Some(xcp_slave) = client.get_slave(slave_id) {
                            let xcp_slave_guard = xcp_slave.lock().await;
                            let cal_count = xcp_slave_guard.cal_manager.params().len();
                            let meas_count = xcp_slave_guard.daq_manager.lock().await.available_count();

                            // Update slave state
                            if let Some(mut slave_state) = state.get_slave(slave_id).await {
                                *slave_state.a2l_path.write().await = path.clone();
                                // Copy the loaded managers to slave state
                                *slave_state.cal_manager.write().await = xcp_slave_guard.cal_manager.clone();
                                *slave_state.daq_manager.write().await = xcp_slave_guard.daq_manager.lock().await.clone();
                                state.add_slave_state(slave_state).await;
                            }

                            state.set_status(format!(
                                "Loaded A2L: {} cal params, {} measurements",
                                cal_count, meas_count
                            )).await;
                        }
                    }
                    Err(e) => {
                        state.set_error(format!("Failed to load A2L: {}", e)).await;
                    }
                }
            }
            None => {
                // No slave selected, load into global state for future connections
                match tokio::task::spawn_blocking({
                    let path = path.clone();
                    move || A2lReader::load(&path)
                }).await {
                    Ok(Ok(reader)) => {
                        let measurements = reader.get_measurements();
                        let characteristics = reader.get_characteristics();
                        let slave_addrs = reader.get_slave_addresses();

                        // Store in current slave if available
                        if let Some(slave) = state.current_slave().await {
                            {
                                let mut cal_manager = slave.cal_manager.write().await;
                                cal_manager.clear_params();
                                for ch in characteristics.clone() {
                                    if let Some(param) = convert_characteristic(ch) {
                                        cal_manager.add_param(param);
                                    }
                                }
                            }

                            {
                                let mut daq_manager = slave.daq_manager.write().await;
                                daq_manager.clear_measurements();
                                for m in measurements.clone() {
                                    if let Some(meas) = convert_measurement(m) {
                                        daq_manager.add_measurement(meas);
                                    }
                                }
                            }

                            *slave.a2l_path.write().await = path.clone();
                        }

                        let addr_info = if slave_addrs.is_empty() {
                            String::new()
                        } else {
                            format!(" Found {} slave address(es)", slave_addrs.len())
                        };

                        state.set_status(format!(
                            "Loaded A2L: {} ({} params, {} measurements){}",
                            path, characteristics.len(), measurements.len(), addr_info
                        )).await;
                    }
                    Ok(Err(e)) => {
                        state.set_error(format!("Failed to load A2L: {}", e)).await;
                    }
                    Err(_) => {
                        state.set_error("A2L loading task failed".to_string()).await;
                    }
                }
            }
        }
    }

    Redirect::to("/a2l")
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
