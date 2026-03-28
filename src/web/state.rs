//! Web Server Shared State
//!
//! Manages the shared state between HTTP handlers and XCP client.
//! Single-user design - no concurrent access handling needed.

use crate::calibration::{CalibrationManager, CalibrationValue};
use crate::daq::DaqManager;
use crate::XcpClient;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Shared application state for web server
#[derive(Clone)]
pub struct AppState {
    /// XCP client for communication
    pub client: Arc<RwLock<XcpClient>>,
    /// Calibration parameter manager
    pub cal_manager: Arc<RwLock<CalibrationManager>>,
    /// DAQ measurement manager
    pub daq_manager: Arc<RwLock<DaqManager>>,
    /// Current connection status
    pub connected: Arc<RwLock<bool>>,
    /// Current slave address
    pub slave_addr: Arc<RwLock<String>>,
    /// Cached calibration values
    pub cal_values: Arc<RwLock<HashMap<String, CalibrationValue>>>,
    /// DAQ running status
    pub daq_running: Arc<RwLock<bool>>,
    /// A2L file path
    pub a2l_path: Arc<RwLock<String>>,
    /// Status message
    pub status: Arc<RwLock<Option<String>>>,
    /// Error message
    pub error: Arc<RwLock<Option<String>>>,
}

impl AppState {
    /// Create new application state
    pub fn new(
        client: XcpClient,
        cal_manager: CalibrationManager,
        daq_manager: DaqManager,
        a2l_path: String,
    ) -> Self {
        Self {
            client: Arc::new(RwLock::new(client)),
            cal_manager: Arc::new(RwLock::new(cal_manager)),
            daq_manager: Arc::new(RwLock::new(daq_manager)),
            connected: Arc::new(RwLock::new(false)),
            slave_addr: Arc::new(RwLock::new("127.0.0.1:5555".to_string())),
            cal_values: Arc::new(RwLock::new(HashMap::new())),
            daq_running: Arc::new(RwLock::new(false)),
            a2l_path: Arc::new(RwLock::new(a2l_path)),
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
}
