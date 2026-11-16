//! Non-interactive DAQ live print CLI subcommand (`daq-print`).
//!
//! Connects to one or more XCP slaves, starts DAQ and prints the latest value
//! of every selected measurement to **stdout** — no MF4/CSV files are written.
//! Two output modes are available:
//!
//! * `mode = "line"` (default) appends one line per event per print tick, so the
//!   output can be captured, `tee`'d or grepped:
//!
//! ```text
//! # xcp-client daq-print | 1 slave(s) | refresh 10ms | CTRL+C to stop
//! # [ecu1] pms-a.a2l via CAN PCAN_USBBUS1 0x7FB
//! # [ecu1] ev0 "Task_10ms" | 10ms | every 100 sample(s) (~1000ms) | 3 signal(s)
//! # [ecu1]   A, B, C
//! [12:00:01.234] ecu1 ev0  A=1  B=25.5  C=ON
//! ```
//!
//! * `mode = "single-line"` keeps a fixed block with one line per event and
//!   redraws it in place (ANSI cursor-up), so the terminal never scrolls. Only
//!   the latest values are visible — nothing accumulates. This mode requires a
//!   terminal and falls back to `line` when stdout is redirected.
//!
//! Each `[[events]]` entry may declare a `decimation`: the event's values are
//! printed once every N samples (a 10 ms event with `decimation = 100` prints
//! once per second). Values are physical values converted through the A2L
//! `COMPU_METHOD`; enumerations are printed as their text.
//!
//! Multiple slaves are supported through one `[[slaves]]` table each, with
//! independent A2L, transport and event configuration. A config with a single
//! top-level `a2l = "..."` (like `daq-record`) is accepted as a shorthand.
//!
//! Usage: `cargo run --release -- daq-print --config daq_print.config.toml`

use std::collections::{HashMap, HashSet};
use std::io::{IsTerminal, Write};
use std::time::{Duration, Instant};

use chrono::Local;
use serde::Deserialize;
use xcp_client::a2l::{A2lReader, CompuMethod};
use xcp_client::transport::transport_display;
use xcp_client::{DaqManager, Measurement, SlaveId, XcpClient};

pub use crate::cli_connection::ConnectionConfig;

/// Default interval for polling the live DAQ snapshots.
const DEFAULT_REFRESH_MS: u64 = 10;

// ============================ Config ============================

/// Top-level TOML structure of a `daq-print` config file.
#[derive(Debug, Deserialize)]
pub struct DaqPrintConfig {
    /// Global print settings
    #[serde(default)]
    pub print: PrintConfig,

    /// Single-slave shorthand: path to the A2L file.
    /// Mutually exclusive with `[[slaves]]`.
    pub a2l: Option<String>,
    /// Single-slave shorthand: connection settings.
    pub connection: Option<ConnectionConfig>,
    /// Single-slave shorthand: DAQ defaults.
    pub daq: Option<DaqConfig>,
    /// Single-slave shorthand: event groups.
    pub events: Option<Vec<EventConfig>>,

    /// One entry per XCP slave to acquire.
    #[serde(default)]
    pub slaves: Vec<SlaveConfig>,
}

/// How data lines are emitted.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PrintMode {
    /// Append one line per event per print tick (default). Capture friendly.
    #[default]
    Line,
    /// Keep one line per event and redraw the block in place. Only the latest
    /// values are visible; requires a terminal.
    #[serde(alias = "inline")]
    SingleLine,
}

impl PrintMode {
    /// Name used in the header and error messages.
    pub fn as_str(self) -> &'static str {
        match self {
            PrintMode::Line => "line",
            PrintMode::SingleLine => "single-line",
        }
    }
}

/// `[print]` — output settings shared by all slaves.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PrintConfig {
    /// Output mode: `"line"` (default, append) or `"single-line"` (redraw in place)
    pub mode: Option<PrintMode>,
    /// Decimation used by events that do not declare their own (default: 1)
    #[serde(alias = "sample_ratio")]
    pub decimation: Option<u32>,
    /// Prefix each data line with the wall-clock time (default: true)
    pub timestamp: Option<bool>,
    /// Print the startup header (default: true)
    pub header: Option<bool>,
    /// Snapshot polling interval in milliseconds (default: 10)
    pub refresh_ms: Option<u64>,
}

impl PrintConfig {
    /// Effective output mode.
    pub fn mode(&self) -> PrintMode {
        self.mode.unwrap_or_default()
    }
    /// Effective default decimation (at least 1).
    pub fn decimation(&self) -> u32 {
        self.decimation.unwrap_or(1).max(1)
    }

    /// Whether data lines get a `[HH:MM:SS.mmm]` prefix.
    pub fn timestamp(&self) -> bool {
        self.timestamp.unwrap_or(true)
    }

    /// Whether the startup header is printed.
    pub fn header(&self) -> bool {
        self.header.unwrap_or(true)
    }

    /// Snapshot polling interval.
    pub fn refresh(&self) -> Duration {
        Duration::from_millis(self.refresh_ms.unwrap_or(DEFAULT_REFRESH_MS).max(1))
    }
}

/// One `[[slaves]]` entry.
#[derive(Debug, Deserialize)]
pub struct SlaveConfig {
    /// Label used in the output lines. Defaults to the A2L file stem.
    pub name: Option<String>,
    /// Path to the A2L description file (required)
    pub a2l: String,
    pub connection: Option<ConnectionConfig>,
    pub daq: Option<DaqConfig>,
    /// Event groups to acquire (empty/omitted = all measurements)
    pub events: Option<Vec<EventConfig>>,
}

/// `[daq]` / `[slaves.daq]` — DAQ defaults.
#[derive(Debug, Deserialize)]
pub struct DaqConfig {
    /// Default event channel for measurements without an explicit assignment
    pub event_channel: Option<u16>,
}

/// One `[[events]]` / `[[slaves.events]]` entry.
#[derive(Debug, Deserialize)]
pub struct EventConfig {
    /// Event channel number (from the A2L `EVENT` definition)
    pub channel: u16,
    /// Measurements carried by this event
    #[serde(default)]
    pub measurements: Vec<String>,
    /// Print this event once every N samples (default: `[print].decimation`)
    #[serde(alias = "sample_ratio")]
    pub decimation: Option<u32>,
}

/// A slave config after shorthand expansion and name resolution.
#[derive(Debug)]
struct ResolvedSlave {
    name: String,
    a2l: String,
    connection: Option<ConnectionConfig>,
    daq: Option<DaqConfig>,
    events: Vec<EventConfig>,
}

/// Expand the single-slave shorthand and make slave names unique.
fn resolve_slaves(config: DaqPrintConfig) -> Result<(PrintConfig, Vec<ResolvedSlave>), String> {
    let DaqPrintConfig {
        print,
        a2l,
        connection,
        daq,
        events,
        slaves,
    } = config;

    if slaves.is_empty() {
        let a2l = a2l.ok_or(
            "No slave configured: add `a2l = \"description.a2l\"` or one or more \
             [[slaves]] sections to the config file",
        )?;
        let name = default_slave_name(&a2l, 0);
        return Ok((
            print,
            vec![ResolvedSlave {
                name,
                a2l,
                connection,
                daq,
                events: events.unwrap_or_default(),
            }],
        ));
    }

    if a2l.is_some() || connection.is_some() || daq.is_some() || events.is_some() {
        return Err(
            "Top-level `a2l`/`[connection]`/`[daq]`/`[[events]]` cannot be combined with \
             [[slaves]]; move those settings into the [[slaves]] entries"
                .to_string(),
        );
    }

    let mut resolved = Vec::with_capacity(slaves.len());
    let mut used: HashMap<String, usize> = HashMap::new();
    for (idx, slave) in slaves.into_iter().enumerate() {
        let base = slave
            .name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| default_slave_name(&slave.a2l, idx));
        let count = used.entry(base.clone()).or_insert(0);
        *count += 1;
        let name = if *count == 1 {
            base
        } else {
            format!("{}#{}", base, count)
        };
        resolved.push(ResolvedSlave {
            name,
            a2l: slave.a2l,
            connection: slave.connection,
            daq: slave.daq,
            events: slave.events.unwrap_or_default(),
        });
    }

    Ok((print, resolved))
}

/// Label for a slave without an explicit name: the A2L file stem.
fn default_slave_name(a2l: &str, idx: usize) -> String {
    std::path::Path::new(a2l)
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("slave{}", idx + 1))
}

// ============================ Printing ============================

/// Everything needed to render one signal of an event line.
#[derive(Debug, Clone)]
pub struct SignalFormat {
    pub name: String,
    /// Float data types are printed with up to 6 decimals.
    pub is_float: bool,
    /// Used for enum text (COMPU_VTAB).
    pub compu_method: CompuMethod,
}

impl SignalFormat {
    /// Render one physical value the way it is printed on stdout.
    pub fn format_value(&self, value: f64) -> String {
        format_value(value, self.is_float, &self.compu_method)
    }
}

/// Render a physical value: enum text when available, else a trimmed number.
pub fn format_value(value: f64, is_float: bool, compu_method: &CompuMethod) -> String {
    if !value.is_finite() {
        return format!("{}", value);
    }

    // Enumeration: print the text if the raw value maps to one.
    if (value - value.round()).abs() < 1e-9 {
        if let Some(text) = compu_method.get_enum_text(value.round() as i64) {
            return text.to_string();
        }
    }

    if is_float || (value - value.round()).abs() > 1e-9 {
        trim_float(format!("{:.6}", value))
    } else {
        format!("{:.0}", value)
    }
}

/// Drop insignificant trailing zeros (`1.500000` → `1.5`, `3.000000` → `3`).
fn trim_float(mut text: String) -> String {
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    text
}

/// Build one output line: `[ts] tag  NAME=value  NAME2=value2 ...`
///
/// Signals that have not been received yet are rendered as `NAME=-`.
pub fn format_line(
    timestamp: Option<&str>,
    tag: &str,
    signals: &[SignalFormat],
    values: &HashMap<String, f64>,
) -> String {
    let mut line = String::with_capacity(64 + signals.len() * 24);
    if let Some(ts) = timestamp {
        line.push('[');
        line.push_str(ts);
        line.push_str("] ");
    }
    line.push_str(tag);
    for signal in signals {
        line.push_str("  ");
        line.push_str(&signal.name);
        line.push('=');
        match values.get(&signal.name) {
            Some(value) => line.push_str(&signal.format_value(*value)),
            None => line.push('-'),
        }
    }
    line
}

/// Decide whether a snapshot should be printed now.
///
/// The first received sample is always printed; afterwards a snapshot is
/// printed once `decimation` new samples have accumulated since the last print.
pub fn should_print(last_samples: u64, samples: u64, decimation: u32) -> bool {
    if samples == 0 {
        return false;
    }
    let needed = if last_samples == 0 {
        1
    } else {
        u64::from(decimation.max(1))
    };
    samples.saturating_sub(last_samples) >= needed
}

/// Render a millisecond duration compactly (`10`, `0.5`, `1000`).
fn format_ms(ms: f64) -> String {
    trim_float(format!("{:.3}", ms))
}

// ======================= In-place (single-line) output =======================

/// Render an in-place block update.
///
/// Moves the cursor up `previous_lines` lines (the block drawn on the previous
/// tick), then rewrites every line after clearing it. If the new block is
/// shorter than the previous one, the extra lines are cleared as well. The
/// cursor ends up on the line right below the block.
///
/// `previous_lines == 0` draws the block for the first time (no cursor move).
pub fn render_inline_update(lines: &[String], previous_lines: usize) -> String {
    let mut out = String::new();
    if previous_lines > 0 {
        out.push_str(&format!("\x1b[{}A", previous_lines));
    }
    let total = previous_lines.max(lines.len());
    for index in 0..total {
        out.push('\r');
        out.push_str("\x1b[2K");
        if let Some(line) = lines.get(index) {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

/// Truncate a rendered line to `width` characters so it cannot wrap (a wrapped
/// line would break the cursor arithmetic of the in-place block).
pub fn truncate_line(line: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if line.chars().count() <= width {
        return line.to_string();
    }
    let mut truncated: String = line.chars().take(width - 1).collect();
    truncated.push('…');
    truncated
}

/// Maximum number of lines the in-place block may occupy (keep one spare row
/// so the block never pushes the header off screen).
fn inline_block_limit(rows: u16) -> usize {
    (rows as usize).saturating_sub(1).max(1)
}

/// Whether the terminal can interpret the ANSI escapes used by in-place
/// redraws. On Windows this also enables virtual-terminal processing for the
/// current console (legacy conhost reports `false`).
fn ansi_supported() -> bool {
    #[cfg(windows)]
    {
        crossterm::ansi_support::supports_ansi()
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// Cap the in-place block to `limit` lines, summarizing omitted events.
fn cap_block(lines: Vec<String>, limit: usize) -> Vec<String> {
    if lines.len() <= limit {
        return lines;
    }
    let keep = limit.saturating_sub(1);
    let omitted = lines.len() - keep;
    let mut capped: Vec<String> = lines[..keep].to_vec();
    capped.push(format!("... (+{} more event(s))", omitted));
    capped
}

// ============================ Runtime ============================

/// Per-event print state of one slave.
struct EventPrinter {
    /// DAQ list this printer reads from
    daq_list: u16,
    /// Short label used in data lines, e.g. `ev0`
    label: String,
    /// Event name from the A2L (for the header)
    event_name: Option<String>,
    /// Event cycle time in ms, if declared in the A2L
    cycle_ms: Option<f64>,
    /// Print once every N samples
    decimation: u32,
    /// Signals in DAQ allocation order
    signals: Vec<SignalFormat>,
    /// Sample counter at the last print (0 = nothing printed yet)
    last_samples: u64,
    /// Number of lines printed for this event
    printed: u64,
    /// Last rendered line (single-line mode keeps it for in-place redraws)
    last_line: Option<String>,
}

/// One connected slave with a running DAQ session.
struct SlaveSession {
    label: String,
    a2l: String,
    transport_label: String,
    client: XcpClient,
    slave_id: SlaveId,
    daq: DaqManager,
    printers: Vec<EventPrinter>,
}

/// Run the daq-print subcommand.
pub async fn run_daq_print(config_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config_text = std::fs::read_to_string(config_path)
        .map_err(|e| format!("Failed to read config file '{}': {}", config_path, e))?;
    let config: DaqPrintConfig = toml::from_str(&config_text)
        .map_err(|e| format!("Failed to parse config file '{}': {}", config_path, e))?;

    let (print, slave_configs) = resolve_slaves(config)?;
    if slave_configs.is_empty() {
        return Err("No slaves configured".into());
    }

    // Connect and start DAQ on every slave before printing anything, so the
    // header lists the real event/signal layout of all sessions.
    let mut sessions: Vec<SlaveSession> = Vec::with_capacity(slave_configs.len());
    for slave_config in slave_configs {
        eprintln!(
            "Connecting slave '{}' (A2L: {})...",
            slave_config.name, slave_config.a2l
        );
        match open_slave(slave_config, &print).await {
            Ok(session) => {
                eprintln!(
                    "  '{}' streaming {} event(s) via {}",
                    session.label,
                    session.printers.len(),
                    session.transport_label
                );
                sessions.push(session);
            }
            Err(e) => {
                // Roll back already started sessions before returning.
                shutdown(&mut sessions, "startup error").await;
                return Err(e.into());
            }
        }
    }

    // In-place redraws only make sense on a terminal with ANSI support; a
    // redirected stdout would receive raw escapes and lose all but the last
    // frame, and legacy consoles would print the escapes literally.
    let mode = match print.mode() {
        PrintMode::SingleLine if !std::io::stdout().is_terminal() => {
            eprintln!(
                "  warning: [print].mode = \"single-line\" needs a terminal; \
                 falling back to \"line\""
            );
            PrintMode::Line
        }
        PrintMode::SingleLine if !ansi_supported() => {
            eprintln!(
                "  warning: terminal does not support ANSI escape sequences; \
                 falling back to \"line\""
            );
            PrintMode::Line
        }
        mode => mode,
    };

    if print.header() {
        print_header(&sessions, &print, mode);
    }
    eprintln!("\nPrinting... Press CTRL+C to stop.\n");

    let started = Instant::now();
    let refresh = print.refresh();
    // Number of lines the in-place block occupied on the previous tick.
    let mut inline_previous = 0usize;

    // Quit channel. The extra sender clone is kept alive so that a non-tty
    // stdin (e.g. `daq-print > capture.txt`) never triggers an immediate quit.
    let (quit_tx, mut quit_rx) = tokio::sync::mpsc::channel::<()>(1);
    let _quit_keepalive = quit_tx.clone();
    if std::io::stdin().is_terminal() {
        // A plain OS thread, deliberately *not* `tokio::io::stdin()`: a pending
        // tokio stdin read lives in the blocking pool, and `Runtime::drop`
        // waits for blocking tasks indefinitely — the process would hang after
        // shutdown until another CTRL+C killed it. A std thread blocked on
        // stdin is simply terminated when the process exits.
        let quit_tx = quit_tx.clone();
        std::thread::Builder::new()
            .name("daq-print-stdin".to_string())
            .spawn(move || {
                let stdin = std::io::stdin();
                let mut line = String::new();
                loop {
                    line.clear();
                    match stdin.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) => {
                            if line.trim() == "q" {
                                let _ = quit_tx.blocking_send(());
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            })
            .ok();
    }

    // Keeps a CTRL+C listener armed for the whole run (including the graceful
    // shutdown) so a second press cannot kill the process mid-shutdown on
    // Windows — see `cli_signal` for details.
    let mut ctrl_c_rx = crate::cli_signal::ctrl_c_channel();

    let quit_reason = loop {
        tokio::select! {
            changed = ctrl_c_rx.changed() => {
                if changed.is_ok() {
                    break "CTRL+C";
                }
                // The listener task ended; do not treat that as a key press.
                continue;
            }
            _ = quit_rx.recv() => break "q",
            _ = tokio::time::sleep(refresh) => {}
        }

        let timestamp = if print.timestamp() {
            Some(Local::now().format("%H:%M:%S%.3f").to_string())
        } else {
            None
        };

        let mut block_changed = false;
        for session in sessions.iter_mut() {
            let snapshots = session.daq.live_snapshots().await;
            for printer in session.printers.iter_mut() {
                let Some(snapshot) = snapshots.get(&printer.daq_list) else {
                    continue;
                };
                if !should_print(printer.last_samples, snapshot.samples, printer.decimation) {
                    continue;
                }
                printer.last_samples = snapshot.samples;
                printer.printed += 1;

                let tag = format!("{} {}", session.label, printer.label);
                let line = format_line(
                    timestamp.as_deref(),
                    &tag,
                    &printer.signals,
                    &snapshot.values,
                );

                match mode {
                    PrintMode::Line => println!("{}", line),
                    PrintMode::SingleLine => {
                        printer.last_line = Some(line);
                        block_changed = true;
                    }
                }
            }
        }

        // Redraw the in-place block when at least one event line changed.
        // Events that are decimated keep showing their previous line.
        if mode == PrintMode::SingleLine && block_changed {
            let mut lines: Vec<String> = sessions
                .iter()
                .flat_map(|session| session.printers.iter())
                .filter_map(|printer| printer.last_line.clone())
                .collect();

            let (width, rows) = crossterm::terminal::size().unwrap_or((0, 0));
            if width > 0 {
                lines = lines
                    .into_iter()
                    .map(|line| truncate_line(&line, width as usize))
                    .collect();
            }
            let limit = if rows > 0 {
                inline_block_limit(rows)
            } else {
                lines.len()
            };
            let lines = cap_block(lines, limit);

            print!("{}", render_inline_update(&lines, inline_previous));
            std::io::stdout().flush()?;
            inline_previous = lines.len();
        }
    };

    eprintln!(
        "\n{} received. Shutting down (extra CTRL+C presses are ignored)...",
        quit_reason
    );
    shutdown(&mut sessions, quit_reason).await;

    let elapsed = started.elapsed().as_secs_f64();
    eprintln!("# captured {:.1}s", elapsed);
    for session in &sessions {
        for printer in &session.printers {
            eprintln!(
                "#   {} {}: {} line(s)",
                session.label, printer.label, printer.printed
            );
        }
    }

    Ok(())
}

/// Connect one slave, select measurements and start its DAQ session.
async fn open_slave(config: ResolvedSlave, print: &PrintConfig) -> Result<SlaveSession, String> {
    let reader = A2lReader::load(&config.a2l).map_err(|e| {
        format!(
            "[{}] failed to load A2L '{}': {}",
            config.name, config.a2l, e
        )
    })?;

    let transport = crate::cli_connection::resolve_connection_config(
        config.connection.as_ref(),
        &reader.get_transport_configs(),
    )
    .map_err(|e| format!("[{}] {}", config.name, e))?;

    // Build the DAQ manager ourselves (instead of `from_a2l`) so that A2L
    // COMPU_METHODs are applied: printed values are physical values and
    // enumerations show their text.
    let mut daq = DaqManager::new();
    for measurement in reader.get_measurements() {
        daq.add_measurement(Measurement::with_type(
            &measurement.name,
            measurement.address,
            measurement.address_ext,
            measurement.data_type,
            measurement.unit.clone(),
            reader.get_compu_method(&measurement.conversion_name),
        ));
    }

    // A2L events (used for validation, header labels and cycle times)
    let events = reader.get_events();
    let mut event_channels: Vec<u16> = events.iter().map(|e| e.channel).collect();
    event_channels.sort_unstable();
    event_channels.dedup();
    if !events.is_empty() {
        daq.set_available_events(events.clone());
    }

    let default_event_channel = config
        .daq
        .as_ref()
        .and_then(|d| d.event_channel)
        .unwrap_or(0);
    if !event_channels.is_empty() && !event_channels.contains(&default_event_channel) {
        return Err(format!(
            "[{}] default event_channel {} not found in A2L. Available: {:?}",
            config.name, default_event_channel, event_channels
        ));
    }

    // Select measurements per [[events]]
    daq.deselect_all();
    let mut decimation_per_channel: HashMap<u16, u32> = HashMap::new();
    if config.events.is_empty() {
        eprintln!(
            "  [{}] no [[events]] configured — selecting all {} measurements",
            config.name,
            daq.available_count()
        );
        daq.select_all();
    } else {
        let all_measurements = daq.all_measurements().to_vec();
        for event in &config.events {
            if !event_channels.is_empty() && !event_channels.contains(&event.channel) {
                return Err(format!(
                    "[{}] event channel {} not found in A2L. Available: {:?}",
                    config.name, event.channel, event_channels
                ));
            }
            if let Some(decimation) = event.decimation {
                decimation_per_channel.insert(event.channel, decimation.max(1));
            }
            if event.measurements.is_empty() {
                eprintln!(
                    "  [{}] event {} has no measurements — skipped",
                    config.name, event.channel
                );
                continue;
            }
            for name in &event.measurements {
                match all_measurements.iter().position(|m| &m.name == name) {
                    Some(idx) => {
                        daq.select_measurement(idx);
                        daq.set_measurement_event(idx, event.channel);
                    }
                    None => eprintln!(
                        "  [{}] measurement '{}' not found in A2L — skipped",
                        config.name, name
                    ),
                }
            }
        }
    }

    if daq.selected_count() == 0 {
        return Err(format!(
            "[{}] no measurements selected — check the [[events]] measurements",
            config.name
        ));
    }
    daq.set_event_channel(default_event_channel);
    daq.set_slave_addr(transport.addr_string());
    daq.set_slave_name(config.name.clone());
    daq.set_live_snapshots_enabled(true);

    // Connect
    let mut client = XcpClient::new();
    let slave_id = client
        .connect_slave(&config.name, transport.clone(), None)
        .await
        .map_err(|e| format!("[{}] connection failed: {}", config.name, e))?;

    // Start DAQ (configure + stream)
    let start_result = {
        let xcp_slave = client
            .get_slave(slave_id)
            .ok_or_else(|| format!("[{}] failed to get slave handle", config.name))?;
        let guard = xcp_slave.lock().await;
        daq.start(&guard, 0).await
    };
    if let Err(e) = start_result {
        let _ = client.disconnect_slave(slave_id).await;
        return Err(format!("[{}] DAQ start failed: {}", config.name, e));
    }

    // Derive one printer per DAQ list (an event channel may need several lists
    // for very large signal sets, so map through the DAQ configuration).
    let mut printers = Vec::new();
    {
        let daq_config = daq.get_config();
        let config_guard = daq_config.read().await;
        for (list_idx, &channel) in config_guard.event_channel_per_list.iter().enumerate() {
            let daq_list = list_idx as u16;
            let mut signals = Vec::new();
            let mut seen = HashSet::new();
            for entry in config_guard
                .odt_entries
                .iter()
                .filter(|e| e.daq_list == daq_list)
            {
                if seen.insert(entry.name.clone()) {
                    signals.push(SignalFormat {
                        name: entry.name.clone(),
                        is_float: entry.data_type.is_float(),
                        compu_method: entry.compu_method.clone(),
                    });
                }
            }

            let event = events.iter().find(|e| e.channel == channel);
            let decimation = decimation_per_channel
                .get(&channel)
                .copied()
                .unwrap_or_else(|| print.decimation());

            printers.push(EventPrinter {
                daq_list,
                label: format!("ev{}", channel),
                event_name: event.map(|e| e.name.clone()),
                cycle_ms: event.map(|e| e.cycle_time_ms()).filter(|c| *c > 0.0),
                decimation,
                signals,
                last_samples: 0,
                printed: 0,
                last_line: None,
            });
        }
    }

    if printers.is_empty() {
        let _ = client.disconnect_slave(slave_id).await;
        return Err(format!(
            "[{}] DAQ started but no event groups were configured",
            config.name
        ));
    }

    Ok(SlaveSession {
        label: config.name,
        a2l: config.a2l,
        transport_label: transport_display(&transport),
        client,
        slave_id,
        daq,
        printers,
    })
}

/// Build the startup header lines (`#`-prefixed so they are easy to filter).
fn header_lines(sessions: &[SlaveSession], print: &PrintConfig, mode: PrintMode) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!(
        "# xcp-client daq-print | {} slave(s) | refresh {}ms | mode {} | CTRL+C to stop",
        sessions.len(),
        print.refresh().as_millis(),
        mode.as_str()
    ));
    for session in sessions {
        lines.push(format!(
            "# [{}] {} via {}",
            session.label, session.a2l, session.transport_label
        ));
        for printer in &session.printers {
            let mut parts: Vec<String> = Vec::new();
            if let Some(name) = printer.event_name.as_deref().filter(|n| !n.is_empty()) {
                parts.push(format!("\"{}\"", name));
            }
            parts.push(match printer.cycle_ms {
                Some(cycle) => format!("{}ms", format_ms(cycle)),
                None => "cycle n/a".to_string(),
            });
            parts.push(match printer.cycle_ms {
                Some(cycle) => format!(
                    "every {} sample(s) (~{}ms)",
                    printer.decimation,
                    format_ms(cycle * f64::from(printer.decimation))
                ),
                None => format!("every {} sample(s)", printer.decimation),
            });
            parts.push(format!("{} signal(s)", printer.signals.len()));
            lines.push(format!(
                "# [{}] {} | {}",
                session.label,
                printer.label,
                parts.join(" | ")
            ));
            let names: Vec<&str> = printer
                .signals
                .iter()
                .map(|signal| signal.name.as_str())
                .collect();
            lines.push(format!("# [{}]   {}", session.label, names.join(", ")));
        }
    }
    lines
}

/// Print the startup header.
fn print_header(sessions: &[SlaveSession], print: &PrintConfig, mode: PrintMode) {
    for line in header_lines(sessions, print, mode) {
        println!("{}", line);
    }
}

/// Stop DAQ on every session and disconnect the slaves.
async fn shutdown(sessions: &mut [SlaveSession], reason: &str) {
    for session in sessions.iter_mut() {
        if let Some(xcp_slave) = session.client.get_slave(session.slave_id) {
            let guard = xcp_slave.lock().await;
            if session.daq.is_running() {
                eprintln!("  [{}] stopping DAQ...", session.label);
                if let Err(e) = session.daq.stop(&guard).await {
                    eprintln!("  [{}] DAQ stop failed: {}", session.label, e);
                }
            }
            eprintln!("  [{}] freeing DAQ resources...", session.label);
            if let Err(e) = guard.free_daq().await {
                eprintln!("  [{}] FREE_DAQ failed: {}", session.label, e);
            }
        }
        eprintln!("  [{}] disconnecting...", session.label);
        if let Err(e) = session.client.disconnect_slave(session.slave_id).await {
            eprintln!("  [{}] disconnect failed: {}", session.label, e);
        }
    }
    eprintln!("# stopped ({})", reason);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signal(name: &str, is_float: bool) -> SignalFormat {
        SignalFormat {
            name: name.to_string(),
            is_float,
            compu_method: CompuMethod::identical(),
        }
    }

    #[test]
    fn test_parse_multi_slave_config() {
        let toml_str = r#"
[print]
decimation = 10
timestamp = false
refresh_ms = 20

[[slaves]]
name = "inverter"
a2l = "pms-a.a2l"

[slaves.connection]
type = "can"
channel = "PCAN_USBBUS1"
can_id = 0x7fb
daq_ids = [0x7fd]

[slaves.daq]
event_channel = 0

[[slaves.events]]
channel = 0
decimation = 100
measurements = ["A", "B"]

[[slaves.events]]
channel = 1
measurements = ["C"]

[[slaves]]
name = "charger"
a2l = "charger.a2l"

[slaves.connection]
addr = "192.168.1.20:5555"

[[slaves.events]]
channel = 2
measurements = ["D"]
"#;
        let config: DaqPrintConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.print.decimation(), 10);
        assert!(!config.print.timestamp());
        assert_eq!(config.print.refresh(), Duration::from_millis(20));
        assert!(config.a2l.is_none());
        assert_eq!(config.slaves.len(), 2);

        let (print, resolved) = resolve_slaves(config).unwrap();
        assert_eq!(print.decimation(), 10);
        assert_eq!(resolved.len(), 2);

        assert_eq!(resolved[0].name, "inverter");
        assert_eq!(resolved[0].a2l, "pms-a.a2l");
        let conn = resolved[0].connection.as_ref().unwrap();
        assert_eq!(conn.transport_type.as_deref(), Some("can"));
        assert_eq!(conn.channel.as_deref(), Some("PCAN_USBBUS1"));
        assert_eq!(conn.can_id, Some(0x7fb));
        assert_eq!(conn.daq_ids, Some(vec![0x7fd]));
        assert_eq!(resolved[0].events.len(), 2);
        assert_eq!(resolved[0].events[0].decimation, Some(100));
        assert_eq!(resolved[0].events[1].measurements, vec!["C"]);

        assert_eq!(resolved[1].name, "charger");
        assert_eq!(
            resolved[1].connection.as_ref().unwrap().addr.as_deref(),
            Some("192.168.1.20:5555")
        );
    }

    #[test]
    fn test_parse_single_slave_shorthand() {
        let toml_str = r#"
a2l = "abc.a2l"

[connection]
addr = "127.0.0.1:5555"

[daq]
event_channel = 1

[[events]]
channel = 1
decimation = 100
measurements = ["speed", "rpm"]
"#;
        let config: DaqPrintConfig = toml::from_str(toml_str).unwrap();
        let (print, resolved) = resolve_slaves(config).unwrap();
        assert_eq!(print.decimation(), 1);
        assert!(print.timestamp());
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].name, "abc");
        assert_eq!(resolved[0].daq.as_ref().unwrap().event_channel, Some(1));
        assert_eq!(resolved[0].events.len(), 1);
        assert_eq!(resolved[0].events[0].decimation, Some(100));
    }

    #[test]
    fn test_resolve_slaves_requires_a2l() {
        let config: DaqPrintConfig = toml::from_str("").unwrap();
        let err = resolve_slaves(config).unwrap_err();
        assert!(err.contains("No slave configured"), "error: {}", err);
    }

    #[test]
    fn test_resolve_slaves_rejects_mixed_shorthand_and_slaves() {
        let toml_str = r#"
a2l = "abc.a2l"

[[slaves]]
a2l = "other.a2l"
"#;
        let config: DaqPrintConfig = toml::from_str(toml_str).unwrap();
        let err = resolve_slaves(config).unwrap_err();
        assert!(err.contains("cannot be combined"), "error: {}", err);
    }

    #[test]
    fn test_resolve_slaves_defaults_and_dedups_names() {
        // No explicit names, both pointing at the same A2L stem.
        let toml_str = r#"
[[slaves]]
a2l = "dir/ecu.a2l"

[[slaves]]
a2l = "other/ecu.a2l"

[[slaves]]
name = "named"
a2l = "third.a2l"
"#;
        let config: DaqPrintConfig = toml::from_str(toml_str).unwrap();
        let (_, resolved) = resolve_slaves(config).unwrap();
        assert_eq!(resolved[0].name, "ecu");
        assert_eq!(resolved[1].name, "ecu#2");
        assert_eq!(resolved[2].name, "named");
    }

    #[test]
    fn test_default_slave_name_fallbacks() {
        assert_eq!(default_slave_name("dir/ecu.a2l", 0), "ecu");
        assert_eq!(default_slave_name("", 2), "slave3");
    }

    #[test]
    fn test_should_print_first_sample() {
        assert!(!should_print(0, 0, 100));
        assert!(should_print(0, 1, 100));
        assert!(should_print(0, 500, 100));
    }

    #[test]
    fn test_should_print_decimation() {
        // 10ms event, decimation 100 → once per second
        assert!(!should_print(1, 50, 100));
        assert!(!should_print(1, 100, 100));
        assert!(should_print(1, 101, 100));
        // decimation 1 prints every sample
        assert!(should_print(7, 8, 1));
        assert!(!should_print(8, 8, 1));
        // decimation 0 is treated as 1
        assert!(should_print(3, 4, 0));
    }

    #[test]
    fn test_format_value_numbers() {
        let identical = CompuMethod::identical();
        assert_eq!(format_value(3.0, false, &identical), "3");
        assert_eq!(format_value(-12.0, false, &identical), "-12");
        assert_eq!(format_value(1.5, true, &identical), "1.5");
        assert_eq!(format_value(1.0, true, &identical), "1");
        assert_eq!(format_value(0.000001, true, &identical), "0.000001");
        assert_eq!(format_value(f64::NAN, true, &identical), "NaN");
    }

    #[test]
    fn test_format_value_linear() {
        // 0.1 factor: raw 255 → 25.5
        let linear = CompuMethod::linear(0.1, 0.0);
        assert_eq!(format_value(25.5, false, &linear), "25.5");
        assert_eq!(format_value(25.0, false, &linear), "25");
    }

    #[test]
    fn test_format_value_enum_text() {
        let vtab = CompuMethod::compu_vtab(vec![(0, "OFF".into()), (1, "ON".into())]);
        assert_eq!(format_value(1.0, false, &vtab), "ON");
        assert_eq!(format_value(0.0, false, &vtab), "OFF");
        // Unknown enum value falls back to the number
        assert_eq!(format_value(7.0, false, &vtab), "7");
    }

    #[test]
    fn test_format_line() {
        let signals = vec![signal("SPEED", true), signal("STATE", false)];
        let mut values = HashMap::new();
        values.insert("SPEED".to_string(), 12.5);
        values.insert("STATE".to_string(), 1.0);

        let line = format_line(Some("12:00:01.234"), "ecu1 ev0", &signals, &values);
        assert_eq!(line, "[12:00:01.234] ecu1 ev0  SPEED=12.5  STATE=1");

        let line = format_line(None, "ecu1 ev0", &signals, &values);
        assert_eq!(line, "ecu1 ev0  SPEED=12.5  STATE=1");
    }

    #[test]
    fn test_format_line_missing_value() {
        let signals = vec![signal("A", false), signal("B", false)];
        let mut values = HashMap::new();
        values.insert("A".to_string(), 2.0);

        let line = format_line(None, "s ev1", &signals, &values);
        assert_eq!(line, "s ev1  A=2  B=-");
    }

    #[test]
    fn test_format_ms() {
        assert_eq!(format_ms(10.0), "10");
        assert_eq!(format_ms(0.5), "0.5");
        assert_eq!(format_ms(1000.0), "1000");
        assert_eq!(format_ms(1234.567), "1234.567");
    }

    #[test]
    fn test_print_config_defaults() {
        let print = PrintConfig::default();
        assert_eq!(print.decimation(), 1);
        assert!(print.timestamp());
        assert!(print.header());
        assert_eq!(print.refresh(), Duration::from_millis(DEFAULT_REFRESH_MS));
    }

    /// Build a session with printers, bypassing the network setup.
    fn session_with_printers(label: &str, printers: Vec<EventPrinter>) -> SlaveSession {
        SlaveSession {
            label: label.to_string(),
            a2l: "ecu.a2l".to_string(),
            transport_label: "CAN PCAN_USBBUS1 0x7FB".to_string(),
            client: XcpClient::new(),
            slave_id: SlaveId::new(),
            daq: DaqManager::new(),
            printers,
        }
    }

    #[test]
    fn test_header_lines_include_event_cycle_and_decimation() {
        let printer = EventPrinter {
            daq_list: 0,
            label: "ev0".to_string(),
            event_name: Some("Task_10ms".to_string()),
            cycle_ms: Some(10.0),
            decimation: 100,
            signals: vec![signal("A", false), signal("B", true)],
            last_samples: 0,
            printed: 0,
            last_line: None,
        };
        let sessions = vec![session_with_printers("inverter", vec![printer])];

        let lines = header_lines(&sessions, &PrintConfig::default(), PrintMode::Line);
        assert_eq!(
            lines[0],
            "# xcp-client daq-print | 1 slave(s) | refresh 10ms | mode line | CTRL+C to stop"
        );
        assert_eq!(lines[1], "# [inverter] ecu.a2l via CAN PCAN_USBBUS1 0x7FB");
        assert_eq!(
            lines[2],
            "# [inverter] ev0 | \"Task_10ms\" | 10ms | every 100 sample(s) (~1000ms) | 2 signal(s)"
        );
        assert_eq!(lines[3], "# [inverter]   A, B");
    }

    #[test]
    fn test_header_lines_without_event_metadata() {
        let printer = EventPrinter {
            daq_list: 0,
            label: "ev2".to_string(),
            event_name: None,
            cycle_ms: None,
            decimation: 1,
            signals: vec![signal("X", false)],
            last_samples: 0,
            printed: 0,
            last_line: None,
        };
        let sessions = vec![session_with_printers("ecu", vec![printer])];

        let lines = header_lines(&sessions, &PrintConfig::default(), PrintMode::SingleLine);
        assert_eq!(
            lines[2],
            "# [ecu] ev2 | cycle n/a | every 1 sample(s) | 1 signal(s)"
        );
    }

    #[test]
    fn test_print_mode_parsing() {
        let toml_str = r#"
[print]
mode = "single-line"
"#;
        let config: DaqPrintConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.print.mode(), PrintMode::SingleLine);
        assert_eq!(config.print.mode().as_str(), "single-line");

        // `inline` is accepted as an alias.
        let toml_str = r#"
[print]
mode = "inline"
"#;
        let config: DaqPrintConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.print.mode(), PrintMode::SingleLine);

        // Default is append mode.
        let config: DaqPrintConfig = toml::from_str("[print]\ntimestamp = false\n").unwrap();
        assert_eq!(config.print.mode(), PrintMode::Line);

        let err = toml::from_str::<DaqPrintConfig>("[print]\nmode = \"scroll\"\n").unwrap_err();
        assert!(err.to_string().contains("scroll"), "error: {}", err);
    }

    #[test]
    fn test_render_inline_update_first_draw() {
        let lines = vec!["line A".to_string(), "line B".to_string()];
        let rendered = render_inline_update(&lines, 0);
        // No cursor movement on the first draw; every line is cleared first.
        assert!(!rendered.contains("\x1b[2A"));
        assert_eq!(rendered, "\r\x1b[2Kline A\n\r\x1b[2Kline B\n");
    }

    #[test]
    fn test_render_inline_update_redraw() {
        let lines = vec!["A=1".to_string(), "B=2".to_string()];
        let rendered = render_inline_update(&lines, 2);
        assert!(rendered.starts_with("\x1b[2A"), "rendered: {:?}", rendered);
        assert_eq!(rendered.matches("\x1b[2K").count(), 2);
    }

    #[test]
    fn test_render_inline_update_clears_removed_lines() {
        // Previous block had 3 lines, the new one only 2 → the third is erased.
        let lines = vec!["A=1".to_string(), "B=2".to_string()];
        let rendered = render_inline_update(&lines, 3);
        assert!(rendered.starts_with("\x1b[3A"));
        assert_eq!(rendered.matches("\x1b[2K").count(), 3);
        assert!(rendered.ends_with("\n"));
    }

    #[test]
    fn test_truncate_line() {
        assert_eq!(truncate_line("short", 10), "short");
        assert_eq!(truncate_line("0123456789", 10), "0123456789");
        assert_eq!(truncate_line("0123456789", 5), "0123…");
        assert_eq!(truncate_line("anything", 0), "");
        // Multi-byte characters are counted as one column each.
        assert_eq!(truncate_line("状态=ON", 4), "状态=…");
    }

    #[test]
    fn test_cap_block() {
        let lines = vec![
            "1".to_string(),
            "2".to_string(),
            "3".to_string(),
            "4".to_string(),
        ];
        assert_eq!(cap_block(lines.clone(), 10), lines);
        assert_eq!(
            cap_block(lines.clone(), 3),
            vec![
                "1".to_string(),
                "2".to_string(),
                "... (+2 more event(s))".to_string()
            ]
        );
        assert_eq!(
            cap_block(lines, 1),
            vec!["... (+4 more event(s))".to_string()]
        );
    }

    #[test]
    fn test_inline_block_limit() {
        assert_eq!(inline_block_limit(24), 23);
        assert_eq!(inline_block_limit(1), 1);
        assert_eq!(inline_block_limit(0), 1);
    }
}
