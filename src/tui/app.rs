//! Application state and logic

use xcp_client::calibration::CalibrationValue;
use xcp_client::protocol::ConnectInfo;
use xcp_client::{CalibrationManager, DaqManager, XcpClient};
use chrono::Local;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tui_input::backend::crossterm::EventHandler as InputEventHandler;
use tui_input::Input;

// Matrix theme colors
const MATRIX_GREEN: Color = Color::Green;
const MATRIX_BRIGHT_GREEN: Color = Color::Rgb(0, 255, 70);

/// Search context for filtering variables
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SearchContext {
    Calibration,
    Daq,
}

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
    /// Combined calibration and DAQ view
    CombinedView,
    /// Input dialog
    Input,
    /// A2L file selection
    SelectA2L,
    /// Search dialog for selecting variables
    SearchDialog,
    /// Event channel selection dialog
    EventSelect,
}

/// Menu items
#[derive(Debug, Clone, Copy)]
pub enum MenuItem {
    Connect,
    Disconnect,
    ListParams,
    CombinedView,
    LoadA2L,
    Exit,
}

impl MenuItem {
    fn items(connected: bool) -> Vec<(MenuItem, &'static str)> {
        if connected {
            vec![
                (MenuItem::Disconnect, "Disconnect"),
                (MenuItem::CombinedView, "Calibration + DAQ View"),
                (MenuItem::ListParams, "Calibration Parameters"),
                (MenuItem::LoadA2L, "Load A2L File"),
                (MenuItem::Exit, "Exit"),
            ]
        } else {
            vec![
                (MenuItem::Connect, "Connect to XCP Slave"),
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
    /// Combined view focus panel
    combined_focus: CombinedFocus,
    /// DAQ display mode (text or chart)
    daq_display_mode: DaqDisplayMode,

    /// Slave addresses from A2L file (ip:port)
    a2l_slave_addrs: Vec<String>,
    /// Currently selected slave address index
    slave_addr_selected: usize,

    /// Search pattern for filtering variables
    search_pattern: String,
    /// Filtered calibration parameter indices
    filtered_cal_indices: Vec<usize>,
    /// Filtered DAQ measurement indices
    filtered_daq_indices: Vec<usize>,
    /// Whether search is active
    is_searching: bool,
    /// Search context (Calibration or Daq)
    search_context: SearchContext,
    /// Cached calibration values (key: param name)
    cal_values: HashMap<String, CalibrationValue>,

    /// Selected calibration parameter indices for CombinedView display
    selected_cal_params: Vec<usize>,
    /// Selected DAQ measurement indices for CombinedView display
    selected_daq_meas: Vec<usize>,

    /// Available DAQ events for event selection
    daq_events: Vec<xcp_client::a2l::DaqEvent>,
    /// Cursor in event selection dialog
    event_select_cursor: usize,

    /// Search dialog focus (input box or list)
    search_dialog_focus: SearchDialogFocus,
    /// Temporarily selected indices in search dialog (before confirming)
    search_dialog_selected: Vec<usize>,
}

/// Search dialog focus area
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SearchDialogFocus {
    /// Input box is focused
    Input,
    /// List is focused
    List,
}

/// Combined view focus panel
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CombinedFocus {
    /// Calibration panel is focused
    Calibration,
    /// DAQ measurement selection panel is focused
    DaqSelection,
}

/// DAQ display mode
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DaqDisplayMode {
    /// Text view with live values
    Text,
    /// Chart view with historical data
    Chart,
}

/// Input dialog action
#[derive(Debug, Clone)]
pub enum InputAction {
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
            combined_focus: CombinedFocus::Calibration,
            daq_display_mode: DaqDisplayMode::Text,
            a2l_slave_addrs: Vec::new(),
            slave_addr_selected: 0,
            search_pattern: String::new(),
            filtered_cal_indices: Vec::new(),
            filtered_daq_indices: Vec::new(),
            is_searching: false,
            search_context: SearchContext::Calibration,
            cal_values: HashMap::new(),
            selected_cal_params: Vec::new(),
            selected_daq_meas: Vec::new(),
            daq_events: Vec::new(),
            event_select_cursor: 0,
            search_dialog_focus: SearchDialogFocus::Input,
            search_dialog_selected: Vec::new(),
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
            AppMode::CombinedView => self.handle_combined_key(key).await,
            AppMode::Input => self.handle_input_key(key).await,
            AppMode::SelectA2L => self.handle_select_a2l_key(key).await,
            AppMode::SearchDialog => self.handle_search_dialog_key(key).await,
            AppMode::EventSelect => self.handle_event_select_key(key).await,
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
                            self.slave_addr_selected = 0;
                        }
                        MenuItem::Disconnect => {
                            self.disconnect().await?;
                        }
                        MenuItem::ListParams => {
                            self.mode = AppMode::ParamList;
                        }
                        MenuItem::LoadA2L => {
                            self.mode = AppMode::SelectA2L;
                            self.input = Input::new(self.a2l_path.clone());
                            self.input_prompt = "A2L File Path".to_string();
                            self.input_action = InputAction::A2LPath;
                        }
                        MenuItem::CombinedView => {
                            self.mode = AppMode::CombinedView;
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
        let addr_count = self.get_available_addresses().len();

        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::Menu;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.slave_addr_selected > 0 {
                    self.slave_addr_selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.slave_addr_selected < addr_count.saturating_sub(1) {
                    self.slave_addr_selected += 1;
                }
            }
            KeyCode::Enter => {
                // Prevent double-connect if already connecting/connected
                if self.loading || self.connected {
                    return Ok(true);
                }
                let addrs = self.get_available_addresses();
                if let Some(addr) = addrs.get(self.slave_addr_selected) {
                    self.connect(addr.clone()).await?;
                }
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(false);
            }
            _ => {}
        }
        Ok(true)
    }

    /// Get list of available slave addresses (default + A2L addresses)
    fn get_available_addresses(&self) -> Vec<String> {
        let mut addrs = vec!["127.0.0.1:5555".to_string()];
        addrs.extend(self.a2l_slave_addrs.clone());
        addrs
    }

    /// Start search mode with given context
    fn start_search(&mut self, context: SearchContext) {
        self.search_context = context;
        self.input = Input::default();
        self.search_pattern.clear();
        // Clear filters to show all available variables initially
        self.filtered_cal_indices.clear();
        self.filtered_daq_indices.clear();
        // Reset search dialog state
        self.search_dialog_focus = SearchDialogFocus::Input;
        // Initialize with currently selected variables
        self.search_dialog_selected = match context {
            SearchContext::Calibration => self.selected_cal_params.clone(),
            SearchContext::Daq => self.selected_daq_meas.clone(),
        };
        // Reset selection to first item
        self.param_selected = 0;
        self.daq_meas_selected = 0;
        self.prev_mode = Some(AppMode::CombinedView);
        self.mode = AppMode::SearchDialog;
    }

    /// Apply current search pattern and update filtered indices
    async fn apply_search(&mut self) {
        let pattern = self.input.value();
        self.search_pattern = pattern.to_string();

        if self.search_pattern.is_empty() {
            // Clear filters
            self.filtered_cal_indices.clear();
            self.filtered_daq_indices.clear();
            return;
        }

        // Try to compile as regex first, fallback to literal match
        let regex = match regex::Regex::new(&format!("(?i){}", &self.search_pattern)) {
            Ok(re) => re,
            Err(_) => {
                // Invalid regex, escape and try again as literal
                match regex::Regex::new(&format!("(?i){}", regex::escape(&self.search_pattern))) {
                    Ok(re) => re,
                    Err(_) => return,
                }
            }
        };

        match self.search_context {
            SearchContext::Calibration => {
                self.filtered_cal_indices = self.cal_manager
                    .params()
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| regex.is_match(&p.name))
                    .map(|(i, _)| i)
                    .collect();
                // Reset selection to first match if any
                if !self.filtered_cal_indices.is_empty() {
                    self.param_selected = self.filtered_cal_indices[0];
                }
            }
            SearchContext::Daq => {
                let daq_manager = self.daq_manager.lock().await;
                self.filtered_daq_indices = daq_manager
                    .all_measurements()
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| regex.is_match(&m.name))
                    .map(|(i, _)| i)
                    .collect();
                // Reset selection to first match if any
                if !self.filtered_daq_indices.is_empty() {
                    self.daq_meas_selected = self.filtered_daq_indices[0];
                }
            }
        }
    }

    /// Clear search and show all items
    fn clear_search(&mut self) {
        self.is_searching = false;
        self.search_pattern.clear();
        self.filtered_cal_indices.clear();
        self.filtered_daq_indices.clear();
        self.input = Input::default();
    }

    /// Format calibration value for display (short format for list view)
    fn format_cal_value_short(&self, value: &CalibrationValue) -> String {
        match value {
            // Scalar types - display directly
            CalibrationValue::U8(v) => format!("{}", v),
            CalibrationValue::U16(v) => format!("{}", v),
            CalibrationValue::U32(v) => format!("{}", v),
            CalibrationValue::U64(v) => format!("{}", v),
            CalibrationValue::I8(v) => format!("{}", v),
            CalibrationValue::I16(v) => format!("{}", v),
            CalibrationValue::I32(v) => format!("{}", v),
            CalibrationValue::I64(v) => format!("{}", v),
            CalibrationValue::F32(v) => format!("{:.6}", v),
            CalibrationValue::F64(v) => format!("{:.6}", v),
            CalibrationValue::Bool(v) => format!("{}", v),

            // Array types - show first 3 values and length
            CalibrationValue::U8Array(arr) => {
                if arr.len() <= 3 {
                    format!("{:?}", arr)
                } else {
                    format!("[{}, {}, {}, ...] len={}", arr[0], arr[1], arr[2], arr.len())
                }
            }
            CalibrationValue::U16Array(arr) => {
                if arr.len() <= 3 {
                    format!("{:?}", arr)
                } else {
                    format!("[{}, {}, {}, ...] len={}", arr[0], arr[1], arr[2], arr.len())
                }
            }
            CalibrationValue::U32Array(arr) => {
                if arr.len() <= 3 {
                    format!("{:?}", arr)
                } else {
                    format!("[{}, {}, {}, ...] len={}", arr[0], arr[1], arr[2], arr.len())
                }
            }
            CalibrationValue::U64Array(arr) => {
                if arr.len() <= 3 {
                    format!("{:?}", arr)
                } else {
                    format!("[{}, {}, {}, ...] len={}", arr[0], arr[1], arr[2], arr.len())
                }
            }
            CalibrationValue::I8Array(arr) => {
                if arr.len() <= 3 {
                    format!("{:?}", arr)
                } else {
                    format!("[{}, {}, {}, ...] len={}", arr[0], arr[1], arr[2], arr.len())
                }
            }
            CalibrationValue::I16Array(arr) => {
                if arr.len() <= 3 {
                    format!("{:?}", arr)
                } else {
                    format!("[{}, {}, {}, ...] len={}", arr[0], arr[1], arr[2], arr.len())
                }
            }
            CalibrationValue::I32Array(arr) => {
                if arr.len() <= 3 {
                    format!("{:?}", arr)
                } else {
                    format!("[{}, {}, {}, ...] len={}", arr[0], arr[1], arr[2], arr.len())
                }
            }
            CalibrationValue::I64Array(arr) => {
                if arr.len() <= 3 {
                    format!("{:?}", arr)
                } else {
                    format!("[{}, {}, {}, ...] len={}", arr[0], arr[1], arr[2], arr.len())
                }
            }
            CalibrationValue::F32Array(arr) => {
                if arr.len() <= 3 {
                    format!("{:?}", arr)
                } else {
                    format!("[{:.2}, {:.2}, {:.2}, ...] len={}", arr[0], arr[1], arr[2], arr.len())
                }
            }
            CalibrationValue::F64Array(arr) => {
                if arr.len() <= 3 {
                    format!("{:?}", arr)
                } else {
                    format!("[{:.2}, {:.2}, {:.2}, ...] len={}", arr[0], arr[1], arr[2], arr.len())
                }
            }
            CalibrationValue::BoolArray(arr) => {
                if arr.len() <= 3 {
                    format!("{:?}", arr)
                } else {
                    format!("[{}, {}, {}, ...] len={}", arr[0], arr[1], arr[2], arr.len())
                }
            }

            // Matrix types - show dimensions and first few values
            CalibrationValue::U8Matrix(m) => {
                let rows = m.len();
                let cols = m.first().map(|r| r.len()).unwrap_or(0);
                format!("[{}x{} matrix]", rows, cols)
            }
            CalibrationValue::U16Matrix(m) => {
                let rows = m.len();
                let cols = m.first().map(|r| r.len()).unwrap_or(0);
                format!("[{}x{} matrix]", rows, cols)
            }
            CalibrationValue::U32Matrix(m) => {
                let rows = m.len();
                let cols = m.first().map(|r| r.len()).unwrap_or(0);
                format!("[{}x{} matrix]", rows, cols)
            }
            CalibrationValue::U64Matrix(m) => {
                let rows = m.len();
                let cols = m.first().map(|r| r.len()).unwrap_or(0);
                format!("[{}x{} matrix]", rows, cols)
            }
            CalibrationValue::I8Matrix(m) => {
                let rows = m.len();
                let cols = m.first().map(|r| r.len()).unwrap_or(0);
                format!("[{}x{} matrix]", rows, cols)
            }
            CalibrationValue::I16Matrix(m) => {
                let rows = m.len();
                let cols = m.first().map(|r| r.len()).unwrap_or(0);
                format!("[{}x{} matrix]", rows, cols)
            }
            CalibrationValue::I32Matrix(m) => {
                let rows = m.len();
                let cols = m.first().map(|r| r.len()).unwrap_or(0);
                format!("[{}x{} matrix]", rows, cols)
            }
            CalibrationValue::I64Matrix(m) => {
                let rows = m.len();
                let cols = m.first().map(|r| r.len()).unwrap_or(0);
                format!("[{}x{} matrix]", rows, cols)
            }
            CalibrationValue::F32Matrix(m) => {
                let rows = m.len();
                let cols = m.first().map(|r| r.len()).unwrap_or(0);
                format!("[{}x{} matrix]", rows, cols)
            }
            CalibrationValue::F64Matrix(m) => {
                let rows = m.len();
                let cols = m.first().map(|r| r.len()).unwrap_or(0);
                format!("[{}x{} matrix]", rows, cols)
            }
            CalibrationValue::BoolMatrix(m) => {
                let rows = m.len();
                let cols = m.first().map(|r| r.len()).unwrap_or(0);
                format!("[{}x{} matrix]", rows, cols)
            }
        }
    }

    /// Get filtered calibration parameter indices
    fn get_filtered_cal_indices(&self) -> Vec<usize> {
        if self.search_pattern.is_empty() || self.filtered_cal_indices.is_empty() {
            (0..self.cal_manager.params().len()).collect()
        } else {
            self.filtered_cal_indices.clone()
        }
    }

    /// Get filtered DAQ measurement indices
    async fn handle_param_list_key(&mut self, key: KeyEvent) -> Result<bool, Box<dyn std::error::Error>> {
        // Handle search input mode - all character input goes to search box
        if self.is_searching {
            match key.code {
                KeyCode::Esc => {
                    // ESC clears search and shows all items
                    self.clear_search();
                    return Ok(true);
                }
                KeyCode::Enter => {
                    // Enter exits search input mode but keeps filter results
                    self.is_searching = false;
                    return Ok(true);
                }
                KeyCode::Up | KeyCode::Down => {
                    // Arrow keys exit search input mode and navigate in filtered list
                    self.is_searching = false;
                    // Fall through to navigation logic
                }
                KeyCode::Char(_) | KeyCode::Backspace => {
                    // All character input (including j, k, s, r, w, q, etc.) goes to search box
                    self.input.handle_event(&crossterm::event::Event::Key(key));
                    self.apply_search().await;
                    return Ok(true);
                }
                _ => {
                    // Other keys exit search input mode
                    self.is_searching = false;
                }
            }
        }

        let indices = self.get_filtered_cal_indices();
        let param_count = indices.len();

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                // Find current position in filtered list
                if let Some(pos) = indices.iter().position(|&i| i == self.param_selected) {
                    if pos > 0 {
                        self.param_selected = indices[pos - 1];
                    }
                } else if !indices.is_empty() {
                    self.param_selected = indices[0];
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(pos) = indices.iter().position(|&i| i == self.param_selected) {
                    if pos < param_count.saturating_sub(1) {
                        self.param_selected = indices[pos + 1];
                    }
                } else if !indices.is_empty() {
                    self.param_selected = indices[0];
                }
            }
            KeyCode::Enter => {
                self.mode = AppMode::ParamDetail;
            }
            KeyCode::Char('/') => {
                self.start_search(SearchContext::Calibration);
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

    async fn handle_combined_key(&mut self, key: KeyEvent) -> Result<bool, Box<dyn std::error::Error>> {
        // Use selected variable lists for display
        let cal_indices = self.selected_cal_params.clone();
        let param_count = cal_indices.len();

        // Get selected DAQ measurement count
        let daq_indices = self.selected_daq_meas.clone();
        let daq_count = daq_indices.len();

        match key.code {
            // Tab to switch focus between panels
            KeyCode::Tab => {
                self.combined_focus = match self.combined_focus {
                    CombinedFocus::Calibration => CombinedFocus::DaqSelection,
                    CombinedFocus::DaqSelection => CombinedFocus::Calibration,
                };
            }
            // Navigation depends on which panel is focused
            KeyCode::Up | KeyCode::Char('k') => {
                match self.combined_focus {
                    CombinedFocus::Calibration => {
                        if let Some(pos) = cal_indices.iter().position(|&i| i == self.param_selected) {
                            if pos > 0 {
                                self.param_selected = cal_indices[pos - 1];
                            }
                        } else if !cal_indices.is_empty() {
                            self.param_selected = cal_indices[0];
                        }
                    }
                    CombinedFocus::DaqSelection => {
                        if let Some(pos) = daq_indices.iter().position(|&i| i == self.daq_meas_selected) {
                            if pos > 0 {
                                self.daq_meas_selected = daq_indices[pos - 1];
                            }
                        } else if !daq_indices.is_empty() {
                            self.daq_meas_selected = daq_indices[0];
                        }
                    }
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                match self.combined_focus {
                    CombinedFocus::Calibration => {
                        if let Some(pos) = cal_indices.iter().position(|&i| i == self.param_selected) {
                            if pos < param_count.saturating_sub(1) {
                                self.param_selected = cal_indices[pos + 1];
                            }
                        } else if !cal_indices.is_empty() {
                            self.param_selected = cal_indices[0];
                        }
                    }
                    CombinedFocus::DaqSelection => {
                        if let Some(pos) = daq_indices.iter().position(|&i| i == self.daq_meas_selected) {
                            if pos < daq_count.saturating_sub(1) {
                                self.daq_meas_selected = daq_indices[pos + 1];
                            }
                        } else if !daq_indices.is_empty() {
                            self.daq_meas_selected = daq_indices[0];
                        }
                    }
                }
            }
            // Space to toggle measurement selection when in DAQ panel
            KeyCode::Char(' ') => {
                if self.combined_focus == CombinedFocus::DaqSelection && daq_count > 0 {
                    let mut daq_manager = self.daq_manager.lock().await;
                    daq_manager.toggle_measurement(self.daq_meas_selected);
                }
            }
            // DAQ controls (work regardless of focus)
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
            // Toggle between text and chart view
            KeyCode::Char('t') => {
                self.daq_display_mode = match self.daq_display_mode {
                    DaqDisplayMode::Text => DaqDisplayMode::Chart,
                    DaqDisplayMode::Chart => DaqDisplayMode::Text,
                };
            }
            // Search
            KeyCode::Char('/') => {
                let context = match self.combined_focus {
                    CombinedFocus::Calibration => SearchContext::Calibration,
                    CombinedFocus::DaqSelection => SearchContext::Daq,
                };
                self.start_search(context);
            }
            // Calibration read/write (only when calibration panel is focused)
            KeyCode::Char('r') => {
                if self.combined_focus == CombinedFocus::Calibration && param_count > 0 {
                    self.read_param().await?;
                }
            }
            KeyCode::Char('w') => {
                if self.combined_focus == CombinedFocus::Calibration && param_count > 0 {
                    self.mode = AppMode::Input;
                    self.prev_mode = Some(AppMode::CombinedView);
                    let param = &self.cal_manager.params()[self.param_selected];
                    self.input = Input::default();
                    self.input_prompt = format!("New value for {}", param.name);
                    self.input_action = InputAction::ParamValue(self.param_selected);
                }
            }
            // Delete selected variable from current panel
            KeyCode::Char('d') => {
                match self.combined_focus {
                    CombinedFocus::Calibration => {
                        if let Some(pos) = cal_indices.iter().position(|&i| i == self.param_selected) {
                            self.selected_cal_params.remove(pos);
                            // Adjust selection
                            if self.selected_cal_params.is_empty() {
                                self.param_selected = 0;
                            } else if pos >= self.selected_cal_params.len() {
                                self.param_selected = self.selected_cal_params[self.selected_cal_params.len() - 1];
                            } else {
                                self.param_selected = self.selected_cal_params[pos];
                            }
                        }
                    }
                    CombinedFocus::DaqSelection => {
                        if let Some(pos) = daq_indices.iter().position(|&i| i == self.daq_meas_selected) {
                            // Also deselect from DAQ manager
                            {
                                let mut daq_manager = self.daq_manager.lock().await;
                                daq_manager.deselect_measurement(self.daq_meas_selected);
                            }
                            self.selected_daq_meas.remove(pos);
                            // Adjust selection
                            if self.selected_daq_meas.is_empty() {
                                self.daq_meas_selected = 0;
                            } else if pos >= self.selected_daq_meas.len() {
                                self.daq_meas_selected = self.selected_daq_meas[self.selected_daq_meas.len() - 1];
                            } else {
                                self.daq_meas_selected = self.selected_daq_meas[pos];
                            }
                        }
                    }
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                if self.daq_running {
                    self.stop_daq().await?;
                }
                self.mode = AppMode::Menu;
            }
            KeyCode::Char('e') => {
                let events = {
                    let daq_manager = self.daq_manager.lock().await;
                    daq_manager.get_available_events().to_vec()
                };
                if !events.is_empty() {
                    let current_channel = {
                        let daq_manager = self.daq_manager.lock().await;
                        daq_manager.get_event_channel()
                    };
                    self.event_select_cursor = events.iter()
                        .position(|ev| ev.channel == current_channel)
                        .unwrap_or(0);
                    self.daq_events = events;
                    self.mode = AppMode::EventSelect;
                } else {
                    self.status = Some("No events available (load A2L with EVENT blocks)".to_string());
                }
            }
            _ => {}
        }
        Ok(true)
    }

    async fn handle_event_select_key(&mut self, key: KeyEvent) -> Result<bool, Box<dyn std::error::Error>> {
        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::CombinedView;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.event_select_cursor > 0 {
                    self.event_select_cursor -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.event_select_cursor < self.daq_events.len().saturating_sub(1) {
                    self.event_select_cursor += 1;
                }
            }
            KeyCode::Enter => {
                if let Some(ev) = self.daq_events.get(self.event_select_cursor) {
                    let channel = ev.channel;
                    let label = ev.display_label();
                    let mut daq_manager = self.daq_manager.lock().await;
                    daq_manager.set_event_channel(channel);
                    drop(daq_manager);
                    self.status = Some(format!("Event channel set to: {}", label));
                }
                self.mode = AppMode::CombinedView;
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(false);
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

    async fn handle_search_dialog_key(&mut self, key: KeyEvent) -> Result<bool, Box<dyn std::error::Error>> {
        match key.code {
            KeyCode::Esc => {
                // Cancel and return to CombinedView without saving selections
                self.mode = AppMode::CombinedView;
                self.prev_mode = None;
                self.input = Input::default();
                self.search_pattern.clear();
                self.search_dialog_selected.clear();
            }
            KeyCode::Enter => {
                // Confirm all selections and return to CombinedView
                match self.search_context {
                    SearchContext::Calibration => {
                        self.selected_cal_params = self.search_dialog_selected.clone();
                    }
                    SearchContext::Daq => {
                        // Update DAQ manager selections
                        let mut daq_manager = self.daq_manager.lock().await;
                        // First collect selected indices, then deselect all
                        let to_deselect: Vec<usize> = daq_manager.selected_indices().to_vec();
                        for idx in to_deselect {
                            daq_manager.deselect_measurement(idx);
                        }
                        // Then select the new set
                        for &idx in &self.search_dialog_selected {
                            daq_manager.select_measurement(idx);
                        }
                        self.selected_daq_meas = self.search_dialog_selected.clone();
                    }
                }
                self.mode = AppMode::CombinedView;
                self.prev_mode = None;
                self.input = Input::default();
                self.search_pattern.clear();
                self.search_dialog_selected.clear();
            }
            KeyCode::Tab => {
                // Toggle focus between input and list
                self.search_dialog_focus = match self.search_dialog_focus {
                    SearchDialogFocus::Input => SearchDialogFocus::List,
                    SearchDialogFocus::List => SearchDialogFocus::Input,
                };
            }
            KeyCode::Char(' ') => {
                // Space to toggle selection (only when list is focused)
                if self.search_dialog_focus == SearchDialogFocus::List {
                    match self.search_context {
                        SearchContext::Calibration => {
                            let indices = if self.search_pattern.is_empty() {
                                (0..self.cal_manager.params().len()).collect::<Vec<_>>()
                            } else {
                                self.filtered_cal_indices.clone()
                            };
                            if let Some(&idx) = indices.get(self.param_selected) {
                                if let Some(pos) = self.search_dialog_selected.iter().position(|&i| i == idx) {
                                    self.search_dialog_selected.remove(pos);
                                } else {
                                    self.search_dialog_selected.push(idx);
                                }
                            }
                        }
                        SearchContext::Daq => {
                            let indices = if self.search_pattern.is_empty() {
                                let daq_manager = self.daq_manager.lock().await;
                                (0..daq_manager.available_count()).collect::<Vec<_>>()
                            } else {
                                self.filtered_daq_indices.clone()
                            };
                            if let Some(&idx) = indices.get(self.daq_meas_selected) {
                                if let Some(pos) = self.search_dialog_selected.iter().position(|&i| i == idx) {
                                    self.search_dialog_selected.remove(pos);
                                } else {
                                    self.search_dialog_selected.push(idx);
                                }
                            }
                        }
                    }
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.search_dialog_focus == SearchDialogFocus::List {
                    match self.search_context {
                        SearchContext::Calibration => {
                            if self.param_selected > 0 {
                                self.param_selected -= 1;
                            }
                        }
                        SearchContext::Daq => {
                            if self.daq_meas_selected > 0 {
                                self.daq_meas_selected -= 1;
                            }
                        }
                    }
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.search_dialog_focus == SearchDialogFocus::List {
                    match self.search_context {
                        SearchContext::Calibration => {
                            let max = if self.search_pattern.is_empty() {
                                self.cal_manager.params().len().saturating_sub(1)
                            } else {
                                self.filtered_cal_indices.len().saturating_sub(1)
                            };
                            if self.param_selected < max {
                                self.param_selected += 1;
                            }
                        }
                        SearchContext::Daq => {
                            let max = if self.search_pattern.is_empty() {
                                let daq_manager = self.daq_manager.lock().await;
                                daq_manager.available_count().saturating_sub(1)
                            } else {
                                self.filtered_daq_indices.len().saturating_sub(1)
                            };
                            if self.daq_meas_selected < max {
                                self.daq_meas_selected += 1;
                            }
                        }
                    }
                }
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(false);
            }
            KeyCode::Char(_) | KeyCode::Backspace => {
                // Handle input only when input box is focused
                if self.search_dialog_focus == SearchDialogFocus::Input {
                    self.input.handle_event(&crossterm::event::Event::Key(key));
                    self.apply_search().await;
                    // Reset selection when search changes
                    self.param_selected = 0;
                    self.daq_meas_selected = 0;
                }
            }
            _ => {}
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
            let slave_addrs = reader.get_slave_addresses();
            Ok::<_, String>((measurements, characteristics, slave_addrs))
        })
        .await?;

        self.loading = false;

        match result {
            Ok((measurements, characteristics, slave_addrs)) => {
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

                // Store slave addresses from A2L (excluding default 127.0.0.1:5555 if present)
                self.a2l_slave_addrs = slave_addrs.into_iter()
                    .filter(|addr| addr != "127.0.0.1:5555")
                    .collect();
                self.slave_addr_selected = 0;

                let meas_count = {
                    self.daq_manager.lock().await.selected_count()
                };
                let addr_info = if self.a2l_slave_addrs.is_empty() {
                    String::new()
                } else {
                    format!(", {} addr", self.a2l_slave_addrs.len())
                };
                self.status = Some(format!("Loaded A2L: {} chars, {} meas{}",
                    self.cal_manager.params().len(), meas_count, addr_info));
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
        // Use with_type to preserve full type information including data_type
        Some(xcp_client::Measurement::with_type(
            &m.name,
            m.address,
            m.address_ext,
            m.data_type,
            m.unit,
            xcp_client::CompuMethod::Identical,
        ))
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

        // Get parameter name for caching the value
        let param_name = params[idx].name.clone();

        self.loading = true;
        self.loading_msg = "Reading parameter...".to_string();

        let result = tokio::spawn(async move {
            let client = client.lock().await;
            let param = &params[idx];
            CalibrationManager::new().read_value(&client, param).await
        })
        .await??;

        self.loading = false;
        // Store the value in cache
        self.cal_values.insert(param_name.clone(), result.clone());
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
        // Get the current slave from client
        let xcp_slave = match client_guard.current_slave() {
            Some(s) => s,
            None => {
                self.loading = false;
                self.error = Some("No slave connected".to_string());
                return Ok(());
            }
        };
        let xcp_slave_guard = xcp_slave.lock().await;
        let result = daq_manager.lock().await.start(&*xcp_slave_guard, 1000).await;
        drop(xcp_slave_guard);
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
            // Get the current slave from client
            let xcp_slave = match client_guard.current_slave() {
                Some(s) => s,
                None => {
                    self.loading = false;
                    self.error = Some("No slave connected".to_string());
                    return Ok(());
                }
            };
            let xcp_slave_guard = xcp_slave.lock().await;
            daq_manager.lock().await.stop(&*xcp_slave_guard).await
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
        if self.daq_running {
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
        let area = frame.size();

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
            AppMode::CombinedView => self.render_combined_view(frame, chunks[1]),
            AppMode::Input => self.render_input(frame, chunks[1]),
            AppMode::SelectA2L => self.render_select_a2l(frame, chunks[1]),
            AppMode::SearchDialog => self.render_search_dialog(frame, chunks[1]),
            AppMode::EventSelect => {
                self.render_combined_view(frame, chunks[1]);
                self.render_event_select(frame, chunks[1]);
            }
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

    fn render_event_select(&self, frame: &mut Frame, area: Rect) {
        let width = 60.min(area.width.saturating_sub(4));
        let height = (self.daq_events.len() as u16 + 4).min(area.height.saturating_sub(4));
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        let dialog_area = Rect { x, y, width, height };

        frame.render_widget(Clear, dialog_area);

        let items: Vec<ListItem> = self.daq_events.iter().enumerate().map(|(i, ev)| {
            let label = ev.display_label();
            let style = if i == self.event_select_cursor {
                Style::default().fg(MATRIX_BRIGHT_GREEN).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(MATRIX_GREEN)
            };
            ListItem::new(label).style(style)
        }).collect();

        let list = List::new(items)
            .block(Block::default()
                .borders(Borders::ALL)
                .title(" Select Event Channel [↑↓ navigate, Enter select, Esc cancel] ")
                .border_style(Style::default().fg(MATRIX_GREEN)));

        frame.render_widget(list, dialog_area);
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let title = if self.connected {
            format!("XCP Client - Connected to {}", self.slave_addr)
        } else {
            "XCP Client - Not Connected".to_string()
        };

        let header = Paragraph::new(title)
            .style(Style::default().fg(MATRIX_BRIGHT_GREEN).add_modifier(Modifier::BOLD))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(MATRIX_GREEN)),
            );

        frame.render_widget(header, area);
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        // Get current local time
        let now = Local::now();
        let time_str = now.format("%Y-%m-%d %H:%M:%S").to_string();

        // Build status line with multiple info
        let mut lines = vec![];

        // Line 1: A2L file path and time
        let a2l_info = if self.a2l_path.is_empty() {
            "No A2L loaded".to_string()
        } else {
            format!("A2L: {}", self.a2l_path)
        };
        let status_text = format!("{} | {}", a2l_info, time_str);
        lines.push(Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled(status_text, Style::default().fg(MATRIX_BRIGHT_GREEN)),
        ]));

        // Line 2: Connection status and command hint
        let conn_status = if self.connected {
            format!("Connected: {} | ", self.slave_addr)
        } else {
            "Disconnected | ".to_string()
        };
        lines.push(Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled(conn_status, Style::default().fg(MATRIX_GREEN)),
            Span::styled("Press q to quit", Style::default().fg(Color::Gray)),
        ]));

        // Line 3: User message/status if available
        if let Some(status) = &self.status {
            lines.push(Line::from(vec![
                Span::styled(" ", Style::default()),
                Span::styled(status.clone(), Style::default().fg(Color::Yellow)),
            ]));
        }

        let footer = Paragraph::new(lines)
            .style(Style::default().fg(Color::Gray))
            .wrap(ratatui::widgets::Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(MATRIX_GREEN)),
            );

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
            .block(
                Block::default()
                    .title(" Menu ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(MATRIX_GREEN)),
            );

        frame.render_widget(menu, area);
    }

    fn render_connect(&self, frame: &mut Frame, area: Rect) {
        let addrs = self.get_available_addresses();

        let items: Vec<ListItem> = addrs
            .iter()
            .enumerate()
            .map(|(i, addr)| {
                let label = if i == 0 {
                    format!("  {} (default)  ", addr)
                } else {
                    format!("  {} (from A2L)  ", addr)
                };
                let style = if i == self.slave_addr_selected {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(label).style(style)
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(" Select Slave Address ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(MATRIX_GREEN)),
            );

        frame.render_widget(list, area);
    }

    fn render_param_list(&self, frame: &mut Frame, area: Rect) {
        // Split area for search box and list
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(if self.is_searching { 3 } else { 0 }),
                Constraint::Min(0),
            ])
            .split(area);

        // Render search box if active
        if self.is_searching {
            let search_box = Paragraph::new(self.input.value())
                .style(Style::default().fg(MATRIX_BRIGHT_GREEN))
                .block(
                    Block::default()
                        .title(" Search (regex) ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(MATRIX_BRIGHT_GREEN)),
                );
            frame.render_widget(search_box, chunks[0]);
        }

        let params = self.cal_manager.params();
        let indices = self.get_filtered_cal_indices();

        let items: Vec<ListItem> = indices
            .iter()
            .filter_map(|&i| params.get(i).map(|param| (i, param)))
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

        let title = if self.search_pattern.is_empty() {
            format!(
                " Calibration Parameters (j/k: navigate, /: search, Enter: details, q: back) - {} items ",
                params.len()
            )
        } else {
            format!(
                " Calibration Parameters (filtered: {}/{}, /: search, ESC: clear) ",
                indices.len(),
                params.len()
            )
        };

        let list = List::new(items).block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(MATRIX_GREEN)),
        );

        frame.render_widget(list, chunks[1]);
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
            .block(
                Block::default()
                    .title(" Parameter Detail ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(MATRIX_GREEN)),
            );

        frame.render_widget(detail, area);
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
                    .border_style(Style::default().fg(MATRIX_GREEN)),
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
                    .border_style(Style::default().fg(MATRIX_GREEN)),
            );

        frame.render_widget(prompt, input_area[0]);

        let help = Paragraph::new(
            "Enter path to A2L file (e.g., hello_xcp.a2l)\nPress Enter to load, Esc to cancel"
        )
            .style(Style::default().fg(Color::Gray));

        frame.render_widget(help, input_area[1]);
    }

    fn render_search_dialog(&self, frame: &mut Frame, area: Rect) {
        // Clear the background
        frame.render_widget(Clear, area);

        // Create a centered popup area
        let popup_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(5),
                Constraint::Percentage(90),
                Constraint::Percentage(5),
            ])
            .split(area);

        let popup_area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(5),
                Constraint::Percentage(90),
                Constraint::Percentage(5),
            ])
            .split(popup_area[1])[1];

        // Draw a block around the popup
        let block = Block::default()
            .title(match self.search_context {
                SearchContext::Calibration => " Search Calibration Parameters ",
                SearchContext::Daq => " Search DAQ Measurements ",
            })
            .borders(Borders::ALL)
            .border_style(Style::default().fg(MATRIX_BRIGHT_GREEN));

        frame.render_widget(block.clone(), popup_area);

        let inner_area = block.inner(popup_area);

        // Split into search input and results list
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(inner_area);

        // Search input box with focus indication
        let input_border_style = if self.search_dialog_focus == SearchDialogFocus::Input {
            Style::default().fg(MATRIX_BRIGHT_GREEN)
        } else {
            Style::default().fg(MATRIX_GREEN)
        };
        let input_title = if self.search_dialog_focus == SearchDialogFocus::Input {
            " Search (regex) [FOCUSED] "
        } else {
            " Search (regex) "
        };
        let search_input = Paragraph::new(self.input.value())
            .style(Style::default().fg(MATRIX_BRIGHT_GREEN))
            .block(
                Block::default()
                    .title(input_title)
                    .borders(Borders::ALL)
                    .border_style(input_border_style),
            );
        frame.render_widget(search_input, chunks[0]);

        // Results list with checkboxes
        let list_border_style = if self.search_dialog_focus == SearchDialogFocus::List {
            Style::default().fg(MATRIX_BRIGHT_GREEN)
        } else {
            Style::default().fg(MATRIX_GREEN)
        };

        let (items, selected_count, filtered_count, total_count) = match self.search_context {
            SearchContext::Calibration => {
                let params = self.cal_manager.params();
                let indices: Vec<usize> = if self.search_pattern.is_empty() {
                    (0..params.len()).collect()
                } else {
                    self.filtered_cal_indices.clone()
                };

                let items: Vec<ListItem> = indices
                    .iter()
                    .enumerate()
                    .filter_map(|(pos, &i)| params.get(i).map(|p| (pos, i, p)))
                    .map(|(pos, i, param)| {
                        let is_cursor = pos == self.param_selected;
                        let is_checked = self.search_dialog_selected.contains(&i);
                        let style = if is_cursor && self.search_dialog_focus == SearchDialogFocus::List {
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD)
                        } else if is_checked {
                            Style::default().fg(MATRIX_BRIGHT_GREEN)
                        } else {
                            Style::default()
                        };
                        let marker = if is_checked { "[x]" } else { "[ ]" };
                        let text = format!(
                            "  {} {:<28} {:<8} 0x{:06X}",
                            marker,
                            param.name.chars().take(28).collect::<String>(),
                            param.data_type.type_name(),
                            param.address
                        );
                        ListItem::new(text).style(style)
                    })
                    .collect();

                (items, self.search_dialog_selected.len(), indices.len(), params.len())
            }
            SearchContext::Daq => {
                if let Ok(daq_manager) = self.daq_manager.try_lock() {
                    let all = daq_manager.all_measurements();
                    let indices: Vec<usize> = if self.search_pattern.is_empty() {
                        (0..all.len()).collect()
                    } else {
                        self.filtered_daq_indices.clone()
                    };

                    let items: Vec<ListItem> = indices
                        .iter()
                        .enumerate()
                        .filter_map(|(pos, &i)| all.get(i).map(|m| (pos, i, m)))
                        .map(|(pos, i, m)| {
                            let is_cursor = pos == self.daq_meas_selected;
                            let is_checked = self.search_dialog_selected.contains(&i);
                            let style = if is_cursor && self.search_dialog_focus == SearchDialogFocus::List {
                                Style::default()
                                    .fg(Color::Yellow)
                                    .add_modifier(Modifier::BOLD)
                            } else if is_checked {
                                Style::default().fg(MATRIX_BRIGHT_GREEN)
                            } else {
                                Style::default()
                            };
                            let marker = if is_checked { "[x]" } else { "[ ]" };
                            let text = format!(
                                "  {} {:<28} 0x{:06X}",
                                marker,
                                m.name.chars().take(28).collect::<String>(),
                                m.address
                            );
                            ListItem::new(text).style(style)
                        })
                        .collect();

                    (items, self.search_dialog_selected.len(), indices.len(), all.len())
                } else {
                    (vec![], 0, 0, 0)
                }
            }
        };

        let list_title = if self.search_dialog_focus == SearchDialogFocus::List {
            format!(
                " Results [{}/{} filtered, {} selected] - Tab: switch, Space: toggle, Enter: confirm ",
                filtered_count, total_count, selected_count
            )
        } else {
            format!(
                " Results [{}/{} filtered, {} selected] - Tab: switch focus ",
                filtered_count, total_count, selected_count
            )
        };

        let list = List::new(items)
            .block(
                Block::default()
                    .title(list_title)
                    .borders(Borders::ALL)
                    .border_style(list_border_style),
            );

        frame.render_widget(list, chunks[1]);
    }

    fn render_combined_view(&self, frame: &mut Frame, area: Rect) {
        // Create horizontal layout: calibration on left, DAQ on right
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        // Left panel: Calibration parameters
        self.render_combined_calibration(frame, chunks[0]);

        // Right panel: DAQ data (status + live data + measurement selection)
        self.render_combined_daq(frame, chunks[1]);
    }

    fn render_combined_calibration(&self, frame: &mut Frame, area: Rect) {
        let params = self.cal_manager.params();
        let indices = &self.selected_cal_params;

        // Show placeholder if no variables selected
        let items: Vec<ListItem> = if indices.is_empty() {
            vec![ListItem::new(
                "  Press '/' to search and add calibration parameters"
            ).style(Style::default().fg(Color::Gray))]
        } else {
            indices
                .iter()
                .filter_map(|&i| params.get(i).map(|param| (i, param)))
                .map(|(i, param)| {
                    let style = if i == self.param_selected {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    // Get cached value if available
                    let value_str = self.cal_values.get(&param.name)
                        .map(|v| self.format_cal_value_short(v))
                        .unwrap_or_else(|| "-".to_string());
                    let text = format!(
                        "  {:<26} {:<8} 0x{:06X} {}",
                        param.name.chars().take(26).collect::<String>(),
                        param.data_type.type_name(),
                        param.address,
                        value_str
                    );
                    ListItem::new(text).style(style)
                })
                .collect()
        };

        let border_style = if self.combined_focus == CombinedFocus::Calibration {
            Style::default().fg(MATRIX_BRIGHT_GREEN)
        } else {
            Style::default().fg(MATRIX_GREEN)
        };

        let title = format!(
            " Calibration (Tab: switch, /: add, j/k: nav, r: read, w: write, d: del) - {} selected ",
            indices.len()
        );

        let list = List::new(items).block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(border_style),
        );

        frame.render_widget(list, area);
    }

    fn render_combined_daq(&self, frame: &mut Frame, area: Rect) {
        // Get measurement info
        let (selected_names, daq_running_count, available_count, all_measurements) =
            if let Ok(daq_manager) = self.daq_manager.try_lock() {
                let names: Vec<String> = daq_manager.selected_measurements()
                    .iter()
                    .map(|m| m.name.clone())
                    .collect();
                let all = daq_manager.all_measurements().to_vec();
                (names, daq_manager.selected_count(), daq_manager.available_count(), all)
            } else {
                (vec![], 0, 0, vec![])
            };

        // Create vertical layout: status (fixed), live data (40%), measurements (60%)
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),      // Status bar
                Constraint::Percentage(35), // Live data
                Constraint::Min(0),         // Measurement selection
            ])
            .split(area);

        // Status bar
        let view_mode_str = match self.daq_display_mode {
            DaqDisplayMode::Text => "TEXT",
            DaqDisplayMode::Chart => "CHART",
        };
        let status_text = format!(
            " Status: {} | Mode: {} | Acquiring: {}/{}",
            if self.daq_running { "RUNNING" } else { "STOPPED" },
            view_mode_str,
            daq_running_count,
            available_count
        );
        let status = Paragraph::new(status_text)
            .style(if self.daq_running {
                Style::default().fg(MATRIX_BRIGHT_GREEN).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Red)
            })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(MATRIX_GREEN)),
            );

        frame.render_widget(status, chunks[0]);

        // Render based on display mode
        match self.daq_display_mode {
            DaqDisplayMode::Text => {
                self.render_daq_text_view(frame, chunks[1], &selected_names, daq_running_count);
            }
            DaqDisplayMode::Chart => {
                self.render_daq_chart_view(frame, chunks[1], &selected_names);
            }
        }

        // Measurement selection panel - show only selected measurements
        let indices = &self.selected_daq_meas;
        let items: Vec<ListItem> = if indices.is_empty() {
            vec![ListItem::new(
                "  Press '/' to search and add measurements"
            ).style(Style::default().fg(Color::Gray))]
        } else {
            indices
                .iter()
                .filter_map(|&i| all_measurements.get(i).map(|m| (i, m)))
                .map(|(i, m)| {
                    let is_cursor = i == self.daq_meas_selected;
                    let style = if is_cursor && self.combined_focus == CombinedFocus::DaqSelection {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(MATRIX_BRIGHT_GREEN)
                    };

                    let text = format!(
                        "  [x] {:<24} 0x{:06X}",
                        m.name.chars().take(24).collect::<String>(),
                        m.address
                    );
                    ListItem::new(text).style(style)
                })
                .collect()
        };

        let border_style = if self.combined_focus == CombinedFocus::DaqSelection {
            Style::default().fg(MATRIX_BRIGHT_GREEN)
        } else {
            Style::default().fg(MATRIX_GREEN)
        };

        let title = format!(
            " Measurements (Tab: switch, /: add, j/k: nav, Space: toggle, d: del) - {} selected ",
            indices.len()
        );

        let list = List::new(items)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(border_style),
            );

        frame.render_widget(list, chunks[2]);
    }

    fn render_daq_text_view(&self, frame: &mut Frame, area: Rect, selected_names: &[String], selected_count: usize) {
        let mut data_lines: Vec<Line> = vec![];

        // Show latest values for each selected measurement
        if let Some(last_sample) = self.daq_samples.last() {
            for name in selected_names {
                let value = last_sample.get(name)
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "N/A".to_string());
                data_lines.push(Line::from(vec![
                    Span::styled(format!("{:<18}", name.chars().take(18).collect::<String>()), Style::default().fg(Color::Yellow)),
                    Span::raw(value),
                ]));
            }
        } else if selected_count > 0 {
            data_lines.push(Line::styled("Waiting for data...", Style::default().fg(Color::Gray)));
        } else {
            data_lines.push(Line::styled("No measurements selected", Style::default().fg(Color::Gray)));
        }

        // Show recent samples history (last 2)
        if !self.daq_samples.is_empty() {
            data_lines.push(Line::from(""));
            data_lines.push(Line::styled("Recent:", Style::default().fg(Color::Gray)));

            for (i, sample) in self.daq_samples.iter().rev().take(2).enumerate() {
                let mut values = vec![];
                for name in selected_names {
                    if let Some(v) = sample.get(name) {
                        values.push(format!("{}: {}", name.split('_').next_back().unwrap_or(name), v));
                    }
                }
                data_lines.push(Line::from(format!("  [{}] {}", i + 1, values.join(", "))));
            }
        }

        let data = Paragraph::new(data_lines).block(
            Block::default()
                .title(" Live Data (s: start/stop, c: clear, t: chart) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(MATRIX_GREEN)),
        );

        frame.render_widget(data, area);
    }

    fn render_daq_chart_view(&self, frame: &mut Frame, area: Rect, selected_names: &[String]) {

        if selected_names.is_empty() {
            let msg = Paragraph::new("No measurements selected")
                .style(Style::default().fg(Color::Gray))
                .block(
                    Block::default()
                        .title(" Chart View (s: start/stop, c: clear, t: text) ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(MATRIX_GREEN)),
                );
            frame.render_widget(msg, area);
            return;
        }

        if self.daq_samples.len() < 2 {
            let msg = Paragraph::new("Need at least 2 samples to display chart")
                .style(Style::default().fg(Color::Gray))
                .block(
                    Block::default()
                        .title(" Chart View (s: start/stop, c: clear, t: text) ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(MATRIX_GREEN)),
                );
            frame.render_widget(msg, area);
            return;
        }

        // Get chart dimensions
        let chart_width = area.width.saturating_sub(4) as usize;
        let chart_height = area.height.saturating_sub(4) as usize;

        if chart_width < 10 || chart_height < 5 {
            let msg = Paragraph::new("Window too small for chart")
                .style(Style::default().fg(Color::Yellow))
                .block(
                    Block::default()
                        .title(" Chart View ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(MATRIX_GREEN)),
                );
            frame.render_widget(msg, area);
            return;
        }

        // Build chart data for each measurement
        let mut chart_lines: Vec<Line> = vec![];
        let samples_to_show = self.daq_samples.len().min(chart_width);
        let start_idx = self.daq_samples.len().saturating_sub(samples_to_show);

        // Find min/max for scaling
        let mut all_values: Vec<f64> = vec![];
        for sample in &self.daq_samples[start_idx..] {
            for name in selected_names {
                if let Some(v) = sample.get(name) {
                    all_values.push(v);
                }
            }
        }

        if all_values.is_empty() {
            let msg = Paragraph::new("No numeric data available")
                .style(Style::default().fg(Color::Gray))
                .block(
                    Block::default()
                        .title(" Chart View (s: start/stop, c: clear, t: text) ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(MATRIX_GREEN)),
                );
            frame.render_widget(msg, area);
            return;
        }

        let min_val = all_values.iter().cloned().fold(f64::INFINITY, f64::min);
        let max_val = all_values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let range = (max_val - min_val).max(1e-10);

        // Color palette for different measurements (Matrix theme)
        let colors = [
            Color::Yellow,
            Color::Cyan,
            MATRIX_BRIGHT_GREEN,
            Color::Magenta,
            Color::Blue,
            Color::Red,
        ];

        // Create chart lines for each measurement
        let mut legend = vec![];
        for (m_idx, name) in selected_names.iter().take(6).enumerate() {
            let color = colors[m_idx % colors.len()];
            legend.push(Span::styled(
                format!("{} ", name.chars().take(12).collect::<String>()),
                Style::default().fg(color),
            ));
        }
        chart_lines.push(Line::from(vec![
            Span::styled("Legend: ", Style::default().fg(Color::Gray)),
            Span::raw(""),
        ]));
        chart_lines.push(Line::from(legend));
        chart_lines.push(Line::from(""));

        // Build ASCII chart
        // For simplicity, show max 3 measurements overlaid
        for (m_idx, name) in selected_names.iter().take(3).enumerate() {
            let color = colors[m_idx % colors.len()];
            let mut line_spans: Vec<Span> = vec![
                Span::styled(format!("{:<10} ", name.chars().take(10).collect::<String>()), Style::default().fg(color)),
            ];

            for sample in &self.daq_samples[start_idx..] {
                let ch = if let Some(v) = sample.get(name) {
                    // Scale to 0-9 for display
                    let scaled = ((v - min_val) / range * 9.0) as u8;
                    match scaled {
                        0 => '▁',
                        1 => '▂',
                        2 => '▃',
                        3 => '▄',
                        4 => '▅',
                        5 => '▆',
                        6 => '▇',
                        7 => '█',
                        8 => '█',
                        _ => ' ',
                    }
                } else {
                    ' '
                };
                line_spans.push(Span::raw(ch.to_string()));
            }
            chart_lines.push(Line::from(line_spans));
        }

        // Add value range info
        chart_lines.push(Line::from(""));
        chart_lines.push(Line::from(vec![
            Span::styled(format!("Min: {:.2} ", min_val), Style::default().fg(Color::Gray)),
            Span::styled(format!("Max: {:.2} ", max_val), Style::default().fg(Color::Gray)),
            Span::styled(format!("Samples: {}", samples_to_show), Style::default().fg(Color::Gray)),
        ]));

        let chart = Paragraph::new(chart_lines)
            .block(Block::default()
                .title(" Chart View (s: start/stop, c: clear, t: text) ")
                .borders(Borders::ALL));

        frame.render_widget(chart, area);
    }

    fn render_error(&self, frame: &mut Frame, area: Rect) {
        // Calculate dynamic height based on error message length (max 15 lines)
        let error_text = self.error.as_deref().unwrap_or("");
        let line_count = error_text.lines().count().clamp(3, 15) as u16;

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
