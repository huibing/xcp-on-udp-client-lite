//! PEAK-System PCAN-Basic CAN device driver.
//!
//! This module contains everything specific to the `PCANBasic.dll` API: dynamic
//! library loading, the `CAN_*` FFI bindings, bit-rate/timing conversion and
//! channel enumeration. It implements the hardware-agnostic
//! [`CanDriver`]/[`CanDevice`] traits from [`super::device`].

use super::device::{
    len_to_dlc, CanDevice, CanDeviceConfig, CanDeviceError, CanDeviceInfo, CanDriver, CanFrame,
};
use libloading::{Library, Symbol};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// PCAN-Basic constants (see PCANBasic.h)
// ---------------------------------------------------------------------------

const PCAN_NONEBUS: u16 = 0x00;

const PCAN_ERROR_OK: u32 = 0x00000;
const PCAN_ERROR_QRCVEMPTY: u32 = 0x00020;

const PCAN_MESSAGE_STANDARD: u8 = 0x00;
const PCAN_MESSAGE_EXTENDED: u8 = 0x02;
const PCAN_MESSAGE_FD: u8 = 0x04;
const PCAN_MESSAGE_ERRFRAME: u8 = 0x40;
const PCAN_MESSAGE_STATUS: u8 = 0x80;

// Classic CAN bit rate timing codes.
const PCAN_BAUD_1M: u16 = 0x0014;
const PCAN_BAUD_800K: u16 = 0x0016;
const PCAN_BAUD_500K: u16 = 0x001C;
const PCAN_BAUD_250K: u16 = 0x011C;
const PCAN_BAUD_125K: u16 = 0x031C;
const PCAN_BAUD_100K: u16 = 0x432F;
const PCAN_BAUD_95K: u16 = 0xC34E;
const PCAN_BAUD_83K: u16 = 0x852B;
const PCAN_BAUD_50K: u16 = 0x472F;
const PCAN_BAUD_47K: u16 = 0x1414;
const PCAN_BAUD_33K: u16 = 0x8B2F;
const PCAN_BAUD_20K: u16 = 0x532F;
const PCAN_BAUD_10K: u16 = 0x672F;
const PCAN_BAUD_5K: u16 = 0x7F7F;

/// Common default nominal bit rate in bit/s.
const PCAN_BAUD_500K_BPS: u32 = 500_000;

// Channel handles (first bus of each family).
const PCAN_PCIBUS1: u16 = 0x41;
const PCAN_USBBUS1: u16 = 0x51;
const PCAN_LANBUS1: u16 = 0x801;

// Parameters.
const PCAN_ALLOW_ECHO_FRAMES: u8 = 0x2C;
const PCAN_DEVICE_ID: u8 = 0x01;
const PCAN_ATTACHED_CHANNELS_COUNT: u8 = 0x2A;
const PCAN_ATTACHED_CHANNELS: u8 = 0x2B;
const PCAN_PARAMETER_OFF: u32 = 0x00;

// Channel condition / feature flags.
const PCAN_CHANNEL_AVAILABLE: u32 = 0x01;
const FEATURE_FD_CAPABLE: u32 = 0x01;

// Device types.
const PCAN_DEVICE_PCI: u8 = 0x04;
const PCAN_DEVICE_USB: u8 = 0x05;
const PCAN_DEVICE_LAN: u8 = 0x08;

const PCAN_LANGUAGE_ENGLISH: u16 = 0x09;

const MAX_CAN_DATA: usize = 64;

const PCAN_DLL_ENV: &str = "PCAN_BASIC_DLL";
const PCAN_BITRATE_ENV: &str = "PCAN_BITRATE";
const PCAN_FD_CLOCK_ENV: &str = "PCAN_FD_CLOCK";

/// Default CAN-FD controller clock used to derive timing from bit rates.
const DEFAULT_FD_CLOCK_HZ: u64 = 80_000_000;

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

/// PCAN-Basic reception timestamp for classic CAN messages (`TPCANTimestamp`).
///
/// Total microseconds since device initialization (see `PCANBasic.h`):
/// `micros + 1000 * millis + 0x1_0000_0000 * 1000 * millis_overflow`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TpcanTimestamp {
    /// Milliseconds base value (0..2^32-1).
    millis: u32,
    /// Roll-arounds of `millis`.
    millis_overflow: u16,
    /// Microseconds part (0..999).
    micros: u16,
}

impl TpcanTimestamp {
    /// Total microseconds since device initialization.
    fn to_micros(&self) -> u64 {
        let micros = self.micros as u64;
        let millis = self.millis as u64;
        let overflow = self.millis_overflow as u64;
        micros + 1_000 * millis + 0x1_0000_0000 * 1_000 * overflow
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TpcanChannelInformation {
    channel_handle: u16,
    device_type: u8,
    controller_number: u8,
    device_features: u32,
    device_name: [i8; 33],
    device_id: u32,
    channel_condition: u32,
}

impl Default for TpcanChannelInformation {
    fn default() -> Self {
        Self {
            channel_handle: 0,
            device_type: 0,
            controller_number: 0,
            device_features: 0,
            device_name: [0; 33],
            device_id: 0,
            channel_condition: 0,
        }
    }
}

impl TpcanChannelInformation {
    fn device_name_string(&self) -> String {
        let bytes: Vec<u8> = self
            .device_name
            .iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c as u8)
            .collect();
        let name = String::from_utf8_lossy(&bytes).trim().to_string();
        if name.is_empty() {
            "PCAN".to_string()
        } else {
            name
        }
    }
}

type CanInitializeFn = unsafe extern "system" fn(u16, u16, u8, u32, u16) -> u32;
type CanInitializeFdFn = unsafe extern "system" fn(u16, *const i8) -> u32;
type CanUninitializeFn = unsafe extern "system" fn(u16) -> u32;
type CanReadFn = unsafe extern "system" fn(u16, *mut TpcanMsg, *mut TpcanTimestamp) -> u32;
type CanReadFdFn = unsafe extern "system" fn(u16, *mut TpcanMsgFd, *mut u64) -> u32;
type CanWriteFn = unsafe extern "system" fn(u16, *const TpcanMsg) -> u32;
type CanWriteFdFn = unsafe extern "system" fn(u16, *const TpcanMsgFd) -> u32;
type CanSetValueFn = unsafe extern "system" fn(u16, u8, *mut c_void, u32) -> u32;
type CanGetValueFn = unsafe extern "system" fn(u16, u8, *mut c_void, u32) -> u32;
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
    get_value: CanGetValueFn,
    get_error_text: CanGetErrorTextFn,
    look_up_channel: CanLookUpChannelFn,
}

impl PcanApi {
    /// Load `PCANBasic.dll` and resolve the required entry points.
    fn load() -> Result<Self, CanDeviceError> {
        let lib = load_pcan_library()?;

        unsafe {
            let initialize = resolve::<CanInitializeFn>(&lib, b"CAN_Initialize\0")?;
            let uninitialize = resolve::<CanUninitializeFn>(&lib, b"CAN_Uninitialize\0")?;
            let read = resolve::<CanReadFn>(&lib, b"CAN_Read\0")?;
            let write = resolve::<CanWriteFn>(&lib, b"CAN_Write\0")?;
            let set_value = resolve::<CanSetValueFn>(&lib, b"CAN_SetValue\0")?;
            let get_value = resolve::<CanGetValueFn>(&lib, b"CAN_GetValue\0")?;
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
                get_value,
                get_error_text,
                look_up_channel,
            })
        }
    }

    /// Human readable text for a PCAN status code.
    fn error_text(&self, status: u32) -> String {
        let mut buf = [0i8; 256];
        let res = unsafe { (self.get_error_text)(status, PCAN_LANGUAGE_ENGLISH, buf.as_mut_ptr()) };
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

    /// Initialize a channel in classic CAN mode.
    fn initialize_classic(&self, handle: u16, baud: u16) -> Result<(), String> {
        let status = unsafe { (self.initialize)(handle, baud, 0, 0, 0) };
        if status == PCAN_ERROR_OK {
            Ok(())
        } else {
            Err(self.error_text(status))
        }
    }

    /// Initialize a channel in CAN-FD mode.
    fn initialize_fd(&self, handle: u16, bitrate: &str) -> Result<(), String> {
        let initialize_fd = self
            .initialize_fd
            .ok_or_else(|| "CAN-FD not supported by this PCANBasic version".to_string())?;
        let mut c_string = bitrate.as_bytes().to_vec();
        c_string.push(0);
        let status = unsafe { initialize_fd(handle, c_string.as_ptr() as *const i8) };
        if status == PCAN_ERROR_OK {
            Ok(())
        } else {
            // Leave the channel clean after a failed init.
            unsafe {
                (self.uninitialize)(handle);
            }
            Err(self.error_text(status))
        }
    }

    /// Release a channel.
    fn uninitialize(&self, handle: u16) -> Result<(), String> {
        let status = unsafe { (self.uninitialize)(handle) };
        if status == PCAN_ERROR_OK {
            Ok(())
        } else {
            Err(self.error_text(status))
        }
    }

    /// Read one frame. Returns `Ok(None)` when the queue is empty. Error/status
    /// frames are returned with an empty payload (callers ignore them).
    fn read_frame(&self, handle: u16, fd: bool) -> Result<Option<CanFrame>, String> {
        unsafe {
            if fd {
                let read_fd = self
                    .read_fd
                    .ok_or_else(|| "CAN-FD not supported by this PCANBasic version".to_string())?;
                let mut msg = TpcanMsgFd::default();
                let mut ts: u64 = 0;
                let status = read_fd(handle, &mut msg, &mut ts);
                if status == PCAN_ERROR_OK {
                    let extended = msg.msg_type & PCAN_MESSAGE_EXTENDED != 0;
                    let timestamp_us = Some(ts);
                    if msg.msg_type & (PCAN_MESSAGE_STATUS | PCAN_MESSAGE_ERRFRAME) != 0 {
                        return Ok(Some(CanFrame {
                            id: msg.id,
                            extended,
                            data: Vec::new(),
                            timestamp_us,
                        }));
                    }
                    let len = super::device::dlc_to_len(msg.dlc).min(msg.data.len());
                    Ok(Some(CanFrame {
                        id: msg.id,
                        extended,
                        data: msg.data[..len].to_vec(),
                        timestamp_us,
                    }))
                } else if status == PCAN_ERROR_QRCVEMPTY {
                    Ok(None)
                } else {
                    Err(self.error_text(status))
                }
            } else {
                let mut msg = TpcanMsg::default();
                let mut ts = TpcanTimestamp::default();
                let status = (self.read)(handle, &mut msg, &mut ts);
                if status == PCAN_ERROR_OK {
                    let extended = msg.msg_type & PCAN_MESSAGE_EXTENDED != 0;
                    let timestamp_us = Some(ts.to_micros());
                    if msg.msg_type & (PCAN_MESSAGE_STATUS | PCAN_MESSAGE_ERRFRAME) != 0 {
                        return Ok(Some(CanFrame {
                            id: msg.id,
                            extended,
                            data: Vec::new(),
                            timestamp_us,
                        }));
                    }
                    let len = (msg.len as usize).min(msg.data.len());
                    Ok(Some(CanFrame {
                        id: msg.id,
                        extended,
                        data: msg.data[..len].to_vec(),
                        timestamp_us,
                    }))
                } else if status == PCAN_ERROR_QRCVEMPTY {
                    Ok(None)
                } else {
                    Err(self.error_text(status))
                }
            }
        }
    }

    /// Transmit one frame.
    fn write_frame(&self, handle: u16, frame: &CanFrame, fd: bool) -> Result<(), String> {
        if frame.data.len() > MAX_CAN_DATA {
            return Err(format!("frame of {} bytes exceeds CAN limit", frame.data.len()));
        }

        let id_type = if frame.extended {
            PCAN_MESSAGE_EXTENDED
        } else {
            PCAN_MESSAGE_STANDARD
        };

        unsafe {
            if fd {
                let write_fd = self
                    .write_fd
                    .ok_or_else(|| "CAN-FD not supported by this PCANBasic version".to_string())?;
                let mut msg = TpcanMsgFd::default();
                msg.id = frame.id;
                msg.msg_type = id_type | PCAN_MESSAGE_FD;
                msg.dlc = len_to_dlc(frame.data.len());
                msg.data[..frame.data.len()].copy_from_slice(&frame.data);
                let status = write_fd(handle, &msg);
                if status == PCAN_ERROR_OK {
                    Ok(())
                } else {
                    Err(self.error_text(status))
                }
            } else {
                if frame.data.len() > 8 {
                    return Err(format!(
                        "frame of {} bytes does not fit in a classic CAN frame",
                        frame.data.len()
                    ));
                }
                let mut msg = TpcanMsg::default();
                msg.id = frame.id;
                msg.msg_type = id_type;
                msg.len = frame.data.len() as u8;
                msg.data[..frame.data.len()].copy_from_slice(&frame.data);
                let status = (self.write)(handle, &msg);
                if status == PCAN_ERROR_OK {
                    Ok(())
                } else {
                    Err(self.error_text(status))
                }
            }
        }
    }

    /// Enumerate attached PCAN channels.
    fn attached_channels(&self) -> Result<Vec<TpcanChannelInformation>, String> {
        let mut count: u32 = 0;
        let status = unsafe {
            (self.get_value)(
                PCAN_NONEBUS,
                PCAN_ATTACHED_CHANNELS_COUNT,
                &mut count as *mut u32 as *mut c_void,
                std::mem::size_of::<u32>() as u32,
            )
        };
        if status != PCAN_ERROR_OK {
            return Err(self.error_text(status));
        }
        if count == 0 {
            return Ok(Vec::new());
        }

        let mut channels = vec![TpcanChannelInformation::default(); count as usize];
        let size = (count as usize * std::mem::size_of::<TpcanChannelInformation>()) as u32;
        let status = unsafe {
            (self.get_value)(
                PCAN_NONEBUS,
                PCAN_ATTACHED_CHANNELS,
                channels.as_mut_ptr() as *mut c_void,
                size,
            )
        };
        if status != PCAN_ERROR_OK {
            return Err(self.error_text(status));
        }
        Ok(channels)
    }

    /// Look up a channel matching a parameter string (e.g. `deviceid=3`).
    fn lookup_channel(&self, params: &str) -> Option<u16> {
        let mut c_string = params.as_bytes().to_vec();
        c_string.push(0);
        let mut found: u16 = 0;
        let status = unsafe { (self.look_up_channel)(c_string.as_ptr() as *const i8, &mut found) };
        if status == PCAN_ERROR_OK && found != 0 {
            Some(found)
        } else {
            None
        }
    }

    /// Look up the first attached channel of any supported family.
    fn lookup_attached_channel(&self) -> Option<u16> {
        for params in [
            "devicetype=PCAN_USB",
            "devicetype=PCAN_PCI",
            "devicetype=PCAN_LAN",
        ] {
            if let Some(found) = self.lookup_channel(params) {
                return Some(found);
            }
        }
        None
    }

    /// Read the PCAN device number (`PCAN_DEVICE_ID`) for a channel.
    fn get_device_id(&self, handle: u16) -> Option<u32> {
        let mut value: u32 = 0;
        let status = unsafe {
            (self.get_value)(
                handle,
                PCAN_DEVICE_ID,
                &mut value as *mut u32 as *mut c_void,
                std::mem::size_of::<u32>() as u32,
            )
        };
        if status == PCAN_ERROR_OK {
            Some(value)
        } else {
            None
        }
    }

    /// Turn a PCAN parameter off (best effort).
    fn set_param_off(&self, handle: u16, parameter: u8) {
        let mut value: u32 = PCAN_PARAMETER_OFF;
        let status = unsafe {
            (self.set_value)(
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
                self.error_text(status)
            );
        }
    }
}

unsafe fn resolve<T>(lib: &Library, name: &[u8]) -> Result<T, CanDeviceError>
where
    T: Copy,
{
    let symbol: Symbol<T> = lib.get(name).map_err(|e| {
        CanDeviceError::Driver(format!(
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
fn load_pcan_library() -> Result<Library, CanDeviceError> {
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

    Err(CanDeviceError::Driver(format!(
        "Unable to load PCANBasic.dll ({}). Set PCAN_BASIC_DLL to the DLL path.",
        last_err.unwrap_or_else(|| "not found".to_string())
    )))
}

// ---------------------------------------------------------------------------
// Channel handle <-> name
// ---------------------------------------------------------------------------

/// Convert a PCAN handle into a stable name like `PCAN_USBBUS1`.
fn channel_handle_name(handle: u16) -> Option<String> {
    fn bus(prefix: &str, base: u16, high_base: u16, handle: u16) -> Option<String> {
        if (base..base + 8).contains(&handle) {
            return Some(format!("{}{}", prefix, handle - base + 1));
        }
        if (high_base..high_base + 8).contains(&handle) {
            return Some(format!("{}{}", prefix, handle - high_base + 9));
        }
        None
    }

    bus("PCAN_PCIBUS", PCAN_PCIBUS1, 0x409, handle)
        .or_else(|| bus("PCAN_USBBUS", PCAN_USBBUS1, 0x509, handle))
        .or_else(|| bus("PCAN_LANBUS", PCAN_LANBUS1, 0x809, handle))
}

/// Parse a channel name/handle string into a PCAN handle.
///
/// `auto`/empty yield `None` so the caller can auto-select a channel.
fn parse_channel_handle(channel: &str) -> Result<Option<u16>, CanDeviceError> {
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

    let norm = upper.replace(' ', "_");
    for (prefixes, base, high_base) in [
        (["PCAN_PCIBUS", "PCIBUS", "PCI"], PCAN_PCIBUS1, 0x409u16),
        (["PCAN_USBBUS", "USBBUS", "USB"], PCAN_USBBUS1, 0x509u16),
        (["PCAN_LANBUS", "LANBUS", "LAN"], PCAN_LANBUS1, 0x809u16),
    ] {
        for prefix in prefixes {
            if let Some(num) = norm.strip_prefix(prefix) {
                if let Ok(n) = num.parse::<u16>() {
                    if (1..=8).contains(&n) {
                        return Ok(Some(base + n - 1));
                    }
                    if (9..=16).contains(&n) {
                        return Ok(Some(high_base + n - 9));
                    }
                }
            }
        }
    }

    Err(CanDeviceError::UnknownChannel(format!(
        "Unknown PCAN channel '{}'. Use a name like PCAN_USBBUS1 or a handle like 0x51.",
        channel
    )))
}

fn pcan_device_type_name(device_type: u8) -> String {
    match device_type {
        PCAN_DEVICE_PCI => "PCAN-PCI",
        PCAN_DEVICE_USB => "PCAN-USB",
        PCAN_DEVICE_LAN => "PCAN-LAN",
        _ => "PCAN",
    }
    .to_string()
}

fn channel_info_from(channel: &TpcanChannelInformation) -> CanDeviceInfo {
    CanDeviceInfo {
        driver: "pcan".to_string(),
        device_id: channel.device_id.to_string(),
        name: channel.device_name_string(),
        device_type: pcan_device_type_name(channel.device_type),
        controller: channel.controller_number,
        channel: channel_handle_name(channel.channel_handle)
            .unwrap_or_else(|| format!("0x{:X}", channel.channel_handle)),
        available: channel.channel_condition & PCAN_CHANNEL_AVAILABLE != 0,
        fd_capable: channel.device_features & FEATURE_FD_CAPABLE != 0,
    }
}

/// Resolve the PCAN handle to open from a device config.
///
/// Priority: explicit `channel` name/handle → stable `device_id` + controller →
/// first available attached channel.
fn resolve_handle(api: &PcanApi, config: &CanDeviceConfig) -> Result<u16, CanDeviceError> {
    if let Some(handle) = parse_channel_handle(&config.channel)? {
        return Ok(handle);
    }

    if let Some(device_id) = config
        .device_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let controller = config.controller.unwrap_or(0);
        let params = format!("deviceid={},controllernumber={}", device_id, controller);
        return api.lookup_channel(&params).ok_or_else(|| {
            CanDeviceError::UnknownChannel(format!(
                "PCAN device id {} controller {} not found",
                device_id, controller
            ))
        });
    }

    api.attached_channels()
        .unwrap_or_default()
        .iter()
        .find(|c| c.channel_condition & PCAN_CHANNEL_AVAILABLE != 0)
        .map(|c| c.channel_handle)
        .or_else(|| api.lookup_attached_channel())
        .ok_or_else(|| CanDeviceError::NoChannel("no attached PCAN channel found".to_string()))
}

// ---------------------------------------------------------------------------
// Bit rate handling
// ---------------------------------------------------------------------------

/// Map a classic CAN bit rate (bit/s) to a PCAN BTR0/BTR1 timing code.
fn baud_bps_to_code(bps: u32) -> Option<u16> {
    Some(match bps {
        1_000_000 => PCAN_BAUD_1M,
        800_000 => PCAN_BAUD_800K,
        500_000 => PCAN_BAUD_500K,
        250_000 => PCAN_BAUD_250K,
        125_000 => PCAN_BAUD_125K,
        100_000 => PCAN_BAUD_100K,
        95_238 | 95_000 => PCAN_BAUD_95K,
        83_333 | 83_000 => PCAN_BAUD_83K,
        50_000 => PCAN_BAUD_50K,
        47_619 | 47_000 => PCAN_BAUD_47K,
        33_333 | 33_000 => PCAN_BAUD_33K,
        20_000 => PCAN_BAUD_20K,
        10_000 => PCAN_BAUD_10K,
        5_000 => PCAN_BAUD_5K,
        _ => return None,
    })
}

/// Derive CAN bit timing registers from a bit rate and controller clock.
///
/// Returns `(brp, tseg1, tseg2, sjw)` with a sample point near 80%.
fn derive_timing(clock: u64, bitrate: u32) -> Option<(u32, u32, u32, u32)> {
    if bitrate == 0 {
        return None;
    }

    const TARGET_TOTAL_TQ: u64 = 20;
    let ideal_brp = (clock as f64 / (bitrate as f64 * TARGET_TOTAL_TQ as f64)).round() as u64;
    let start = ideal_brp.max(1);

    // Search outwards from the ideal prescaler for an exact divisor.
    for delta in 0u64..64 {
        let candidates = if delta == 0 {
            vec![start]
        } else {
            vec![start + delta, start.saturating_sub(delta)]
        };

        for brp in candidates {
            if brp == 0 {
                continue;
            }
            let denom = brp * bitrate as u64;
            if denom == 0 || clock % denom != 0 {
                continue;
            }
            let total = clock / denom; // total time quanta including sync segment
            if !(8..=385).contains(&total) {
                continue;
            }
            let tseg2 = ((total as f64 * 0.2).round() as u64).clamp(1, 128);
            let tseg1 = total.saturating_sub(1 + tseg2);
            if tseg1 < 1 {
                continue;
            }
            let sjw = tseg2.min(16);
            return Some((brp as u32, tseg1 as u32, tseg2 as u32, sjw as u32));
        }
    }
    None
}

/// Build a PCAN-Basic CAN-FD bit rate string from nominal/data bit rates.
///
/// The controller clock defaults to 80 MHz and can be overridden with the
/// `PCAN_FD_CLOCK` environment variable.
fn build_fd_bitrate_string(nominal_bps: u32, data_bps: u32) -> Result<String, CanDeviceError> {
    let clock = std::env::var(PCAN_FD_CLOCK_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_FD_CLOCK_HZ);

    let (nom_brp, nom_tseg1, nom_tseg2, nom_sjw) =
        derive_timing(clock, nominal_bps).ok_or_else(|| {
            CanDeviceError::Unsupported(format!(
                "cannot derive CAN-FD nominal timing for {} bit/s (clock {} Hz)",
                nominal_bps, clock
            ))
        })?;
    let (data_brp, data_tseg1, data_tseg2, data_sjw) =
        derive_timing(clock, data_bps).ok_or_else(|| {
            CanDeviceError::Unsupported(format!(
                "cannot derive CAN-FD data timing for {} bit/s (clock {} Hz)",
                data_bps, clock
            ))
        })?;

    Ok(format!(
        "f_clock={clock},nom_brp={nom_brp},nom_tseg1={nom_tseg1},nom_tseg2={nom_tseg2},\
         nom_sjw={nom_sjw},data_brp={data_brp},data_tseg1={data_tseg1},data_tseg2={data_tseg2},\
         data_sjw={data_sjw}"
    ))
}

/// Parse the `PCAN_BITRATE` environment variable.
///
/// Returns `(classic_baud, fd_bitrate)` where exactly one is `Some`.
fn parse_bitrate(value: &str) -> Result<(Option<u16>, Option<String>), CanDeviceError> {
    let v = value.trim();
    if v.is_empty() {
        return Err(CanDeviceError::Unsupported("empty bit rate".to_string()));
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
        None => Err(CanDeviceError::Unsupported(format!(
            "invalid PCAN bit rate '{}'",
            value
        ))),
    }
}

/// Bit rate selection derived from configuration/environment.
#[derive(Debug, Clone, PartialEq, Eq)]
enum BitrateChoice {
    /// Use classic CAN at the given BTR0/BTR1 code.
    Classic(u16),
    /// Use CAN-FD with the given bit rate string.
    Fd(String),
}

/// Decide how to initialize the channel from explicit config, falling back to
/// the `PCAN_BITRATE` environment variable and finally classic 500k.
fn resolve_bitrate(config: &CanDeviceConfig) -> Result<BitrateChoice, CanDeviceError> {
    // 1. Explicit CAN-FD config (data-phase bit rate present).
    if let Some(data_bps) = config.data_baudrate {
        let nominal = config.baudrate.unwrap_or(PCAN_BAUD_500K_BPS);
        return Ok(BitrateChoice::Fd(build_fd_bitrate_string(nominal, data_bps)?));
    }

    // 2. Explicit classic config.
    if let Some(bps) = config.baudrate {
        let code = baud_bps_to_code(bps).ok_or_else(|| {
            CanDeviceError::Unsupported(format!("unsupported classic CAN bit rate {} bit/s", bps))
        })?;
        return Ok(BitrateChoice::Classic(code));
    }

    // 3. Fall back to the PCAN_BITRATE environment variable.
    if let Ok(val) = std::env::var(PCAN_BITRATE_ENV) {
        if !val.trim().is_empty() {
            let (classic, fd) = parse_bitrate(&val)?;
            return Ok(if let Some(fd_str) = fd {
                BitrateChoice::Fd(fd_str)
            } else {
                BitrateChoice::Classic(classic.unwrap_or(PCAN_BAUD_500K))
            });
        }
    }

    // 4. Nothing specified: default to classic 500k (no FD probing).
    Ok(BitrateChoice::Classic(PCAN_BAUD_500K))
}

// ---------------------------------------------------------------------------
// CanDriver / CanDevice
// ---------------------------------------------------------------------------

/// PEAK-System PCAN-Basic driver.
pub struct PcanDriver;

impl PcanDriver {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PcanDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl CanDriver for PcanDriver {
    fn name(&self) -> &'static str {
        "pcan"
    }

    fn description(&self) -> &'static str {
        "PEAK-System PCAN-Basic"
    }

    fn list_channels(&self) -> Vec<CanDeviceInfo> {
        let api = match PcanApi::load() {
            Ok(api) => api,
            Err(e) => {
                tracing::debug!("PCAN driver unavailable: {}", e);
                return Vec::new();
            }
        };

        match api.attached_channels() {
            Ok(channels) => channels.iter().map(channel_info_from).collect(),
            Err(e) => {
                tracing::warn!("PCAN channel enumeration failed: {}", e);
                Vec::new()
            }
        }
    }

    fn open(&self, config: &CanDeviceConfig) -> Result<Box<dyn CanDevice>, CanDeviceError> {
        let api = Arc::new(PcanApi::load()?);

        let handle = resolve_handle(&api, config)?;

        let choice = resolve_bitrate(config)?;
        let mut fd = false;
        let bitrate = match &choice {
            BitrateChoice::Classic(code) => {
                api.initialize_classic(handle, *code)
                    .map_err(CanDeviceError::OpenFailed)?;
                format!("{} bit/s", code_to_bps(*code).unwrap_or(0))
            }
            BitrateChoice::Fd(fd_str) => {
                api.initialize_fd(handle, fd_str)
                    .map_err(CanDeviceError::OpenFailed)?;
                fd = true;
                describe_fd_bitrate(config)
            }
        };

        // Disable echo so our own transmitted frames are not routed back up.
        api.set_param_off(handle, PCAN_ALLOW_ECHO_FRAMES);

        // Prefer the metadata reported by the driver for this exact handle;
        // fall back to a synthesized entry when enumeration is unavailable.
        let attached = api.attached_channels().unwrap_or_default();
        let mut info = attached
            .iter()
            .find(|c| c.channel_handle == handle)
            .map(channel_info_from)
            .unwrap_or_else(|| CanDeviceInfo {
                driver: "pcan".to_string(),
                device_id: api
                    .get_device_id(handle)
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| format!("0x{:X}", handle)),
                name: "PCAN".to_string(),
                device_type: channel_handle_name(handle)
                    .map(|name| {
                        if name.contains("USB") {
                            "PCAN-USB"
                        } else if name.contains("PCI") {
                            "PCAN-PCI"
                        } else if name.contains("LAN") {
                            "PCAN-LAN"
                        } else {
                            "PCAN"
                        }
                        .to_string()
                    })
                    .unwrap_or_else(|| "PCAN".to_string()),
                controller: 0,
                channel: channel_handle_name(handle).unwrap_or_else(|| format!("0x{:X}", handle)),
                available: true,
                fd_capable: fd,
            });
        // We just opened the channel, so it is available and may now be FD.
        info.available = true;
        info.fd_capable = info.fd_capable || fd;

        tracing::info!(
            "Opened PCAN {} device {} channel {} ({}, {})",
            info.device_type,
            info.device_id,
            info.channel,
            info.controller,
            bitrate
        );

        Ok(Box::new(PcanDevice {
            api,
            handle,
            fd,
            info,
            bitrate,
            closed: AtomicBool::new(false),
        }))
    }
}

/// One opened PCAN channel.
pub struct PcanDevice {
    api: Arc<PcanApi>,
    handle: u16,
    fd: bool,
    info: CanDeviceInfo,
    bitrate: String,
    closed: AtomicBool,
}

impl CanDevice for PcanDevice {
    fn send_frame(&self, frame: &CanFrame) -> Result<(), CanDeviceError> {
        self.api
            .write_frame(self.handle, frame, self.fd)
            .map_err(CanDeviceError::SendFailed)
    }

    fn recv_frame(&self, timeout: Option<Duration>) -> Result<Option<CanFrame>, CanDeviceError> {
        match timeout {
            None => self
                .api
                .read_frame(self.handle, self.fd)
                .map_err(CanDeviceError::RecvFailed),
            Some(limit) => {
                let start = Instant::now();
                loop {
                    match self.api.read_frame(self.handle, self.fd) {
                        Ok(Some(frame)) => return Ok(Some(frame)),
                        Ok(None) => {
                            if start.elapsed() >= limit {
                                return Ok(None);
                            }
                            std::thread::sleep(Duration::from_micros(200));
                        }
                        Err(e) => return Err(CanDeviceError::RecvFailed(e)),
                    }
                }
            }
        }
    }

    fn close(&mut self) -> Result<(), CanDeviceError> {
        if self.closed.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        self.api
            .uninitialize(self.handle)
            .map_err(CanDeviceError::Driver)
    }

    fn info(&self) -> CanDeviceInfo {
        self.info.clone()
    }

    fn is_fd(&self) -> bool {
        self.fd
    }

    fn bitrate_description(&self) -> String {
        self.bitrate.clone()
    }
}

impl Drop for PcanDevice {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

/// Map a classic BTR0/BTR1 code back to a bit rate (for display only).
fn code_to_bps(code: u16) -> Option<u32> {
    Some(match code {
        PCAN_BAUD_1M => 1_000_000,
        PCAN_BAUD_800K => 800_000,
        PCAN_BAUD_500K => 500_000,
        PCAN_BAUD_250K => 250_000,
        PCAN_BAUD_125K => 125_000,
        PCAN_BAUD_100K => 100_000,
        PCAN_BAUD_95K => 95_238,
        PCAN_BAUD_83K => 83_333,
        PCAN_BAUD_50K => 50_000,
        PCAN_BAUD_47K => 47_619,
        PCAN_BAUD_33K => 33_333,
        PCAN_BAUD_20K => 20_000,
        PCAN_BAUD_10K => 10_000,
        PCAN_BAUD_5K => 5_000,
        _ => return None,
    })
}

fn describe_fd_bitrate(config: &CanDeviceConfig) -> String {
    let nominal = config.baudrate.unwrap_or(PCAN_BAUD_500K_BPS) / 1000;
    let data = config.data_baudrate.unwrap_or(0) / 1000;
    format!("CAN-FD {}k/{}k", nominal, data)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(parse_channel_handle("USB2").unwrap(), Some(0x52));
        assert_eq!(parse_channel_handle("PCAN_USBBUS9").unwrap(), Some(0x509));
        assert!(parse_channel_handle("nonsense").is_err());
    }

    #[test]
    fn test_channel_handle_name() {
        assert_eq!(channel_handle_name(0x51).unwrap(), "PCAN_USBBUS1");
        assert_eq!(channel_handle_name(0x52).unwrap(), "PCAN_USBBUS2");
        assert_eq!(channel_handle_name(0x509).unwrap(), "PCAN_USBBUS9");
        assert_eq!(channel_handle_name(0x41).unwrap(), "PCAN_PCIBUS1");
        assert_eq!(channel_handle_name(0x801).unwrap(), "PCAN_LANBUS1");
        assert_eq!(channel_handle_name(0x99), None);
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
        assert_eq!(
            expand_dll_candidate(r"E:\pc\x64\PCANBasic.dll"),
            vec![r"E:\pc\x64\PCANBasic.dll".to_string()]
        );
        let c = expand_dll_candidate(r"E:\nonexistent\dir");
        assert_eq!(c.len(), 2);
        assert!(c[1].ends_with("PCANBasic.dll"));
    }

    #[test]
    fn test_baud_bps_to_code() {
        assert_eq!(baud_bps_to_code(1_000_000), Some(PCAN_BAUD_1M));
        assert_eq!(baud_bps_to_code(500_000), Some(PCAN_BAUD_500K));
        assert_eq!(baud_bps_to_code(250_000), Some(PCAN_BAUD_250K));
        assert_eq!(baud_bps_to_code(125_000), Some(PCAN_BAUD_125K));
        assert_eq!(baud_bps_to_code(100_000), Some(PCAN_BAUD_100K));
        assert_eq!(baud_bps_to_code(123_456), None);
    }

    #[test]
    fn test_derive_timing_exact() {
        // 80 MHz / (20 * 500k) = brp 8, total 20 tq.
        let (brp, tseg1, tseg2, sjw) = derive_timing(80_000_000, 500_000).unwrap();
        assert_eq!(brp, 8);
        assert_eq!(brp as u64 * 500_000 * (1 + tseg1 + tseg2) as u64, 80_000_000);
        assert!(tseg1 >= 1 && tseg2 >= 1 && sjw >= 1);
        assert_eq!(sjw, tseg2.min(16));
    }

    #[test]
    fn test_derive_timing_rejects_zero() {
        assert_eq!(derive_timing(80_000_000, 0), None);
    }

    #[test]
    fn test_build_fd_bitrate_string() {
        let s = build_fd_bitrate_string(500_000, 2_000_000).unwrap();
        for key in [
            "f_clock=80000000",
            "nom_brp=",
            "nom_tseg1=",
            "nom_tseg2=",
            "nom_sjw=",
            "data_brp=",
            "data_tseg1=",
            "data_tseg2=",
            "data_sjw=",
        ] {
            assert!(s.contains(key), "missing {} in {}", key, s);
        }
        assert!(!s.contains(' '), "unexpected whitespace in {}", s);
    }

    #[test]
    fn test_msg_struct_sizes_match_pcan() {
        // PCAN requires exact layouts: TPCANMsg = 16, TPCANMsgFD = 72,
        // TPCANTimestamp = 8, TPCANChannelInformation = 52 bytes.
        assert_eq!(std::mem::size_of::<TpcanMsg>(), 16);
        assert_eq!(std::mem::size_of::<TpcanMsgFd>(), 72);
        assert_eq!(std::mem::size_of::<TpcanTimestamp>(), 8);
        assert_eq!(std::mem::size_of::<TpcanChannelInformation>(), 52);
    }

    #[test]
    fn test_tpcan_timestamp_to_micros() {
        // Microseconds only.
        let ts = TpcanTimestamp {
            millis: 0,
            millis_overflow: 0,
            micros: 42,
        };
        assert_eq!(ts.to_micros(), 42);

        // Milliseconds + microseconds.
        let ts = TpcanTimestamp {
            millis: 3,
            millis_overflow: 0,
            micros: 7,
        };
        assert_eq!(ts.to_micros(), 3_007);

        // One roll-around of millis (2^32 ms) + 1 ms.
        let ts = TpcanTimestamp {
            millis: 1,
            millis_overflow: 1,
            micros: 0,
        };
        assert_eq!(ts.to_micros(), 0x1_0000_0000 * 1_000 + 1_000);
    }
}
