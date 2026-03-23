//! XCP Client Core v2 - with unified CTO/DAQ message routing
//!
//! This version uses a MessageRouter to separate CTO responses and DAQ packets:
//! - All packets from slave are received by MessageRouter
//! - PID >= 0xFC: CTO (command responses) -> cto_rx channel
//! - PID < 0xFC: DTO (DAQ data) -> dto_rx channel
//!
//! This allows concurrent DAQ reception and command sending

use crate::protocol::{
    commands, parse_connect_response, parse_get_id_response, parse_response,
    ConnectInfo, ErrorCode, Mta, Response,
};
use crate::transport::{MessageRouter, TransportError, UdpTransport, XcpMessage};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::time::{timeout, Duration};

/// XCP Client Error
#[derive(Debug, thiserror::Error)]
pub enum XcpError {
    #[error("Transport error: {0}")]
    Transport(#[from] TransportError),

    #[error("XCP error: {0}")]
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
}

/// XCP Client State
#[derive(Debug, Clone)]
pub struct ClientState {
    pub connected: bool,
    pub connect_info: Option<ConnectInfo>,
    pub slave_id: Option<String>,
}

impl Default for ClientState {
    fn default() -> Self {
        Self {
            connected: false,
            connect_info: None,
            slave_id: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DAQResolutionInfo {
    pub odt_entry_granularity_daq: u8,
    pub max_odt_entry_size_daq: u8,
    pub odt_entry_granularity_stim: u8,
    pub max_odt_entry_size_stim: u8,
    pub timestamp_mode: u8,
    pub timestamp_ticks: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct DAQProcessorInfo {
    pub daq_properties: u8,
    pub max_daq: u16,
    pub max_event_channel: u16,
    pub min_daq: u8,
    pub daq_key_byte: u8,
}

/// XCP Master Client v2 - with message routing
pub struct XcpClient {
    transport: Option<Arc<UdpTransport>>,
    router: Option<Arc<MessageRouter>>,
    state: Arc<RwLock<ClientState>>,
    /// Channel for CTO responses (used by commands)
    cto_rx: Option<Arc<Mutex<mpsc::Receiver<XcpMessage>>>>,
}

impl XcpClient {
    /// Create a new XCP client
    pub fn new() -> Self {
        Self {
            transport: None,
            router: None,
            state: Arc::new(RwLock::new(ClientState::default())),
            cto_rx: None,
        }
    }

    /// Connect to XCP slave and start message router
    pub async fn connect(&mut self, addr: &str) -> Result<ConnectInfo, XcpError> {
        // Check if already connected
        {
            let state = self.state.read().await;
            if state.connected {
                return Err(XcpError::AlreadyConnected);
            }
        }

        // Create transport
        let transport = Arc::new(UdpTransport::connect(addr).await?);

        // Create message router
        let router = Arc::new(MessageRouter::new(100));

        // Reset counter tracking for new connection
        router.reset_counter().await;

        // Get channels
        let cto_rx = router.get_cto_receiver();
        let _dto_rx = router.get_dto_receiver();

        // Clone for router task
        let router_clone = router.clone();
        let transport_for_router = transport.clone();

        // Start message router task
        tokio::spawn(async move {
            router_clone.start(transport_for_router).await;
        });

        // Give router task time to start receiving
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Store references
        self.transport = Some(transport);
        self.router = Some(router);
        self.cto_rx = Some(cto_rx);

        // Send CONNECT command using new mechanism
        let connect_cmd = commands::connect();
        let response = self.send_command_and_wait(&connect_cmd).await?;

        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        let res_data = match parsed {
            Response::Res(data) => data,
            Response::Err(err) => return Err(XcpError::Protocol(err)),
            _ => return Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        };

        let connect_info = parse_connect_response(&res_data).map_err(XcpError::ParseError)?;

        // Update state
        {
            let mut state = self.state.write().await;
            state.connected = true;
            state.connect_info = Some(connect_info.clone());
        }

        // Try to get slave ID
        let _ = self.get_id(0).await;

        Ok(connect_info)
    }

    /// Send command and wait for CTO response
    async fn send_command_and_wait(&self, command: &[u8]) -> Result<Vec<u8>, XcpError> {
        let transport = self.transport.as_ref().ok_or(XcpError::NotConnected)?;
        let cto_rx = self.cto_rx.as_ref().ok_or(XcpError::NotConnected)?;

        // Send command only (response will be received by MessageRouter)
        transport.send_command_only(command).await?;

        // Wait for response (filter for matching counter if needed)
        let mut rx = cto_rx.lock().await;

        match timeout(Duration::from_secs(1), rx.recv()).await {
            Ok(Some(msg)) => {
                // msg.data already contains complete XCP packet (4-byte transport header + payload)
                // Return as-is for backward compatibility with existing response parsers
                Ok(msg.data)
            }
            Ok(None) => Err(XcpError::NotConnected),
            Err(_) => Err(XcpError::ResponseTimeout),
        }
    }

    /// Get DTO receiver for DAQ manager
    pub fn get_dto_receiver(&self) -> Option<Arc<Mutex<mpsc::Receiver<XcpMessage>>>> {
        self.router.as_ref().map(|r| r.get_dto_receiver())
    }

    /// Receive a DAQ/DTO packet from the dedicated channel
    /// This is the preferred way for DAQ manager to receive data
    pub async fn recv_daq_packet(&self) -> Result<Vec<u8>, XcpError> {
        let router = self.router.as_ref().ok_or(XcpError::NotConnected)?;
        let dto_rx = router.get_dto_receiver();

        let mut rx = dto_rx.lock().await;

        match timeout(Duration::from_secs(30), rx.recv()).await {
            Ok(Some(msg)) => {
                // msg.data already contains complete XCP packet (4-byte transport header + payload)
                Ok(msg.data)
            }
            Ok(None) => Err(XcpError::NotConnected),
            Err(_) => Err(XcpError::ResponseTimeout),
        }
    }

    /// Disconnect from XCP slave
    pub async fn disconnect(&mut self) -> Result<(), XcpError> {
        // Send DISCONNECT command
        let _ = self.send_command_and_wait(&commands::disconnect()).await;

        // Stop router
        if let Some(router) = &self.router {
            router.stop().await;
        }

        // Update state
        {
            let mut state = self.state.write().await;
            state.connected = false;
            state.connect_info = None;
            state.slave_id = None;
        }

        self.transport = None;
        self.router = None;
        self.cto_rx = None;

        Ok(())
    }

    /// Get identification from slave
    pub async fn get_id(&self, id_type: u8) -> Result<String, XcpError> {
        let response = self.send_command_and_wait(&commands::get_id(id_type)).await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        let res_data = match parsed {
            Response::Res(data) => data,
            Response::Err(err) => return Err(XcpError::Protocol(err)),
            _ => return Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        };

        let (_, id_string) = parse_get_id_response(&res_data).map_err(XcpError::ParseError)?;

        // Update state
        {
            let mut state = self.state.write().await;
            state.slave_id = Some(id_string.clone());
        }

        Ok(id_string)
    }

    /// Read memory from slave using SHORT_UPLOAD
    pub async fn read_memory(&self, mta: Mta, length: u8) -> Result<Vec<u8>, XcpError> {
        let response = self.send_command_and_wait(&commands::short_upload(mta, length)).await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        match parsed {
            Response::Res(data) => Ok(data),
            Response::Err(err) => Err(XcpError::Protocol(err)),
            _ => Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Write memory to slave using SHORT_DOWNLOAD
    pub async fn write_memory(&self, mta: Mta, data: &[u8]) -> Result<(), XcpError> {
        let response = self.send_command_and_wait(&commands::short_download(mta, data)).await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        match parsed {
            Response::Res(_) => Ok(()),
            Response::Err(err) => Err(XcpError::Protocol(err)),
            _ => Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Set MTA for subsequent UPLOAD/DOWNLOAD operations
    pub async fn set_mta(&self, mta: Mta) -> Result<(), XcpError> {
        let response = self.send_command_and_wait(&commands::set_mta(mta)).await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        match parsed {
            Response::Res(_) => Ok(()),
            Response::Err(err) => Err(XcpError::Protocol(err)),
            _ => Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Upload (read) data using current MTA
    pub async fn upload(&self, length: u8) -> Result<Vec<u8>, XcpError> {
        let response = self.send_command_and_wait(&commands::upload(length)).await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        match parsed {
            Response::Res(data) => Ok(data),
            Response::Err(err) => Err(XcpError::Protocol(err)),
            _ => Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Download (write) data to current MTA
    pub async fn download(&self, data: &[u8]) -> Result<(), XcpError> {
        let response = self.send_command_and_wait(&commands::download(data)).await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        match parsed {
            Response::Res(_) => Ok(()),
            Response::Err(err) => Err(XcpError::Protocol(err)),
            _ => Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Read a U32 value from memory
    pub async fn read_u32(&self, mta: Mta) -> Result<u32, XcpError> {
        let data = self.read_memory(mta, 4).await?;
        if data.len() < 4 {
            return Err(XcpError::InvalidResponse("Not enough data for u32".to_string()));
        }
        Ok(u32::from_be_bytes([data[0], data[1], data[2], data[3]]))
    }

    /// Write a U32 value to memory
    pub async fn write_u32(&self, mta: Mta, value: u32) -> Result<(), XcpError> {
        let bytes = value.to_be_bytes();
        self.write_memory(mta, &bytes).await
    }

    /// Read a U8 value from memory
    pub async fn read_u8(&self, mta: Mta) -> Result<u8, XcpError> {
        let data = self.read_memory(mta, 1).await?;
        if data.is_empty() {
            return Err(XcpError::InvalidResponse("Not enough data for u8".to_string()));
        }
        Ok(data[0])
    }

    /// Write a U8 value to memory
    pub async fn write_u8(&self, mta: Mta, value: u8) -> Result<(), XcpError> {
        self.write_memory(mta, &[value]).await
    }

    /// Read a U8 array from memory
    pub async fn read_u8_array(&self, mta: Mta, count: usize) -> Result<Vec<u8>, XcpError> {
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
    pub async fn write_u8_array(&self, mta: Mta, data: &[u8]) -> Result<(), XcpError> {
        // Set initial MTA
        self.set_mta(mta).await?;

        // Write in chunks (max 255 bytes per DOWNLOAD)
        for chunk in data.chunks(255) {
            self.download(chunk).await?;
        }

        Ok(())
    }

    /// Get DAQ resolution info
    pub async fn get_daq_resolution_info(&self) -> Result<DAQResolutionInfo, XcpError> {
        let response = self.send_command_and_wait(&commands::get_daq_resolution_info()).await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        match parsed {
            Response::Res(data) if data.len() == 7 => {
                Ok(DAQResolutionInfo {
                    odt_entry_granularity_daq: data[0],
                    max_odt_entry_size_daq: data[1],
                    odt_entry_granularity_stim: data[2],
                    max_odt_entry_size_stim: data[3],
                    timestamp_mode: data[4],
                    timestamp_ticks: u16::from_be_bytes([data[5], data[6]]),
                })
            }
            Response::Res(data) => Err(XcpError::InvalidResponse(format!(
                "Invalid response length: {}",
                data.len()
            ))),
            Response::Err(err) => Err(XcpError::Protocol(err)),
            _ => Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Get DAQ processor info
    pub async fn get_daq_processor_info(&self) -> Result<DAQProcessorInfo, XcpError> {
        let response = self.send_command_and_wait(&commands::get_daq_processor_info()).await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        match parsed {
            Response::Res(data) if data.len() == 7 => {
                Ok(DAQProcessorInfo {
                    daq_properties: data[0],
                    max_daq: u16::from_le_bytes([data[1], data[2]]),
                    max_event_channel: u16::from_le_bytes([data[3], data[4]]),
                    min_daq: data[5],
                    daq_key_byte: data[6],
                })
            }
            _ => Err(XcpError::InvalidResponse("Invalid response length".to_string())),
        }
    }

    /// Free DAQ resources
    pub async fn free_daq(&self) -> Result<(), XcpError> {
        let response = self.send_command_and_wait(&commands::free_daq()).await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        match parsed {
            Response::Res(_) => Ok(()),
            Response::Err(err) => Err(XcpError::Protocol(err)),
            _ => Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Allocate DAQ lists
    pub async fn alloc_daq(&self, daq_count: u16) -> Result<(), XcpError> {
        let response = self.send_command_and_wait(&commands::alloc_daq(daq_count)).await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        match parsed {
            Response::Res(_) => Ok(()),
            Response::Err(err) => Err(XcpError::Protocol(err)),
            _ => Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Allocate ODTs
    pub async fn alloc_odt(&self, daq_list: u16, odt_count: u8) -> Result<(), XcpError> {
        let response = self
            .send_command_and_wait(&commands::alloc_odt(daq_list, odt_count))
            .await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        match parsed {
            Response::Res(_) => Ok(()),
            Response::Err(err) => Err(XcpError::Protocol(err)),
            _ => Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Allocate ODT entries
    pub async fn alloc_odt_entry(
        &self,
        daq_list: u16,
        odt: u8,
        entry_count: u8,
    ) -> Result<(), XcpError> {
        let response = self
            .send_command_and_wait(&commands::alloc_odt_entry(daq_list, odt, entry_count))
            .await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        match parsed {
            Response::Res(_) => Ok(()),
            Response::Err(err) => Err(XcpError::Protocol(err)),
            _ => Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Set DAQ pointer
    pub async fn set_daq_ptr(&self, daq_list: u16, odt: u8, entry: u8) -> Result<(), XcpError> {
        let response = self
            .send_command_and_wait(&commands::set_daq_ptr(daq_list, odt, entry))
            .await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        match parsed {
            Response::Res(_) => Ok(()),
            Response::Err(err) => Err(XcpError::Protocol(err)),
            _ => Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Write DAQ entry configuration
    pub async fn write_daq(&self, bit_offset: u8, mta: Mta, data_size: u8) -> Result<(), XcpError> {
        let response = self
            .send_command_and_wait(&commands::write_daq(bit_offset, mta, data_size))
            .await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        match parsed {
            Response::Res(_) => Ok(()),
            Response::Err(err) => Err(XcpError::Protocol(err)),
            _ => Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Set DAQ list mode
    pub async fn set_daq_list_mode(
        &self,
        mode: u8,
        daq_list: u16,
        event_channel: u16,
        prescaler: u8,
        priority: u8,
    ) -> Result<(), XcpError> {
        let response = self
            .send_command_and_wait(&commands::set_daq_list_mode(
                mode, daq_list, event_channel, prescaler, priority,
            ))
            .await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        match parsed {
            Response::Res(_) => Ok(()),
            Response::Err(err) => Err(XcpError::Protocol(err)),
            _ => Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Start or stop a DAQ list
    pub async fn start_stop_daq_list(&self, mode: u8, daq_list: u16) -> Result<(), XcpError> {
        let response = self
            .send_command_and_wait(&commands::start_stop_daq_list(mode, daq_list))
            .await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        match parsed {
            Response::Res(_) => Ok(()),
            Response::Err(err) => Err(XcpError::Protocol(err)),
            _ => Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Start/stop synch
    pub async fn start_stop_synch(&self, mode: u8) -> Result<(), XcpError> {
        let response = self
            .send_command_and_wait(&commands::start_stop_synch(mode))
            .await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        match parsed {
            Response::Res(_) => Ok(()),
            Response::Err(err) => Err(XcpError::Protocol(err)),
            _ => Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Get client state
    pub fn state(&self) -> Arc<RwLock<ClientState>> {
        self.state.clone()
    }

    /// Check if connected
    pub async fn is_connected(&self) -> bool {
        let state = self.state.read().await;
        state.connected
    }
}

impl Default for XcpClient {
    fn default() -> Self {
        Self::new()
    }
}
