/// Shared Tauri app state (mirrors web/state.rs AppState).
///
/// Wraps XcpClient and per-device managers without any HTTP/Axum dependency.
/// Registered with Tauri via `.manage(Arc<RwLock<TauriAppState>>)`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use xcp_client::a2l::A2lTransportConfig;
use xcp_client::calibration::{CalibrationManager, CalibrationValue};
use xcp_client::checksum::VerificationReport;
use xcp_client::daq::DaqManager;
use xcp_client::hex::HexImage;
use xcp_client::slave::{PageMode, SlaveId};
use xcp_client::web::compare::CompareSession;
use xcp_client::XcpClient;

// ─── Device configuration (from config.toml, same as web/state.rs) ────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    pub name: String,
    pub address: String,
    #[serde(default)]
    pub a2l_path: Option<String>,
    #[serde(default)]
    pub max_cto: Option<u16>,
}

/// Root config.toml structure.
#[derive(Debug, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub devices: Vec<DeviceConfig>,
}

// ─── Per-device runtime state ──────────────────────────────────────────────────

#[derive(Clone)]
pub struct DeviceState {
    pub id: String,
    pub name: String,
    pub address: String,
    pub a2l_path: String,
    pub original_filename: String,
    pub max_cto: u16,
    pub transport_configs: Vec<A2lTransportConfig>,
    pub connected: bool,
    pub slave_id: Option<SlaveId>,
    pub cal_manager: Arc<RwLock<CalibrationManager>>,
    pub daq_manager: Arc<RwLock<DaqManager>>,
    pub cal_values: Arc<RwLock<HashMap<String, CalibrationValue>>>,
    pub daq_running: Arc<RwLock<bool>>,
    pub page_mode: Arc<RwLock<PageMode>>,
    pub custom_name: Arc<RwLock<Option<String>>>,
    /// HEX image loaded for checksum verification
    pub hex_image: Arc<RwLock<Option<HexImage>>>,
    /// Filename of the loaded HEX file (display only)
    pub hex_filename: Arc<RwLock<Option<String>>>,
    /// Last checksum verification report
    pub checksum_report: Arc<RwLock<Option<VerificationReport>>>,
    /// Original calibration values keyed by param index (for experiment diff)
    pub cal_original_values: Arc<RwLock<HashMap<usize, String>>>,
}

impl DeviceState {
    pub fn new(id: String, name: String, address: String) -> Self {
        Self {
            id,
            name,
            address,
            a2l_path: String::new(),
            original_filename: String::new(),
            max_cto: 8,
            transport_configs: Vec::new(),
            connected: false,
            slave_id: None,
            cal_manager: Arc::new(RwLock::new(CalibrationManager::new())),
            daq_manager: Arc::new(RwLock::new(DaqManager::new())),
            cal_values: Arc::new(RwLock::new(HashMap::new())),
            daq_running: Arc::new(RwLock::new(false)),
            page_mode: Arc::new(RwLock::new(PageMode::Working)),
            custom_name: Arc::new(RwLock::new(None)),
            hex_image: Arc::new(RwLock::new(None)),
            hex_filename: Arc::new(RwLock::new(None)),
            checksum_report: Arc::new(RwLock::new(None)),
            cal_original_values: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn display_name(&self) -> String {
        futures::executor::block_on(async {
            self.custom_name
                .read()
                .await
                .clone()
                .unwrap_or_else(|| self.name.clone())
        })
    }
}

// ─── Top-level shared state ────────────────────────────────────────────────────

pub struct TauriAppState {
    pub client: XcpClient,
    pub devices: Vec<DeviceState>,
    pub current_device_id: Option<String>,
    pub compare_sessions: Vec<CompareSession>,
    pub experiment_active: bool,
}

impl TauriAppState {
    pub fn new() -> Self {
        let config = Self::load_config();
        let mut devices = Vec::new();
        for (i, dev) in config.devices.iter().enumerate() {
            let id = format!("device_{}", i);
            devices.push(DeviceState::new(id, dev.name.clone(), dev.address.clone()));
        }
        Self {
            client: XcpClient::new(),
            devices,
            current_device_id: None,
            compare_sessions: Vec::new(),
            experiment_active: false,
        }
    }

    fn load_config() -> AppConfig {
        std::fs::read_to_string("config.toml")
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn get_device(&self, id: &str) -> Option<&DeviceState> {
        self.devices.iter().find(|d| d.id == id)
    }

    pub fn get_device_mut(&mut self, id: &str) -> Option<&mut DeviceState> {
        self.devices.iter_mut().find(|d| d.id == id)
    }

    pub fn current_device(&self) -> Option<&DeviceState> {
        self.current_device_id
            .as_deref()
            .and_then(|id| self.get_device(id))
    }
    pub fn find_device_by_slave_id(&self, sid: SlaveId) -> Option<&DeviceState> {
        self.devices.iter().find(|d| d.slave_id == Some(sid))
    }

    pub fn find_device_by_slave_id_mut(&mut self, sid: SlaveId) -> Option<&mut DeviceState> {
        self.devices.iter_mut().find(|d| d.slave_id == Some(sid))
    }
}

// Type alias for Tauri state injection.
pub type SharedState = Arc<RwLock<TauriAppState>>;
