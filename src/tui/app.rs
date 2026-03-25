//! Application state and logic

use xcp_client::protocol::ConnectInfo;
use xcp_client::{CalibrationManager, DaqManager, XcpClient};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tui_input::backend::crossterm::EventHandler as InputEventHandler;
use tui_input::Input;

/// Application mode
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AppMode {
    /// Main menu
    Menu,
    /// Connection dialog
    Connect,
    /// Parameter list
    ParamList,
    /// Parameter detail/edit
    ParamDetail,
    /// DAQ view
    DaqView,
    /// DAQ measurement selection
    DaqSelectMeasurements,
    /// Input dialog
    Input,
    /// A2L file selection
    SelectA2L,
}

/// Menu items
#[derive(Debug, Clone, Copy)]
pub enum MenuItem {
    Connect,
    Disconnect,
    ListParams,
    DaqView,
    DaqSelectMeas,
    LoadA2L,
    Exit,
}

impl MenuItem {
    fn items(connected: bool) -> Vec<(MenuItem, &'static str)> {
        if connected {
            vec![
                (MenuItem::Disconnect, "Disconnect"),
                (MenuItem::ListParams, "Calibration Parameters"),
                (MenuItem::DaqView, "DAQ Acquisition"),
                (MenuItem::DaqSelectMeas, "Select DAQ Measurements"),
                (MenuItem::LoadA2L, "Load A2L File"),
                (MenuItem::Exit, "Exit"),
            ]
        } else {
            vec![
                (MenuItem::Connect, "Connect to XCP Slave"),
                (MenuItem::DaqView, "DAQ Acquisition"),
                (MenuItem::DaqSelectMeas, "Select DAQ Measurements"),
                (MenuItem::LoadA2L, "Load A2L File"),
                (MenuItem::Exit, "Exit"),
            ]
        }
    }
}

/// DAQ data sample
#[derive(Debug, Clone, Default)]
pub struct DaqSample {
    /// Measurement values (name -> physical value)
    pub values: std::collections::HashMap<String, f64>,
}

impl DaqSample {
    /// Get a measurement value
    pub fn get(&self, name: &str) -> Option<f64> {
        self.values.get(name).copied()
    }
}

/// Application state
pub struct App {
    /// XCP client
    client: Arc<Mutex<XcpClient>>,
    /// Calibration manager
    cal_manager: CalibrationManager,
    /// DAQ manager
    daq_manager: Arc<Mutex<DaqManager>>,

    /// Current mode
    mode: AppMode,
    /// Previous mode (for input dialog)
    prev_mode: Option<AppMode>,

    /// Menu state
    menu_selected: usize,
    /// Parameter list state
    param_selected: usize,

    /// Connection state
    connected: bool,
    slave_addr: String,
    connect_info: Option<ConnectInfo>,

    /// Input state
    input: Input,
    input_prompt: String,
    input_action: InputAction,

    /// Status message
    status: Option<String>,
    /// Error message
    error: Option<String>,

    /// DAQ live data
    daq_samples: Vec<DaqSample>,
    daq_running: bool,

    /// DAQ measurement selection state
    daq_meas_selected: usize,

    /// Loading state
    loading: bool,
    loading_msg: String,

    /// A2L file path
    a2l_path: String,
}

/// Input dialog action
#[derive(Debug, Clone)]
pub enum InputAction {
    SlaveAddress,
    ParamValue(usize),
    A2LPath,
    None,
}

impl App {
    /// Create new application
    pub fn new(client: XcpClient, cal_manager: CalibrationManager, daq_manager: DaqManager, a2l_path: String) -> Self {
        Self {
            client: Arc::new(Mutex::new(client)),
            cal_manager,
            daq_manager: Arc::new(Mutex::new(daq_manager)),
            mode: AppMode::Menu,
            prev_mode: None,
            menu_selected: 0,
            param_selected: 0,
            connected: false,
            slave_addr: "127.0.0.1:5555".to_string(),
            connect_info: None,
            input: Input::default(),
            input_prompt: String::new(),
            input_action: InputAction::None,
            status: Some(format!("A2L: {}", a2l_path)),
            error: None,
            daq_samples: Vec::new(),
            daq_running: false,
            daq_meas_selected: 0,
            loading: false,
            loading_msg: String::new(),
            a2l_path,
        }
    }

    /// Set error message
    pub fn set_error(&mut self, msg: String) {
        self.error = Some(msg);
    }

    /// Handle key input
    /// Returns Ok(true) to continue, Ok(false) to exit
    pub async fn handle_key(&mut self, key: KeyEvent) -> Result<bool, Box<dyn std::error::Error>> {
        // Handle loading state - only allow Ctrl+C to exit
        if self.loading {
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                // Disconnect if connected before exit
                if self.connected {
                    let _ = self.disconnect().await;
                }
                return Ok(false);
            }
            return Ok(true);
        }

        // Clear error on any key (except Ctrl+C)
        if self.error.is_some() {
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                // Disconnect if connected before exit
                if self.connected {
                    let _ = self.disconnect().await;
                }
                return Ok(false);
            }
            self.error = None;
            return Ok(true);
        }

        // Handle Ctrl+C to exit at any time - disconnect first if connected
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if self.connected {
                // Try to disconnect gracefully
                let _ = self.disconnect().await;
            }
            return Ok(false);
        }

        // Handle mode-specific keys, catching any errors
        let result = match self.mode {
            AppMode::Menu => self.handle_menu_key(key).await,
            AppMode::Connect => self.handle_connect_key(key).await,
            AppMode::ParamList => self.handle_param_list_key(key).await,
            AppMode::ParamDetail => self.handle_param_detail_key(key).await,
            AppMode::DaqView => self.handle_daq_key(key).await,
            AppMode::DaqSelectMeasurements => self.handle_daq_select_key(key).await,
            AppMode::Input => self.handle_input_key(key).await,
            AppMode::SelectA2L => self.handle_select_a2l_key(key).await,
        };

        // Catch errors and display them instead of propagating
        match result {
            Ok(should_continue) => Ok(should_continue),
            Err(e) => {
                self.error = Some(format!("Error: {}", e));
                Ok(true)
            }
        }
    }

    async fn handle_menu_key(&mut self, key: KeyEvent) -> Result<bool, Box<dyn std::error::Error>> {
        let items = MenuItem::items(self.connected);

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.menu_selected > 0 {
                    self.menu_selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.menu_selected < items.len() - 1 {
                    self.menu_selected += 1;
                }
            }
            KeyCode::Enter => {
                if let Some((item, _)) = items.get(self.menu_selected) {
                    match item {
                        MenuItem::Connect => {
                            self.mode = AppMode::Connect;
                            self.input = Input::new(self.slave_addr.clone());
                            self.input_prompt = "Slave Address".to_string();
                            self.input_action = InputAction::SlaveAddress;
                        }
                        MenuItem::Disconnect => {
                            self.disconnect().await?;
                        }
                        MenuItem::ListParams => {
                            self.mode = AppMode::ParamList;
                        }
                        MenuItem::DaqView => {
                            self.mode = AppMode::DaqView;
                        }
                        MenuItem::DaqSelectMeas => {
                            self.mode = AppMode::DaqSelectMeasurements;
                            self.daq_meas_selected = 0;
                        }
                        MenuItem::LoadA2L => {
                            self.mode = AppMode::SelectA2L;
                            self.input = Input::new(self.a2l_path.clone());
                            self.input_prompt = "A2L File Path".to_string();
                            self.input_action = InputAction::A2LPath;
                        }
                        MenuItem::Exit => {
                            if self.connected {
                                self.disconnect().await?;
                            }
                            return Ok(false);
                        }
                    }
                }
            }
            KeyCode::Char('q') | KeyCode::Esc => {
                if self.connected {
                    self.disconnect().await?;
                }
                return Ok(false);
            }
            _ => {}
        }
        Ok(true)
    }

    async fn handle_connect_key(&mut self, key: KeyEvent) -> Result<bool, Box<dyn std::error::Error>> {
        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::Menu;
            }
            KeyCode::Enter => {
                let addr = self.input.value().to_string();
                if addr.is_empty() {
                    self.error = Some("Address cannot be empty".to_string());
                } else {
                    self.connect(addr).await?;
                }
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(false);
            }
            _ => {
                self.input.handle_event(&crossterm::event::Event::Key(key));
            }
        }
        Ok(true)
    }

    async fn handle_param_list_key(&mut self, key: KeyEvent) -> Result<bool, Box<dyn std::error::Error>> {
        let param_count = self.cal_manager.params().len();

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.param_selected > 0 {
                    self.param_selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.param_selected < param_count - 1 {
                    self.param_selected += 1;
                }
            }
            KeyCode::Enter => {
                self.mode = AppMode::ParamDetail;
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.mode = AppMode::Menu;
            }
            _ => {}
        }
        Ok(true)
    }

    async fn handle_param_detail_key(&mut self, key: KeyEvent) -> Result<bool, Box<dyn std::error::Error>> {
        match key.code {
            KeyCode::Char('r') => {
                self.read_param().await?;
            }
            KeyCode::Char('w') => {
                self.mode = AppMode::Input;
                self.prev_mode = Some(AppMode::ParamDetail);
                let param = &self.cal_manager.params()[self.param_selected];
                self.input = Input::default();
                self.input_prompt = format!("New value for {}", param.name);
                self.input_action = InputAction::ParamValue(self.param_selected);
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.mode = AppMode::ParamList;
            }
            _ => {}
        }
        Ok(true)
    }

    async fn handle_daq_key(&mut self, key: KeyEvent) -> Result<bool, Box<dyn std::error::Error>> {
        match key.code {
            KeyCode::Char('s') => {
                if self.daq_running {
                    self.stop_daq().await?;
                } else {
                    self.start_daq().await?;
                }
            }
            KeyCode::Char('c') => {
                self.daq_samples.clear();
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                if self.daq_running {
                    self.stop_daq().await?;
                }
                self.mode = AppMode::Menu;
            }
            _ => {}
        }
        Ok(true)
    }

    async fn handle_daq_select_key(&mut self, key: KeyEvent) -> Result<bool, Box<dyn std::error::Error>> {
        // Get the count of available measurements
        let available_count = {
            let daq_manager = self.daq_manager.lock().await;
            daq_manager.available_count()
        };

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.daq_meas_selected > 0 {
                    self.daq_meas_selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.daq_meas_selected < available_count.saturating_sub(1) {
                    self.daq_meas_selected += 1;
                }
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                // Toggle selection
                let mut daq_manager = self.daq_manager.lock().await;
                daq_manager.toggle_measurement(self.daq_meas_selected);
            }
            KeyCode::Char('a') => {
                // Select all
                let mut daq_manager = self.daq_manager.lock().await;
                daq_manager.select_all();
            }
            KeyCode::Char('d') => {
                // Deselect all
                let mut daq_manager = self.daq_manager.lock().await;
                daq_manager.deselect_all();
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.mode = AppMode::Menu;
            }
            _ => {}
        }
        Ok(true)
    }

    async fn handle_input_key(&mut self, key: KeyEvent) -> Result<bool, Box<dyn std::error::Error>> {
        match key.code {
            KeyCode::Esc => {
                self.mode = self.prev_mode.unwrap_or(AppMode::Menu);
                self.prev_mode = None;
                self.input = Input::default();
            }
            KeyCode::Enter => {
                let value = self.input.value().to_string();
                match &self.input_action {
                    InputAction::SlaveAddress => {
                        if value.is_empty() {
                            self.error = Some("Address cannot be empty".to_string());
                        } else {
                            self.connect(value).await?;
                            self.mode = AppMode::Menu;
                        }
                    }
                    InputAction::ParamValue(idx) => {
                        self.write_param(*idx, &value).await?;
                        self.mode = self.prev_mode.unwrap_or(AppMode::ParamList);
                    }
                    InputAction::A2LPath => {
                        self.load_a2l_file(&value).await?;
                        self.mode = AppMode::Menu;
                    }
                    InputAction::None => {}
                }
                self.prev_mode = None;
                self.input = Input::default();
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(false);
            }
            _ => {
                self.input.handle_event(&crossterm::event::Event::Key(key));
            }
        }
        Ok(true)
    }

    async fn handle_select_a2l_key(&mut self, key: KeyEvent) -> Result<bool, Box<dyn std::error::Error>> {
        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::Menu;
                self.input = Input::default();
            }
            KeyCode::Enter => {
                let path = self.input.value().to_string();
                if path.is_empty() {
                    self.error = Some("A2L file path cannot be empty".to_string());
                } else {
                    self.load_a2l_file(&path).await?;
                    self.mode = AppMode::Menu;
                }
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(false);
            }
            _ => {
                self.input.handle_event(&crossterm::event::Event::Key(key));
            }
        }
        Ok(true)
    }

    async fn load_a2l_file(&mut self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        use xcp_client::A2lReader;

        self.loading = true;
        self.loading_msg = format!("Loading A2L file: {}...", path);

        let path_clone = path.to_string();

        let result = tokio::task::spawn_blocking(move || {
            let reader = A2lReader::load(&path_clone)?;
            let measurements = reader.get_measurements();
            let characteristics = reader.get_characteristics();
            Ok::<_, String>((measurements, characteristics))
        })
        .await?;

        self.loading = false;

        match result {
            Ok((measurements, characteristics)) => {
                // Clear existing params and load from A2L
                self.cal_manager.clear_params();
                for ch in characteristics {
                    if let Some(param) = Self::convert_characteristic(ch) {
                        self.cal_manager.add_param(param);
                    }
                }

                // Clear existing measurements and load from A2L
                {
                    let mut daq_manager = self.daq_manager.lock().await;
                    daq_manager.clear_measurements();
                    for m in measurements {
                        if let Some(meas) = Self::convert_measurement(m) {
                            daq_manager.add_measurement(meas);
                        }
                    }
                }

                self.a2l_path = path.to_string();
                let meas_count = {
                    self.daq_manager.lock().await.selected_count()
                };
                self.status = Some(format!("Loaded A2L: {} chars, {} meas",
                    self.cal_manager.params().len(), meas_count));
            }
            Err(e) => {
                self.error = Some(format!("Failed to load A2L: {}", e));
            }
        }

        Ok(())
    }

    fn convert_characteristic(ch: xcp_client::A2lCharacteristic) -> Option<xcp_client::CalibrationParam> {
        use xcp_client::{CalibrationParam, CalibrationType};

        let data_type = match ch.data_type {
            // Scalar types
            xcp_client::A2lDataType::U8 => CalibrationType::U8,
            xcp_client::A2lDataType::U16 => CalibrationType::U16,
            xcp_client::A2lDataType::U32 => CalibrationType::U32,
            xcp_client::A2lDataType::U64 => CalibrationType::U64,
            xcp_client::A2lDataType::I8 => CalibrationType::I8,
            xcp_client::A2lDataType::I16 => CalibrationType::I16,
            xcp_client::A2lDataType::I32 => CalibrationType::I32,
            xcp_client::A2lDataType::I64 => CalibrationType::I64,
            xcp_client::A2lDataType::F32 => CalibrationType::F32,
            xcp_client::A2lDataType::F64 => CalibrationType::F64,
            xcp_client::A2lDataType::Bool => CalibrationType::Bool,
            // Array types
            xcp_client::A2lDataType::U8Array(n) => CalibrationType::U8Array(n),
            xcp_client::A2lDataType::U16Array(n) => CalibrationType::U16Array(n),
            xcp_client::A2lDataType::U32Array(n) => CalibrationType::U32Array(n),
            xcp_client::A2lDataType::U64Array(n) => CalibrationType::U64Array(n),
            xcp_client::A2lDataType::I8Array(n) => CalibrationType::I8Array(n),
            xcp_client::A2lDataType::I16Array(n) => CalibrationType::I16Array(n),
            xcp_client::A2lDataType::I32Array(n) => CalibrationType::I32Array(n),
            xcp_client::A2lDataType::I64Array(n) => CalibrationType::I64Array(n),
            xcp_client::A2lDataType::F32Array(n) => CalibrationType::F32Array(n),
            xcp_client::A2lDataType::F64Array(n) => CalibrationType::F64Array(n),
            xcp_client::A2lDataType::BoolArray(n) => CalibrationType::BoolArray(n),
            // Matrix types
            xcp_client::A2lDataType::U8Matrix(rows, cols) => CalibrationType::U8Matrix(rows, cols),
            xcp_client::A2lDataType::U16Matrix(rows, cols) => CalibrationType::U16Matrix(rows, cols),
            xcp_client::A2lDataType::U32Matrix(rows, cols) => CalibrationType::U32Matrix(rows, cols),
            xcp_client::A2lDataType::U64Matrix(rows, cols) => CalibrationType::U64Matrix(rows, cols),
            xcp_client::A2lDataType::I8Matrix(rows, cols) => CalibrationType::I8Matrix(rows, cols),
            xcp_client::A2lDataType::I16Matrix(rows, cols) => CalibrationType::I16Matrix(rows, cols),
            xcp_client::A2lDataType::I32Matrix(rows, cols) => CalibrationType::I32Matrix(rows, cols),
            xcp_client::A2lDataType::I64Matrix(rows, cols) => CalibrationType::I64Matrix(rows, cols),
            xcp_client::A2lDataType::F32Matrix(rows, cols) => CalibrationType::F32Matrix(rows, cols),
            xcp_client::A2lDataType::F64Matrix(rows, cols) => CalibrationType::F64Matrix(rows, cols),
            xcp_client::A2lDataType::BoolMatrix(rows, cols) => CalibrationType::BoolMatrix(rows, cols),
        };

        let mut param = CalibrationParam::new(&ch.name, ch.address, ch.address_ext, data_type)
            .with_description(&ch.description)
            .with_range(ch.min, ch.max);

        if let Some(unit) = ch.unit {
            param = param.with_unit(&unit);
        }

        Some(param)
    }

    fn convert_measurement(m: xcp_client::A2lMeasurement) -> Option<xcp_client::Measurement> {
        let size = m.data_type.size();
        Some(xcp_client::Measurement::new(&m.name, m.address, m.address_ext, size))
    }

    async fn connect(&mut self, addr: String) -> Result<(), Box<dyn std::error::Error>> {
        self.loading = true;
        self.loading_msg = format!("Connecting to {}...", addr);

        let client = self.client.clone();
        let addr_clone = addr.clone();

        let result = tokio::spawn(async move {
            let mut client = client.lock().await;
            client.connect(&addr_clone).await
        })
        .await??;

        self.loading = false;

        self.connected = true;
        self.slave_addr = addr;
        self.connect_info = Some(result);
        self.mode = AppMode::Menu;
        self.status = Some("Connected successfully".to_string());
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.loading = true;
        self.loading_msg = "Disconnecting...".to_string();

        let client = self.client.clone();
        tokio::spawn(async move {
            let mut client = client.lock().await;
            client.disconnect().await
        })
        .await??;

        self.loading = false;

        self.connected = false;
        self.connect_info = None;
        self.status = Some("Disconnected".to_string());
        Ok(())
    }

    async fn read_param(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let client = self.client.clone();
        let idx = self.param_selected;
        let params = self.cal_manager.params().to_vec();

        self.loading = true;
        self.loading_msg = "Reading parameter...".to_string();

        let result = tokio::spawn(async move {
            let client = client.lock().await;
            let param = &params[idx];
            CalibrationManager::new().read_value(&client, param).await
        })
        .await??;

        self.loading = false;
        self.status = Some(format!("Value: {}", result.format()));
        Ok(())
    }

    async fn write_param(&mut self, idx: usize, value: &str) -> Result<(), Box<dyn std::error::Error>> {
        let client = self.client.clone();
        let params = self.cal_manager.params().to_vec();
        let param = params[idx].clone();
        let value_str = value.to_string();

        // Parse value
        let cal_manager = CalibrationManager::new();
        let parsed = cal_manager.parse_value(&param, &value_str);
        match parsed {
            Ok(cal_value) => {
                self.loading = true;
                self.loading_msg = "Writing parameter...".to_string();

                tokio::spawn(async move {
                    let client = client.lock().await;
                    cal_manager.write_value(&client, &param, &cal_value).await
                })
                .await??;

                self.loading = false;
                self.status = Some("Write successful".to_string());
            }
            Err(e) => {
                self.error = Some(format!("Invalid value: {}", e));
            }
        }
        Ok(())
    }

    async fn start_daq(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let client = self.client.clone();
        let daq_manager = self.daq_manager.clone();
        let addr = self.slave_addr.clone();

        self.loading = true;
        self.loading_msg = "Starting DAQ...".to_string();

        // Update slave address
        {
            let mut daq = daq_manager.lock().await;
            daq.set_slave_addr(addr);
        }

        let client_guard = client.lock().await;
        let result = daq_manager.lock().await.start(&client_guard, 1000).await;
        drop(client_guard);

        self.loading = false;

        match result {
            Ok(()) => {
                self.daq_running = true;
                self.status = Some("DAQ started".to_string());
            }
            Err(e) => {
                self.error = Some(format!("DAQ start failed: {}", e));
            }
        }
        Ok(())
    }

    async fn stop_daq(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let daq_manager = self.daq_manager.clone();

        self.loading = true;
        self.loading_msg = "Stopping DAQ...".to_string();

        let result = {
            let client_guard = self.client.lock().await;
            daq_manager.lock().await.stop(&*client_guard).await
        };

        self.loading = false;

        match result {
            Ok(()) => {
                self.daq_running = false;
                self.status = Some("DAQ stopped".to_string());
            }
            Err(e) => {
                self.error = Some(format!("DAQ stop failed: {}", e));
            }
        }
        Ok(())
    }

    /// Called on tick
    pub async fn on_tick(&mut self) {
        // Update DAQ status
        self.daq_running = self.daq_manager.lock().await.is_running();

        // Get latest DAQ sample from manager (if DAQ is running)
        if self.daq_running && self.mode == AppMode::DaqView {
            let record = {
                let daq_manager = self.daq_manager.lock().await;
                daq_manager.get_latest_sample().await
            };
            if let Some(record) = record {
                let sample = DaqSample {
                    values: record.values,
                };
                self.daq_samples.push(sample);
                if self.daq_samples.len() > 50 {
                    self.daq_samples.remove(0);
                }
            }
        }
    }

    /// Render the application
    pub fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();

        // Create main layout
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(3)])
            .split(area);

        // Header
        self.render_header(frame, chunks[0]);

        // Main content
        match self.mode {
            AppMode::Menu => self.render_menu(frame, chunks[1]),
            AppMode::Connect => self.render_connect(frame, chunks[1]),
            AppMode::ParamList => self.render_param_list(frame, chunks[1]),
            AppMode::ParamDetail => self.render_param_detail(frame, chunks[1]),
            AppMode::DaqView => self.render_daq_view(frame, chunks[1]),
            AppMode::DaqSelectMeasurements => self.render_daq_select(frame, chunks[1]),
            AppMode::Input => self.render_input(frame, chunks[1]),
            AppMode::SelectA2L => self.render_select_a2l(frame, chunks[1]),
        }

        // Footer
        self.render_footer(frame, chunks[2]);

        // Overlay: Error
        if self.error.is_some() {
            self.render_error(frame, area);
        }

        // Overlay: Loading
        if self.loading {
            self.render_loading(frame, area);
        }
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let title = if self.connected {
            format!("XCP Client - Connected to {}", self.slave_addr)
        } else {
            "XCP Client - Not Connected".to_string()
        };

        let header = Paragraph::new(title)
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            );

        frame.render_widget(header, area);
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let text = if let Some(status) = &self.status {
            // Truncate status if too long to fit in footer
            let max_len = area.width.saturating_sub(4) as usize;
            if status.len() > max_len {
                format!(" {}...", &status[..max_len.saturating_sub(3)])
            } else {
                format!(" {}", status)
            }
        } else {
            " Press q to quit".to_string()
        };

        let footer = Paragraph::new(text)
            .style(Style::default().fg(Color::Gray))
            .wrap(ratatui::widgets::Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL));

        frame.render_widget(footer, area);
    }

    fn render_menu(&self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = MenuItem::items(self.connected)
            .iter()
            .enumerate()
            .map(|(i, (_, label))| {
                let style = if i == self.menu_selected {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(format!("  {}  ", label)).style(style)
            })
            .collect();

        let menu = List::new(items)
            .block(Block::default().title(" Menu ").borders(Borders::ALL));

        frame.render_widget(menu, area);
    }

    fn render_connect(&self, frame: &mut Frame, area: Rect) {
        let input_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(area);

        let prompt = Paragraph::new(self.input.value())
            .style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .title(format!(" {} ", self.input_prompt))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green)),
            );

        frame.render_widget(prompt, input_area[0]);

        // Help text
        let help = Paragraph::new(
            "Enter slave address (e.g., 127.0.0.1:5555)\nPress Enter to connect, Esc to cancel",
        )
        .style(Style::default().fg(Color::Gray));

        frame.render_widget(help, input_area[1]);
    }

    fn render_param_list(&self, frame: &mut Frame, area: Rect) {
        let params = self.cal_manager.params();

        let items: Vec<ListItem> = params
            .iter()
            .enumerate()
            .map(|(i, param)| {
                let style = if i == self.param_selected {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                let text = format!(
                    "  {:<28} {:<10} 0x{:08X}  ",
                    param.name,
                    param.data_type.type_name(),
                    param.address
                );
                ListItem::new(text).style(style)
            })
            .collect();

        let list = List::new(items).block(
            Block::default()
                .title(" Calibration Parameters (j/k: navigate, Enter: details, q: back) ")
                .borders(Borders::ALL),
        );

        frame.render_widget(list, area);
    }

    fn render_param_detail(&self, frame: &mut Frame, area: Rect) {
        let param = &self.cal_manager.params()[self.param_selected];

        let info = vec![
            Line::from(vec![
                Span::styled("Name: ", Style::default().fg(Color::Cyan)),
                Span::raw(&param.name),
            ]),
            Line::from(vec![
                Span::styled("Type: ", Style::default().fg(Color::Cyan)),
                Span::raw(param.data_type.type_name()),
            ]),
            Line::from(vec![
                Span::styled("Address: ", Style::default().fg(Color::Cyan)),
                Span::raw(format!("0x{:08X} (ext: {})", param.address, param.address_ext)),
            ]),
            Line::from(""),
        ];

        let mut lines = info;

        if !param.description.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("Description: ", Style::default().fg(Color::Cyan)),
                Span::raw(&param.description),
            ]));
        }

        if let Some(unit) = &param.unit {
            lines.push(Line::from(vec![
                Span::styled("Unit: ", Style::default().fg(Color::Cyan)),
                Span::raw(unit),
            ]));
        }

        if let (Some(min), Some(max)) = (param.min, param.max) {
            lines.push(Line::from(vec![
                Span::styled("Range: ", Style::default().fg(Color::Cyan)),
                Span::raw(format!("{} - {}", min, max)),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Commands: ", Style::default().fg(Color::Yellow)),
            Span::raw("r: read, w: write, q: back"),
        ]));

        let detail = Paragraph::new(lines)
            .block(Block::default().title(" Parameter Detail ").borders(Borders::ALL));

        frame.render_widget(detail, area);
    }

    fn render_daq_view(&self, frame: &mut Frame, area: Rect) {
        // Try to get measurement names - use blocking lock since this is a sync function
        let (selected_names, available_count, selected_count) = if let Ok(daq_manager) = self.daq_manager.try_lock() {
            let names: Vec<String> = daq_manager.selected_measurements()
                .iter()
                .map(|m| m.name.clone())
                .collect();
            (names, daq_manager.available_count(), daq_manager.selected_count())
        } else {
            (vec![], 0, 0)
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(6), Constraint::Min(0)])
            .split(area);

        // Status panel
        let mut status_lines = vec![
            Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::Cyan)),
                if self.daq_running {
                    Span::styled(
                        "RUNNING",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::styled("STOPPED", Style::default().fg(Color::Red))
                },
            ]),
            Line::from(vec![
                Span::styled("Selected: ", Style::default().fg(Color::Cyan)),
                Span::raw(format!("{}/{}", selected_count, available_count)),
            ]),
        ];

        // Show measurement names
        if !selected_names.is_empty() {
            let names_text = selected_names.join(", ");
            status_lines.push(Line::from(vec![
                Span::styled("Measurements: ", Style::default().fg(Color::Cyan)),
                Span::raw(names_text),
            ]));
        } else {
            status_lines.push(Line::from(vec![
                Span::styled("Measurements: ", Style::default().fg(Color::Cyan)),
                Span::styled("None selected! Press 'm' to select.", Style::default().fg(Color::Yellow)),
            ]));
        }

        let status = Paragraph::new(status_lines)
            .block(Block::default().title(" DAQ Status ").borders(Borders::ALL));

        frame.render_widget(status, chunks[0]);

        // Data panel
        let mut data_lines: Vec<Line> = vec![];

        // Show latest values for each selected measurement
        if let Some(last_sample) = self.daq_samples.last() {
            data_lines.push(Line::styled("Latest values:", Style::default().fg(Color::Gray)));
            for name in &selected_names {
                let value = last_sample.get(name)
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "N/A".to_string());
                data_lines.push(Line::from(vec![
                    Span::styled(format!("  {}: ", name), Style::default().fg(Color::Yellow)),
                    Span::raw(value),
                ]));
            }
        }

        // Show last few samples
        if !self.daq_samples.is_empty() {
            data_lines.push(Line::from(""));
            data_lines.push(Line::styled("Recent samples:", Style::default().fg(Color::Gray)));

            for (i, sample) in self.daq_samples.iter().rev().take(5).enumerate() {
                let mut sample_text = format!("  [{}] ", i + 1);
                for (j, name) in selected_names.iter().enumerate() {
                    if j > 0 {
                        sample_text.push_str(" | ");
                    }
                    let value = sample.get(name)
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".to_string());
                    sample_text.push_str(&format!("{}: {}", name, value));
                }
                data_lines.push(Line::from(sample_text));
            }
        }

        let data = Paragraph::new(data_lines).block(
            Block::default()
                .title(" Live Data (s: start/stop, c: clear, m: select measurements, q: back) ")
                .borders(Borders::ALL),
        );

        frame.render_widget(data, chunks[1]);
    }

    fn render_input(&self, frame: &mut Frame, area: Rect) {
        let input_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(area);

        let prompt = Paragraph::new(self.input.value())
            .style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .title(format!(" {} ", self.input_prompt))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green)),
            );

        frame.render_widget(prompt, input_area[0]);

        let help = Paragraph::new("Enter value and press Enter, Esc to cancel")
            .style(Style::default().fg(Color::Gray));

        frame.render_widget(help, input_area[1]);
    }

    fn render_select_a2l(&self, frame: &mut Frame, area: Rect) {
        let input_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(area);

        let prompt = Paragraph::new(self.input.value())
            .style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .title(" A2L File Path ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green)),
            );

        frame.render_widget(prompt, input_area[0]);

        let help = Paragraph::new(
            "Enter path to A2L file (e.g., hello_xcp.a2l)\nPress Enter to load, Esc to cancel"
        )
            .style(Style::default().fg(Color::Gray));

        frame.render_widget(help, input_area[1]);
    }

    fn render_daq_select(&self, frame: &mut Frame, area: Rect) {
        // Get measurements info - use try_lock since this is a sync function
        let (all_measurements, selected_indices) = if let Ok(daq_manager) = self.daq_manager.try_lock() {
            let all: Vec<(String, u32, u8)> = daq_manager.all_measurements()
                .iter()
                .map(|m| (m.name.clone(), m.address, m.address_ext))
                .collect();
            let selected: Vec<usize> = daq_manager.selected_indices().to_vec();
            (all, selected)
        } else {
            (vec![], vec![])
        };

        if all_measurements.is_empty() {
            let empty = Paragraph::new("No measurements available. Load an A2L file first.")
                .style(Style::default().fg(Color::Yellow))
                .block(Block::default().title(" Select DAQ Measurements ").borders(Borders::ALL));
            frame.render_widget(empty, area);
            return;
        }

        // Build list items
        let items: Vec<ListItem> = all_measurements
            .iter()
            .enumerate()
            .map(|(i, (name, addr, ext))| {
                let is_selected = selected_indices.contains(&i);
                let style = if i == self.daq_meas_selected {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else if is_selected {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::Gray)
                };

                let marker = if is_selected { "[x]" } else { "[ ]" };
                let text = format!("  {} {:<30} 0x{:08X} (ext: {})  ", marker, name, addr, ext);
                ListItem::new(text).style(style)
            })
            .collect();

        let selected_count = selected_indices.len();
        let title = format!(
            " Select DAQ Measurements (j/k: navigate, Space/Enter: toggle, a: all, d: none, q: back) - {}/{} selected ",
            selected_count,
            all_measurements.len()
        );

        let list = List::new(items)
            .block(Block::default().title(title).borders(Borders::ALL));

        frame.render_widget(list, area);
    }

    fn render_error(&self, frame: &mut Frame, area: Rect) {
        // Calculate dynamic height based on error message length (max 15 lines)
        let error_text = self.error.as_deref().unwrap_or("");
        let line_count = error_text.lines().count().max(3).min(15) as u16;

        let error_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(line_count + 2)])
            .split(area);

        let error = Paragraph::new(error_text)
            .style(
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            )
            .wrap(ratatui::widgets::Wrap { trim: true })
            .block(
                Block::default()
                    .title(" Error (press any key to dismiss) ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Red)),
            );

        frame.render_widget(Clear, error_area[1]);
        frame.render_widget(error, error_area[1]);
    }

    fn render_loading(&self, frame: &mut Frame, area: Rect) {
        let loading = Paragraph::new(format!("\n  {}  \n", self.loading_msg))
            .style(Style::default().fg(Color::Yellow))
            .block(
                Block::default()
                    .title(" Loading ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            );

        frame.render_widget(Clear, area);
        frame.render_widget(loading, area);
    }
}
