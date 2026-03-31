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

    #[error("Calibration write error: {0}")]
    CalibrationWrite(#[from] CalibrationWriteError),
}

/// Calibration write error types
/// These errors indicate why a calibration write operation failed
#[derive(Debug, thiserror::Error)]
pub enum CalibrationWriteError {
    #[error("Address 0x{0:08X} (ext {1}) is not within any defined memory segment")]
    AddressNotInSegment(u32, u8),

    #[error("Segment {0} has no pages that allow XCP write access")]
    SegmentNoWriteAllowed(u8),

    #[error("Current page {page} in segment {segment} does not allow XCP write access. Please switch to a page with write permission.")]
    CurrentPageNoWrite { segment: u8, page: u8 },

    #[error("Page state not initialized. Call update_segment_page_state() first.")]
    PageStateNotInitialized,

    #[error("Type mismatch: {0}")]
    TypeMismatch(String),

    #[error("Write failed: {0}")]
    WriteFailed(String),
}

/// Page mode for user-friendly page switching
///
/// Users typically don't care about segment numbers - they just want to
/// either calibrate (working mode) or view reference data (reference mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageMode {
    /// Working page - allows calibration (write access)
    /// Switches all segments to a page that allows XCP write access
    Working,
    /// Reference page - read-only mode
    /// Switches all segments to a page without write access
    Reference,
}

impl PageMode {
    /// Get a human-readable description
    pub fn description(&self) -> &'static str {
        match self {
            PageMode::Working => "Working (calibration enabled)",
            PageMode::Reference => "Reference (read-only)",
        }
    }
}

/// Segment page state - stores the current active page for each segment
#[derive(Debug, Clone)]
pub struct SegmentPageState {
    /// Segment number
    pub segment: u8,
    /// Current active page for XCP access (0-255)
    pub xcp_page: u8,
    /// Current active page for ECU access (0-255)
    pub ecu_page: Option<u8>,
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

    /// Current page state for each segment (keyed by segment number)
    segment_pages: Vec<SegmentPageState>,
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
            segment_pages: Vec::new(),
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

        tracing::info!(
            "Connected: max_cto={}, max_dto={}, resource={:?}",
            connect_info.max_cto, connect_info.max_dto, connect_info.resource
        );

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

    /// Get the current calibration page for a specific segment
    ///
    /// # Arguments
    /// * `segment` - Logical segment number (0-255)
    /// * `mode` - Access mode: 0x01 for ECU access, 0x02 for XCP access
    ///
    /// # Returns
    /// The current active page number for the specified segment and access mode
    pub async fn get_cal_page(&self, segment: u8, mode: u8) -> Result<u8, SlaveError> {
        let response = self.send_command_and_wait(&commands::get_cal_page(mode, segment)).await?;
        let parsed = parse_response(&response).map_err(|e| SlaveError::ParseError(e.to_string()))?;

        let res_data = match parsed {
            Response::Res(data) => data,
            Response::Err(err) => return Err(SlaveError::Protocol(err)),
            _ => return Err(SlaveError::InvalidResponse("Unexpected response type".to_string())),
        };

        let page = super::protocol::parse_cal_page_response(&res_data)
            .map_err(|e| SlaveError::ParseError(e.to_string()))?;

        Ok(page)
    }

    /// Set the current calibration page for a specific segment
    ///
    /// This command switches the active calibration page for the specified segment.
    /// Use this to switch to a page with write permission when needed.
    ///
    /// # Arguments
    /// * `segment` - Logical segment number (0-255)
    /// * `page` - Page number to switch to (0-255)
    /// * `mode` - Access mode: 0x01 for ECU access, 0x02 for XCP access, 0x03 for both
    ///
    /// # Returns
    /// Ok(()) on success
    ///
    /// # Example
    /// ```ignore
    /// // Switch to page 1 for XCP access on segment 0
    /// slave.set_cal_page(0, 1, 0x02).await?;
    /// ```
    pub async fn set_cal_page(&mut self, segment: u8, page: u8, mode: u8) -> Result<(), SlaveError> {
        let response = self.send_command_and_wait(&commands::set_cal_page(mode, segment, page)).await?;
        let parsed = parse_response(&response).map_err(|e| SlaveError::ParseError(e.to_string()))?;

        match parsed {
            Response::Res(_) => {
                // Update local segment page state
                if let Some(state) = self.segment_pages.iter_mut().find(|s| s.segment == segment) {
                    if mode & 0x02 != 0 {
                        state.xcp_page = page;
                    }
                    if mode & 0x01 != 0 {
                        state.ecu_page = Some(page);
                    }
                }
                tracing::info!(
                    "Set segment {} page to {} (mode=0x{:02X})",
                    segment, page, mode
                );
                Ok(())
            }
            Response::Err(err) => Err(SlaveError::Protocol(err)),
            _ => Err(SlaveError::InvalidResponse("Unexpected response type".to_string())),
        }
    }

    /// Query and update the current page state for all segments
    ///
    /// This method queries the XCP access page for each segment defined in the A2L file.
    /// It should be called after loading A2L data to determine which pages are currently active.
    ///
    /// # Returns
    /// A reference to the updated segment page state vector
    pub async fn update_segment_page_state(&mut self) -> Result<&[SegmentPageState], SlaveError> {
        // Get the number of segments from A2L data
        let num_segments = self.cal_manager.get_memory_segment_count();

        if num_segments == 0 {
            tracing::debug!("No memory segments defined, skipping GET_CAL_PAGE");
            return Ok(&self.segment_pages);
        }

        self.segment_pages.clear();

        // Query XCP access page for each segment
        // Mode 0x02 = XCP access
        for segment in 0..num_segments {
            match self.get_cal_page(segment as u8, 0x02).await {
                Ok(xcp_page) => {
                    tracing::debug!(
                        "Segment {} current XCP page: {}",
                        segment, xcp_page
                    );
                    self.segment_pages.push(SegmentPageState {
                        segment: segment as u8,
                        xcp_page,
                        ecu_page: None, // Don't query ECU page by default
                    });
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to get cal page for segment {}: {:?}",
                        segment, e
                    );
                    // Still add the segment with default page 0
                    self.segment_pages.push(SegmentPageState {
                        segment: segment as u8,
                        xcp_page: 0,
                        ecu_page: None,
                    });
                }
            }
        }

        Ok(&self.segment_pages)
    }

    /// Get the current segment page state
    pub fn get_segment_pages(&self) -> &[SegmentPageState] {
        &self.segment_pages
    }

    /// Check if a specific segment allows write access on its current page
    ///
    /// This checks the A2L page definition for write access permission
    /// on the currently active page for XCP access.
    pub fn is_write_allowed_for_segment(&self, segment: u8) -> bool {
        // Find the current page state for this segment
        let current_page = self.segment_pages
            .iter()
            .find(|s| s.segment == segment)
            .map(|s| s.xcp_page);

        if let Some(page_num) = current_page {
            // Check the A2L definition for this page
            self.cal_manager.is_page_write_allowed(segment, page_num)
        } else {
            false
        }
    }

    /// Find a page that allows XCP write access for a segment
    ///
    /// Returns the first page number that allows XCP write access,
    /// or None if no such page exists.
    ///
    /// # Arguments
    /// * `segment` - Segment number to search
    ///
    /// # Returns
    /// - Some(page_number) if a write-allowed page is found
    /// - None if no pages allow write access
    pub fn find_write_allowed_page(&self, segment: u8) -> Option<u8> {
        self.cal_manager.find_write_allowed_page(segment)
    }

    /// Switch to a page that allows write access for a segment
    ///
    /// This is a convenience method that finds a write-allowed page
    /// and switches to it. Useful when `check_write_permission` returns
    /// `CurrentPageNoWrite` error.
    ///
    /// # Arguments
    /// * `segment` - Segment number to switch page for
    ///
    /// # Returns
    /// - Ok(new_page) if successfully switched to a write-allowed page
    /// - Err if no write-allowed page exists or switch failed
    pub async fn switch_to_write_page(&mut self, segment: u8) -> Result<u8, SlaveError> {
        // Find a page that allows write
        let new_page = self.find_write_allowed_page(segment)
            .ok_or_else(|| SlaveError::CalibrationWrite(
                CalibrationWriteError::SegmentNoWriteAllowed(segment)
            ))?;

        // Check if we're already on that page
        let current_page = self.segment_pages
            .iter()
            .find(|s| s.segment == segment)
            .map(|s| s.xcp_page);

        if current_page == Some(new_page) {
            tracing::debug!("Already on write-allowed page {} for segment {}", new_page, segment);
            return Ok(new_page);
        }

        // Switch to the write-allowed page
        self.set_cal_page(segment, new_page, 0x02).await?;

        tracing::info!(
            "Switched segment {} to write-allowed page {}",
            segment, new_page
        );

        Ok(new_page)
    }

    /// Find a read-only (reference) page for a segment
    ///
    /// Returns the first page number that does NOT allow XCP write access,
    /// or None if all pages allow write access.
    ///
    /// # Arguments
    /// * `segment` - Segment number to search
    ///
    /// # Returns
    /// - Some(page_number) if a read-only page is found
    /// - None if all pages allow write access
    pub fn find_read_only_page(&self, segment: u8) -> Option<u8> {
        self.cal_manager.find_read_only_page(segment)
    }

    /// Find the appropriate page for a given mode
    ///
    /// # Arguments
    /// * `segment` - Segment number
    /// * `mode` - Page mode (Working or Reference)
    ///
    /// # Returns
    /// - Some(page_number) if a matching page is found
    /// - None if no matching page exists for this segment
    pub fn find_page_for_mode(&self, segment: u8, mode: PageMode) -> Option<u8> {
        match mode {
            PageMode::Working => self.find_write_allowed_page(segment),
            PageMode::Reference => self.find_read_only_page(segment),
        }
    }

    /// Switch all segments to the specified page mode
    ///
    /// This is the user-friendly API for page switching. Users don't need to
    /// know about segment numbers - they just want to either calibrate (Working)
    /// or view reference data (Reference).
    ///
    /// This method will:
    /// 1. Iterate over all segments that have page information
    /// 2. For each segment, find the appropriate page for the requested mode
    /// 3. Switch to that page (if different from current page)
    ///
    /// # Arguments
    /// * `mode` - Page mode: Working (calibration enabled) or Reference (read-only)
    ///
    /// # Returns
    /// A vector of (segment, new_page) pairs for segments that were actually switched.
    /// Segments that were already on the correct page are not included.
    ///
    /// # Example
    /// ```ignore
    /// // Switch to working mode for calibration
    /// let switched = slave.switch_all_pages(PageMode::Working).await?;
    /// println!("Switched {} segments to working page", switched.len());
    ///
    /// // Switch back to reference (read-only) mode
    /// let switched = slave.switch_all_pages(PageMode::Reference).await?;
    /// ```
    pub async fn switch_all_pages(&mut self, mode: PageMode) -> Result<Vec<(u8, u8)>, SlaveError> {
        let mut switched = Vec::new();
        let num_segments = self.cal_manager.get_memory_segment_count();

        if num_segments == 0 {
            tracing::debug!("No memory segments defined, nothing to switch");
            return Ok(switched);
        }

        tracing::info!(
            "Switching all {} segments to {} mode",
            num_segments,
            mode.description()
        );

        for segment in 0..num_segments {
            let segment = segment as u8;

            // Find the appropriate page for this mode
            let target_page = match self.find_page_for_mode(segment, mode) {
                Some(page) => page,
                None => {
                    tracing::debug!(
                        "Segment {} has no {:?} page, skipping",
                        segment, mode
                    );
                    continue;
                }
            };

            // Check current page
            let current_page = self.segment_pages
                .iter()
                .find(|s| s.segment == segment)
                .map(|s| s.xcp_page);

            if current_page == Some(target_page) {
                tracing::debug!(
                    "Segment {} already on {} page {}",
                    segment, mode.description(), target_page
                );
                continue;
            }

            // Switch to the target page
            match self.set_cal_page(segment, target_page, 0x02).await {
                Ok(()) => {
                    tracing::info!(
                        "Segment {} switched to {} page {}",
                        segment, mode.description(), target_page
                    );
                    switched.push((segment, target_page));
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to switch segment {} to page {}: {:?}",
                        segment, target_page, e
                    );
                    // Continue with other segments even if one fails
                }
            }
        }

        tracing::info!(
            "Page switch complete: {} segments switched to {} mode",
            switched.len(),
            mode.description()
        );

        Ok(switched)
    }

    /// Get the current effective page mode
    ///
    /// Analyzes all segments and returns the overall mode:
    /// - PageMode::Working if at least one segment is on a write-allowed page
    /// - PageMode::Reference if all segments are on read-only pages
    ///
    /// This is useful for UI display to show the current calibration state.
    pub fn get_current_page_mode(&self) -> PageMode {
        let num_segments = self.cal_manager.get_memory_segment_count();

        if num_segments == 0 {
            // No segments defined, default to working for backwards compatibility
            return PageMode::Working;
        }

        for state in &self.segment_pages {
            if self.cal_manager.is_page_write_allowed(state.segment, state.xcp_page) {
                return PageMode::Working;
            }
        }

        PageMode::Reference
    }

    /// Check write permission for a calibration parameter
    ///
    /// This performs all the necessary checks before a calibration write:
    /// 1. Find the segment containing the parameter's address
    /// 2. Check if the segment has any write-allowed pages
    /// 3. Check if the current active page allows write
    ///
    /// # Returns
    /// - Ok(segment, page) if write is allowed
    /// - Err with specific error if write is not allowed
    pub fn check_write_permission(&self, address: u32, address_ext: u8) -> Result<(u8, u8), CalibrationWriteError> {
        // Step 1: Find the segment for this address
        let segment = self.cal_manager.find_segment_for_address(address, address_ext);

        match segment {
            None => {
                // Address not in any segment - check if we have segment info at all
                if self.cal_manager.get_memory_segment_count() == 0 {
                    // No segments defined in A2L - allow write for backwards compatibility
                    // Return a dummy segment/page
                    Ok((0, 0))
                } else {
                    // Segments are defined but address doesn't match any
                    Err(CalibrationWriteError::AddressNotInSegment(address, address_ext))
                }
            }
            Some(seg_num) => {
                // Step 2: Check if this segment has any write-allowed pages
                if !self.cal_manager.is_segment_calibration_allowed(seg_num) {
                    return Err(CalibrationWriteError::SegmentNoWriteAllowed(seg_num));
                }

                // Step 3: Check if page state is initialized
                if self.segment_pages.is_empty() {
                    return Err(CalibrationWriteError::PageStateNotInitialized);
                }

                // Find current page for this segment
                let current_page = self.segment_pages
                    .iter()
                    .find(|s| s.segment == seg_num)
                    .map(|s| s.xcp_page);

                match current_page {
                    None => Err(CalibrationWriteError::PageStateNotInitialized),
                    Some(page_num) => {
                        // Step 4: Check if current page allows write
                        if !self.cal_manager.is_page_write_allowed(seg_num, page_num) {
                            Err(CalibrationWriteError::CurrentPageNoWrite {
                                segment: seg_num,
                                page: page_num,
                            })
                        } else {
                            Ok((seg_num, page_num))
                        }
                    }
                }
            }
        }
    }

    /// Write a calibration parameter value with permission checks
    ///
    /// This method performs the following steps:
    /// 1. Check write permission for the parameter's address
    /// 2. If not allowed, return an appropriate error
    /// 3. If allowed, perform the write operation
    ///
    /// # Arguments
    /// * `param_name` - Name of the calibration parameter
    /// * `value` - Value to write
    ///
    /// # Returns
    /// Ok(()) on success, or an error describing why the write failed
    pub async fn write_calibration_value(
        &mut self,
        param_name: &str,
        value: &crate::calibration::CalibrationValue,
    ) -> Result<(), SlaveError> {
        // Get the parameter
        let param = self.cal_manager.get_param(param_name)
            .ok_or_else(|| SlaveError::A2lError(format!("Parameter '{}' not found", param_name)))?;

        let mta = param.mta();

        // Check write permission
        self.check_write_permission(mta.address, mta.extension)
            .map_err(SlaveError::CalibrationWrite)?;

        // Permission granted - perform the write
        self.write_value_direct(param, value).await
    }

    /// Write a calibration value directly (without permission checks)
    ///
    /// This is an internal method that performs the actual XCP write operation.
    /// External callers should use `write_calibration_value` which includes permission checks.
    async fn write_value_direct(
        &self,
        param: &crate::calibration::CalibrationParam,
        value: &crate::calibration::CalibrationValue,
    ) -> Result<(), SlaveError> {
        use crate::calibration::{CalibrationType, CalibrationValue};

        let mta = param.mta();

        // Validate type match and write
        match (&param.data_type, value) {
            (CalibrationType::U8, CalibrationValue::U8(v)) => {
                self.write_u8(mta, *v).await?;
            }
            (CalibrationType::U16, CalibrationValue::U16(v)) => {
                self.write_memory(mta, &v.to_le_bytes()).await?;
            }
            (CalibrationType::U32, CalibrationValue::U32(v)) => {
                self.write_u32(mta, *v).await?;
            }
            (CalibrationType::U64, CalibrationValue::U64(v)) => {
                self.write_memory(mta, &v.to_le_bytes()).await?;
            }
            (CalibrationType::I8, CalibrationValue::I8(v)) => {
                self.write_u8(mta, *v as u8).await?;
            }
            (CalibrationType::I16, CalibrationValue::I16(v)) => {
                self.write_memory(mta, &v.to_le_bytes()).await?;
            }
            (CalibrationType::I32, CalibrationValue::I32(v)) => {
                self.write_memory(mta, &v.to_le_bytes()).await?;
            }
            (CalibrationType::I64, CalibrationValue::I64(v)) => {
                self.write_memory(mta, &v.to_le_bytes()).await?;
            }
            (CalibrationType::F32, CalibrationValue::F32(v)) => {
                self.write_memory(mta, &v.to_le_bytes()).await?;
            }
            (CalibrationType::F64, CalibrationValue::F64(v)) => {
                self.write_memory(mta, &v.to_le_bytes()).await?;
            }
            (CalibrationType::Bool, CalibrationValue::Bool(v)) => {
                self.write_u8(mta, if *v { 1 } else { 0 }).await?;
            }
            // Array types
            (CalibrationType::U8Array(_), CalibrationValue::U8Array(v)) => {
                self.write_u8_array(mta, v).await?;
            }
            (CalibrationType::U16Array(_), CalibrationValue::U16Array(v)) => {
                let flat: Vec<u8> = v.iter().flat_map(|&x| x.to_le_bytes()).collect();
                self.write_memory(mta, &flat).await?;
            }
            (CalibrationType::U32Array(_), CalibrationValue::U32Array(v)) => {
                let flat: Vec<u8> = v.iter().flat_map(|&x| x.to_le_bytes()).collect();
                self.write_memory(mta, &flat).await?;
            }
            (CalibrationType::U64Array(_), CalibrationValue::U64Array(v)) => {
                let flat: Vec<u8> = v.iter().flat_map(|&x| x.to_le_bytes()).collect();
                self.write_memory(mta, &flat).await?;
            }
            (CalibrationType::I8Array(_), CalibrationValue::I8Array(v)) => {
                let flat: Vec<u8> = v.iter().map(|&x| x as u8).collect();
                self.write_u8_array(mta, &flat).await?;
            }
            (CalibrationType::I16Array(_), CalibrationValue::I16Array(v)) => {
                let flat: Vec<u8> = v.iter().flat_map(|&x| x.to_le_bytes()).collect();
                self.write_memory(mta, &flat).await?;
            }
            (CalibrationType::I32Array(_), CalibrationValue::I32Array(v)) => {
                let flat: Vec<u8> = v.iter().flat_map(|&x| x.to_le_bytes()).collect();
                self.write_memory(mta, &flat).await?;
            }
            (CalibrationType::I64Array(_), CalibrationValue::I64Array(v)) => {
                let flat: Vec<u8> = v.iter().flat_map(|&x| x.to_le_bytes()).collect();
                self.write_memory(mta, &flat).await?;
            }
            (CalibrationType::F32Array(_), CalibrationValue::F32Array(v)) => {
                let flat: Vec<u8> = v.iter().flat_map(|&x| x.to_le_bytes()).collect();
                self.write_memory(mta, &flat).await?;
            }
            (CalibrationType::F64Array(_), CalibrationValue::F64Array(v)) => {
                let flat: Vec<u8> = v.iter().flat_map(|&x| x.to_le_bytes()).collect();
                self.write_memory(mta, &flat).await?;
            }
            (CalibrationType::BoolArray(_), CalibrationValue::BoolArray(v)) => {
                let flat: Vec<u8> = v.iter().map(|&x| if x { 1 } else { 0 }).collect();
                self.write_u8_array(mta, &flat).await?;
            }
            // Matrix types
            (CalibrationType::U8Matrix(_, _), CalibrationValue::U8Matrix(mat)) => {
                let flat: Vec<u8> = mat.iter().flatten().copied().collect();
                self.write_u8_array(mta, &flat).await?;
            }
            (CalibrationType::U16Matrix(_, _), CalibrationValue::U16Matrix(mat)) => {
                let flat: Vec<u8> = mat.iter().flatten().flat_map(|&x| x.to_le_bytes()).collect();
                self.write_memory(mta, &flat).await?;
            }
            (CalibrationType::U32Matrix(_, _), CalibrationValue::U32Matrix(mat)) => {
                let flat: Vec<u8> = mat.iter().flatten().flat_map(|&x| x.to_le_bytes()).collect();
                self.write_memory(mta, &flat).await?;
            }
            (CalibrationType::U64Matrix(_, _), CalibrationValue::U64Matrix(mat)) => {
                let flat: Vec<u8> = mat.iter().flatten().flat_map(|&x| x.to_le_bytes()).collect();
                self.write_memory(mta, &flat).await?;
            }
            (CalibrationType::I8Matrix(_, _), CalibrationValue::I8Matrix(mat)) => {
                let flat: Vec<u8> = mat.iter().flatten().map(|&x| x as u8).collect();
                self.write_u8_array(mta, &flat).await?;
            }
            (CalibrationType::I16Matrix(_, _), CalibrationValue::I16Matrix(mat)) => {
                let flat: Vec<u8> = mat.iter().flatten().flat_map(|&x| x.to_le_bytes()).collect();
                self.write_memory(mta, &flat).await?;
            }
            (CalibrationType::I32Matrix(_, _), CalibrationValue::I32Matrix(mat)) => {
                let flat: Vec<u8> = mat.iter().flatten().flat_map(|&x| x.to_le_bytes()).collect();
                self.write_memory(mta, &flat).await?;
            }
            (CalibrationType::I64Matrix(_, _), CalibrationValue::I64Matrix(mat)) => {
                let flat: Vec<u8> = mat.iter().flatten().flat_map(|&x| x.to_le_bytes()).collect();
                self.write_memory(mta, &flat).await?;
            }
            (CalibrationType::F32Matrix(_, _), CalibrationValue::F32Matrix(mat)) => {
                let flat: Vec<u8> = mat.iter().flatten().flat_map(|&x| x.to_le_bytes()).collect();
                self.write_memory(mta, &flat).await?;
            }
            (CalibrationType::F64Matrix(_, _), CalibrationValue::F64Matrix(mat)) => {
                let flat: Vec<u8> = mat.iter().flatten().flat_map(|&x| x.to_le_bytes()).collect();
                self.write_memory(mta, &flat).await?;
            }
            (CalibrationType::BoolMatrix(_, _), CalibrationValue::BoolMatrix(mat)) => {
                let flat: Vec<u8> = mat.iter().flatten().map(|&x| if x { 1 } else { 0 }).collect();
                self.write_u8_array(mta, &flat).await?;
            }
            _ => {
                return Err(SlaveError::CalibrationWrite(
                    CalibrationWriteError::TypeMismatch(format!(
                        "Type mismatch for parameter {}",
                        param.name
                    ))
                ));
            }
        }

        Ok(())
    }

    /// Read memory from slave using SHORT_UPLOAD
    pub async fn read_memory(&self, mta: Mta, length: u8) -> Result<Vec<u8>, SlaveError> {
        // SHORT_UPLOAD response: [RES, data...]
        // Maximum data we can read is max_cto - 1
        let max_per_read = self.max_upload_size();

        if length as usize > max_per_read {
            tracing::warn!(
                "Requested read of {} bytes exceeds max CTO limit ({}), truncating",
                length, max_per_read
            );
        }

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

    /// Get maximum bytes that can be read in a single SHORT_UPLOAD or UPLOAD command
    /// SHORT_UPLOAD response: [RES (1 byte), data...]
    /// So max data = max_cto - 1
    pub fn max_upload_size(&self) -> usize {
        self.info.connect_info
            .as_ref()
            .map(|ci| (ci.max_cto as usize).saturating_sub(1))
            .unwrap_or(254) // Default to 254 if not connected
    }

    /// Get maximum bytes that can be written in a single SHORT_DOWNLOAD command
    /// SHORT_DOWNLOAD: [CMD, LEN, reserved, ext, addr(4), data...]
    /// So max data = max_cto - 8 (header overhead)
    pub fn max_download_size(&self) -> usize {
        self.info.connect_info
            .as_ref()
            .map(|ci| (ci.max_cto as usize).saturating_sub(8))
            .unwrap_or(247) // Default to 247 if not connected
    }

    /// Read a large amount of data from slave, using SET_MTA + UPLOAD as needed
    /// This handles reads larger than what SHORT_UPLOAD can handle in one command
    ///
    /// # Arguments
    /// * `mta` - Memory transfer address
    /// * `size` - Total bytes to read
    /// * `max_per_read_override` - Optional override for maximum bytes per read
    pub async fn read_large_data(&self, mta: Mta, size: usize, max_per_read_override: Option<usize>) -> Result<Vec<u8>, SlaveError> {
        // Use the override if provided, otherwise use the default from connect_info
        let max_per_read = max_per_read_override.unwrap_or_else(|| self.max_upload_size());

        if size <= max_per_read {
            // Small enough for single SHORT_UPLOAD
            return self.read_memory(mta, size as u8).await;
        }

        tracing::debug!(
            "Reading {} bytes in chunks (max per read: {})",
            size, max_per_read
        );

        // Use SET_MTA + UPLOAD for large reads
        self.set_mta(mta).await?;

        let mut result = Vec::with_capacity(size);
        let mut remaining = size;

        while remaining > 0 {
            let to_read = remaining.min(max_per_read).min(255) as u8;
            let data = self.upload(to_read).await?;
            result.extend_from_slice(&data);
            remaining -= to_read as usize;
        }

        Ok(result)
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
    /// Handles large reads by using SET_MTA + UPLOAD when needed
    pub async fn read_u8_array(&self, mta: Mta, count: usize) -> Result<Vec<u8>, SlaveError> {
        let max_per_read = self.max_upload_size();

        if count <= max_per_read {
            // Small enough for single SHORT_UPLOAD
            return self.read_memory(mta, count as u8).await;
        }

        // For large arrays, use SET_MTA + UPLOAD sequence
        self.read_large_data(mta, count, None).await
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
        // Should display full UUID string (36 chars: 32 hex + 4 dashes)
        // e.g., "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
        assert_eq!(display.len(), 36);
        // Verify it's a valid UUID format
        assert!(uuid::Uuid::parse_str(&display).is_ok());
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

    #[test]
    fn test_calibration_write_error_display() {
        // Test AddressNotInSegment error
        let err = CalibrationWriteError::AddressNotInSegment(0x12345678, 0x02);
        assert!(err.to_string().contains("0x12345678"));
        assert!(err.to_string().contains("not within any defined memory segment"));

        // Test SegmentNoWriteAllowed error
        let err = CalibrationWriteError::SegmentNoWriteAllowed(2);
        assert!(err.to_string().contains("Segment 2"));
        assert!(err.to_string().contains("no pages that allow XCP write access"));

        // Test CurrentPageNoWrite error
        let err = CalibrationWriteError::CurrentPageNoWrite { segment: 0, page: 0 };
        assert!(err.to_string().contains("page 0"));
        assert!(err.to_string().contains("segment 0"));
        assert!(err.to_string().contains("Please switch to a page with write permission"));

        // Test PageStateNotInitialized error
        let err = CalibrationWriteError::PageStateNotInitialized;
        assert!(err.to_string().contains("update_segment_page_state()"));
    }

    #[test]
    fn test_calibration_write_error_from_slave_error() {
        // Test that CalibrationWriteError can be converted to SlaveError
        let cal_err = CalibrationWriteError::SegmentNoWriteAllowed(1);
        let slave_err: SlaveError = cal_err.into();
        assert!(matches!(slave_err, SlaveError::CalibrationWrite(_)));
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

