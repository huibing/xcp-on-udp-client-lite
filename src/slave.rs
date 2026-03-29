//! XCP Slave Management
//!
//! Provides structures for managing multiple XCP slave connections.
//! Each slave has its own transport, calibration manager, and DAQ manager.

use crate::calibration::CalibrationManager;
use crate::daq::DaqManager;
use crate::protocol::{
    commands, parse_connect_response, parse_get_id_response, parse_response, ConnectInfo, ErrorCode,
    Mta, Response,
};
use crate::transport::{
    MessageRouter, Transport, TransportConfig, TransportError, TransportFactory, XcpMessage,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{timeout, Duration};
use uuid::Uuid;

/// Slave unique identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SlaveId(pub Uuid);

impl SlaveId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SlaveId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SlaveId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.to_string())
    }
}

/// Slave information (for display)
#[derive(Debug, Clone)]
pub struct SlaveInfo {
    pub id: SlaveId,
    pub name: String,
    pub transport_config: TransportConfig,
    pub slave_id_string: Option<String>,
    pub connected: bool,
    pub connect_info: Option<ConnectInfo>,
}

impl SlaveInfo {
    /// Get display name
    pub fn display_name(&self) -> String {
        if let Some(slave_id) = &self.slave_id_string {
            format!("{} [{}]", self.name, slave_id)
        } else {
            self.name.clone()
        }
    }

    /// Get address string
    pub fn addr_string(&self) -> String {
        self.transport_config.addr_string()
    }
}

/// Slave error types
#[derive(Debug, thiserror::Error)]
pub enum SlaveError {
    #[error("Transport error: {0}")]
    Transport(#[from] TransportError),

    #[error("XCP protocol error: {0}")]
    Protocol(#[from] ErrorCode),

    #[error("Response parse error: {0}")]
    ParseError(String),

    #[error("Not connected")]
    NotConnected,

    #[error("Already connected")]
    AlreadyConnected,

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("DAQ error: {0}")]
    DaqError(String),

    #[error("Timeout waiting for response")]
    ResponseTimeout,

    #[error("A2L error: {0}")]
    A2lError(String),
}

/// Single XCP Slave connection context
pub struct XcpSlave {
    /// Slave information
    pub info: SlaveInfo,

    /// Transport layer (dynamic dispatch)
    transport: Arc<dyn Transport>,

    /// Message router
    router: Arc<MessageRouter>,

    /// CTO response receiver
    cto_rx: Arc<Mutex<mpsc::Receiver<XcpMessage>>>,

    /// Calibration manager (with A2L data)
    pub cal_manager: CalibrationManager,

    /// DAQ manager
    pub daq_manager: Arc<Mutex<DaqManager>>,

    /// A2L file path
    pub a2l_path: Option<PathBuf>,
}

impl XcpSlave {
    /// Create a new slave connection
    pub async fn connect(
        name: impl Into<String>,
        transport_config: TransportConfig,
    ) -> Result<(Self, SlaveId), SlaveError> {
        let id = SlaveId::new();
        let name = name.into();

        tracing::info!("Connecting to slave '{}' at {}...", name, transport_config.addr_string());

        // Create transport
        let transport = TransportFactory::create(&transport_config)
            .await
            .map_err(SlaveError::Transport)?;

        // Create message router
        let router = Arc::new(MessageRouter::new(100));
        router.reset_counter().await;

        let cto_rx = router.get_cto_receiver();
        let _dto_rx = router.get_dto_receiver();

        // Start router task
        let router_clone = router.clone();
        let transport_for_router = transport.clone();
        tokio::spawn(async move {
            router_clone.start(transport_for_router).await;
        });

        // Give router time to start
        tokio::time::sleep(Duration::from_millis(10)).await;

        let mut slave = Self {
            info: SlaveInfo {
                id,
                name,
                transport_config,
                slave_id_string: None,
                connected: false,
                connect_info: None,
            },
            transport,
            router,
            cto_rx,
            cal_manager: CalibrationManager::new(),
            daq_manager: Arc::new(Mutex::new(DaqManager::new())),
            a2l_path: None,
        };

        // Send CONNECT command
        let connect_info = slave.send_connect().await?;
        slave.info.connected = true;
        slave.info.connect_info = Some(connect_info);

        // Try to get slave ID
        if let Ok(slave_id) = slave.get_id(0).await {
            slave.info.slave_id_string = Some(slave_id);
        }

        tracing::info!(
            "Connected to slave '{}' (ID: {:?}) - Resource: {:?}",
            slave.info.name,
            id,
            slave.info.connect_info.as_ref().map(|i| i.resource)
        );

        Ok((slave, id))
    }

    /// Send CONNECT command
    async fn send_connect(&mut self) -> Result<ConnectInfo, SlaveError> {
        let connect_cmd = commands::connect();
        let response = self.send_command_and_wait(&connect_cmd).await?;

        let parsed = parse_response(&response).map_err(|e| SlaveError::ParseError(e.to_string()))?;

        let res_data = match parsed {
            Response::Res(data) => data,
            Response::Err(err) => return Err(SlaveError::Protocol(err)),
            _ => return Err(SlaveError::InvalidResponse("Unexpected response type".to_string())),
        };

        let connect_info = parse_connect_response(&res_data)
            .map_err(|e| SlaveError::ParseError(e.to_string()))?;

        Ok(connect_info)
    }

    /// Send command and wait for CTO response
    pub async fn send_command_and_wait(&self, command: &[u8]) -> Result<Vec<u8>, SlaveError> {
        // Send command
        self.transport
            .send_command_only(command)
            .await
            .map_err(SlaveError::Transport)?;

        // Wait for response
        let mut rx = self.cto_rx.lock().await;

        match timeout(Duration::from_secs(1), rx.recv()).await {
            Ok(Some(msg)) => {
                // Check for negative response
                if let Ok(Response::Err(err_code)) = parse_response(&msg.data) {
                    let cmd_code = command.get(4).copied().unwrap_or(0xFF);
                    tracing::error!(
                        "XCP Negative Response from {}: cmd=0x{:02X}, err={:?}",
                        self.info.name,
                        cmd_code,
                        err_code
                    );
                }
                Ok(msg.data)
            }
            Ok(None) => Err(SlaveError::NotConnected),
            Err(_) => Err(SlaveError::ResponseTimeout),
        }
    }

    /// Get identification from slave
    pub async fn get_id(&self, id_type: u8) -> Result<String, SlaveError> {
        let response = self.send_command_and_wait(&commands::get_id(id_type)).await?;
        let parsed = parse_response(&response).map_err(|e| SlaveError::ParseError(e.to_string()))?;

        let res_data = match parsed {
            Response::Res(data) => data,
            Response::Err(err) => return Err(SlaveError::Protocol(err)),
            _ => return Err(SlaveError::InvalidResponse("Unexpected response type".to_string())),
        };

        let (_, id_string) = parse_get_id_response(&res_data)
            .map_err(|e| SlaveError::ParseError(e.to_string()))?;

        Ok(id_string)
    }

    /// Read memory from slave using SHORT_UPLOAD
    pub async fn read_memory(&self, mta: Mta, length: u8) -> Result<Vec<u8>, SlaveError> {
        let response = self
            .send_command_and_wait(&commands::short_upload(mta, length))
            .await?;
        let parsed = parse_response(&response).map_err(|e| SlaveError::ParseError(e.to_string()))?;

        match parsed {
            Response::Res(data) => Ok(data),
            Response::Err(err) => Err(SlaveError::Protocol(err)),
            _ => Err(SlaveError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Write memory to slave using SHORT_DOWNLOAD
    pub async fn write_memory(&self, mta: Mta, data: &[u8]) -> Result<(), SlaveError> {
        let response = self
            .send_command_and_wait(&commands::short_download(mta, data))
            .await?;
        let parsed = parse_response(&response).map_err(|e| SlaveError::ParseError(e.to_string()))?;

        match parsed {
            Response::Res(_) => Ok(()),
            Response::Err(err) => Err(SlaveError::Protocol(err)),
            _ => Err(SlaveError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Set MTA for subsequent UPLOAD/DOWNLOAD operations
    pub async fn set_mta(&self, mta: Mta) -> Result<(), SlaveError> {
        let response = self.send_command_and_wait(&commands::set_mta(mta)).await?;
        let parsed = parse_response(&response).map_err(|e| SlaveError::ParseError(e.to_string()))?;

        match parsed {
            Response::Res(_) => Ok(()),
            Response::Err(err) => Err(SlaveError::Protocol(err)),
            _ => Err(SlaveError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Upload (read) data using current MTA
    pub async fn upload(&self, length: u8) -> Result<Vec<u8>, SlaveError> {
        let response = self.send_command_and_wait(&commands::upload(length)).await?;
        let parsed = parse_response(&response).map_err(|e| SlaveError::ParseError(e.to_string()))?;

        match parsed {
            Response::Res(data) => Ok(data),
            Response::Err(err) => Err(SlaveError::Protocol(err)),
            _ => Err(SlaveError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Download (write) data to current MTA
    pub async fn download(&self, data: &[u8]) -> Result<(), SlaveError> {
        let response = self.send_command_and_wait(&commands::download(data)).await?;
        let parsed = parse_response(&response).map_err(|e| SlaveError::ParseError(e.to_string()))?;

        match parsed {
            Response::Res(_) => Ok(()),
            Response::Err(err) => Err(SlaveError::Protocol(err)),
            _ => Err(SlaveError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Read a U32 value from memory
    pub async fn read_u32(&self, mta: Mta) -> Result<u32, SlaveError> {
        let data = self.read_memory(mta, 4).await?;
        if data.len() < 4 {
            return Err(SlaveError::InvalidResponse("Not enough data for u32".to_string()));
        }
        Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
    }

    /// Write a U32 value to memory
    pub async fn write_u32(&self, mta: Mta, value: u32) -> Result<(), SlaveError> {
        let bytes = value.to_le_bytes();
        self.write_memory(mta, &bytes).await
    }

    /// Read a U8 value from memory
    pub async fn read_u8(&self, mta: Mta) -> Result<u8, SlaveError> {
        let data = self.read_memory(mta, 1).await?;
        if data.is_empty() {
            return Err(SlaveError::InvalidResponse("Not enough data for u8".to_string()));
        }
        Ok(data[0])
    }

    /// Write a U8 value to memory
    pub async fn write_u8(&self, mta: Mta, value: u8) -> Result<(), SlaveError> {
        self.write_memory(mta, &[value]).await
    }

    /// Read a U8 array from memory
    pub async fn read_u8_array(&self, mta: Mta, count: usize) -> Result<Vec<u8>, SlaveError> {
        let mut result = Vec::with_capacity(count);
        let mut remaining = count;
        let mut current_addr = mta;

        while remaining > 0 {
            let to_read = remaining.min(255) as u8;
            let data = self.read_memory(current_addr, to_read).await?;
            result.extend_from_slice(&data);
            remaining -= to_read as usize;
            current_addr.address += to_read as u32;
        }

        Ok(result)
    }

    /// Write a U8 array to memory
    pub async fn write_u8_array(&self, mta: Mta, data: &[u8]) -> Result<(), SlaveError> {
        self.set_mta(mta).await?;

        for chunk in data.chunks(255) {
            self.download(chunk).await?;
        }

        Ok(())
    }

    /// Load A2L file for this slave
    pub fn load_a2l<P: AsRef<Path>>(&mut self, path: P) -> Result<(), String> {
        let path = path.as_ref();

        tracing::info!("Loading A2L file '{}' for slave '{}'...", path.display(), self.info.name);

        // Load calibration parameters
        self.cal_manager = CalibrationManager::from_a2l(path)?;

        // Load DAQ measurements
        let daq_manager = DaqManager::from_a2l(path)?;
        let meas_count = daq_manager.available_count();
        self.daq_manager = Arc::new(Mutex::new(daq_manager));

        self.a2l_path = Some(path.to_path_buf());

        tracing::info!(
            "Loaded A2L for slave '{}': {} cal params, {} measurements",
            self.info.name,
            self.cal_manager.params().len(),
            meas_count
        );

        Ok(())
    }

    /// Get DTO receiver for DAQ
    pub fn get_dto_receiver(&self) -> Arc<Mutex<mpsc::Receiver<XcpMessage>>> {
        self.router.get_dto_receiver()
    }

    /// Disconnect from slave
    pub async fn disconnect(&mut self) -> Result<(), SlaveError> {
        tracing::info!("Disconnecting from slave '{}'...", self.info.name);

        // Send DISCONNECT command
        let _ = self.send_command_and_wait(&commands::disconnect()).await;

        // Stop router
        self.router.stop().await;

        // Close transport
        self.transport.close().await.map_err(SlaveError::Transport)?;

        self.info.connected = false;

        tracing::info!("Disconnected from slave '{}'", self.info.name);

        Ok(())
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.info.connected
    }

    /// Get transport type
    pub fn transport_type(&self) -> crate::transport::TransportType {
        self.transport.transport_type()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slave_id_creation() {
        let id1 = SlaveId::new();
        let id2 = SlaveId::new();
        assert_ne!(id1.0, id2.0); // Two IDs should be different
    }

    #[test]
    fn test_slave_id_default() {
        let id: SlaveId = Default::default();
        // Should create a valid UUID (not nil)
        assert_ne!(id.0.to_string(), "00000000-0000-0000-0000-000000000000");
    }

    #[test]
    fn test_slave_id_display() {
        let id = SlaveId::new();
        let display = format!("{}", id);
        // Should display short form (first part of UUID)
        assert_eq!(display.len(), 8); // e.g., "a1b2c3d4"
    }

    #[test]
    fn test_slave_info_display_name_with_slave_id() {
        let info = SlaveInfo {
            id: SlaveId::new(),
            name: "ECU1".to_string(),
            transport_config: TransportConfig::Udp {
                addr: "127.0.0.1:5555".to_string(),
            },
            slave_id_string: Some("ABC123".to_string()),
            connected: true,
            connect_info: None,
        };
        assert_eq!(info.display_name(), "ECU1 [ABC123]");
    }

    #[test]
    fn test_slave_info_display_name_without_slave_id() {
        let info = SlaveInfo {
            id: SlaveId::new(),
            name: "ECU1".to_string(),
            transport_config: TransportConfig::Udp {
                addr: "127.0.0.1:5555".to_string(),
            },
            slave_id_string: None,
            connected: true,
            connect_info: None,
        };
        assert_eq!(info.display_name(), "ECU1");
    }

    #[test]
    fn test_slave_info_addr_string_udp() {
        let info = SlaveInfo {
            id: SlaveId::new(),
            name: "ECU1".to_string(),
            transport_config: TransportConfig::Udp {
                addr: "192.168.1.100:5555".to_string(),
            },
            slave_id_string: None,
            connected: false,
            connect_info: None,
        };
        assert_eq!(info.addr_string(), "192.168.1.100:5555");
    }

    #[test]
    fn test_slave_info_addr_string_can() {
        let info = SlaveInfo {
            id: SlaveId::new(),
            name: "ECU1".to_string(),
            transport_config: TransportConfig::Can {
                channel: "can0".to_string(),
                can_id: 0x123,
            },
            slave_id_string: None,
            connected: false,
            connect_info: None,
        };
        assert_eq!(info.addr_string(), "can0:123");
    }

    #[test]
    fn test_slave_id_clone() {
        let id1 = SlaveId::new();
        let id2 = id1.clone();
        assert_eq!(id1.0, id2.0);
    }

    #[test]
    fn test_slave_id_copy() {
        let id1 = SlaveId::new();
        let id2 = id1; // Copy, not move
        let _ = id1; // Can still use id1 after "move" because it's Copy
        assert_eq!(id1.0, id2.0);
    }

    #[test]
    fn test_slave_id_hash() {
        use std::collections::HashMap;
        let mut map: HashMap<SlaveId, String> = HashMap::new();
        let id = SlaveId::new();
        map.insert(id, "Test".to_string());
        assert_eq!(map.get(&id), Some(&"Test".to_string()));
    }

    #[test]
    fn test_slave_event_clone() {
        let id = SlaveId::new();
        let info = SlaveInfo {
            id,
            name: "Test".to_string(),
            transport_config: TransportConfig::Udp {
                addr: "127.0.0.1:5555".to_string(),
            },
            slave_id_string: None,
            connected: true,
            connect_info: None,
        };
        let event = SlaveEvent::Connected { id, info };
        let cloned = event.clone();
        match (event, cloned) {
            (SlaveEvent::Connected { id: id1, .. }, SlaveEvent::Connected { id: id2, .. }) => {
                assert_eq!(id1.0, id2.0);
            }
            _ => panic!("Event type mismatch"),
        }
    }

    #[test]
    fn test_slave_error_conversions() {
        // Test that SlaveError can be created from TransportError
        let transport_err = TransportError::Timeout;
        let slave_err: SlaveError = transport_err.into();
        match slave_err {
            SlaveError::Transport(_) => (), // Expected
            _ => panic!("Wrong error type"),
        }

        // Test that SlaveError can be created from ErrorCode
        let protocol_err = ErrorCode::CommandSyntax;
        let slave_err: SlaveError = protocol_err.into();
        match slave_err {
            SlaveError::Protocol(_) => (), // Expected
            _ => panic!("Wrong error type"),
        }
    }

    #[test]
    fn test_slave_error_display() {
        let err = SlaveError::NotConnected;
        assert!(err.to_string().contains("Not connected"));

        let err = SlaveError::ResponseTimeout;
        assert!(err.to_string().contains("Timeout"));

        let err = SlaveError::DaqError("test".to_string());
        assert!(err.to_string().contains("DAQ error"));
        assert!(err.to_string().contains("test"));
    }
}

/// Slave events (for UI updates and monitoring)
#[derive(Debug, Clone)]
pub enum SlaveEvent {
    /// Slave connected
    Connected { id: SlaveId, info: SlaveInfo },
    /// Slave disconnected
    Disconnected { id: SlaveId },
    /// A2L file loaded
    A2lLoaded { id: SlaveId, cal_count: usize, meas_count: usize },
    /// DAQ started
    DaqStarted { id: SlaveId },
    /// DAQ stopped
    DaqStopped { id: SlaveId },
    /// Error occurred
    Error { id: SlaveId, error: String },
}

