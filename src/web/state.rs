//! Web Server Shared State
//!
//! Manages the shared state between HTTP handlers and XCP client.
//! Single-user design - no concurrent access handling needed.
//! Multi-slave support: tracks multiple slaves with their own A2L data.

use crate::calibration::{CalibrationManager, CalibrationValue};
use crate::daq::DaqManager;
use crate::slave::{SlaveId, SlaveInfo};
use crate::variable::{VariableSearchManager, DeviceVariable};
use crate::XcpClient;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

/// Slave state containing A2L data and managers for a specific slave
#[derive(Clone)]
pub struct SlaveState {
    /// Slave ID
    pub id: SlaveId,
    /// Slave info
    pub info: SlaveInfo,
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
}
