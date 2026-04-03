//! Web Server Shared State
//!
//! Manages the shared state between HTTP handlers and XCP client.
//! Single-user design - no concurrent access handling needed.
//! Multi-slave support: tracks multiple slaves with their own A2L data.

use crate::calibration::{CalibrationManager, CalibrationValue};
use crate::daq::DaqManager;
use crate::slave::{PageMode, SlaveId, SlaveInfo};
use crate::variable::{DeviceVariable, VariableSearchManager};
use crate::web::templates::TemplateEngine;
use crate::XcpClient;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

/// Device state - represents a physical device with its A2L file
#[derive(Clone)]
pub struct DeviceState {
    /// Unique device ID
    pub id: String,
    /// Device name (from A2L or user-defined)
    pub name: String,
    /// IP address from A2L
    pub address: String,
    /// A2L file path (temp file path)
    pub a2l_path: String,
    /// Original A2L filename (user's uploaded filename)
    pub original_filename: String,
    /// MAX_CTO from A2L (max command transfer object size)
    pub max_cto: u16,
    /// Is connected to device
    pub connected: bool,
    /// XCP slave ID when connected
    pub slave_id: Option<SlaveId>,
    /// Calibration parameter manager
    pub cal_manager: Arc<RwLock<CalibrationManager>>,
    /// DAQ measurement manager
    pub daq_manager: Arc<RwLock<DaqManager>>,
    /// Cached calibration values
    pub cal_values: Arc<RwLock<HashMap<String, CalibrationValue>>>,
    /// DAQ running status
    pub daq_running: Arc<RwLock<bool>>,
    /// Current page mode (Working or Reference)
    pub page_mode: Arc<RwLock<PageMode>>,
}

impl DeviceState {
    /// Create new device state from A2L
    pub fn new(id: String, name: String, address: String, a2l_path: String, original_filename: String, max_cto: u16) -> Self {
        Self {
            id,
            name,
            address,
            a2l_path,
            original_filename,
            max_cto,
            connected: false,
            slave_id: None,
            cal_manager: Arc::new(RwLock::new(CalibrationManager::new())),
            daq_manager: Arc::new(RwLock::new(DaqManager::new())),
            cal_values: Arc::new(RwLock::new(HashMap::new())),
            daq_running: Arc::new(RwLock::new(false)),
            page_mode: Arc::new(RwLock::new(PageMode::Reference)), // Default to Reference (read-only)
        }
    }

    /// Get maximum bytes that can be read in a single SHORT_UPLOAD or UPLOAD command
    /// SHORT_UPLOAD response: [RES (1 byte), data...]
    /// So max data = max_cto - 1
    pub fn max_upload_size(&self) -> usize {
        (self.max_cto as usize).saturating_sub(1)
    }
}

/// Slave state containing A2L data and managers for a specific slave
#[derive(Clone)]
pub struct SlaveState {
    /// Slave ID
    pub id: SlaveId,
    /// Slave info
    pub info: SlaveInfo,
    /// MAX_CTO from A2L (max command transfer object size)
    pub max_cto: u16,
    /// Calibration parameter manager (slave-specific A2L data)
    pub cal_manager: Arc<RwLock<CalibrationManager>>,
    /// DAQ measurement manager (slave-specific A2L data)
    pub daq_manager: Arc<RwLock<DaqManager>>,
    /// Cached calibration values
    pub cal_values: Arc<RwLock<HashMap<String, CalibrationValue>>>,
    /// DAQ running status
    pub daq_running: Arc<RwLock<bool>>,
    /// A2L file path
    pub a2l_path: Arc<RwLock<String>>,
}

impl SlaveState {
    /// Create new slave state
    pub fn new(id: SlaveId, info: SlaveInfo) -> Self {
        Self {
            id,
            info,
            max_cto: 255, // Default to 255
            cal_manager: Arc::new(RwLock::new(CalibrationManager::new())),
            daq_manager: Arc::new(RwLock::new(DaqManager::new())),
            cal_values: Arc::new(RwLock::new(HashMap::new())),
            daq_running: Arc::new(RwLock::new(false)),
            a2l_path: Arc::new(RwLock::new(String::new())),
        }
    }

    /// Get display name for this slave
    pub fn display_name(&self) -> String {
        self.info.display_name()
    }

    /// Get maximum bytes that can be read in a single SHORT_UPLOAD or UPLOAD command
    /// SHORT_UPLOAD response: [RES (1 byte), data...]
    /// So max data = max_cto - 1
    pub fn max_upload_size(&self) -> usize {
        (self.max_cto as usize).saturating_sub(1)
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.info.connected
    }
}

/// Shared application state for web server
#[derive(Clone)]
pub struct AppState {
    /// XCP client for communication (manages multiple slaves)
    pub client: Arc<Mutex<XcpClient>>,
    /// Current selected slave for operations
    pub current_slave_id: Arc<RwLock<Option<SlaveId>>>,
    /// Map of slave states by ID
    pub slave_states: Arc<RwLock<HashMap<SlaveId, SlaveState>>>,
    /// Status message
    pub status: Arc<RwLock<Option<String>>>,
    /// Error message
    pub error: Arc<RwLock<Option<String>>>,
    /// Template engine for rendering HTML
    pub templates: Arc<TemplateEngine>,
    /// Devices list (A2L files loaded, each representing a device)
    pub devices: Arc<RwLock<Vec<DeviceState>>>,
    /// Current selected device ID
    pub current_device_id: Arc<RwLock<Option<String>>>,
}

impl AppState {
    /// Create new application state
    pub fn new(
        client: XcpClient,
        _cal_manager: CalibrationManager,  // Kept for API compatibility
        _daq_manager: DaqManager,          // Kept for API compatibility
        _a2l_path: String,                 // Kept for API compatibility
    ) -> Self {
        Self {
            client: Arc::new(Mutex::new(client)),
            current_slave_id: Arc::new(RwLock::new(None)),
            slave_states: Arc::new(RwLock::new(HashMap::new())),
            status: Arc::new(RwLock::new(None)),
            error: Arc::new(RwLock::new(None)),
            templates: Arc::new(TemplateEngine::new()),
            devices: Arc::new(RwLock::new(Vec::new())),
            current_device_id: Arc::new(RwLock::new(None)),
        }
    }

    /// Set status message
    pub async fn set_status(&self, msg: String) {
        *self.status.write().await = Some(msg);
    }

    /// Set error message
    pub async fn set_error(&self, msg: String) {
        *self.error.write().await = Some(msg);
    }

    /// Clear error message
    pub async fn clear_error(&self) {
        *self.error.write().await = None;
    }

    /// Get and clear status
    pub async fn take_status(&self) -> Option<String> {
        self.status.write().await.take()
    }

    /// Get and clear error
    pub async fn take_error(&self) -> Option<String> {
        self.error.write().await.take()
    }

    /// Get current slave state
    pub async fn current_slave(&self) -> Option<SlaveState> {
        let id = *self.current_slave_id.read().await;
        if let Some(id) = id {
            self.slave_states.read().await.get(&id).cloned()
        } else {
            None
        }
    }

    /// Get a specific slave state
    pub async fn get_slave(&self, id: SlaveId) -> Option<SlaveState> {
        self.slave_states.read().await.get(&id).cloned()
    }

    /// Add or update slave state
    pub async fn add_slave_state(&self, state: SlaveState) {
        self.slave_states.write().await.insert(state.id, state);
    }

    /// Remove slave state
    pub async fn remove_slave_state(&self, id: SlaveId) {
        self.slave_states.write().await.remove(&id);
    }

    /// Set current slave
    pub async fn set_current_slave(&self, id: Option<SlaveId>) {
        *self.current_slave_id.write().await = id;
    }

    /// Get all slave states
    pub async fn all_slaves(&self) -> Vec<SlaveState> {
        self.slave_states.read().await.values().cloned().collect()
    }

    /// Get total calibration count across all slaves
    pub async fn total_cal_count(&self) -> usize {
        let mut count = 0;
        for state in self.slave_states.read().await.values() {
            count += state.cal_manager.read().await.params().len();
        }
        count
    }

    /// Get total measurement count across all slaves
    pub async fn total_meas_count(&self) -> usize {
        let mut count = 0;
        for state in self.slave_states.read().await.values() {
            count += state.daq_manager.read().await.available_count();
        }
        count
    }

    /// Get connected slave count
    pub async fn connected_count(&self) -> usize {
        self.slave_states.read().await.values().filter(|s| s.is_connected()).count()
    }

    /// Create variable search manager
    pub async fn variable_search(&self) -> VariableSearchManager {
        VariableSearchManager::new(self.client.clone())
    }

    /// Search variables across all slaves
    pub async fn search_variables(&self, query: &str) -> Vec<DeviceVariable> {
        let search = self.variable_search().await;
        search.search(query).await
    }

    /// Get all devices
    pub async fn all_devices(&self) -> Vec<DeviceState> {
        self.devices.read().await.clone()
    }

    /// Get device by ID
    pub async fn get_device(&self, id: &str) -> Option<DeviceState> {
        self.devices.read().await.iter().find(|d| d.id == id).cloned()
    }

    /// Add a new device
    pub async fn add_device(&self, device: DeviceState) {
        self.devices.write().await.push(device);
    }

    /// Update device connection status
    pub async fn update_device_connection(&self, device_id: &str, connected: bool, slave_id: Option<SlaveId>) {
        let mut devices = self.devices.write().await;
        if let Some(device) = devices.iter_mut().find(|d| d.id == device_id) {
            device.connected = connected;
            device.slave_id = slave_id;
        }
    }

    /// Update device connection status by slave ID
    pub async fn update_device_connection_by_slave_id(&self, slave_id: SlaveId, connected: bool) {
        let mut devices = self.devices.write().await;
        if let Some(device) = devices.iter_mut().find(|d| d.slave_id == Some(slave_id)) {
            device.connected = connected;
            if !connected {
                device.slave_id = None;
            }
        }
    }

    /// Remove a device
    pub async fn remove_device(&self, id: &str) {
        self.devices.write().await.retain(|d| d.id != id);
    }

    /// Get current device
    pub async fn current_device(&self) -> Option<DeviceState> {
        let id = self.current_device_id.read().await.clone();
        if let Some(id) = id {
            self.devices.read().await.iter().find(|d| d.id == id).cloned()
        } else {
            None
        }
    }

    /// Set current device
    pub async fn set_current_device(&self, id: Option<String>) {
        *self.current_device_id.write().await = id;
    }

    /// Check if address already exists in any device
    pub async fn address_exists(&self, address: &str) -> bool {
        self.devices.read().await.iter().any(|d| d.address == address)
    }

    /// Get total calibration count from all devices
    pub async fn total_device_cal_count(&self) -> usize {
        let mut count = 0;
        for device in self.devices.read().await.iter() {
            count += device.cal_manager.read().await.params().len();
        }
        count
    }

    /// Get total measurement count from all devices
    pub async fn total_device_meas_count(&self) -> usize {
        let mut count = 0;
        for device in self.devices.read().await.iter() {
            count += device.daq_manager.read().await.available_count();
        }
        count
    }

    /// Get connected device count
    pub async fn connected_device_count(&self) -> usize {
        self.devices.read().await.iter().filter(|d| d.connected).count()
    }
}
