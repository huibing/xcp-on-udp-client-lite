/// Shared Tauri app state (mirrors web/state.rs AppState).
///
/// Wraps XcpClient and per-device managers without any HTTP/Axum dependency.
/// Registered with Tauri via `.manage(Arc<RwLock<TauriAppState>>)`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use xcp_client::a2l::{A2lReader, A2lTransportConfig};
use xcp_client::calibration::{CalibrationManager, CalibrationValue};
use xcp_client::checksum::VerificationReport;
use xcp_client::daq::DaqManager;
use xcp_client::hex::HexImage;
use xcp_client::slave::{PageMode, SlaveId};
use xcp_client::transport::TransportType;
use xcp_client::web::compare::CompareSession;
use xcp_client::xcp_packet_log::XcpPacketLog;
use xcp_client::XcpClient;

// ─── Device configuration (from config.toml, same as web/state.rs) ────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub a2l_path: Option<String>,
    #[serde(default)]
    pub hex_path: Option<String>,
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
    pub hex_path: String,
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
    /// Reference HEX image (user-uploaded; never modified)
    pub hex_image: Arc<RwLock<Option<HexImage>>>,
    /// Working page HEX image (copy of reference, patched on every calibration write)
    pub working_hex_image: Arc<RwLock<Option<HexImage>>>,
    /// On-disk path for the working page HEX (derived from hex_path; persisted after writes)
    pub working_hex_path: String,
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
            hex_path: String::new(),
            max_cto: 255,
            transport_configs: Vec::new(),
            connected: false,
            slave_id: None,
            cal_manager: Arc::new(RwLock::new(CalibrationManager::new())),
            daq_manager: Arc::new(RwLock::new(DaqManager::new())),
            cal_values: Arc::new(RwLock::new(HashMap::new())),
            daq_running: Arc::new(RwLock::new(false)),
            page_mode: Arc::new(RwLock::new(PageMode::Reference)),
            custom_name: Arc::new(RwLock::new(None)),
            hex_image: Arc::new(RwLock::new(None)),
            working_hex_image: Arc::new(RwLock::new(None)),
            working_hex_path: String::new(),
            hex_filename: Arc::new(RwLock::new(None)),
            checksum_report: Arc::new(RwLock::new(None)),
            cal_original_values: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn new_empty(id: String, name: String) -> Self {
        Self::new(id, name, String::new())
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

    pub fn resolve_transport_config(
        &self,
        requested: Option<TransportType>,
    ) -> Result<A2lTransportConfig, String> {
        if let Some(transport_type) = requested {
            return self
                .transport_configs
                .iter()
                .find(|config| config.transport_type() == transport_type)
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "Transport protocol {} is not available for device {}",
                        transport_type.as_str(),
                        self.name
                    )
                });
        }

        for transport_type in [TransportType::Udp, TransportType::Can, TransportType::Tcp] {
            if let Some(config) = self
                .transport_configs
                .iter()
                .find(|config| config.transport_type() == transport_type)
            {
                return Ok(config.clone());
            }
        }

        if !self.address.is_empty() {
            return Ok(A2lTransportConfig::Udp {
                addr: self.address.clone(),
            });
        }

        Err(format!(
            "No transport protocol configured for device {}",
            self.name
        ))
    }
}

// ─── Top-level shared state ────────────────────────────────────────────────────

pub struct TauriAppState {
    pub client: XcpClient,
    pub devices: Vec<DeviceState>,
    pub current_device_id: Option<String>,
    pub compare_sessions: Vec<CompareSession>,
    pub experiment_active: bool,
    pub packet_log: Arc<XcpPacketLog>,
}

impl TauriAppState {
    pub fn new() -> Self {
        let config = Self::load_config();
        let mut devices = Vec::new();
        for (i, dev) in config.devices.iter().enumerate() {
            let id = dev
                .id
                .clone()
                .unwrap_or_else(|| format!("device_{}", i));
            let mut device = DeviceState::new_empty(id, dev.name.clone());
            device.address = dev.address.clone();
            if let Some(max_cto) = dev.max_cto {
                device.max_cto = max_cto;
            }
            if let Some(a2l_path) = dev.a2l_path.as_deref() {
                device.a2l_path = a2l_path.to_string();
                device.original_filename = file_name_or_path(a2l_path);
                if let Err(error) = Self::restore_a2l(&mut device, a2l_path) {
                    tracing::warn!("Failed to restore A2L for '{}': {}", device.name, error);
                }
            }
            if let Some(hex_path) = dev.hex_path.as_deref() {
                device.hex_path = hex_path.to_string();
                if let Err(error) = Self::restore_hex(&mut device, hex_path) {
                    tracing::warn!("Failed to restore HEX for '{}': {}", device.name, error);
                }
            }
            devices.push(device);
        }
        Self {
            client: XcpClient::new(),
            devices,
            current_device_id: None,
            compare_sessions: Vec::new(),
            experiment_active: false,
            packet_log: Arc::new(XcpPacketLog::new()),
        }
    }

    fn load_config() -> AppConfig {
        std::fs::read_to_string(Self::config_path())
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save_config(&self) -> Result<(), String> {
        self.save_config_to_path(&Self::config_path())
    }

    fn save_config_to_path(&self, path: &Path) -> Result<(), String> {
        let mut root = match std::fs::read_to_string(path) {
            Ok(content) => toml::from_str::<toml::Value>(&content)
                .ok()
                .and_then(|value| value.as_table().cloned())
                .unwrap_or_default(),
            Err(_) => toml::map::Map::new(),
        };

        let devices = self
            .devices
            .iter()
            .map(|device| {
                let mut table = toml::map::Map::new();
                table.insert("id".into(), toml::Value::String(device.id.clone()));
                table.insert(
                    "name".into(),
                    toml::Value::String(device.display_name()),
                );
                table.insert(
                    "address".into(),
                    toml::Value::String(device.address.clone()),
                );
                if !device.a2l_path.is_empty() {
                    table.insert(
                        "a2l_path".into(),
                        toml::Value::String(device.a2l_path.clone()),
                    );
                }
                if !device.hex_path.is_empty() {
                    table.insert(
                        "hex_path".into(),
                        toml::Value::String(device.hex_path.clone()),
                    );
                }
                table.insert(
                    "max_cto".into(),
                    toml::Value::Integer(i64::from(device.max_cto)),
                );
                toml::Value::Table(table)
            })
            .collect::<Vec<_>>();
        root.insert("devices".into(), toml::Value::Array(devices));

        let content = toml::to_string_pretty(&toml::Value::Table(root))
            .map_err(|e| format!("Failed to serialize config.toml: {}", e))?;
        std::fs::write(path, content).map_err(|e| format!("Failed to write config.toml: {}", e))
    }

    fn config_path() -> PathBuf {
        PathBuf::from("config.toml")
    }

    fn restore_a2l(device: &mut DeviceState, file_path: &str) -> Result<(), String> {
        Self::restore_a2l_pub(device, file_path)
    }

    /// Public alias for `restore_a2l`, callable from other command modules.
    pub fn restore_a2l_pub(device: &mut DeviceState, file_path: &str) -> Result<(), String> {
        let reader = A2lReader::load(file_path).map_err(|e| format!("A2L parse error: {}", e))?;
        let characteristics = reader.get_characteristics();
        let measurements = reader.get_measurements();
        let memory_segments = reader.get_memory_segments();
        let events = reader.get_events();
        let max_cto = reader.get_max_cto().unwrap_or(255);
        let transport_configs = reader.get_transport_configs();
        let device_addr = transport_configs
            .first()
            .map(display_device_address)
            .or_else(|| reader.get_slave_addresses().first().cloned())
            .unwrap_or_else(|| device.address.clone());

        let mut cal_manager = CalibrationManager::new();
        for characteristic in characteristics {
            if let Some(param) = convert_characteristic(characteristic, &reader) {
                cal_manager.add_param(param);
            }
        }
        // Also expose AXIS_PTS objects as calibration parameters (1-D arrays)
        for axis_ch in reader.get_axis_pts_as_characteristics() {
            if let Some(param) = convert_characteristic(axis_ch, &reader) {
                cal_manager.add_param(param);
            }
        }
        cal_manager.set_memory_segments(memory_segments);

        let mut daq_manager = DaqManager::new();
        for measurement in measurements {
            if let Some(meas) = convert_measurement(measurement, &reader) {
                daq_manager.add_measurement(meas);
            }
        }
        if !events.is_empty() {
            daq_manager.set_available_events(events);
        }

        device.address = device_addr;
        device.a2l_path = file_path.to_string();
        device.original_filename = file_name_or_path(file_path);
        device.max_cto = max_cto;
        device.transport_configs = transport_configs;
        futures::executor::block_on(async {
            *device.cal_manager.write().await = cal_manager;
            *device.daq_manager.write().await = daq_manager;
        });
        Ok(())
    }

    fn restore_hex(device: &mut DeviceState, file_path: &str) -> Result<(), String> {
        let hex_str = std::fs::read_to_string(file_path)
            .map_err(|e| format!("Failed to read HEX file '{}': {}", file_path, e))?;
        let hex_image = HexImage::from_str(&hex_str)
            .map_err(|e| format!("HEX parse error '{}': {}", file_path, e))?;
        let file_name = file_name_or_path(file_path);

        // Derive working page HEX path (e.g. firmware.hex → firmware_working.hex)
        let working_path = derive_working_hex_path(file_path);

        // Try to load an existing working page HEX from disk; fall back to cloning reference
        let working_hex = std::fs::read_to_string(&working_path)
            .ok()
            .and_then(|s| HexImage::from_str(&s).ok())
            .unwrap_or_else(|| hex_image.clone());

        device.hex_path = file_path.to_string();
        device.working_hex_path = working_path;
        futures::executor::block_on(async {
            *device.hex_image.write().await = Some(hex_image);
            *device.working_hex_image.write().await = Some(working_hex);
            *device.hex_filename.write().await = Some(file_name);
            *device.checksum_report.write().await = None;
        });
        Ok(())
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

fn display_device_address(config: &A2lTransportConfig) -> String {
    match config {
        A2lTransportConfig::Udp { addr } | A2lTransportConfig::Tcp { addr } => addr.clone(),
        A2lTransportConfig::Can { can_id } => format!("CAN 0x{:X}", can_id),
    }
}

fn file_name_or_path(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_string()
}

/// Derive the working page HEX path from the reference HEX path.
/// E.g. `/path/to/firmware.hex` → `/path/to/firmware_working.hex`
pub fn derive_working_hex_path(hex_path: &str) -> String {
    let p = Path::new(hex_path);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("firmware");
    let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("hex");
    let new_name = format!("{}_{}.{}", stem, "working", ext);
    p.with_file_name(new_name)
        .to_string_lossy()
        .into_owned()
}

fn convert_characteristic(
    characteristic: xcp_client::a2l::A2lCharacteristic,
    reader: &A2lReader,
) -> Option<xcp_client::calibration::CalibrationParam> {
    crate::commands::helpers::convert_characteristic(characteristic, reader)
}

fn convert_measurement(
    measurement: xcp_client::a2l::A2lMeasurement,
    reader: &A2lReader,
) -> Option<xcp_client::daq::Measurement> {
    crate::commands::helpers::convert_measurement(measurement, reader)
}

// Type alias for Tauri state injection.
pub type SharedState = Arc<RwLock<TauriAppState>>;
