//! HTTP Routes
//!
//! All route handlers for the web interface.

use axum::{
    extract::{Form, Path, Query, State},
    http::header::SET_COOKIE,
    response::{AppendHeaders, Html, IntoResponse, Redirect},
};
use std::collections::HashMap;

use crate::calibration::{CalibrationManager, CalibrationValue};
use crate::daq::DaqManager;
use crate::web::auth::{clear_session, try_create_session, verify_credentials};
use crate::web::state::AppState;
use crate::web::templates;
use crate::XcpClient;

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
    let connected = *state.connected.read().await;
    let slave_addr = state.slave_addr.read().await.clone();
    let a2l_path = state.a2l_path.read().await.clone();
    let daq_running = *state.daq_running.read().await;

    let cal_count = state.cal_manager.read().await.params().len();
    let meas_count = state.daq_manager.read().await.available_count();

    let status = state.take_status().await;
    let error = state.take_error().await;

    let content = templates::dashboard_page(
        connected,
        &slave_addr,
        &a2l_path,
        cal_count,
        meas_count,
        daq_running,
    );

    Html(templates::page("Dashboard", &content, status.as_deref(), error.as_deref()))
}

/// Connect page (GET /connect)
pub async fn connect_page(State(state): State<AppState>) -> Html<String> {
    let daq_manager = state.daq_manager.read().await;
    let slave_addrs: Vec<String> = daq_manager
        .all_measurements()
        .iter()
        .filter_map(|m| {
            // Get unique addresses from measurements
            Some(format!("{}:5555", m.address))
        })
        .collect();

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

    let mut client = state.client.write().await;

    match client.connect(&address).await {
        Ok(info) => {
            *state.connected.write().await = true;
            *state.slave_addr.write().await = address;
            state
                .set_status(format!(
                    "Connected (max CTO={}, max DTO={})",
                    info.max_cto, info.max_dto
                ))
                .await;
        }
        Err(e) => {
            state.set_error(format!("Connection failed: {}", e)).await;
        }
    }

    Redirect::to("/")
}

/// Disconnect API (GET /api/disconnect)
pub async fn disconnect_api(State(state): State<AppState>) -> impl IntoResponse {
    let mut client = state.client.write().await;

    if let Err(e) = client.disconnect().await {
        state.set_error(format!("Disconnect error: {}", e)).await;
    } else {
        *state.connected.write().await = false;
        state.set_status("Disconnected".to_string()).await;
    }

    Redirect::to("/")
}

/// Calibration page (GET /calibration)
pub async fn calibration_page(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Html<String> {
    let search = params.get("search").cloned().unwrap_or_default();
    let cal_manager = state.cal_manager.read().await;
    let cal_values = state.cal_values.read().await;

    let params: Vec<_> = cal_manager
        .params()
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            if search.is_empty() {
                true
            } else {
                p.name.to_lowercase().contains(&search.to_lowercase())
            }
        })
        .map(|(i, p)| {
            let value = cal_values.get(&p.name).map(|v| v.format());
            (
                i,
                p.name.clone(),
                p.data_type.type_name(),
                p.address,
                value,
            )
        })
        .collect();

    Html(templates::calibration_page(&params, 0))
}

/// Read calibration parameter (GET /api/cal/read/:idx)
pub async fn cal_read(
    State(state): State<AppState>,
    Path(idx): Path<usize>,
) -> impl IntoResponse {
    let connected = *state.connected.read().await;

    if !connected {
        state.set_error("Not connected to XCP slave".to_string()).await;
        return Redirect::to("/calibration");
    }

    let cal_manager = state.cal_manager.read().await;
    let params = cal_manager.params().to_vec();

    if let Some(param) = params.get(idx) {
        let client = state.client.read().await;
        let param = param.clone();

        match CalibrationManager::new().read_value(&client, &param).await {
            Ok(value) => {
                let value_str = value.format();
                state.cal_values.write().await.insert(param.name.clone(), value);
                state.set_status(format!("{} = {}", param.name, value_str)).await;
            }
            Err(e) => {
                state
                    .set_error(format!("Failed to read {}: {}", param.name, e))
                    .await;
            }
        }
    }

    Redirect::to("/calibration")
}

/// Write calibration page (GET /cal/write/:idx)
pub async fn cal_write_page(
    State(state): State<AppState>,
    Path(idx): Path<usize>,
) -> Html<String> {
    let cal_manager = state.cal_manager.read().await;
    let cal_values = state.cal_values.read().await;

    if let Some(param) = cal_manager.params().get(idx) {
        let current = cal_values.get(&param.name).map(|v| v.format());
        Html(templates::cal_write_page(
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

/// Write calibration API (POST /api/cal/write/:idx)
pub async fn cal_write_api(
    State(state): State<AppState>,
    Path(idx): Path<usize>,
    Form(form): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    let connected = *state.connected.read().await;

    if !connected {
        state.set_error("Not connected to XCP slave".to_string()).await;
        return Redirect::to("/calibration");
    }

    let value_str = form.get("value").cloned().unwrap_or_default();

    let cal_manager = state.cal_manager.read().await;
    let params = cal_manager.params().to_vec();

    if let Some(param) = params.get(idx) {
        let cal_manager = CalibrationManager::new();
        let param = param.clone();

        match cal_manager.parse_value(&param, &value_str) {
            Ok(value) => {
                let client = state.client.read().await;
                match cal_manager.write_value(&client, &param, &value).await {
                    Ok(_) => {
                        state
                            .set_status(format!("Wrote {} = {}", param.name, value_str))
                            .await;
                    }
                    Err(e) => {
                        state
                            .set_error(format!("Failed to write {}: {}", param.name, e))
                            .await;
                    }
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

/// DAQ page (GET /daq)
pub async fn daq_page(State(state): State<AppState>) -> Html<String> {
    let daq_manager = state.daq_manager.read().await;
    let daq_running = *state.daq_running.read().await;

    let all_meas = daq_manager.all_measurements();
    let selected = daq_manager.selected_indices();

    let measurements: Vec<_> = all_meas
        .iter()
        .enumerate()
        .map(|(i, m)| (i, m.name.clone(), selected.contains(&i)))
        .collect();

    // Get latest DAQ values
    let latest_values = if daq_running {
        daq_manager
            .get_latest_sample()
            .await
            .map(|r| {
                r.values
                    .into_iter()
                    .map(|(k, v)| (k, v))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let selected_indices: Vec<_> = selected.iter().copied().collect();

    Html(templates::daq_page(
        &measurements,
        &selected_indices,
        daq_running,
        &latest_values,
    ))
}

/// DAQ start API (GET /api/daq/start)
pub async fn daq_start(State(state): State<AppState>) -> impl IntoResponse {
    let connected = *state.connected.read().await;

    if !connected {
        state.set_error("Not connected to XCP slave".to_string()).await;
        return Redirect::to("/daq");
    }

    let client = state.client.read().await;
    let mut daq_manager = state.daq_manager.write().await;
    let slave_addr = state.slave_addr.read().await.clone();

    daq_manager.set_slave_addr(slave_addr);

    match daq_manager.start(&*client, 1000).await {
        Ok(_) => {
            *state.daq_running.write().await = true;
            state.set_status("DAQ started".to_string()).await;
        }
        Err(e) => {
            state.set_error(format!("DAQ start failed: {}", e)).await;
        }
    }

    Redirect::to("/daq")
}

/// DAQ stop API (GET /api/daq/stop)
pub async fn daq_stop(State(state): State<AppState>) -> impl IntoResponse {
    let client = state.client.read().await;
    let mut daq_manager = state.daq_manager.write().await;

    match daq_manager.stop(&*client).await {
        Ok(_) => {
            *state.daq_running.write().await = false;
            state.set_status("DAQ stopped".to_string()).await;
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
    if let Some(idx_str) = form.get("idx") {
        if let Ok(idx) = idx_str.parse::<usize>() {
            let mut daq_manager = state.daq_manager.write().await;
            let is_selected = daq_manager.selected_indices().contains(&idx);

            if is_selected {
                daq_manager.deselect_measurement(idx);
            } else {
                daq_manager.select_measurement(idx);
            }
        }
    }

    Redirect::to("/daq")
}

/// DAQ clear data (GET /api/daq/clear)
pub async fn daq_clear(State(state): State<AppState>) -> impl IntoResponse {
    let mut daq_manager = state.daq_manager.write().await;
    daq_manager.clear_samples();
    Redirect::to("/daq")
}

/// DAQ data partial (GET /api/daq/data) - for AJAX updates
pub async fn daq_data(State(state): State<AppState>) -> Html<String> {
    let daq_manager = state.daq_manager.read().await;

    let values = daq_manager
        .get_latest_sample()
        .await
        .map(|r| {
            r.values
                .into_iter()
                .map(|(k, v)| (k, v))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Html(templates::daq_data_partial(&values))
}

/// A2L page (GET /a2l)
pub async fn a2l_page(State(state): State<AppState>) -> Html<String> {
    let path = state.a2l_path.read().await.clone();
    Html(templates::a2l_page(&path))
}

/// A2L load API (POST /api/a2l/load)
pub async fn a2l_load(
    State(state): State<AppState>,
    Form(form): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    if let Some(path) = form.get("path") {
        use crate::A2lReader;

        match tokio::task::spawn_blocking({
            let path = path.clone();
            move || A2lReader::load(&path)
        })
        .await
        {
            Ok(Ok(reader)) => {
                let measurements = reader.get_measurements();
                let characteristics = reader.get_characteristics();
                let slave_addrs = reader.get_slave_addresses();

                // Update calibration manager
                {
                    let mut cal_manager = state.cal_manager.write().await;
                    cal_manager.clear_params();
                    for ch in characteristics {
                        if let Some(param) = convert_characteristic(ch) {
                            cal_manager.add_param(param);
                        }
                    }
                }

                // Update DAQ manager
                {
                    let mut daq_manager = state.daq_manager.write().await;
                    daq_manager.clear_measurements();
                    for m in measurements {
                        if let Some(meas) = convert_measurement(m) {
                            daq_manager.add_measurement(meas);
                        }
                    }
                }

                *state.a2l_path.write().await = path.clone();

                let addr_info = if slave_addrs.is_empty() {
                    String::new()
                } else {
                    format!(
                        " Found {} slave address(es)",
                        slave_addrs.len()
                    )
                };

                state
                    .set_status(format!("Loaded A2L: {}{}", path, addr_info))
                    .await;
            }
            Ok(Err(e)) => {
                state
                    .set_error(format!("Failed to load A2L: {}", e))
                    .await;
            }
            Err(_) => {
                state.set_error("A2L loading task failed".to_string()).await;
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
