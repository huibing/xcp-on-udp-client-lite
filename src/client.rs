//! XCP Client Core

use crate::protocol::{
    commands, parse_response, parse_connect_response, parse_get_id_response,
    ConnectInfo, ErrorCode, Mta, Response,
};
use crate::transport::{UdpTransport, TransportError};
use std::sync::Arc;
use tokio::sync::RwLock;

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

/// XCP Master Client
pub struct XcpClient {
    transport: Option<Arc<UdpTransport>>,
    state: Arc<RwLock<ClientState>>,
}

impl XcpClient {
    /// Create a new XCP client
    pub fn new() -> Self {
        Self {
            transport: None,
            state: Arc::new(RwLock::new(ClientState::default())),
        }
    }

    /// Connect to XCP slave
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

        // Send CONNECT command
        let response = transport.send_command(&commands::connect()).await?;
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

        // Reset slave counter tracking on new connection
        transport.reset_slave_counter().await;

        self.transport = Some(transport);

        // Try to get slave ID
        let _ = self.get_id(0).await; // Ignore errors for optional command

        Ok(connect_info)
    }

    /// Disconnect from XCP slave
    pub async fn disconnect(&mut self) -> Result<(), XcpError> {
        let transport = self.transport.as_ref().ok_or(XcpError::NotConnected)?;

        // Send DISCONNECT command
        let response = transport.send_command(&commands::disconnect()).await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        match parsed {
            Response::Res(_) => {}
            Response::Err(err) => return Err(XcpError::Protocol(err)),
            _ => return Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        }

        // Update state
        {
            let mut state = self.state.write().await;
            state.connected = false;
            state.connect_info = None;
            state.slave_id = None;
        }

        self.transport = None;

        Ok(())
    }

    /// Get identification from slave
    pub async fn get_id(&self, id_type: u8) -> Result<String, XcpError> {
        let transport = self.transport.as_ref().ok_or(XcpError::NotConnected)?;

        let response = transport.send_command(&commands::get_id(id_type)).await?;
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
        let transport = self.transport.as_ref().ok_or(XcpError::NotConnected)?;

        let response = transport.send_command(&commands::short_upload(mta, length)).await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        match parsed {
            Response::Res(data) => Ok(data),
            Response::Err(err) => Err(XcpError::Protocol(err)),
            _ => Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Write memory to slave using SHORT_DOWNLOAD
    pub async fn write_memory(&self, mta: Mta, data: &[u8]) -> Result<(), XcpError> {
        let transport = self.transport.as_ref().ok_or(XcpError::NotConnected)?;

        let response = transport.send_command(&commands::short_download(mta, data)).await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        match parsed {
            Response::Res(_) => Ok(()),
            Response::Err(err) => Err(XcpError::Protocol(err)),
            _ => Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Set MTA for subsequent UPLOAD/DOWNLOAD operations
    pub async fn set_mta(&self, mta: Mta) -> Result<(), XcpError> {
        let transport = self.transport.as_ref().ok_or(XcpError::NotConnected)?;

        let response = transport.send_command(&commands::set_mta(mta)).await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        match parsed {
            Response::Res(_) => Ok(()),
            Response::Err(err) => Err(XcpError::Protocol(err)),
            _ => Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Upload (read) data using current MTA
    pub async fn upload(&self, length: u8) -> Result<Vec<u8>, XcpError> {
        let transport = self.transport.as_ref().ok_or(XcpError::NotConnected)?;

        let response = transport.send_command(&commands::upload(length)).await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        match parsed {
            Response::Res(data) => Ok(data),
            Response::Err(err) => Err(XcpError::Protocol(err)),
            _ => Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Download (write) data to current MTA
    pub async fn download(&self, data: &[u8]) -> Result<(), XcpError> {
        let transport = self.transport.as_ref().ok_or(XcpError::NotConnected)?;

        let response = transport.send_command(&commands::download(data)).await?;
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

    /// Free DAQ resources
    pub async fn free_daq(&self) -> Result<(), XcpError> {
        let transport = self.transport.as_ref().ok_or(XcpError::NotConnected)?;

        let response = transport.send_command(&commands::free_daq()).await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        match parsed {
            Response::Res(_) => Ok(()),
            Response::Err(err) => Err(XcpError::Protocol(err)),
            _ => Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Allocate DAQ lists
    pub async fn alloc_daq(&self, daq_count: u16) -> Result<(), XcpError> {
        let transport = self.transport.as_ref().ok_or(XcpError::NotConnected)?;

        let response = transport.send_command(&commands::alloc_daq(daq_count)).await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        match parsed {
            Response::Res(_) => Ok(()),
            Response::Err(err) => Err(XcpError::Protocol(err)),
            _ => Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Allocate ODTs
    pub async fn alloc_odt(&self, daq_list: u16, odt_count: u8) -> Result<(), XcpError> {
        let transport = self.transport.as_ref().ok_or(XcpError::NotConnected)?;

        let response = transport.send_command(&commands::alloc_odt(daq_list, odt_count)).await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        match parsed {
            Response::Res(_) => Ok(()),
            Response::Err(err) => Err(XcpError::Protocol(err)),
            _ => Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Allocate ODT entries
    pub async fn alloc_odt_entry(&self, daq_list: u16, odt: u8, entry_count: u8) -> Result<(), XcpError> {
        let transport = self.transport.as_ref().ok_or(XcpError::NotConnected)?;

        let response = transport.send_command(&commands::alloc_odt_entry(daq_list, odt, entry_count)).await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        match parsed {
            Response::Res(_) => Ok(()),
            Response::Err(err) => Err(XcpError::Protocol(err)),
            _ => Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Set DAQ pointer
    pub async fn set_daq_ptr(&self, daq_list: u16, odt: u8, entry: u8) -> Result<(), XcpError> {
        let transport = self.transport.as_ref().ok_or(XcpError::NotConnected)?;

        let response = transport.send_command(&commands::set_daq_ptr(daq_list, odt, entry)).await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        match parsed {
            Response::Res(_) => Ok(()),
            Response::Err(err) => Err(XcpError::Protocol(err)),
            _ => Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Write DAQ entry configuration
    pub async fn write_daq(&self, bit_offset: u8, mta: Mta, data_size: u8) -> Result<(), XcpError> {
        let transport = self.transport.as_ref().ok_or(XcpError::NotConnected)?;

        let response = transport.send_command(&commands::write_daq(bit_offset, mta, data_size)).await?;
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
        let transport = self.transport.as_ref().ok_or(XcpError::NotConnected)?;

        let response = transport.send_command(&commands::set_daq_list_mode(
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
    /// mode: 0 = stop, 1 = start, 2 = start selected
    pub async fn start_stop_daq_list(&self, mode: u8, daq_list: u16) -> Result<(), XcpError> {
        let transport = self.transport.as_ref().ok_or(XcpError::NotConnected)?;

        let response = transport.send_command(&commands::start_stop_daq_list(mode, daq_list)).await?;
        let parsed = parse_response(&response).map_err(XcpError::ParseError)?;

        match parsed {
            Response::Res(_) => Ok(()),
            Response::Err(err) => Err(XcpError::Protocol(err)),
            _ => Err(XcpError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Get transport for DAQ reception
    pub fn transport(&self) -> Option<Arc<UdpTransport>> {
        self.transport.clone()
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
