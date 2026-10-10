//! CAN Transport for XCP on CAN (PCAN / PEAK-System)
//!
//! XCP on CAN maps a single XCP packet into one CAN frame. The CAN identifier
//! selects the direction/object:
//! - Master → Slave commands are sent with the configured `can_id`.
//! - Slave → Master responses/DAQ data arrive on other identifiers and are
//!   forwarded to the XCP layer, which classifies packets by PID.
//!
//! The transport talks to the PEAK-System PCAN-Basic driver (`PCANBasic.dll`)
//! dynamically through `libloading`, so no import library is required at build
//! time and non-Windows builds still compile.
//!
//! Note: a transport instance owns one PCAN channel and forwards every received
//! frame to the XCP layer. Use one channel (or one transport) per XCP slave when
//! multiple slaves share a bus, since frames are not demultiplexed by CAN ID here.
//!
//! Configuration:
//! - `PCAN_BASIC_DLL` env var: path to `PCANBasic.dll`. May be the full file
//!   path (e.g. `...\x64\PCANBasic.dll`) or just the directory containing it.
//! - `PCAN_BITRATE` env var: bit rate selection. Accepts named classic rates
//!   (`1M`, `500K`, `250K`, `125K`, `100K`), a raw BTR0/BTR1 code, or a full
//!   CAN-FD bit rate string (contains `=`), in which case CAN-FD is used. When
//!   unset, CAN-FD is attempted first with a default timing and the transport
//!   falls back to classic CAN at 500k.

use super::transport::{Transport, TransportError, TransportType};
use async_trait::async_trait;
use libloading::{Library, Symbol};
use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// PCAN-Basic constants (see PCANBasic.h)
// ---------------------------------------------------------------------------

const PCAN_ERROR_OK: u32 = 0x00000;
const PCAN_ERROR_QRCVEMPTY: u32 = 0x00020;
const PCAN_ERROR_ILLPARAMTYPE: u32 = 0x04000;

const PCAN_MESSAGE_STANDARD: u8 = 0x00;
const PCAN_MESSAGE_EXTENDED: u8 = 0x02;
const PCAN_MESSAGE_FD: u8 = 0x04;

// Classic CAN bit rate timing codes.
const PCAN_BAUD_1M: u16 = 0x0014;
const PCAN_BAUD_500K: u16 = 0x001C;
const PCAN_BAUD_250K: u16 = 0x011C;
const PCAN_BAUD_125K: u16 = 0x031C;
const PCAN_BAUD_100K: u16 = 0x432F;

// Channel handles.
const PCAN_USBBUS1: u16 = 0x51;
const PCAN_PCIBUS1: u16 = 0x41;
const PCAN_LANBUS1: u16 = 0x801;

// Parameters.
const PCAN_ALLOW_ECHO_FRAMES: u8 = 0x2C;
const PCAN_PARAMETER_OFF: u32 = 0x00;

const PCAN_LANGUAGE_ENGLISH: u16 = 0x09;

// Misc limits.
const MAX_CAN_DATA: usize = 64;
const DEFAULT_TIMEOUT_MS: u64 = 1000;
const DEFAULT_FD_BITRATE: &str =
    "f_clock=80000000,nom_brp=10,nom_tseg1=5,nom_tseg2=2,nom_sjw=1,\
     data_brp=4,data_tseg1=7,data_tseg2=2,data_sjw=1";

const PCAN_DLL_ENV: &str = "PCAN_BASIC_DLL";
const PCAN_BITRATE_ENV: &str = "PCAN_BITRATE";

/// Known DLL locations, tried after the environment override.
const PCAN_DLL_FALLBACK_PATHS: &[&str] = &[
    r"E:\workspace\c\PCANBasic\x64\PCANBasic.dll",
    r"E:\workspace\c\PCANBasic\x86\PCANBasic.dll",
];

// ---------------------------------------------------------------------------
// PCAN-Basic FFI types
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
struct TpcanMsg {
    id: u32,
    msg_type: u8,
    len: u8,
    data: [u8; 8],
}

impl Default for TpcanMsg {
    fn default() -> Self {
        Self {
            id: 0,
            msg_type: 0,
            len: 0,
            data: [0; 8],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TpcanMsgFd {
    id: u32,
    msg_type: u8,
    dlc: u8,
    data: [u8; 64],
}

impl Default for TpcanMsgFd {
    fn default() -> Self {
        Self {
            id: 0,
            msg_type: 0,
            dlc: 0,
            data: [0; 64],
        }
    }
}

type CanInitializeFn = unsafe extern "system" fn(u16, u16, u8, u32, u16) -> u32;
type CanInitializeFdFn = unsafe extern "system" fn(u16, *const i8) -> u32;
type CanUninitializeFn = unsafe extern "system" fn(u16) -> u32;
type CanReadFn = unsafe extern "system" fn(u16, *mut TpcanMsg, *mut c_void) -> u32;
type CanReadFdFn = unsafe extern "system" fn(u16, *mut TpcanMsgFd, *mut u64) -> u32;
type CanWriteFn = unsafe extern "system" fn(u16, *const TpcanMsg) -> u32;
type CanWriteFdFn = unsafe extern "system" fn(u16, *const TpcanMsgFd) -> u32;
type CanSetValueFn = unsafe extern "system" fn(u16, u8, *mut c_void, u32) -> u32;
type CanGetErrorTextFn = unsafe extern "system" fn(u32, u16, *mut i8) -> u32;
type CanLookUpChannelFn = unsafe extern "system" fn(*const i8, *mut u16) -> u32;

/// Wrapper around the loaded PCANBasic library and its function pointers.
struct PcanApi {
    // Keep the library alive for as long as the function pointers are used.
    _lib: Library,
    initialize: CanInitializeFn,
    initialize_fd: Option<CanInitializeFdFn>,
    uninitialize: CanUninitializeFn,
    read: CanReadFn,
    read_fd: Option<CanReadFdFn>,
    write: CanWriteFn,
    write_fd: Option<CanWriteFdFn>,
    set_value: CanSetValueFn,
    get_error_text: CanGetErrorTextFn,
    look_up_channel: CanLookUpChannelFn,
}

impl PcanApi {
    /// Load `PCANBasic.dll` and resolve the required entry points.
    fn load() -> Result<Self, TransportError> {
        let lib = load_pcan_library()?;

        unsafe {
            let initialize = resolve::<CanInitializeFn>(&lib, b"CAN_Initialize\0")?;
            let uninitialize = resolve::<CanUninitializeFn>(&lib, b"CAN_Uninitialize\0")?;
            let read = resolve::<CanReadFn>(&lib, b"CAN_Read\0")?;
            let write = resolve::<CanWriteFn>(&lib, b"CAN_Write\0")?;
            let set_value = resolve::<CanSetValueFn>(&lib, b"CAN_SetValue\0")?;
            let get_error_text = resolve::<CanGetErrorTextFn>(&lib, b"CAN_GetErrorText\0")?;
            let look_up_channel =
                resolve::<CanLookUpChannelFn>(&lib, b"CAN_LookUpChannel\0")?;

            // CAN-FD entry points are optional (older DLLs may lack them).
            let initialize_fd = resolve_optional::<CanInitializeFdFn>(&lib, b"CAN_InitializeFD\0");
            let read_fd = resolve_optional::<CanReadFdFn>(&lib, b"CAN_ReadFD\0");
            let write_fd = resolve_optional::<CanWriteFdFn>(&lib, b"CAN_WriteFD\0");

            Ok(Self {
                _lib: lib,
                initialize,
                initialize_fd,
                uninitialize,
                read,
                read_fd,
                write,
                write_fd,
                set_value,
                get_error_text,
                look_up_channel,
            })
        }
    }

    /// Human readable text for a PCAN status code.
    fn error_text(&self, status: u32) -> String {
        let mut buf = [0i8; 256];
        let res = unsafe {
            (self.get_error_text)(status, PCAN_LANGUAGE_ENGLISH, buf.as_mut_ptr())
        };
        if res == PCAN_ERROR_OK {
            let bytes: Vec<u8> = buf
                .iter()
                .take_while(|&&c| c != 0)
                .map(|&c| c as u8)
                .collect();
            String::from_utf8_lossy(&bytes).trim().to_string()
        } else {
            format!("0x{:08X}", status)
        }
    }

    /// Read one frame from the channel. Returns `Ok(None)` when the queue is empty.
    fn read_frame(&self, handle: u16, fd: bool) -> Result<Option<Vec<u8>>, u32> {
        unsafe {
            if fd {
                let read_fd = match self.read_fd {
                    Some(f) => f,
                    None => return Err(PCAN_ERROR_ILLPARAMTYPE),
                };
                let mut msg = TpcanMsgFd::default();
                let status = read_fd(handle, &mut msg, std::ptr::null_mut());
                if status == PCAN_ERROR_OK {
                    let len = dlc_to_len(msg.dlc).min(msg.data.len());
                    Ok(Some(msg.data[..len].to_vec()))
                } else if status == PCAN_ERROR_QRCVEMPTY {
                    Ok(None)
                } else {
                    Err(status)
                }
            } else {
                let mut msg = TpcanMsg::default();
                let status = (self.read)(handle, &mut msg, std::ptr::null_mut());
                if status == PCAN_ERROR_OK {
                    let len = (msg.len as usize).min(msg.data.len());
                    Ok(Some(msg.data[..len].to_vec()))
                } else if status == PCAN_ERROR_QRCVEMPTY {
                    Ok(None)
                } else {
                    Err(status)
                }
            }
        }
    }

    /// Transmit one frame on the channel.
    fn write_frame(
        &self,
        handle: u16,
        can_id: u32,
        extended: bool,
        fd: bool,
        payload: &[u8],
    ) -> Result<(), u32> {
        if payload.len() > MAX_CAN_DATA {
            return Err(PCAN_ERROR_ILLPARAMTYPE);
        }

        let id_type = if extended {
            PCAN_MESSAGE_EXTENDED
        } else {
            PCAN_MESSAGE_STANDARD
        };

        unsafe {
            if fd {
                let write_fd = match self.write_fd {
                    Some(f) => f,
                    None => return Err(PCAN_ERROR_ILLPARAMTYPE),
                };
                let mut msg = TpcanMsgFd::default();
                msg.id = can_id;
                msg.msg_type = id_type | PCAN_MESSAGE_FD;
                msg.dlc = len_to_dlc(payload.len());
                msg.data[..payload.len()].copy_from_slice(payload);
                let status = write_fd(handle, &msg);
                if status == PCAN_ERROR_OK {
                    Ok(())
                } else {
                    Err(status)
                }
            } else {
                let mut msg = TpcanMsg::default();
                msg.id = can_id;
                msg.msg_type = id_type;
                msg.len = payload.len() as u8;
                msg.data[..payload.len()].copy_from_slice(payload);
                let status = (self.write)(handle, &msg);
                if status == PCAN_ERROR_OK {
                    Ok(())
                } else {
                    Err(status)
                }
            }
        }
    }
}

unsafe fn resolve<T>(lib: &Library, name: &[u8]) -> Result<T, TransportError>
where
    T: Copy,
{
    let symbol: Symbol<T> = lib.get(name).map_err(|e| {
        TransportError::ConnectFailed(format!(
            "PCANBasic.dll is missing symbol {}: {}",
            String::from_utf8_lossy(&name[..name.len() - 1]),
            e
        ))
    })?;
    Ok(*symbol)
}

unsafe fn resolve_optional<T>(lib: &Library, name: &[u8]) -> Option<T>
where
    T: Copy,
{
    lib.get::<T>(name).ok().map(|s| *s)
}

/// Expand a single `PCAN_BASIC_DLL` value into candidate DLL file paths.
///
/// Accepts either a full path to the DLL file (e.g. `...\x64\PCANBasic.dll`)
/// or just the directory that contains it (e.g. `...\x64`).
fn expand_dll_candidate(value: &str) -> Vec<String> {
    let v = value.trim();
    let path = std::path::Path::new(v);

    if path.is_dir() {
        return vec![path
            .join("PCANBasic.dll")
            .to_string_lossy()
            .into_owned()];
    }

    if v.to_ascii_lowercase().ends_with(".dll") {
        return vec![v.to_string()];
    }

    // Ambiguous value: try it as a file first, then assume it was a directory.
    vec![
        v.to_string(),
        path.join("PCANBasic.dll").to_string_lossy().into_owned(),
    ]
}

/// Locate and load the PCANBasic shared library.
fn load_pcan_library() -> Result<Library, TransportError> {
    let mut candidates: Vec<String> = Vec::new();

    if let Ok(path) = std::env::var(PCAN_DLL_ENV) {
        if !path.trim().is_empty() {
            candidates.extend(expand_dll_candidate(&path));
        }
    }

    // Next to the running executable.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("PCANBasic.dll").to_string_lossy().into_owned());
        }
    }

    // Known install locations.
    for p in PCAN_DLL_FALLBACK_PATHS {
        candidates.push((*p).to_string());
    }

    // Let the OS resolve from PATH / current directory last.
    candidates.push("PCANBasic.dll".to_string());

    let mut last_err: Option<String> = None;
    for candidate in candidates {
        match unsafe { Library::new(&candidate) } {
            Ok(lib) => {
                tracing::info!("Loaded PCANBasic library from {}", candidate);
                return Ok(lib);
            }
            Err(e) => {
                tracing::debug!("Failed to load PCANBasic from {}: {}", candidate, e);
                last_err = Some(e.to_string());
            }
        }
    }

    Err(TransportError::ConnectFailed(format!(
        "Unable to load PCANBasic.dll ({}). Set the PCAN_BASIC_DLL environment variable to the DLL path.",
        last_err.unwrap_or_else(|| "not found".to_string())
    )))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Decode a CAN-FD DLC code into the number of data bytes.
pub fn dlc_to_len(dlc: u8) -> usize {
    match dlc {
        0..=8 => dlc as usize,
        9 => 12,
        10 => 16,
        11 => 20,
        12 => 24,
        13 => 32,
        14 => 48,
        _ => 64,
    }
}

/// Encode a data byte count into the smallest CAN-FD DLC code that fits.
pub fn len_to_dlc(len: usize) -> u8 {
    match len {
        0..=8 => len as u8,
        9..=12 => 9,
        13..=16 => 10,
        17..=20 => 11,
        21..=24 => 12,
        25..=32 => 13,
        33..=48 => 14,
        _ => 15,
    }
}

/// Parse a channel string into a PCAN handle. Supports named channels,
/// hex (`0x51`) and decimal (`81`) handles. `auto`/empty yield `None` so the
/// caller can look up an attached channel at runtime.
pub fn parse_channel_handle(channel: &str) -> Result<Option<u16>, TransportError> {
    let c = channel.trim();
    if c.is_empty() || c.eq_ignore_ascii_case("auto") {
        return Ok(None);
    }

    let upper = c.to_ascii_uppercase();
    if let Some(hex) = upper.strip_prefix("0X") {
        if let Ok(v) = u16::from_str_radix(hex, 16) {
            return Ok(Some(v));
        }
    }
    if let Ok(v) = c.parse::<u16>() {
        return Ok(Some(v));
    }

    let named = match upper.replace(' ', "_").as_str() {
        "PCAN_USBBUS1" | "PCAN_USB1" | "USB1" => Some(PCAN_USBBUS1),
        "PCAN_PCIBUS1" | "PCAN_PCI1" | "PCI1" => Some(PCAN_PCIBUS1),
        "PCAN_LANBUS1" | "PCAN_LAN1" | "LAN1" => Some(PCAN_LANBUS1),
        _ => None,
    };

    match named {
        Some(handle) => Ok(Some(handle)),
        None => Err(TransportError::InvalidAddress(format!(
            "Unknown PCAN channel '{}'. Use a handle like 0x51 or 'auto'.",
            channel
        ))),
    }
}

/// Parse the `PCAN_BITRATE` environment variable.
///
/// Returns `(classic_baud, fd_bitrate)` where exactly one is `Some`.
fn parse_bitrate(value: &str) -> Result<(Option<u16>, Option<String>), TransportError> {
    let v = value.trim();
    if v.is_empty() {
        return Err(TransportError::InvalidAddress("empty bit rate".to_string()));
    }

    // A CAN-FD bit rate string always contains '='.
    if v.contains('=') {
        return Ok((None, Some(v.to_string())));
    }

    let upper = v.to_ascii_uppercase();
    let classic = match upper.as_str() {
        "1M" | "1MBIT" | "1000000" => Some(PCAN_BAUD_1M),
        "500K" | "500KBIT" | "500000" => Some(PCAN_BAUD_500K),
        "250K" | "250KBIT" | "250000" => Some(PCAN_BAUD_250K),
        "125K" | "125KBIT" | "125000" => Some(PCAN_BAUD_125K),
        "100K" | "100KBIT" | "100000" => Some(PCAN_BAUD_100K),
        _ => None,
    };
    if let Some(baud) = classic {
        return Ok((Some(baud), None));
    }

    // Raw BTR0/BTR1 code, either hex (0x...) or decimal.
    let raw = upper
        .strip_prefix("0X")
        .and_then(|h| u16::from_str_radix(h, 16).ok())
        .or_else(|| v.parse::<u16>().ok());
    match raw {
        Some(code) => Ok((Some(code), None)),
        None => Err(TransportError::InvalidAddress(format!(
            "Invalid PCAN bit rate '{}'",
            value
        ))),
    }
}

fn resolve_bitrate() -> Result<BitrateChoice, TransportError> {
    match std::env::var(PCAN_BITRATE_ENV) {
        Ok(val) if !val.trim().is_empty() => {
            let (classic, fd) = parse_bitrate(&val)?;
            if let Some(fd_str) = fd {
                Ok(BitrateChoice::Fd(fd_str))
            } else {
                Ok(BitrateChoice::Classic(classic.unwrap_or(PCAN_BAUD_500K)))
            }
        }
        _ => Ok(BitrateChoice::Auto),
    }
}

/// Bit rate selection derived from configuration/environment.
#[derive(Debug, Clone, PartialEq, Eq)]
enum BitrateChoice {
    /// Use classic CAN at the given BTR0/BTR1 code.
    Classic(u16),
    /// Use CAN-FD with the given bit rate string.
    Fd(String),
    /// Try CAN-FD first, then fall back to classic 500k.
    Auto,
}

// ---------------------------------------------------------------------------
// CanTransport
// ---------------------------------------------------------------------------

/// CAN transport for XCP on CAN using the PEAK-System PCAN-Basic driver.
pub struct CanTransport {
    /// Requested channel string (for logging).
    channel: String,
    /// Resolved PCAN channel handle.
    handle: u16,
    /// CAN ID used for master → slave commands.
    can_id: u32,
    /// True when `can_id` uses a 29-bit identifier.
    extended: bool,
    /// True when CAN-FD framing is used.
    fd: bool,
    /// Loaded PCAN-Basic API.
    api: Arc<PcanApi>,
    /// Command/response timeout.
    timeout_ms: u64,
    /// Frames received by the background reader thread.
    rx: Arc<Mutex<VecDeque<Vec<u8>>>>,
    /// Signal to stop the reader thread.
    reader_stop: Arc<AtomicBool>,
    /// Background reader thread handle.
    reader: Mutex<Option<JoinHandle<()>>>,
    /// Connection state.
    connected: AtomicBool,
}

impl CanTransport {
    /// Create a new CAN transport over PCAN.
    ///
    /// # Arguments
    /// * `channel` - PCAN channel handle/name, or `auto` to pick an attached device.
    /// * `can_id`  - CAN ID used to send XCP commands to the slave.
    pub async fn connect(channel: &str, can_id: u32) -> Result<Self, TransportError> {
        let api = Arc::new(PcanApi::load()?);

        let handle = match parse_channel_handle(channel)? {
            Some(h) => h,
            None => lookup_attached_channel(&api),
        };

        let bitrate = resolve_bitrate()?;

        // Determine which init strategies to attempt. "Auto" (the default)
        // tries CAN-FD first and falls back to classic CAN.
        let (try_fd, fd_bitrate, classic_baud, allow_classic_fallback) = match &bitrate {
            BitrateChoice::Classic(baud) => (false, None, *baud, true),
            BitrateChoice::Fd(fd_str) => (true, Some(fd_str.clone()), PCAN_BAUD_500K, false),
            BitrateChoice::Auto => (true, None, PCAN_BAUD_500K, true),
        };

        let mut fd = false;
        let mut init_status = PCAN_ERROR_OK;

        if try_fd {
            let fd_str = fd_bitrate
                .clone()
                .unwrap_or_else(|| DEFAULT_FD_BITRATE.to_string());
            match init_fd(&api, handle, &fd_str) {
                Ok(()) => {
                    fd = true;
                    tracing::info!("PCAN channel {} initialized in CAN-FD mode", handle);
                }
                Err(status) => {
                    if !allow_classic_fallback {
                        return Err(TransportError::ConnectFailed(format!(
                            "Failed to initialize PCAN channel {} in CAN-FD mode: {}",
                            handle,
                            api.error_text(status)
                        )));
                    }
                    tracing::warn!(
                        "CAN-FD init failed on channel {} ({}); falling back to classic CAN",
                        handle,
                        api.error_text(status)
                    );
                    init_status = status;
                }
            }
        }

        if !fd {
            // Classic CAN initialization.
            let status = unsafe { (api.initialize)(handle, classic_baud, 0, 0, 0) };
            if status != PCAN_ERROR_OK {
                // If we already tried (and failed) FD, report both attempts.
                let text = api.error_text(status);
                unsafe { (api.uninitialize)(handle) };
                return Err(TransportError::ConnectFailed(format!(
                    "Failed to initialize PCAN channel {} (classic baud 0x{:04X}): {}{}",
                    handle,
                    classic_baud,
                    text,
                    if init_status != PCAN_ERROR_OK {
                        format!(" (previous FD error: {})", api.error_text(init_status))
                    } else {
                        String::new()
                    }
                )));
            }
            tracing::info!(
                "PCAN channel {} initialized in classic CAN mode (baud 0x{:04X})",
                handle,
                classic_baud
            );
        }

        // Disable echo so our own transmitted frames are not routed back up.
        set_param_off(&api, handle, PCAN_ALLOW_ECHO_FRAMES);

        let rx = Arc::new(Mutex::new(VecDeque::new()));
        let reader_stop = Arc::new(AtomicBool::new(false));
        let reader = spawn_reader(api.clone(), handle, fd, rx.clone(), reader_stop.clone());

        let extended = can_id > 0x7FF;

        Ok(Self {
            channel: channel.to_string(),
            handle,
            can_id,
            extended,
            fd,
            api,
            timeout_ms: DEFAULT_TIMEOUT_MS,
            rx,
            reader_stop,
            reader: Mutex::new(Some(reader)),
            connected: AtomicBool::new(true),
        })
    }

    /// Get the CAN channel name.
    pub fn channel(&self) -> &str {
        &self.channel
    }

    /// Get the command CAN ID.
    pub fn can_id(&self) -> u32 {
        self.can_id
    }

    /// Get the resolved PCAN channel handle.
    pub fn handle(&self) -> u16 {
        self.handle
    }

    /// Whether CAN-FD framing is active.
    pub fn is_fd(&self) -> bool {
        self.fd
    }

    /// Set the command/response timeout.
    pub fn set_timeout(&mut self, timeout_ms: u64) {
        self.timeout_ms = timeout_ms;
    }

    /// Transmit a normalized XCP payload as a single CAN frame.
    fn send_frame(&self, payload: &[u8]) -> Result<(), TransportError> {
        if payload.is_empty() {
            return Err(TransportError::SendFailed(
                "refusing to send empty XCP payload".to_string(),
            ));
        }
        if !self.fd && payload.len() > 8 {
            return Err(TransportError::SendFailed(format!(
                "XCP payload of {} bytes does not fit in a classic CAN frame",
                payload.len()
            )));
        }

        tracing::trace!(
            "Sending XCP-on-CAN frame: id=0x{:X}, fd={}, len={}, data=[{:02X?}]",
            self.can_id,
            self.fd,
            payload.len(),
            payload
        );

        self.api
            .write_frame(self.handle, self.can_id, self.extended, self.fd, payload)
            .map_err(|status| {
                TransportError::SendFailed(format!(
                    "CAN write failed: {}",
                    self.api.error_text(status)
                ))
            })
    }

    async fn recv_payload(
        &self,
        timeout_ms: Option<u64>,
    ) -> Result<Option<Vec<u8>>, TransportError> {
        let start = Instant::now();
        loop {
            {
                let mut queue = self.rx.lock().unwrap();
                if let Some(payload) = queue.pop_front() {
                    return Ok(Some(payload));
                }
            }

            if !self.connected.load(Ordering::SeqCst) {
                return Err(TransportError::ConnectionClosed);
            }

            if let Some(timeout) = timeout_ms {
                if start.elapsed() >= Duration::from_millis(timeout) {
                    return Ok(None);
                }
            }

            tokio::time::sleep(Duration::from_micros(500)).await;
        }
    }
}

#[async_trait]
impl Transport for CanTransport {
    async fn send_command_only(&self, command: &[u8]) -> Result<(), TransportError> {
        self.send_frame(command)
    }

    async fn send_command(&self, command: &[u8]) -> Result<Vec<u8>, TransportError> {
        self.send_frame(command)?;
        match self.recv_payload(Some(self.timeout_ms)).await? {
            Some(payload) => Ok(payload),
            None => Err(TransportError::Timeout),
        }
    }

    async fn recv_packet(&self) -> Result<Vec<u8>, TransportError> {
        match self.recv_payload(None).await? {
            Some(payload) => Ok(payload),
            None => Err(TransportError::ConnectionClosed),
        }
    }

    async fn try_recv_packet(&self, timeout_ms: u64) -> Result<Option<Vec<u8>>, TransportError> {
        self.recv_payload(Some(timeout_ms)).await
    }

    async fn close(&self) -> Result<(), TransportError> {
        tracing::trace!(
            "Closing CAN transport on {} (handle 0x{:X}, id 0x{:X})",
            self.channel,
            self.handle,
            self.can_id
        );

        self.connected.store(false, Ordering::SeqCst);
        self.reader_stop.store(true, Ordering::SeqCst);

        if let Some(handle) = self.reader.lock().unwrap().take() {
            let _ = handle.join();
        }

        unsafe {
            (self.api.uninitialize)(self.handle);
        }

        Ok(())
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Can
    }

    fn target_addr(&self) -> String {
        format!("{}:0x{:03X}", self.channel, self.can_id)
    }
}

impl Drop for CanTransport {
    fn drop(&mut self) {
        self.reader_stop.store(true, Ordering::SeqCst);
        if let Ok(mut guard) = self.reader.lock() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }
        // Only uninitialize if close() did not already do it.
        if self.connected.swap(false, Ordering::SeqCst) {
            unsafe {
                (self.api.uninitialize)(self.handle);
            }
        }
    }
}

/// Start the background thread that drains the PCAN receive queue.
fn spawn_reader(
    api: Arc<PcanApi>,
    handle: u16,
    fd: bool,
    rx: Arc<Mutex<VecDeque<Vec<u8>>>>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("pcan-reader".to_string())
        .spawn(move || {
            tracing::debug!("PCAN reader thread started (handle 0x{:X}, fd={})", handle, fd);
            while !stop.load(Ordering::SeqCst) {
                let mut received_any = false;
                loop {
                    match api.read_frame(handle, fd) {
                        Ok(Some(payload)) => {
                            if !payload.is_empty() {
                                rx.lock().unwrap().push_back(payload);
                            }
                            received_any = true;
                        }
                        Ok(None) => break,
                        Err(status) => {
                            tracing::warn!(
                                "PCAN read error on handle 0x{:X}: {}",
                                handle,
                                api.error_text(status)
                            );
                            break;
                        }
                    }
                }

                // Poll fast when idle; yield briefly when frames are streaming.
                let sleep_us = if received_any { 0 } else { 500 };
                if sleep_us > 0 {
                    std::thread::sleep(Duration::from_micros(sleep_us));
                }
            }
            tracing::debug!("PCAN reader thread stopped (handle 0x{:X})", handle);
        })
        .expect("failed to spawn PCAN reader thread")
}

/// Initialize a channel in CAN-FD mode.
fn init_fd(api: &PcanApi, handle: u16, bitrate: &str) -> Result<(), u32> {
    let initialize_fd = match api.initialize_fd {
        Some(f) => f,
        None => return Err(PCAN_ERROR_ILLPARAMTYPE),
    };
    let mut c_string = bitrate.as_bytes().to_vec();
    c_string.push(0);
    let status = unsafe { initialize_fd(handle, c_string.as_ptr() as *const i8) };
    if status == PCAN_ERROR_OK {
        Ok(())
    } else {
        // Leave the channel clean before retrying classic init.
        unsafe {
            (api.uninitialize)(handle);
        }
        Err(status)
    }
}

/// Turn a PCAN parameter off (best effort).
fn set_param_off(api: &PcanApi, handle: u16, parameter: u8) {
    let mut value: u32 = PCAN_PARAMETER_OFF;
    let status = unsafe {
        (api.set_value)(
            handle,
            parameter,
            &mut value as *mut u32 as *mut c_void,
            std::mem::size_of::<u32>() as u32,
        )
    };
    if status != PCAN_ERROR_OK {
        tracing::debug!(
            "PCAN SetValue(0x{:02X}) returned {}",
            parameter,
            api.error_text(status)
        );
    }
}

/// Look up the first attached PCAN channel.
fn lookup_attached_channel(api: &PcanApi) -> u16 {
    for params in [
        "devicetype=PCAN_USB",
        "devicetype=PCAN_PCI",
        "devicetype=PCAN_LAN",
    ] {
        let mut c_string = params.as_bytes().to_vec();
        c_string.push(0);
        let mut found: u16 = 0;
        let status = unsafe { (api.look_up_channel)(c_string.as_ptr() as *const i8, &mut found) };
        if status == PCAN_ERROR_OK && found != 0 {
            tracing::info!("Auto-selected PCAN channel 0x{:X} ({})", found, params);
            return found;
        }
    }

    tracing::warn!("No attached PCAN channel found; defaulting to PCAN_USBBUS1");
    PCAN_USBBUS1
}

/// Create a CAN transport from configuration.
#[allow(dead_code)]
pub async fn create_can_transport(
    channel: &str,
    can_id: u32,
) -> Result<CanTransport, TransportError> {
    CanTransport::connect(channel, can_id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dlc_len_roundtrip_classic() {
        for len in 0..=8usize {
            assert_eq!(dlc_to_len(len_to_dlc(len)), len);
        }
    }

    #[test]
    fn test_dlc_to_len_fd_sizes() {
        assert_eq!(dlc_to_len(9), 12);
        assert_eq!(dlc_to_len(10), 16);
        assert_eq!(dlc_to_len(11), 20);
        assert_eq!(dlc_to_len(12), 24);
        assert_eq!(dlc_to_len(13), 32);
        assert_eq!(dlc_to_len(14), 48);
        assert_eq!(dlc_to_len(15), 64);
    }

    #[test]
    fn test_len_to_dlc_buckets() {
        assert_eq!(len_to_dlc(0), 0);
        assert_eq!(len_to_dlc(8), 8);
        assert_eq!(len_to_dlc(9), 9);
        assert_eq!(len_to_dlc(12), 9);
        assert_eq!(len_to_dlc(13), 10);
        assert_eq!(len_to_dlc(33), 14);
        assert_eq!(len_to_dlc(64), 15);
        assert_eq!(len_to_dlc(100), 15);
    }

    #[test]
    fn test_parse_channel_handle_variants() {
        assert_eq!(parse_channel_handle("auto").unwrap(), None);
        assert_eq!(parse_channel_handle("").unwrap(), None);
        assert_eq!(parse_channel_handle("0x51").unwrap(), Some(PCAN_USBBUS1));
        assert_eq!(parse_channel_handle("81").unwrap(), Some(PCAN_USBBUS1));
        assert_eq!(
            parse_channel_handle("PCAN_USBBUS1").unwrap(),
            Some(PCAN_USBBUS1)
        );
        assert_eq!(
            parse_channel_handle("pcan_pcibus1").unwrap(),
            Some(PCAN_PCIBUS1)
        );
        assert!(parse_channel_handle("nonsense").is_err());
    }

    #[test]
    fn test_parse_bitrate_classic_names() {
        assert_eq!(parse_bitrate("1M").unwrap(), (Some(PCAN_BAUD_1M), None));
        assert_eq!(parse_bitrate("500k").unwrap(), (Some(PCAN_BAUD_500K), None));
        assert_eq!(parse_bitrate("250K").unwrap(), (Some(PCAN_BAUD_250K), None));
        assert_eq!(parse_bitrate("125K").unwrap(), (Some(PCAN_BAUD_125K), None));
        assert_eq!(parse_bitrate("100K").unwrap(), (Some(PCAN_BAUD_100K), None));
    }

    #[test]
    fn test_parse_bitrate_raw_and_fd() {
        assert_eq!(parse_bitrate("0x001C").unwrap(), (Some(0x001C), None));
        assert_eq!(parse_bitrate("28").unwrap(), (Some(28), None));
        let (classic, fd) = parse_bitrate("f_clock=80000000,nom_brp=10").unwrap();
        assert_eq!(classic, None);
        assert!(fd.unwrap().contains("f_clock"));
        assert!(parse_bitrate("").is_err());
        assert!(parse_bitrate("bogus").is_err());
    }

    #[test]
    fn test_expand_dll_candidate() {
        // Full file path is used as-is.
        assert_eq!(
            expand_dll_candidate(r"E:\pc\x64\PCANBasic.dll"),
            vec![r"E:\pc\x64\PCANBasic.dll".to_string()]
        );
        // Non-existent, non-dll value is tried both as file and as directory.
        let c = expand_dll_candidate(r"E:\nonexistent\dir");
        assert_eq!(c.len(), 2);
        assert!(c[1].ends_with("PCANBasic.dll"));
    }

    #[test]
    fn test_msg_struct_sizes_match_pcan() {
        // PCAN requires exact layouts: TPCANMsg = 16 bytes, TPCANMsgFD = 72 bytes.
        assert_eq!(std::mem::size_of::<TpcanMsg>(), 16);
        assert_eq!(std::mem::size_of::<TpcanMsgFd>(), 72);
    }
}
