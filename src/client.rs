//! XCP Client Core v3 - Multi-Slave support
//!
//! This version supports multiple XCP slave connections:
//! - Each slave has its own transport, message router, and state
//! - Provides backward-compatible single-slave API
//! - Event system for monitoring slave state changes

use crate::protocol::{ConnectInfo, ErrorCode, Mta};
use crate::slave::{SlaveError, SlaveEvent, SlaveId, SlaveInfo, XcpSlave};
use crate::transport::{TransportConfig, TransportError};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

/// XCP Client Error
#[derive(Debug, thiserror::Error)]
pub enum XcpError {
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

    #[error("Slave error: {0}")]
    Slave(#[from] SlaveError),

    #[error("Slave not found: {0}")]
    SlaveNotFound(String),

    #[error("No slaves connected")]
    NoSlavesConnected,
}

/// DAQ Resolution Info
#[derive(Debug, Clone, Copy)]
pub struct DAQResolutionInfo {
    pub odt_entry_granularity_daq: u8,
    pub max_odt_entry_size_daq: u8,
    pub odt_entry_granularity_stim: u8,
    pub max_odt_entry_size_stim: u8,
    pub timestamp_mode: u8,
    pub timestamp_ticks: u16,
}

/// DAQ Processor Info
#[derive(Debug, Clone, Copy)]
pub struct DAQProcessorInfo {
    pub daq_properties: u8,
    pub max_daq: u16,
    pub max_event_channel: u16,
    pub min_daq: u8,
    pub daq_key_byte: u8,
}

/// Legacy Client State (for backward compatibility)
#[derive(Debug, Clone, Default)]
pub struct ClientState {
    pub connected: bool,
    pub connect_info: Option<ConnectInfo>,
    pub slave_id: Option<String>,
}

/// XCP Client - Multi-Slave Manager
///
/// Manages multiple XCP slave connections. Provides both multi-slave API
/// and backward-compatible single-slave API.
pub struct XcpClient {
    /// All slave connections (internal access only)
    slaves: HashMap<SlaveId, Arc<Mutex<XcpSlave>>>,

    /// Current active slave ID (for backward compatibility)
    current_slave_id: Option<SlaveId>,

    /// Event broadcast channel
    pub(crate) event_tx: broadcast::Sender<SlaveEvent>,
}

/// Slave accessor for iteration
pub struct SlaveAccessor {
    id: SlaveId,
    slave: Arc<Mutex<XcpSlave>>,
}

impl SlaveAccessor {
    pub fn id(&self) -> SlaveId {
        self.id
    }

    pub fn slave(&self) -> Arc<Mutex<XcpSlave>> {
        self.slave.clone()
    }
}

impl XcpClient {
    /// Create a new XCP client
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(100);
        Self {
            slaves: HashMap::new(),
            current_slave_id: None,
            event_tx,
        }
    }

    // ==================== Multi-Slave API ====================

    /// Connect to a new slave
    pub async fn connect_slave(
        &mut self,
        name: impl Into<String>,
        transport_config: TransportConfig,
    ) -> Result<SlaveId, XcpError> {
        let name = name.into();
        let name_for_log = name.clone();

        let (slave, id) = XcpSlave::connect(name, transport_config).await?;
        let slave_arc = Arc::new(Mutex::new(slave));

        self.slaves.insert(id, slave_arc.clone());

        // Set as current if first slave
        if self.current_slave_id.is_none() {
            self.current_slave_id = Some(id);
        }

        // Broadcast event
        let info = slave_arc.lock().await.info.clone();
        let _ = self.event_tx.send(SlaveEvent::Connected { id, info });

        tracing::info!("Connected to slave '{}' with ID {:?}", name_for_log, id);

        Ok(id)
    }

    /// Connect to a slave with UDP transport (convenience method)
    pub async fn connect_slave_udp(
        &mut self,
        name: impl Into<String>,
        addr: &str,
    ) -> Result<SlaveId, XcpError> {
        let config = TransportConfig::Udp {
            addr: addr.to_string(),
        };
        self.connect_slave(name, config).await
    }

    /// Disconnect a specific slave
    pub async fn disconnect_slave(&mut self, id: SlaveId) -> Result<(), XcpError> {
        if let Some(slave) = self.slaves.remove(&id) {
            let mut slave = slave.lock().await;
            slave.disconnect().await?;

            // Broadcast event
            let _ = self.event_tx.send(SlaveEvent::Disconnected { id });

            // Update current slave if needed
            if self.current_slave_id == Some(id) {
                self.current_slave_id = self.slaves.keys().next().copied();
            }

            tracing::info!("Disconnected slave {:?}", id);
        }

        Ok(())
    }

    /// Disconnect all slaves
    pub async fn disconnect_all(&mut self) -> Result<(), XcpError> {
        let ids: Vec<SlaveId> = self.slaves.keys().copied().collect();
        for id in ids {
            self.disconnect_slave(id).await.ok();
        }
        Ok(())
    }

    /// Get a slave by ID
    pub fn get_slave(&self, id: SlaveId) -> Option<Arc<Mutex<XcpSlave>>> {
        self.slaves.get(&id).cloned()
    }

    /// Get the current slave
    pub fn current_slave(&self) -> Option<Arc<Mutex<XcpSlave>>> {
        self.current_slave_id.and_then(|id| self.get_slave(id))
    }

    /// Set the current slave
    pub fn set_current_slave(&mut self, id: SlaveId) -> Result<(), XcpError> {
        if self.slaves.contains_key(&id) {
            self.current_slave_id = Some(id);
            Ok(())
        } else {
            Err(XcpError::SlaveNotFound(format!("{:?}", id)))
        }
    }

    /// Get all slave IDs
    pub fn slave_ids(&self) -> Vec<SlaveId> {
        self.slaves.keys().copied().collect()
    }

    /// Get all slaves for iteration (internal use)
    pub(crate) fn iter_slaves(&self) -> impl Iterator<Item = (SlaveId, Arc<Mutex<XcpSlave>>)> + '_ {
        self.slaves.iter().map(|(id, slave)| (*id, slave.clone()))
    }

    /// Get all slave infos
    pub async fn slave_infos(&self) -> Vec<SlaveInfo> {
        let mut infos = Vec::new();
        for slave in self.slaves.values() {
            let slave = slave.lock().await;
            infos.push(slave.info.clone());
        }
        infos
    }

    /// Load A2L file for a specific slave
    pub async fn load_a2l_for_slave<P: AsRef<std::path::Path>>(
        &mut self,
        id: SlaveId,
        path: P,
    ) -> Result<(), XcpError> {
        let slave = self
            .slaves
            .get(&id)
            .ok_or_else(|| XcpError::SlaveNotFound(format!("{:?}", id)))?;

        let mut slave = slave.lock().await;
        slave.load_a2l(path).map_err(|e| XcpError::ParseError(e))?;

        // Broadcast event
        let cal_count = slave.cal_manager.params().len();
        let meas_count = slave.daq_manager.lock().await.available_count();
        let _ = self.event_tx.send(SlaveEvent::A2lLoaded {
            id,
            cal_count,
            meas_count,
        });

        Ok(())
    }

    /// Subscribe to slave events
    pub fn subscribe_events(&self) -> broadcast::Receiver<SlaveEvent> {
        self.event_tx.subscribe()
    }

    /// Get number of connected slaves
    pub fn slave_count(&self) -> usize {
        self.slaves.len()
    }

    /// Check if any slave is connected
    pub fn has_slaves(&self) -> bool {
        !self.slaves.is_empty()
    }

    // ==================== Backward Compatibility API ====================

    /// Connect to a single slave (backward compatible)
    ///
    /// This method maintains backward compatibility with single-slave usage.
    /// For multi-slave scenarios, use `connect_slave` instead.
    pub async fn connect(&mut self, addr: &str) -> Result<ConnectInfo, XcpError> {
        // Check if already connected to this address
        for (id, slave) in &self.slaves {
            let slave = slave.lock().await;
            if slave.info.addr_string() == addr && slave.is_connected() {
                // Already connected, set as current and return info
                self.current_slave_id = Some(*id);
                return Ok(slave.info.connect_info.clone().unwrap_or_default());
            }
        }

        // Connect new slave with default name
        let name = format!("Slave-{}", self.slaves.len() + 1);
        let id = self.connect_slave_udp(name, addr).await?;
        self.current_slave_id = Some(id);

        let slave = self.get_slave(id).unwrap();
        let slave = slave.lock().await;

        Ok(slave.info.connect_info.clone().unwrap_or_default())
    }

    /// Disconnect from current slave (backward compatible)
    pub async fn disconnect(&mut self) -> Result<(), XcpError> {
        if let Some(id) = self.current_slave_id {
            self.disconnect_slave(id).await
        } else {
            Err(XcpError::NotConnected)
        }
    }

    /// Check if connected to any slave (backward compatible)
    pub async fn is_connected(&self) -> bool {
        if let Some(id) = self.current_slave_id {
            if let Some(slave) = self.get_slave(id) {
                let slave = slave.lock().await;
                return slave.is_connected();
            }
        }
        false
    }

    /// Get DTO receiver for current slave (backward compatible)
    /// Returns None if no slave is connected
    pub async fn get_dto_receiver(
        &self,
    ) -> Option<Arc<Mutex<tokio::sync::mpsc::Receiver<crate::transport::XcpMessage>>>> {
        if let Some(id) = self.current_slave_id {
            if let Some(slave) = self.get_slave(id) {
                let slave = slave.lock().await;
                return Some(slave.get_dto_receiver());
            }
        }
        None
    }

    /// Get current state (backward compatible)
    pub async fn state(&self) -> ClientState {
        if let Some(id) = self.current_slave_id {
            if let Some(slave) = self.get_slave(id) {
                let slave = slave.lock().await;
                return ClientState {
                    connected: slave.info.connected,
                    connect_info: slave.info.connect_info.clone(),
                    slave_id: slave.info.slave_id_string.clone(),
                };
            }
        }
        ClientState::default()
    }

    /// Get ID from current slave (backward compatible)
    pub async fn get_id(&self, id_type: u8) -> Result<String, XcpError> {
        let slave = self.current_slave().ok_or(XcpError::NotConnected)?;
        let slave = slave.lock().await;
        slave.get_id(id_type).await.map_err(|e| e.into())
    }

    /// Read memory from current slave (backward compatible)
    pub async fn read_memory(&self, mta: Mta, length: u8) -> Result<Vec<u8>, XcpError> {
        let slave = self.current_slave().ok_or(XcpError::NotConnected)?;
        let slave = slave.lock().await;
        slave.read_memory(mta, length).await.map_err(|e| e.into())
    }

    /// Write memory to current slave (backward compatible)
    pub async fn write_memory(&self, mta: Mta, data: &[u8]) -> Result<(), XcpError> {
        let slave = self.current_slave().ok_or(XcpError::NotConnected)?;
        let slave = slave.lock().await;
        slave.write_memory(mta, data).await.map_err(|e| e.into())
    }

    /// Set MTA on current slave (backward compatible)
    pub async fn set_mta(&self, mta: Mta) -> Result<(), XcpError> {
        let slave = self.current_slave().ok_or(XcpError::NotConnected)?;
        let slave = slave.lock().await;
        slave.set_mta(mta).await.map_err(|e| e.into())
    }

    /// Upload data from current slave (backward compatible)
    pub async fn upload(&self, length: u8) -> Result<Vec<u8>, XcpError> {
        let slave = self.current_slave().ok_or(XcpError::NotConnected)?;
        let slave = slave.lock().await;
        slave.upload(length).await.map_err(|e| e.into())
    }

    /// Download data to current slave (backward compatible)
    pub async fn download(&self, data: &[u8]) -> Result<(), XcpError> {
        let slave = self.current_slave().ok_or(XcpError::NotConnected)?;
        let slave = slave.lock().await;
        slave.download(data).await.map_err(|e| e.into())
    }

    /// Read U32 from current slave (backward compatible)
    pub async fn read_u32(&self, mta: Mta) -> Result<u32, XcpError> {
        let slave = self.current_slave().ok_or(XcpError::NotConnected)?;
        let slave = slave.lock().await;
        slave.read_u32(mta).await.map_err(|e| e.into())
    }

    /// Write U32 to current slave (backward compatible)
    pub async fn write_u32(&self, mta: Mta, value: u32) -> Result<(), XcpError> {
        let slave = self.current_slave().ok_or(XcpError::NotConnected)?;
        let slave = slave.lock().await;
        slave.write_u32(mta, value).await.map_err(|e| e.into())
    }

    /// Read U8 from current slave (backward compatible)
    pub async fn read_u8(&self, mta: Mta) -> Result<u8, XcpError> {
        let slave = self.current_slave().ok_or(XcpError::NotConnected)?;
        let slave = slave.lock().await;
        slave.read_u8(mta).await.map_err(|e| e.into())
    }

    /// Write U8 to current slave (backward compatible)
    pub async fn write_u8(&self, mta: Mta, value: u8) -> Result<(), XcpError> {
        let slave = self.current_slave().ok_or(XcpError::NotConnected)?;
        let slave = slave.lock().await;
        slave.write_u8(mta, value).await.map_err(|e| e.into())
    }

    /// Read U8 array from current slave (backward compatible)
    pub async fn read_u8_array(&self, mta: Mta, count: usize) -> Result<Vec<u8>, XcpError> {
        let slave = self.current_slave().ok_or(XcpError::NotConnected)?;
        let slave = slave.lock().await;
        slave.read_u8_array(mta, count).await.map_err(|e| e.into())
    }

    /// Write U8 array to current slave (backward compatible)
    pub async fn write_u8_array(&self, mta: Mta, data: &[u8]) -> Result<(), XcpError> {
        let slave = self.current_slave().ok_or(XcpError::NotConnected)?;
        let slave = slave.lock().await;
        slave.write_u8_array(mta, data).await.map_err(|e| e.into())
    }

    // ==================== DAQ Methods (Backward Compatible) ====================

    /// Get DAQ resolution info from current slave
    pub async fn get_daq_resolution_info(&self) -> Result<DAQResolutionInfo, XcpError> {
        let slave = self.current_slave().ok_or(XcpError::NotConnected)?;
        let slave = slave.lock().await;
        slave.get_daq_resolution_info().await.map_err(|e| e.into())
    }

    /// Get DAQ processor info from current slave
    pub async fn get_daq_processor_info(&self) -> Result<DAQProcessorInfo, XcpError> {
        let slave = self.current_slave().ok_or(XcpError::NotConnected)?;
        let slave = slave.lock().await;
        slave.get_daq_processor_info().await.map_err(|e| e.into())
    }

    /// Get DAQ clock from current slave
    pub async fn get_daq_clock(&self) -> Result<u32, XcpError> {
        let slave = self.current_slave().ok_or(XcpError::NotConnected)?;
        let slave = slave.lock().await;
        slave.get_daq_clock().await.map_err(|e| e.into())
    }

    /// Free DAQ resources on current slave
    pub async fn free_daq(&self) -> Result<(), XcpError> {
        let slave = self.current_slave().ok_or(XcpError::NotConnected)?;
        let slave = slave.lock().await;
        slave.free_daq().await.map_err(|e| e.into())
    }

    /// Allocate DAQ lists on current slave
    pub async fn alloc_daq(&self, daq_count: u16) -> Result<(), XcpError> {
        let slave = self.current_slave().ok_or(XcpError::NotConnected)?;
        let slave = slave.lock().await;
        slave.alloc_daq(daq_count).await.map_err(|e| e.into())
    }

    /// Allocate ODTs on current slave
    pub async fn alloc_odt(&self, daq_list: u16, odt_count: u8) -> Result<(), XcpError> {
        let slave = self.current_slave().ok_or(XcpError::NotConnected)?;
        let slave = slave.lock().await;
        slave
            .alloc_odt(daq_list, odt_count)
            .await
            .map_err(|e| e.into())
    }

    /// Allocate ODT entries on current slave
    pub async fn alloc_odt_entry(
        &self,
        daq_list: u16,
        odt: u8,
        entry_count: u8,
    ) -> Result<(), XcpError> {
        let slave = self.current_slave().ok_or(XcpError::NotConnected)?;
        let slave = slave.lock().await;
        slave
            .alloc_odt_entry(daq_list, odt, entry_count)
            .await
            .map_err(|e| e.into())
    }

    /// Set DAQ pointer on current slave
    pub async fn set_daq_ptr(&self, daq_list: u16, odt: u8, entry: u8) -> Result<(), XcpError> {
        let slave = self.current_slave().ok_or(XcpError::NotConnected)?;
        let slave = slave.lock().await;
        slave
            .set_daq_ptr(daq_list, odt, entry)
            .await
            .map_err(|e| e.into())
    }

    /// Write DAQ entry on current slave
    pub async fn write_daq(&self, bit_offset: u8, mta: Mta, data_size: u8) -> Result<(), XcpError> {
        let slave = self.current_slave().ok_or(XcpError::NotConnected)?;
        let slave = slave.lock().await;
        slave
            .write_daq(bit_offset, mta, data_size)
            .await
            .map_err(|e| e.into())
    }

    /// Set DAQ list mode on current slave
    pub async fn set_daq_list_mode(
        &self,
        mode: u8,
        daq_list: u16,
        event_channel: u16,
        prescaler: u8,
        priority: u8,
    ) -> Result<(), XcpError> {
        let slave = self.current_slave().ok_or(XcpError::NotConnected)?;
        let slave = slave.lock().await;
        slave
            .set_daq_list_mode(mode, daq_list, event_channel, prescaler, priority)
            .await
            .map_err(|e| e.into())
    }

    /// Start/stop DAQ list on current slave
    pub async fn start_stop_daq_list(&self, mode: u8, daq_list: u16) -> Result<(), XcpError> {
        let slave = self.current_slave().ok_or(XcpError::NotConnected)?;
        let slave = slave.lock().await;
        slave
            .start_stop_daq_list(mode, daq_list)
            .await
            .map_err(|e| e.into())
    }

    /// Start/stop synch on current slave
    pub async fn start_stop_synch(&self, mode: u8) -> Result<(), XcpError> {
        let slave = self.current_slave().ok_or(XcpError::NotConnected)?;
        let slave = slave.lock().await;
        slave.start_stop_synch(mode).await.map_err(|e| e.into())
    }
}

impl Default for XcpClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xcp_client_new() {
        let client = XcpClient::new();
        assert_eq!(client.slave_count(), 0);
        assert!(!client.has_slaves());
        assert!(client.current_slave_id.is_none());
    }

    #[test]
    fn test_xcp_client_default() {
        let client: XcpClient = Default::default();
        assert_eq!(client.slave_count(), 0);
    }

    #[test]
    fn test_slave_id_hashmap() {
        let mut client = XcpClient::new();
        let id1 = SlaveId::new();
        let id2 = SlaveId::new();

        // Just test that HashMap operations work with SlaveId
        assert_eq!(client.slave_count(), 0);
        client.slaves.reserve(10); // Reserve capacity without inserting
        assert!(client.slaves.capacity() >= 10);

        // Test ID uniqueness
        assert_ne!(id1.0, id2.0);
    }

    #[test]
    fn test_transport_config_udp_creation() {
        let config = TransportConfig::Udp {
            addr: "127.0.0.1:5555".to_string(),
        };
        assert_eq!(
            config.transport_type(),
            crate::transport::TransportType::Udp
        );
    }

    #[test]
    fn test_transport_config_can_creation() {
        let config = TransportConfig::Can {
            channel: "can0".to_string(),
            can_id: 0x123,
        };
        assert_eq!(
            config.transport_type(),
            crate::transport::TransportType::Can
        );
    }

    #[test]
    fn test_transport_config_tcp_creation() {
        let config = TransportConfig::Tcp {
            addr: "127.0.0.1:5555".to_string(),
        };
        assert_eq!(
            config.transport_type(),
            crate::transport::TransportType::Tcp
        );
    }

    #[test]
    fn test_slave_info_creation() {
        let info = SlaveInfo {
            id: SlaveId::new(),
            name: "TestSlave".to_string(),
            transport_config: TransportConfig::Udp {
                addr: "192.168.1.1:5555".to_string(),
            },
            slave_id_string: Some("ABC123".to_string()),
            connected: true,
            connect_info: Some(ConnectInfo::default()),
        };

        assert_eq!(info.name, "TestSlave");
        assert_eq!(info.addr_string(), "192.168.1.1:5555");
        assert!(info.connected);
    }

    #[test]
    fn test_slave_event_variants() {
        let id = SlaveId::new();
        let info = SlaveInfo {
            id,
            name: "Test".to_string(),
            transport_config: TransportConfig::Udp {
                addr: "127.0.0.1:5555".to_string(),
            },
            slave_id_string: None,
            connected: false,
            connect_info: None,
        };

        // Test all event variants can be created
        let _ = SlaveEvent::Connected {
            id,
            info: info.clone(),
        };
        let _ = SlaveEvent::Disconnected { id };
        let _ = SlaveEvent::A2lLoaded {
            id,
            cal_count: 10,
            meas_count: 20,
        };
        let _ = SlaveEvent::DaqStarted { id };
        let _ = SlaveEvent::DaqStopped { id };
        let _ = SlaveEvent::Error {
            id,
            error: "test".to_string(),
        };
    }

    #[test]
    fn test_xcp_error_variants() {
        // Test error creation
        let err = XcpError::NotConnected;
        assert!(err.to_string().contains("Not connected"));

        let err = XcpError::NoSlavesConnected;
        assert!(err.to_string().contains("No slaves"));

        let err = XcpError::SlaveNotFound("test-id".to_string());
        assert!(err.to_string().contains("Slave not found"));
        assert!(err.to_string().contains("test-id"));
    }

    #[tokio::test]
    async fn test_xcp_client_subscribe_events() {
        let client = XcpClient::new();
        let mut rx = client.subscribe_events();

        // Send a test event
        let id = SlaveId::new();
        let info = SlaveInfo {
            id,
            name: "Test".to_string(),
            transport_config: TransportConfig::Udp {
                addr: "127.0.0.1:5555".to_string(),
            },
            slave_id_string: None,
            connected: false,
            connect_info: None,
        };
        let event = SlaveEvent::Connected { id, info };

        // Should be able to send without error (though receiver may not get it due to timing)
        let _ = client.event_tx.send(event);

        // Try to receive (may or may not get the event depending on timing)
        let result = tokio::time::timeout(tokio::time::Duration::from_millis(100), rx.recv()).await;
        assert!(result.is_ok()); // Should not timeout
    }

    #[test]
    fn test_daq_resolution_info_default() {
        let info = DAQResolutionInfo {
            odt_entry_granularity_daq: 1,
            max_odt_entry_size_daq: 255,
            odt_entry_granularity_stim: 1,
            max_odt_entry_size_stim: 255,
            timestamp_mode: 0,
            timestamp_ticks: 1000,
        };

        assert_eq!(info.odt_entry_granularity_daq, 1);
        assert_eq!(info.max_odt_entry_size_daq, 255);
    }

    #[test]
    fn test_daq_processor_info_default() {
        let info = DAQProcessorInfo {
            daq_properties: 0,
            max_daq: 16,
            max_event_channel: 8,
            min_daq: 1,
            daq_key_byte: 0,
        };

        assert_eq!(info.max_daq, 16);
        assert_eq!(info.max_event_channel, 8);
    }
}
