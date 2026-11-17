//! ETAS ES582 CAN/CAN-FD device driver (BOA V2 / OCI).
//!
//! This module talks to ETAS interface hardware through the **Basic OpenAPI
//! (BOA)** runtime that ships with *EBI-IP* / *INCA*: the `dll-csiBind.dll`
//! search-for-service layer (`CSI_*`) and the `dll-ocdProxy.dll` Open
//! Controller Interface (`OCI_*`). The hardware itself is reached through an
//! *Open Controller Driver* plugin (`ES582_1_OCD_CAN.dll`), which BOA loads
//! from its `OCDs` directory.
//!
//! Everything is loaded dynamically with [`libloading`] (no import library, no
//! build-time dependency on the ETAS SDK), mirroring [`super::pcan`]:
//!
//! 1. [`BoaApi::load`] loads `dll-ocdProxy.dll` + `dll-csiBind.dll`.
//! 2. [`Es582Driver::list_channels`] creates a CSI protocol tree, asks it for
//!    every node offering `UUID_OCICAN` and turns each URI into a
//!    [`CanDeviceInfo`].
//! 3. [`Es582Driver::open`] binds an OCI CAN controller to that URI, configures
//!    the bit rate, creates a Tx/Rx queue with an accept-all frame filter and
//!    returns an [`Es582Device`] implementing [`CanDevice`].
//!
//! The XCP framing itself (one XCP packet per CAN frame, PID filtering,
//! `MAX_DLC_REQUIRED` padding) lives in the CAN link module ([`super::open`]); this module is
//! pure hardware access.
//!
//! Notes on the FFI: the OCI structures are versioned and grow over time, so
//! instead of mirroring every `#[repr(C)]` layout we allocate zeroed buffers and
//! write only the documented field offsets (see [`cfg`], [`cap`], [`msg`]). That
//! keeps the driver compatible with future BOA releases without recompiling.
//!
//! Receiving uses the OCI `onFrame` callback (see [`RxContext`]): BOA pushes
//! every frame that passes the hardware filters from its own receive thread, so
//! `recv_frame` only drains a local queue. Polling (`OCI_ReadCANDataEx`) costs a
//! USB round trip per frame and is only kept as `ES582_RX_MODE=poll` for
//! comparison.
//!
//! Environment variables:
//! - `BOA_DLL_DIR` — directory holding `dll-ocdProxy.dll` / `dll-csiBind.dll`
//!   (defaults to `%ProgramFiles%\ETAS\BOA_V2\Bin\x64\Dll\Framework`).
//! - `BOA_OCD_DIR` — OCD plugin directory added to the DLL search path
//!   (defaults to `%ProgramFiles%\ETAS\ED-EBI_V2\Bin\x64\Dll\OCDs`).
//! - `ES582_URI` — channel URI to use when the config does not pin one.
//! - `ES582_RX_MODE` — `callback` (default) or `poll` (diagnostics only).
//! - `ES582_SELF_RECEPTION`, `ES582_*_BTL_CYCLES`, `ES582_*_SAMPLE_POINT`,
//!   `ES582_*_SJW` — bring-up/timing overrides (see [`CanTiming`]).

use super::device::{
    CanDevice, CanDeviceConfig, CanDeviceError, CanDeviceInfo, CanDriver, CanFrame,
};
use libloading::{Library, Symbol};
use std::collections::VecDeque;
use std::ffi::{c_char, c_void, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// BOA / OCI constants (see Include/Common/boaerror.h, Include/OCI/*.h)
// ---------------------------------------------------------------------------

/// `BOA_RESULT_FLAG_ERROR` — bit 31 marks a failed call.
const BOA_RESULT_FLAG_ERROR: u32 = 0x8000_0000;

const BOA_ERR_PROTOCOL_VERSION_NOT_SUPPORTED: u32 = 0x8000_2000;
const BOA_ERR_NO_INTERFACE: u32 = 0x8000_2004;
const BOA_ERR_ALREADY_CONNECTED: u32 = 0x8000_1007;

/// `OCI_NO_HANDLE`
const OCI_NO_HANDLE: i32 = -1;
/// Transmit timeout: queue the frame without waiting for it to leave the
/// controller.
///
/// `OCI_WriteCANData*` blocks while the Tx queue is full; with `OCI_NO_TIME` a
/// bus that stops acknowledging frames would block the caller forever. `0`
/// queues without waiting for transmission (XCP does not need the completion
/// anyway) and reports a full queue as an error instead of hanging.
const TX_TIMEOUT: i64 = 0;

/// `UUID_OCICAN` from `OCI/ocican.h`.
const UUID_OCICAN: [u8; 16] = [
    0xA6, 0xCF, 0x48, 0x01, 0x13, 0x8F, 0x44, 0x3d, 0x9C, 0x68, 0xAD, 0x5D, 0xE3, 0x3A, 0xBD, 0xF9,
];

/// `CSI_NODE_MIN` / `CSI_NODE_MAX` — search the whole node tree.
const CSI_NODE_MIN: u32 = 0;
const CSI_NODE_MAX: u32 = 0x7FFF;

/// Root URI for a USB-only scan: ETAS CAN interfaces (ES582, ES581.4, …) hang
/// off it, and skipping the Ethernet/vNet discovery plugins makes the scan
/// much faster.
const CSI_USB_ROOT: &str = "ETAS://USB";

/// `OCI_CAN_MSG_FLAG_*`
const CAN_MSG_FLAG_EXTENDED: u16 = 1;
const CAN_MSG_FLAG_FD_DATA_BIT_RATE: u16 = 8;
const CAN_MSG_FLAG_FD_DATA: u16 = 64;

/// `OCI_CANMessageDataType`
const CAN_RX_MESSAGE: u32 = 1;
const CAN_TX_MESSAGE: u32 = 2;
const CAN_ERROR_FRAME: u32 = 3;
const CAN_BUS_EVENT: u32 = 4;
const CANFDRX_MESSAGE: u32 = 8;
const CANFDTX_MESSAGE: u32 = 9;

/// `OCI_CANFDSUPPORT`
const CANFD_SUPPORT: u32 = 1 << 0;

/// `OCI_CONTROLLER_MODE_RUNNING`
const CONTROLLER_MODE_RUNNING: u32 = 0;

/// `OCI_SELF_RECEPTION_OFF` / `OCI_SELF_RECEPTION_ON`.
const SELF_RECEPTION_OFF: u32 = 0;
const SELF_RECEPTION_ON: u32 = 1;

/// `OCI_CAN_ONE_SAMPLE_PER_BIT`, `OCI_CAN_SINGLE_SYNC_EDGE`,
/// `OCI_CAN_MEDIA_HIGH_SPEED`, `OCI_BUSMODE_ACTIVE`.
const CAN_ONE_SAMPLE_PER_BIT: u32 = 1;
const CAN_SINGLE_SYNC_EDGE: u32 = 1;
const CAN_MEDIA_HIGH_SPEED: u32 = 1;
const BUSMODE_ACTIVE: u32 = 2;

/// `OCI_CAN_RXMODE_CAN_FRAMES_USING_CAN_MESSAGE` / `OCI_CANFDRXMODE_CANFD_FRAMES_USING_CANFD_MESSAGE`
/// / `OCI_CANFDTX_USE_CAN_AND_CANFD_FRAMES` (pre-v1.6 CAN-FD configuration).
const CANFD_RX_CAN_FRAMES_USING_CAN_MESSAGE: u32 = 2;
const CANFD_RX_FRAMES_USING_CANFD_MESSAGE: u32 = 4;
const CANFD_TX_CAN_AND_CANFD_FRAMES: u32 = 4;

/// Default nominal bit rate when the configuration declares none.
const DEFAULT_BAUDRATE: u32 = 500_000;

/// Buffer size used for every OCI structure we hand to BOA. The real structs
/// are far smaller (112 bytes for `OCI_CANConfiguration`, 96 for
/// `OCI_CANMessageEx`); the slack keeps the driver working when a future BOA
/// release appends fields.
const STRUCT_BUF: usize = 512;

/// Messages fetched per `OCI_ReadCANData*` call in polling mode.
const RX_BATCH: usize = 16;

/// Timeout used by the polling receive path.
///
/// A zero-timeout OCI read costs one USB round trip (~18 ms on an ES582) and
/// returns a single frame, so polling with `timeout = 0` caps the driver at
/// ~55 frames/s and lets the OCD's receive queue overflow on a busy bus
/// (150 frames/s of periodic traffic is enough). A short blocking read returns
/// as soon as data is available.
///
/// The default receive path is the OCI callback (see [`RxContext`]); polling is
/// only a fallback for comparison (`ES582_RX_MODE=poll`).
const RX_POLL_TIMEOUT_MS: u64 = 10;

/// Maximum number of frames buffered between the OCI callback and
/// [`CanDevice::recv_frame`]. Beyond this the oldest frames are dropped (and
/// counted) so a stalled consumer cannot exhaust memory.
const RX_QUEUE_CAPACITY: usize = 200_000;

/// `sizeof(OCI_CANMessageEx)` — the buffer size the receive callback is given.
const CAN_MESSAGE_EX_SIZE: usize = 96;

/// How long a "non-blocking" `recv_frame(None)` waits for a frame.
///
/// The OCI callback already queues frames, so this only lets the transport's
/// reader thread sleep on the condition variable instead of spinning.
const RX_WAIT_SLICE: Duration = Duration::from_millis(1);

/// `sizeof(OCI_CANMessage)` — the stride used by `OCI_ReadCANData` (the
/// non-Ex entry point takes an array of structs, not of pointers).
const CAN_MESSAGE_SIZE: usize = 40;

/// `OCI_URIName` is a `char[256]`.
const URI_NAME_LEN: usize = 256;

/// `sizeof(OCI_CANRxFilter)` — `frameIDValue`, `frameIDMask`, `tag`.
const FILTER_SIZE: usize = 12;

/// Mask matching all 29 CAN identifier bits.
const CAN_ID_MASK: u32 = 0x1FFF_FFFF;

/// Known OCI_CAN API versions, newest first. BOA requires an exact version
/// match, so the driver walks this list until `OCI_CreateCANControllerVersion`
/// accepts one.
const API_VERSIONS: [BoaVersion; 7] = [
    BoaVersion {
        major: 1,
        minor: 6,
        bugfix: 0,
        build: 0,
    },
    BoaVersion {
        major: 1,
        minor: 5,
        bugfix: 0,
        build: 0,
    },
    BoaVersion {
        major: 1,
        minor: 4,
        bugfix: 0,
        build: 0,
    },
    BoaVersion {
        major: 1,
        minor: 3,
        bugfix: 0,
        build: 0,
    },
    BoaVersion {
        major: 1,
        minor: 2,
        bugfix: 0,
        build: 0,
    },
    BoaVersion {
        major: 1,
        minor: 1,
        bugfix: 0,
        build: 0,
    },
    BoaVersion {
        major: 1,
        minor: 0,
        bugfix: 0,
        build: 0,
    },
];

const BOA_DLL_DIR_ENV: &str = "BOA_DLL_DIR";
const BOA_OCD_DIR_ENV: &str = "BOA_OCD_DIR";
const ES582_URI_ENV: &str = "ES582_URI";
/// Set to `1` to receive self-transmitted frames (bring-up/loopback tests).
const ES582_SELF_RECEPTION_ENV: &str = "ES582_SELF_RECEPTION";

/// Receive path: `callback` (default, OCI `onFrame` callback) or `poll`
/// (`OCI_ReadCANDataEx`).
const ES582_RX_MODE_ENV: &str = "ES582_RX_MODE";

/// Bit timing overrides (nominal phase): `BTL_CYCLES`, `SAMPLE_POINT`, `SJW`.
const ES582_BTL_ENV: &str = "ES582_BTL_CYCLES";
const ES582_SAMPLE_POINT_ENV: &str = "ES582_SAMPLE_POINT";
const ES582_SJW_ENV: &str = "ES582_SJW";
/// Bit timing overrides (CAN-FD data phase).
const ES582_FD_BTL_ENV: &str = "ES582_FD_BTL_CYCLES";
const ES582_FD_SAMPLE_POINT_ENV: &str = "ES582_FD_SAMPLE_POINT";
const ES582_FD_SJW_ENV: &str = "ES582_FD_SJW";

/// Known framework DLL locations, tried after the environment override.
const BOA_DLL_FALLBACK_DIRS: &[&str] = &[
    r"C:\Program Files\ETAS\BOA_V2\Bin\x64\Dll\Framework",
    r"C:\Program Files (x86)\ETAS\BOA_V2\Bin\x64\Dll\Framework",
];

/// Known OCD plugin directories (contain `ES582_1_OCD_CAN.dll`).
const BOA_OCD_FALLBACK_DIRS: &[&str] = &[
    r"C:\Program Files\ETAS\ED-EBI_V2\Bin\x64\Dll\OCDs",
    r"C:\Program Files (x86)\ETAS\ED-EBI_V2\Bin\x64\Dll\OCDs",
    r"C:\Program Files\ETAS\BOA_V2\Bin\x64\Dll\OCDs",
];

// ---------------------------------------------------------------------------
// Field offsets of the OCI structures (see Include/OCI/ocican*.h)
// ---------------------------------------------------------------------------

/// Offsets inside `OCI_CANConfiguration` (verified against the C headers with
/// `offsetof` on MSVC x64: sizeof = 116).
#[allow(dead_code)] // documents the full layout; not every field is written
mod cfg {
    pub const BAUDRATE: usize = 0;
    pub const SAMPLE_POINT: usize = 4;
    pub const SAMPLES_PER_BIT: usize = 8;
    pub const BTL_CYCLES: usize = 12;
    pub const SJW: usize = 16;
    pub const SYNC_EDGE: usize = 20;
    pub const PHYSICAL_MEDIA: usize = 24;
    pub const SELF_RECEPTION_MODE: usize = 28;
    pub const BUS_PARTICIPATION_MODE: usize = 32;
    pub const CAN_FD_ENABLED: usize = 36;
    pub const CAN_FD_CONFIG: usize = 40;
    pub const CAN_TX_POLICY: usize = 80;
    pub const CAN_XL_ENABLED: usize = 84;
    pub const CAN_XL_CONFIG: usize = 88;
    pub const CAN_20_ENABLED: usize = 112;

    /// Offsets inside the nested `OCI_CANFDConfiguration` (relative to
    /// [`CAN_FD_CONFIG`]). `canFdRxConfig` is a struct of two enums, so
    /// `canFdTxConfig` sits at +32, not +28.
    pub const FD_DATA_BIT_RATE: usize = CAN_FD_CONFIG;
    pub const FD_DATA_SAMPLE_POINT: usize = CAN_FD_CONFIG + 4;
    pub const FD_DATA_BTL_CYCLES: usize = CAN_FD_CONFIG + 8;
    pub const FD_DATA_SJW: usize = CAN_FD_CONFIG + 12;
    pub const FD_FLAGS: usize = CAN_FD_CONFIG + 16;
    pub const FD_TX_SSP_OFFSET: usize = CAN_FD_CONFIG + 20;
    pub const FD_RX_CAN_MODE: usize = CAN_FD_CONFIG + 24;
    pub const FD_RX_FD_MODE: usize = CAN_FD_CONFIG + 28;
    pub const FD_TX_CONFIG: usize = CAN_FD_CONFIG + 32;
    pub const FD_TX_SSP_FILTER_WINDOW: usize = CAN_FD_CONFIG + 36;
}

/// Offsets inside `OCI_CANRxQueueConfiguration` (sizeof = 40, verified with
/// `offsetof`). Each callback slot is an `OCI_CANRxCallbackSingleMsg`: a union
/// of `function`/`functionEx` (8 bytes) followed by `userData` (8 bytes).
#[allow(dead_code)]
mod rx_queue {
    pub const ON_FRAME_EX: usize = 0;
    pub const ON_FRAME_USER_DATA: usize = 8;
    pub const ON_EVENT_EX: usize = 16;
    pub const ON_EVENT_USER_DATA: usize = 24;
    pub const SELF_RECEPTION_MODE: usize = 32;
}

/// Offsets inside `OCI_CANControllerCapabilities` (sizeof = 84).
#[allow(dead_code)] // documents the full layout; not every field is read
mod cap {
    pub const SAMPLES_PER_BIT: usize = 0;
    pub const SYNC_EDGE: usize = 4;
    pub const PHYSICAL_MEDIA: usize = 8;
    pub const BUS_EVENTS: usize = 16;
    pub const ERROR_FRAMES: usize = 20;
    pub const MESSAGE_FLAGS: usize = 24;
    pub const CAN_FD_SUPPORT: usize = 28;
    pub const CAN_FD_MAX_DATA_SIZE: usize = 32;
    pub const CAN_FD_MAX_QUALIFIED_DATA_RATE: usize = 36;
    pub const CAN_FD_MAX_DATA_RATE: usize = 40;
    pub const CAN_FD_RX_CAN_MODE: usize = 44;
    pub const CAN_FD_RX_FD_MODE: usize = 48;
    pub const CAN_FD_TX_MODE: usize = 52;
    pub const CAN_BUS_PARTICIPATION_MODE: usize = 56;
    pub const CAN_TX_POLICIES: usize = 60;
}

/// Offsets inside `OCI_CANControllerStatus`.
#[allow(dead_code)]
mod status {
    pub const STATE_CODE: usize = 4;
}

/// Offsets inside `OCI_TimerCapabilities`.
#[allow(dead_code)]
mod timer {
    pub const FORMAT: usize = 40;
    pub const TICK_FREQUENCY: usize = 44;
    pub const TICKS_PER_INCREMENT: usize = 48;
}

/// Offsets inside `OCI_CANMessage` / `OCI_CANMessageEx` and their payload
/// union members. The union starts at byte 8 (`type`, `reserved`), and every
/// message variant shares the same prefix, which lets one decoder handle both
/// classic CAN and CAN-FD.
#[allow(dead_code)] // documents the full layout; not every field is read
mod msg {
    pub const TYPE: usize = 0;
    pub const PAYLOAD: usize = 8;

    // `OCI_CANRxMessage` / `OCI_CANFDRxMessage` (relative to PAYLOAD).
    pub const RX_TIMESTAMP: usize = PAYLOAD;
    pub const RX_TAG: usize = PAYLOAD + 8;
    pub const RX_FRAME_ID: usize = PAYLOAD + 12;
    pub const RX_FLAGS: usize = PAYLOAD + 16;
    pub const RX_LEN: usize = PAYLOAD + 19;
    pub const RX_DATA: usize = PAYLOAD + 24;
    pub const RX_DATA_MAX: usize = 64;

    // `OCI_CANTxMessage` / `OCI_CANFDTxMessage` (relative to PAYLOAD).
    pub const TX_FRAME_ID: usize = PAYLOAD;
    pub const TX_FLAGS: usize = PAYLOAD + 4;
    pub const TX_LEN: usize = PAYLOAD + 7;
    pub const TX_DATA: usize = PAYLOAD + 8;
    pub const TX_DATA_MAX: usize = 64;

    // `OCI_CANErrorFrameMessage` / `OCI_CANEventMessage`.
    pub const EVENT_TIMESTAMP: usize = PAYLOAD;
    pub const EVENT_CODE: usize = PAYLOAD + 12;
}

// ---------------------------------------------------------------------------
// FFI types
// ---------------------------------------------------------------------------

/// `BOA_Version` (4 bytes, dword aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BoaVersion {
    major: u8,
    minor: u8,
    bugfix: u8,
    build: u8,
}

impl BoaVersion {
    fn at_least(&self, major: u8, minor: u8) -> bool {
        self.major > major || (self.major == major && self.minor >= minor)
    }
}

impl std::fmt::Display for BoaVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}.{}.{}.{}",
            self.major, self.minor, self.bugfix, self.build
        )
    }
}

/// `BOA_UuidVersion` (16-byte UUID + 4-byte version).
#[repr(C)]
struct BoaUuidVersion {
    uuid: [u8; 16],
    version: BoaVersion,
}

/// `CSI_NodeRange`.
#[repr(C)]
struct CsiNodeRange {
    min: u32,
    max: u32,
}

type CsiCreateProtocolTreeFn =
    unsafe extern "system" fn(*const c_char, CsiNodeRange, *mut *mut c_void) -> u32;
type CsiDestroyProtocolTreeFn = unsafe extern "system" fn(*mut c_void) -> u32;
type CsiGetUriForUuidFn = unsafe extern "system" fn(
    *mut c_void,
    *const BoaUuidVersion,
    *mut c_char,
    i32,
    *mut i32,
) -> u32;

type CreateCanControllerVersionFn =
    unsafe extern "system" fn(*const c_char, *const BoaVersion, *mut i32) -> u32;
type DestroyCanControllerFn = unsafe extern "system" fn(i32) -> u32;
type OpenCanControllerFn = unsafe extern "system" fn(i32, *mut u8, *mut u8) -> u32;
type GetCanControllerCapabilitiesFn = unsafe extern "system" fn(i32, *mut u8) -> u32;
type GetCanControllerStatusFn = unsafe extern "system" fn(i32, *mut u8) -> u32;
type CreateCanRxQueueFn = unsafe extern "system" fn(i32, *mut u8, *mut i32) -> u32;
type CreateCanTxQueueFn = unsafe extern "system" fn(i32, *mut u8, *mut i32) -> u32;
type AddCanFrameFilterFn = unsafe extern "system" fn(i32, *mut u8, u32) -> u32;
type ReadCanDataFn = unsafe extern "system" fn(i32, i64, *mut u8, u32, *mut u32, *mut u32) -> u32;
type ReadCanDataExFn =
    unsafe extern "system" fn(i32, i64, *mut *mut u8, u32, *mut u32, *mut u32) -> u32;
type WriteCanDataFn = unsafe extern "system" fn(i32, i64, *mut u8, u32, *mut u32) -> u32;
type WriteCanDataExFn = unsafe extern "system" fn(i32, i64, *mut *mut u8, u32, *mut u32) -> u32;
type GetErrorFn = unsafe extern "system" fn(i32, u32, *mut c_char, u32) -> u32;
type GetTimerCapabilitiesFn = unsafe extern "system" fn(i32, *mut u8) -> u32;

/// `OCI_CANRxCallbackFunctionSingleMsgEx`: called by BOA from its own receive
/// thread for every frame that passes the hardware filters.
type CanRxCallbackExFn = unsafe extern "system" fn(*mut c_void, *mut u8);

/// Wrapper around the loaded BOA framework DLLs and their function pointers.
struct BoaApi {
    // Keep both libraries alive for as long as the function pointers are used.
    _oci_lib: Library,
    _csi_lib: Library,
    csi_create_protocol_tree: CsiCreateProtocolTreeFn,
    csi_destroy_protocol_tree: CsiDestroyProtocolTreeFn,
    csi_get_uri_for_uuid: CsiGetUriForUuidFn,
    create_can_controller_version: CreateCanControllerVersionFn,
    destroy_can_controller: DestroyCanControllerFn,
    open_can_controller: OpenCanControllerFn,
    get_can_controller_capabilities: GetCanControllerCapabilitiesFn,
    get_can_controller_status: Option<GetCanControllerStatusFn>,
    create_can_rx_queue: CreateCanRxQueueFn,
    create_can_tx_queue: CreateCanTxQueueFn,
    add_can_frame_filter: AddCanFrameFilterFn,
    read_can_data: Option<ReadCanDataFn>,
    read_can_data_ex: Option<ReadCanDataExFn>,
    write_can_data: Option<WriteCanDataFn>,
    write_can_data_ex: Option<WriteCanDataExFn>,
    get_error: Option<GetErrorFn>,
    get_timer_capabilities: Option<GetTimerCapabilitiesFn>,
}

impl BoaApi {
    /// Load the BOA framework libraries and resolve the OCI/CSI entry points.
    fn load() -> Result<Self, CanDeviceError> {
        let (oci_lib, csi_lib) = load_boa_libraries()?;

        unsafe {
            let csi_create_protocol_tree = resolve(&csi_lib, b"CSI_CreateProtocolTree\0")?;
            let csi_destroy_protocol_tree = resolve(&csi_lib, b"CSI_DestroyProtocolTree\0")?;
            let csi_get_uri_for_uuid = resolve(&csi_lib, b"CSI_GetUriForUuid\0")?;

            let create_can_controller_version =
                resolve(&oci_lib, b"OCI_CreateCANControllerVersion\0")?;
            let destroy_can_controller = resolve(&oci_lib, b"OCI_DestroyCANController\0")?;
            let open_can_controller = resolve(&oci_lib, b"OCI_OpenCANController\0")?;
            let get_can_controller_capabilities =
                resolve(&oci_lib, b"OCI_GetCANControllerCapabilities\0")?;
            let create_can_rx_queue = resolve(&oci_lib, b"OCI_CreateCANRxQueue\0")?;
            let create_can_tx_queue = resolve(&oci_lib, b"OCI_CreateCANTxQueue\0")?;
            let add_can_frame_filter = resolve(&oci_lib, b"OCI_AddCANFrameFilter\0")?;

            let get_can_controller_status =
                resolve_optional(&oci_lib, b"OCI_GetCANControllerStatus\0");
            let read_can_data = resolve_optional(&oci_lib, b"OCI_ReadCANData\0");
            let read_can_data_ex = resolve_optional(&oci_lib, b"OCI_ReadCANDataEx\0");
            let write_can_data = resolve_optional(&oci_lib, b"OCI_WriteCANData\0");
            let write_can_data_ex = resolve_optional(&oci_lib, b"OCI_WriteCANDataEx\0");
            let get_error = resolve_optional(&oci_lib, b"OCI_GetError\0");
            let get_timer_capabilities = resolve_optional(&oci_lib, b"OCI_GetTimerCapabilities\0");

            // BOA locates its OCD plugins relative to its own DLLs; nothing else
            // to configure here.

            Ok(Self {
                _oci_lib: oci_lib,
                _csi_lib: csi_lib,
                csi_create_protocol_tree,
                csi_destroy_protocol_tree,
                csi_get_uri_for_uuid,
                create_can_controller_version,
                destroy_can_controller,
                open_can_controller,
                get_can_controller_capabilities,
                get_can_controller_status,
                create_can_rx_queue,
                create_can_tx_queue,
                add_can_frame_filter,
                read_can_data,
                read_can_data_ex,
                write_can_data,
                write_can_data_ex,
                get_error,
                get_timer_capabilities,
            })
        }
    }

    /// Human readable text for an OCI result code.
    fn error_text(&self, handle: i32, code: u32) -> String {
        let Some(get_error) = self.get_error else {
            return format!("0x{:08X}", code);
        };
        let mut buf = [0 as c_char; 512];
        let status = unsafe { get_error(handle, code, buf.as_mut_ptr(), buf.len() as u32) };
        let text = if status & BOA_RESULT_FLAG_ERROR == 0 {
            let bytes: Vec<u8> = buf
                .iter()
                .take_while(|&&c| c != 0)
                .map(|&c| c as u8)
                .collect();
            String::from_utf8_lossy(&bytes).trim().to_string()
        } else {
            String::new()
        };
        if text.is_empty() {
            format!("0x{:08X}", code)
        } else {
            format!("0x{:08X} ({})", code, text)
        }
    }

    /// Collect the OCI CAN channel URIs reported by the CSI search.
    ///
    /// The result is cached for [`URI_CACHE_TTL`] because a scan takes ~2 s
    /// (every attached ETAS interface is probed).
    fn channel_uris(&self) -> Result<Vec<String>, CanDeviceError> {
        if let Some(cached) = cached_uris() {
            tracing::debug!("ES582: using {} cached channel URI(s)", cached.len());
            return Ok(cached);
        }
        let uris = self.scan_uris()?;
        store_uris(&uris);
        Ok(uris)
    }

    /// Run a CSI scan and return every OCI CAN URI found.
    ///
    /// A USB-scoped tree is tried first: it skips the Ethernet/vNet discovery
    /// plugins, which dominate the scan time. If it finds nothing (a device on
    /// another medium, or an older OCD layout) the whole tree is scanned.
    fn scan_uris(&self) -> Result<Vec<String>, CanDeviceError> {
        let mut last_error: Option<CanDeviceError> = None;
        for root in [CSI_USB_ROOT, ""] {
            match self.scan_uris_at(root) {
                Ok(uris) if !uris.is_empty() => return Ok(uris),
                Ok(_) if root.is_empty() => return Ok(Vec::new()),
                Ok(_) => {
                    tracing::debug!("ES582: no channel under '{}', scanning everything", root);
                }
                Err(error) => {
                    tracing::debug!("ES582: scan of '{}' failed: {}", root, error);
                    last_error = Some(error);
                }
            }
        }
        match last_error {
            Some(error) => Err(error),
            None => Ok(Vec::new()),
        }
    }

    /// Run a CSI scan rooted at `root` (`""` = whole tree).
    fn scan_uris_at(&self, root: &str) -> Result<Vec<String>, CanDeviceError> {
        let mut tree: *mut c_void = std::ptr::null_mut();
        let range = CsiNodeRange {
            min: CSI_NODE_MIN,
            max: CSI_NODE_MAX,
        };
        let c_root = CString::new(root).expect("CSI root CString");
        let started = Instant::now();
        let status = unsafe { (self.csi_create_protocol_tree)(c_root.as_ptr(), range, &mut tree) };
        tracing::debug!(
            "CSI_CreateProtocolTree('{}') -> 0x{:08X} (tree={:?}, {:.0} ms)",
            root,
            status,
            tree,
            started.elapsed().as_secs_f64() * 1000.0
        );
        if status & BOA_RESULT_FLAG_ERROR != 0 {
            return Err(CanDeviceError::Driver(format!(
                "CSI_CreateProtocolTree failed: 0x{:08X}",
                status
            )));
        }
        if tree.is_null() {
            return Err(CanDeviceError::Driver(format!(
                "CSI_CreateProtocolTree returned no tree (status 0x{:08X})",
                status
            )));
        }

        let mut uris: Vec<String> = Vec::new();
        // The search result is filtered by (UUID, version), so probe every
        // version an OCD might offer and de-duplicate the URIs.
        for version in API_VERSIONS {
            let id = BoaUuidVersion {
                uuid: UUID_OCICAN,
                version,
            };

            // First call with room for a single name: CSI fills the array and
            // reports the total number of matches in `count`.
            let mut first = [0 as c_char; URI_NAME_LEN];
            let mut count: i32 = 0;
            let status = unsafe {
                (self.csi_get_uri_for_uuid)(tree, &id, first.as_mut_ptr(), 1, &mut count)
            };
            if status & BOA_RESULT_FLAG_ERROR != 0 && count <= 0 {
                continue;
            }
            let total = count.max(0) as usize;
            if total == 0 {
                continue;
            }

            let mut all: Vec<[c_char; URI_NAME_LEN]> = vec![[0; URI_NAME_LEN]; total];
            let mut got: i32 = 0;
            let status = unsafe {
                (self.csi_get_uri_for_uuid)(
                    tree,
                    &id,
                    all.as_mut_ptr() as *mut c_char,
                    total as i32,
                    &mut got,
                )
            };
            if status & BOA_RESULT_FLAG_ERROR != 0 && got <= 0 {
                continue;
            }

            for entry in all.iter().take(got.max(0) as usize) {
                let bytes: Vec<u8> = entry
                    .iter()
                    .take_while(|&&c| c != 0)
                    .map(|&c| c as u8)
                    .collect();
                let uri = String::from_utf8_lossy(&bytes).trim().to_string();
                if !uri.is_empty() && !uris.contains(&uri) {
                    uris.push(uri);
                }
            }
        }

        unsafe {
            (self.csi_destroy_protocol_tree)(tree);
        }

        uris.sort();
        Ok(uris)
    }

    /// Create an OCI CAN controller bound to `uri`, negotiating the API version.
    fn create_controller(&self, uri: &str) -> Result<(i32, BoaVersion), CanDeviceError> {
        let c_uri = CString::new(uri)
            .map_err(|e| CanDeviceError::UnknownChannel(format!("invalid URI '{}': {}", uri, e)))?;

        let mut last_status = 0u32;
        for version in API_VERSIONS {
            let mut handle: i32 = OCI_NO_HANDLE;
            let status = unsafe {
                (self.create_can_controller_version)(c_uri.as_ptr(), &version, &mut handle)
            };
            if status & BOA_RESULT_FLAG_ERROR == 0 {
                return Ok((handle, version));
            }
            last_status = status;
            if status != BOA_ERR_PROTOCOL_VERSION_NOT_SUPPORTED && status != BOA_ERR_NO_INTERFACE {
                break;
            }
        }

        Err(CanDeviceError::OpenFailed(format!(
            "OCI_CreateCANControllerVersion({}) failed: {}",
            uri,
            self.error_text(OCI_NO_HANDLE, last_status)
        )))
    }

    /// Query `OCI_CANControllerCapabilities` (zeroed on failure).
    fn capabilities(&self, handle: i32) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; STRUCT_BUF];
        let status = unsafe { (self.get_can_controller_capabilities)(handle, buf.as_mut_ptr()) };
        if status & BOA_RESULT_FLAG_ERROR != 0 {
            tracing::debug!(
                "OCI_GetCANControllerCapabilities failed: {}",
                self.error_text(handle, status)
            );
            return None;
        }
        Some(buf)
    }

    /// Query `OCI_CANControllerStatus` state bits.
    fn controller_state(&self, handle: i32) -> Option<u32> {
        let get_status = self.get_can_controller_status?;
        let mut buf = vec![0u8; STRUCT_BUF];
        let status = unsafe { get_status(handle, buf.as_mut_ptr()) };
        if status & BOA_RESULT_FLAG_ERROR != 0 {
            return None;
        }
        Some(get_u32(&buf, status::STATE_CODE))
    }

    /// Query the controller clock (ticks per second) used for timestamps.
    fn timer_tick_hz(&self, handle: i32) -> u32 {
        let Some(get_caps) = self.get_timer_capabilities else {
            return 0;
        };
        let mut buf = vec![0u8; STRUCT_BUF];
        let status = unsafe { get_caps(handle, buf.as_mut_ptr()) };
        if status & BOA_RESULT_FLAG_ERROR != 0 {
            return 0;
        }
        get_u32(&buf, timer::TICK_FREQUENCY)
    }
}

unsafe fn resolve<T>(lib: &Library, name: &[u8]) -> Result<T, CanDeviceError>
where
    T: Copy,
{
    let symbol: Symbol<T> = lib.get(name).map_err(|e| {
        CanDeviceError::Driver(format!(
            "BOA DLL is missing symbol {}: {}",
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

/// Locate and load `dll-ocdProxy.dll` and `dll-csiBind.dll`.
///
/// The framework DLLs depend on each other and on the OCD plugins, which
/// Windows resolves through the standard search order; the framework directory
/// is therefore prepended to `PATH` before the libraries are loaded.
fn load_boa_libraries() -> Result<(Library, Library), CanDeviceError> {
    let dir = boa_dll_dir().ok_or_else(|| {
        CanDeviceError::Driver(format!(
            "BOA V2 framework not found. Set {} to the directory containing \
             dll-ocdProxy.dll and dll-csiBind.dll",
            BOA_DLL_DIR_ENV
        ))
    })?;

    // The framework, its ESS providers and the OCD plugins all load further
    // DLLs by name, so every BOA directory has to be on the search path.
    for path_dir in boa_path_dirs(&dir) {
        prepend_to_path(&path_dir);
    }

    let oci_path = std::path::Path::new(&dir).join("dll-ocdProxy.dll");
    let csi_path = std::path::Path::new(&dir).join("dll-csiBind.dll");

    let oci_lib = unsafe { Library::new(&oci_path) }.map_err(|e| {
        CanDeviceError::Driver(format!("failed to load {}: {}", oci_path.display(), e))
    })?;
    let csi_lib = unsafe { Library::new(&csi_path) }.map_err(|e| {
        CanDeviceError::Driver(format!("failed to load {}: {}", csi_path.display(), e))
    })?;

    tracing::debug!("Loaded BOA framework DLLs from {}", dir);
    Ok((oci_lib, csi_lib))
}

/// Resolve the BOA framework directory (`BOA_DLL_DIR` or a known location).
fn boa_dll_dir() -> Option<String> {
    if let Ok(dir) = std::env::var(BOA_DLL_DIR_ENV) {
        let dir = dir.trim();
        if !dir.is_empty() {
            return Some(dir.to_string());
        }
    }
    BOA_DLL_FALLBACK_DIRS
        .iter()
        .find(|dir| std::path::Path::new(dir).join("dll-ocdProxy.dll").is_file())
        .map(|dir| (*dir).to_string())
}

/// Resolve the OCD plugin directory (`BOA_OCD_DIR` or a known location).
fn ocd_search_dir() -> Option<String> {
    if let Ok(dir) = std::env::var(BOA_OCD_DIR_ENV) {
        let dir = dir.trim();
        if !dir.is_empty() {
            return Some(dir.to_string());
        }
    }
    BOA_OCD_FALLBACK_DIRS
        .iter()
        .find(|dir| {
            std::path::Path::new(dir)
                .join("ES582_1_OCD_CAN.dll")
                .is_file()
        })
        .map(|dir| (*dir).to_string())
        .or_else(|| {
            BOA_OCD_FALLBACK_DIRS
                .iter()
                .find(|dir| std::path::Path::new(dir).is_dir())
                .map(|dir| (*dir).to_string())
        })
}

/// Every directory that has to be on the DLL search path for BOA to work.
fn boa_path_dirs(framework_dir: &str) -> Vec<String> {
    let mut dirs = vec![framework_dir.to_string()];
    let framework = std::path::Path::new(framework_dir);

    // `...\Dll` and `...\Dll\vNetOCDs` hold the ESS providers
    // (dll-csiEssProcessRoot.dll, dll-ser2csiEss.dll).
    if let Some(parent) = framework.parent() {
        dirs.push(parent.to_string_lossy().into_owned());
        dirs.push(parent.join("vNetOCDs").to_string_lossy().into_owned());
        dirs.push(parent.join("OCDs").to_string_lossy().into_owned());
        dirs.push(parent.join("Localization").to_string_lossy().into_owned());
    }
    if let Some(ocd_dir) = ocd_search_dir() {
        dirs.push(ocd_dir);
    }

    dirs.retain(|dir| std::path::Path::new(dir).is_dir());
    dirs
}

/// Prepend `dir` to the process `PATH` so dependent DLLs resolve.
fn prepend_to_path(dir: &str) {
    let current = std::env::var("PATH").unwrap_or_default();
    let already = std::env::split_paths(&current).any(|p| {
        p.to_string_lossy()
            .trim_end_matches('\\')
            .eq_ignore_ascii_case(dir.trim_end_matches('\\'))
    });
    if already {
        return;
    }
    let joined = if current.is_empty() {
        dir.to_string()
    } else {
        format!("{};{}", dir, current)
    };
    std::env::set_var("PATH", joined);
}

// ---------------------------------------------------------------------------
// Raw buffer helpers
// ---------------------------------------------------------------------------

fn get_u32(buf: &[u8], offset: usize) -> u32 {
    buf.get(offset..offset + 4)
        .and_then(|s| s.try_into().ok())
        .map(u32::from_le_bytes)
        .unwrap_or(0)
}

fn get_u16(buf: &[u8], offset: usize) -> u16 {
    buf.get(offset..offset + 2)
        .and_then(|s| s.try_into().ok())
        .map(u16::from_le_bytes)
        .unwrap_or(0)
}

fn get_u8(buf: &[u8], offset: usize) -> u8 {
    buf.get(offset).copied().unwrap_or(0)
}

fn get_i64(buf: &[u8], offset: usize) -> i64 {
    buf.get(offset..offset + 8)
        .and_then(|s| s.try_into().ok())
        .map(i64::from_le_bytes)
        .unwrap_or(0)
}

fn set_u32(buf: &mut [u8], offset: usize, value: u32) {
    if let Some(slot) = buf.get_mut(offset..offset + 4) {
        slot.copy_from_slice(&value.to_le_bytes());
    }
}

fn set_u64(buf: &mut [u8], offset: usize, value: u64) {
    if let Some(slot) = buf.get_mut(offset..offset + 8) {
        slot.copy_from_slice(&value.to_le_bytes());
    }
}

fn set_u16(buf: &mut [u8], offset: usize, value: u16) {
    if let Some(slot) = buf.get_mut(offset..offset + 2) {
        slot.copy_from_slice(&value.to_le_bytes());
    }
}

fn set_u8(buf: &mut [u8], offset: usize, value: u8) {
    if let Some(slot) = buf.get_mut(offset) {
        *slot = value;
    }
}

#[allow(dead_code)]
fn set_i64(buf: &mut [u8], offset: usize, value: i64) {
    if let Some(slot) = buf.get_mut(offset..offset + 8) {
        slot.copy_from_slice(&value.to_le_bytes());
    }
}
// ---------------------------------------------------------------------------
// Channel description
// ---------------------------------------------------------------------------

/// Detailed information about one ES582 CAN channel.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Es582ChannelDetails {
    /// OCI URI used to bind the controller, e.g. `ETAS:/ES582/CAN:1`.
    pub uri: String,
    /// OCI_CAN API version negotiated with the OCD.
    pub api_version: String,
    /// Whether the hardware reports CAN-FD capability.
    pub fd_capable: bool,
    /// Maximum CAN-FD payload (0 when CAN-FD is unsupported).
    pub fd_max_data_size: u32,
    /// Maximum CAN-FD data-phase bit rate (0 when unsupported).
    pub fd_max_data_rate: u32,
    /// Qualified CAN-FD data-phase bit rate (0 when unsupported).
    pub fd_max_qualified_data_rate: u32,
    /// Supported CAN-FD receive modes for classic CAN frames.
    pub fd_rx_can_mode: u32,
    /// Supported CAN-FD receive modes for CAN-FD frames.
    pub fd_rx_fd_mode: u32,
    /// Supported CAN-FD transmit modes.
    pub fd_tx_mode: u32,
    /// Supported `samplesPerBit` flags.
    pub samples_per_bit: u32,
    /// Supported physical media flags.
    pub physical_media: u32,
    /// Supported bus event flags.
    pub bus_events: u32,
    /// Supported message flags (extended/remote/FD/...).
    pub message_flags: u32,
    /// Supported bus participation modes.
    pub bus_participation_mode: u32,
    /// Supported transmit policies.
    pub tx_policies: u32,
    /// Controller clock in ticks per second (0 when unknown).
    pub timer_tick_hz: u32,
    /// Current controller state bits (0 when unknown).
    pub state_code: u32,
    /// True when the channel could be bound (i.e. is not in use elsewhere).
    pub available: bool,
}

impl Es582ChannelDetails {
    /// Human readable controller state.
    pub fn state_label(&self) -> &'static str {
        match self.state_code & 0x0F {
            0x01 => "error-active",
            0x03 | 0x05 => "warning",
            0x06 => "error-passive",
            0x0E => "bus-off",
            0 => "unknown",
            _ => "other",
        }
    }
}

/// One enumerated ES582 channel: the transport-facing description plus details.
#[derive(Debug, Clone)]
pub struct Es582ChannelInfo {
    pub info: CanDeviceInfo,
    pub details: Es582ChannelDetails,
}

// ---------------------------------------------------------------------------
// URI parsing
// ---------------------------------------------------------------------------

/// Split an OCI URI into a display name, a stable device id and a controller
/// index.
///
/// OCI URIs look like `ETAS:/ES582/CAN:1`; the last path segment names the
/// protocol port and the remaining path identifies the hardware. The device id
/// keeps that hardware path (with `:` replaced by `;` so it survives the
/// `driver:device_id:controller` selection token).
fn parse_uri(uri: &str) -> (String, String, u8) {
    let trimmed = uri.trim();
    let path = trimmed
        .strip_prefix("ETAS:")
        .or_else(|| trimmed.strip_prefix("etas:"))
        .unwrap_or(trimmed);
    let path = path.trim_start_matches('/');
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    if segments.is_empty() {
        return (trimmed.to_string(), sanitize_id(trimmed), 0);
    }

    let last = segments[segments.len() - 1];
    let (device_segments, controller) = match last.rsplit_once(':') {
        Some((proto, index)) if proto.eq_ignore_ascii_case("CAN") => (
            &segments[..segments.len() - 1],
            index.trim().parse::<u8>().unwrap_or(0),
        ),
        _ => (&segments[..], 0),
    };

    let device_path = device_segments.join("/");
    let name = device_segments
        .last()
        .map(|s| (*s).to_string())
        .unwrap_or_else(|| "ES582".to_string());

    let device_id = if device_path.is_empty() {
        sanitize_id(trimmed)
    } else {
        sanitize_id(&device_path)
    };

    (name, device_id, controller)
}

/// Make a URI component safe for the `driver:device_id:controller` token.
fn sanitize_id(value: &str) -> String {
    value.replace(':', ";").replace('\\', "/")
}

/// Decode a sanitized device id back to a URI fragment.
fn unsanitize_id(value: &str) -> String {
    value.replace(';', ":")
}

/// Read an optional `u32` environment variable.
fn env_u32(key: &str) -> Option<u32> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .and_then(|v| v.parse::<u32>().ok())
}

/// Read a boolean-ish environment variable (`1`/`true`/`yes`).
fn env_flag(key: &str) -> bool {
    std::env::var(key)
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            v == "1" || v == "true" || v == "yes"
        })
        .unwrap_or(false)
}

/// Bit timing handed to the OCD, which derives the controller registers from
/// it (`OCI_CANConfiguration` / `OCI_CANFDConfiguration`).
///
/// `btl_cycles` is the number of time quanta per bit (SYNC + TSEG1 + TSEG2), so
/// a candidate is only realisable when `controller_clock / (bitrate * btl)` is
/// an integer. The controller clock of the ES582 family is not published, so
/// [`CanTiming::candidates`] lists prescaler-friendly values that cover the
/// usual 40 MHz / 80 MHz oscillators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CanTiming {
    btl_cycles: u32,
    sample_point: u32,
    sjw: u32,
}

impl CanTiming {
    /// Timing candidates for the nominal (arbitration) or the CAN-FD data phase.
    ///
    /// An environment override (see [`ES582_BTL_ENV`] and friends) pins a single
    /// candidate, which is useful for unusual bit rates.
    fn candidates(data_phase: bool) -> Vec<Self> {
        let (btl_env, sp_env, sjw_env) = if data_phase {
            (
                ES582_FD_BTL_ENV,
                ES582_FD_SAMPLE_POINT_ENV,
                ES582_FD_SJW_ENV,
            )
        } else {
            (ES582_BTL_ENV, ES582_SAMPLE_POINT_ENV, ES582_SJW_ENV)
        };

        let sample_point = env_u32(sp_env).unwrap_or(80).min(100);
        let sjw = env_u32(sjw_env).unwrap_or(1).max(1);

        if let Some(btl) = env_u32(btl_env) {
            return vec![Self {
                btl_cycles: btl.clamp(8, 80),
                sample_point,
                sjw,
            }];
        }

        let mut candidates = vec![
            // Common prescalers for 40 MHz and 80 MHz controllers.
            Self {
                btl_cycles: 10,
                sample_point,
                sjw,
            },
            Self {
                btl_cycles: 16,
                sample_point,
                sjw,
            },
            Self {
                btl_cycles: 20,
                sample_point,
                sjw,
            },
            Self {
                btl_cycles: 8,
                sample_point,
                sjw,
            },
            Self {
                btl_cycles: 25,
                sample_point,
                sjw,
            },
            Self {
                btl_cycles: 40,
                sample_point,
                sjw,
            },
            Self {
                btl_cycles: 80,
                sample_point,
                sjw,
            },
        ];
        if data_phase {
            // ETAS demo configurations sample the data phase later.
            candidates.insert(
                1,
                Self {
                    btl_cycles: 12,
                    sample_point: if sample_point == 80 { 60 } else { sample_point },
                    sjw: sjw.max(3),
                },
            );
        }
        candidates
    }

    fn describe(&self) -> String {
        format!(
            "btl={} sp={}% sjw={}",
            self.btl_cycles, self.sample_point, self.sjw
        )
    }
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

/// ETAS ES582 CAN/CAN-FD driver (BOA V2 / OCI).
pub struct Es582Driver;

impl Es582Driver {
    pub fn new() -> Self {
        Self
    }

    /// Open a channel and return the concrete device type (instead of
    /// `Box<dyn CanDevice>`) so callers can query hardware details.
    ///
    /// The candidate URIs are tried in order (cheapest first). A reconstructed
    /// URI that fails to bind invalidates the scan cache and falls back to a
    /// fresh enumeration, so a re-plugged device is still found.
    pub fn open_device(&self, config: &CanDeviceConfig) -> Result<Es582Device, CanDeviceError> {
        let api = Arc::new(BoaApi::load()?);
        let mut candidates = resolve_uri_candidates(&api, config)?;
        let mut last_error: Option<CanDeviceError> = None;
        let mut refreshed = false;

        while let Some(uri) = candidates.first().cloned() {
            candidates.remove(0);
            match self.open_at(api.clone(), uri.clone(), config) {
                Ok(device) => return Ok(device),
                Err(error) => {
                    tracing::debug!("ES582: opening {} failed: {}", uri, error);
                    last_error = Some(error);
                    // The enumeration may be stale (re-plug, different USB
                    // port). Rescan once, but only keep channels that still
                    // match what the caller asked for — silently opening a
                    // different device/channel would be worse than an error.
                    if candidates.is_empty() && !refreshed {
                        refreshed = true;
                        invalidate_uris();
                        let fresh = api.scan_uris().unwrap_or_default();
                        store_uris(&fresh);
                        let known = fresh.clone();
                        candidates = fresh
                            .into_iter()
                            .filter(|candidate| candidate != &uri)
                            .filter(|candidate| selection_matches(candidate, config))
                            .collect();
                        if candidates.is_empty() && !known.is_empty() {
                            last_error = Some(CanDeviceError::UnknownChannel(format!(
                                "ES582 selection not found (known: {})",
                                known.join(", ")
                            )));
                        }
                    }
                }
            }
        }

        Err(last_error
            .unwrap_or_else(|| CanDeviceError::NoChannel("no ES582 CAN channel found".to_string())))
    }

    /// Bind a controller to `uri` and configure queues/filters.
    fn open_at(
        &self,
        api: Arc<BoaApi>,
        uri: String,
        config: &CanDeviceConfig,
    ) -> Result<Es582Device, CanDeviceError> {
        let (handle, version) = api.create_controller(&uri)?;
        let mut device = Es582Device {
            api: api.clone(),
            uri: uri.clone(),
            controller: handle,
            tx_queue: OCI_NO_HANDLE,
            rx_queue: OCI_NO_HANDLE,
            info: CanDeviceInfo {
                driver: "es582".to_string(),
                device_id: String::new(),
                name: "ES582".to_string(),
                device_type: "ES582".to_string(),
                controller: 0,
                channel: uri.clone(),
                available: true,
                fd_capable: false,
            },
            version,
            fd: false,
            bitrate: String::new(),
            tick_hz: 0,
            rx_mode: RxMode::from_env(),
            rx_context: Arc::new(RxContext::new()),
            pending: Mutex::new(VecDeque::new()),
            closed: AtomicBool::new(false),
        };

        device.configure(config)?;
        Ok(device)
    }

    /// Enumerate every OCI CAN channel with full hardware details.
    ///
    /// Each channel is bound briefly to query its capabilities, then released
    /// again, so this also reports whether the channel is currently free.
    pub fn probe_channels(&self) -> Vec<Es582ChannelInfo> {
        let api = match BoaApi::load() {
            Ok(api) => Arc::new(api),
            Err(error) => {
                tracing::debug!("ES582 driver unavailable: {}", error);
                return Vec::new();
            }
        };

        let uris = match api.channel_uris() {
            Ok(uris) => uris,
            Err(error) => {
                tracing::warn!("ES582 channel enumeration failed: {}", error);
                return Vec::new();
            }
        };

        uris.iter()
            .filter_map(|uri| probe_channel(&api, uri))
            .collect()
    }
}

impl Default for Es582Driver {
    fn default() -> Self {
        Self::new()
    }
}

/// Bind `uri`, read its capabilities and release it again.
fn probe_channel(api: &BoaApi, uri: &str) -> Option<Es582ChannelInfo> {
    let (name, device_id, controller) = parse_uri(uri);
    let (handle, version) = match api.create_controller(uri) {
        Ok(result) => result,
        Err(error) => {
            // Still list the channel so the UI can show it as unavailable.
            let available = !error
                .to_string()
                .contains(&format!("{:08X}", BOA_ERR_ALREADY_CONNECTED));
            tracing::debug!("ES582 channel {} not usable: {}", uri, error);
            return Some(Es582ChannelInfo {
                info: CanDeviceInfo {
                    driver: "es582".to_string(),
                    device_id,
                    name,
                    device_type: "ES582".to_string(),
                    controller,
                    channel: uri.to_string(),
                    available,
                    fd_capable: false,
                },
                details: Es582ChannelDetails {
                    uri: uri.to_string(),
                    api_version: "-".to_string(),
                    fd_capable: false,
                    fd_max_data_size: 0,
                    fd_max_data_rate: 0,
                    fd_max_qualified_data_rate: 0,
                    fd_rx_can_mode: 0,
                    fd_rx_fd_mode: 0,
                    fd_tx_mode: 0,
                    samples_per_bit: 0,
                    physical_media: 0,
                    bus_events: 0,
                    message_flags: 0,
                    bus_participation_mode: 0,
                    tx_policies: 0,
                    timer_tick_hz: 0,
                    state_code: 0,
                    available,
                },
            });
        }
    };

    let caps = api.capabilities(handle);
    let state_code = api.controller_state(handle).unwrap_or(0);
    let timer_tick_hz = api.timer_tick_hz(handle);
    unsafe {
        (api.destroy_can_controller)(handle);
    }

    let cap_u32 = |offset: usize| caps.as_ref().map(|c| get_u32(c, offset)).unwrap_or(0);
    let fd_capable = cap_u32(cap::CAN_FD_SUPPORT) & CANFD_SUPPORT != 0;

    let details = Es582ChannelDetails {
        uri: uri.to_string(),
        api_version: version.to_string(),
        fd_capable,
        fd_max_data_size: cap_u32(cap::CAN_FD_MAX_DATA_SIZE),
        fd_max_data_rate: cap_u32(cap::CAN_FD_MAX_DATA_RATE),
        fd_max_qualified_data_rate: cap_u32(cap::CAN_FD_MAX_QUALIFIED_DATA_RATE),
        fd_rx_can_mode: cap_u32(cap::CAN_FD_RX_CAN_MODE),
        fd_rx_fd_mode: cap_u32(cap::CAN_FD_RX_FD_MODE),
        fd_tx_mode: cap_u32(cap::CAN_FD_TX_MODE),
        samples_per_bit: cap_u32(cap::SAMPLES_PER_BIT),
        physical_media: cap_u32(cap::PHYSICAL_MEDIA),
        bus_events: cap_u32(cap::BUS_EVENTS),
        message_flags: cap_u32(cap::MESSAGE_FLAGS),
        bus_participation_mode: cap_u32(cap::CAN_BUS_PARTICIPATION_MODE),
        tx_policies: cap_u32(cap::CAN_TX_POLICIES),
        timer_tick_hz,
        state_code,
        available: true,
    };

    Some(Es582ChannelInfo {
        info: CanDeviceInfo {
            driver: "es582".to_string(),
            device_id,
            name,
            device_type: "ES582".to_string(),
            controller,
            channel: uri.to_string(),
            available: true,
            fd_capable,
        },
        details,
    })
}

impl CanDriver for Es582Driver {
    fn name(&self) -> &'static str {
        "es582"
    }

    fn description(&self) -> &'static str {
        "ETAS ES582 via BOA V2 (OCI)"
    }

    fn list_channels(&self) -> Vec<CanDeviceInfo> {
        self.probe_channels().into_iter().map(|c| c.info).collect()
    }

    fn open(&self, config: &CanDeviceConfig) -> Result<Box<dyn CanDevice>, CanDeviceError> {
        Ok(Box::new(self.open_device(config)?))
    }
}

/// How long the enumerated URI list is reused.
///
/// The desktop UI enumerates for the device list and again when connecting, so
/// without a cache the second call pays for another full CSI scan (~2 s). The
/// TTL stays short so a manual "refresh devices" a few seconds later re-scans;
/// an open that fails on a cached URI also forces a rescan.
const URI_CACHE_TTL: Duration = Duration::from_secs(5);

/// Process-wide cache of the enumerated OCI CAN URIs (`Instant`, list).
static URI_CACHE: Mutex<Option<(Instant, Vec<String>)>> = Mutex::new(None);

/// Return the cached URI list when it is still fresh.
fn cached_uris() -> Option<Vec<String>> {
    let guard = URI_CACHE.lock().ok()?;
    let (stamp, uris) = guard.as_ref()?;
    if stamp.elapsed() <= URI_CACHE_TTL {
        Some(uris.clone())
    } else {
        None
    }
}

/// Store a fresh URI list.
fn store_uris(uris: &[String]) {
    if let Ok(mut guard) = URI_CACHE.lock() {
        *guard = Some((Instant::now(), uris.to_vec()));
    }
}

/// Drop the cache (hardware may have changed, or a cached URI failed to open).
fn invalidate_uris() {
    if let Ok(mut guard) = URI_CACHE.lock() {
        *guard = None;
    }
}

/// Does `uri` satisfy the caller's selection (`device_id`/`controller`)?
///
/// Used after a rescan so a failed open never falls back to an unrelated
/// channel.
fn selection_matches(uri: &str, config: &CanDeviceConfig) -> bool {
    let (_, id, controller) = parse_uri(uri);
    let channel = config.channel.trim();

    if let Some(device_id) = config
        .device_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let wanted = unsanitize_id(device_id);
        let wanted = wanted.trim_start_matches("ETAS:/").trim_start_matches('/');
        let id = unsanitize_id(&id);
        let id = id.trim_start_matches("ETAS:/").trim_start_matches('/');
        return id.eq_ignore_ascii_case(wanted)
            && config.controller.map(|c| c == controller).unwrap_or(true);
    }

    if !channel.is_empty() && !channel.eq_ignore_ascii_case("auto") && !channel.contains('/') {
        if let Ok(index) = channel
            .trim_start_matches("CAN:")
            .trim_start_matches("can:")
            .parse::<u8>()
        {
            return index == controller;
        }
    }

    true
}

/// Resolve the URIs to try, cheapest first.
///
/// The ES582 URIs are fully described by `device_id` + `controller`
/// (`ETAS://USB/ES582.1:114913/CAN:1`), so the common case — a device picked in
/// the UI — needs no enumeration at all. Enumerating costs ~2 s because
/// `CSI_CreateProtocolTree` probes every attached ETAS interface.
fn resolve_uri_candidates(
    api: &BoaApi,
    config: &CanDeviceConfig,
) -> Result<Vec<String>, CanDeviceError> {
    let channel = config.channel.trim();
    let mut candidates: Vec<String> = Vec::new();

    // 1. An explicit URI ("ETAS://...") wins.
    if !channel.is_empty() && !channel.eq_ignore_ascii_case("auto") && channel.contains('/') {
        candidates.push(channel.to_string());
        return Ok(candidates);
    }

    // 2. A stable device id selects the matching channel. The id is the
    //    sanitized URI path, so the URI can be rebuilt without a scan.
    let device_id = config
        .device_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(device_id) = device_id {
        let wanted = unsanitize_id(device_id);
        let wanted = wanted.trim_start_matches("ETAS:/").trim_start_matches('/');
        if let Some(controller) = config.controller {
            candidates.push(format!(
                "ETAS://{}/CAN:{}",
                wanted.trim_start_matches("ETAS://"),
                controller
            ));
        }

        // Fallback: enumerate and match (handles a device id that does not
        // encode a port, or a controller index that no longer exists).
        for uri in api.channel_uris()? {
            let (_, id, ctrl) = parse_uri(&uri);
            let id = unsanitize_id(&id);
            let id = id.trim_start_matches("ETAS:/").trim_start_matches('/');
            let matches = id.eq_ignore_ascii_case(wanted)
                && config.controller.map(|c| c == ctrl).unwrap_or(true);
            if matches && !candidates.contains(&uri) {
                candidates.push(uri);
            }
        }

        if candidates.is_empty() {
            return Err(CanDeviceError::UnknownChannel(format!(
                "ES582 device '{}'{} not found",
                device_id,
                config
                    .controller
                    .map(|c| format!(" controller {}", c))
                    .unwrap_or_default()
            )));
        }
        return Ok(candidates);
    }

    // 3. A channel name that is not a URI (e.g. "CAN:1" or "1") selects by
    //    controller index.
    let uris = api.channel_uris()?;
    if !channel.is_empty() && !channel.eq_ignore_ascii_case("auto") {
        let wanted = channel.trim();
        if let Ok(index) = wanted
            .trim_start_matches("CAN:")
            .trim_start_matches("can:")
            .parse::<u8>()
        {
            if let Some(uri) = uris.iter().find(|uri| parse_uri(uri).2 == index) {
                return Ok(vec![uri.clone()]);
            }
        }
        return Err(CanDeviceError::UnknownChannel(format!(
            "ES582 channel '{}' not found (known: {})",
            channel,
            uris.join(", ")
        )));
    }

    // 4. Environment override, then the first reported channel.
    if let Ok(uri) = std::env::var(ES582_URI_ENV) {
        let uri = uri.trim();
        if !uri.is_empty() {
            return Ok(vec![uri.to_string()]);
        }
    }

    if uris.is_empty() {
        return Err(CanDeviceError::NoChannel(
            "no ES582 CAN channel found".to_string(),
        ));
    }
    Ok(uris)
}

// ---------------------------------------------------------------------------
// Receive path
// ---------------------------------------------------------------------------

/// How frames are taken off the OCI receive queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RxMode {
    /// OCI `onFrame` callback (default). BOA pushes every accepted frame from
    /// its own receive thread; `recv_frame` then only drains a local queue.
    Callback,
    /// `OCI_ReadCANDataEx` polling. Each call costs one USB round trip and
    /// returns a single frame, so this is only useful for comparison/debugging.
    Poll,
}

impl RxMode {
    /// Read the mode from `ES582_RX_MODE` (defaults to `callback`).
    fn from_env() -> Self {
        match std::env::var(ES582_RX_MODE_ENV) {
            Ok(value) => {
                let value = value.trim().to_ascii_lowercase();
                if value == "poll" || value == "read" || value == "polling" {
                    RxMode::Poll
                } else {
                    RxMode::Callback
                }
            }
            Err(_) => RxMode::Callback,
        }
    }
}

/// Shared state between the OCI receive callback and `recv_frame`.
///
/// BOA may invoke the callback one last time *after* `OCI_DestroyCANController`
/// has returned, so the device never frees this allocation: on teardown it only
/// sets [`RxContext::closed`] and leaks one reference (a few hundred bytes per
/// opened channel). The callback checks that flag and returns immediately.
struct RxContext {
    /// Buffered frames, oldest first.
    queue: Mutex<RxQueue>,
    /// Signalled whenever a frame is pushed or the context is closed.
    ready: Condvar,
    /// Controller clock in ticks per second (0 = unknown), used to convert the
    /// hardware timestamps to microseconds.
    tick_hz: AtomicU32,
    /// Frames discarded because the queue was full.
    dropped: AtomicU64,
    /// Set on teardown; the callback becomes a no-op.
    closed: AtomicBool,
}

#[derive(Default)]
struct RxQueue {
    frames: VecDeque<CanFrame>,
}

impl RxContext {
    fn new() -> Self {
        Self {
            queue: Mutex::new(RxQueue::default()),
            ready: Condvar::new(),
            tick_hz: AtomicU32::new(0),
            dropped: AtomicU64::new(0),
            closed: AtomicBool::new(false),
        }
    }

    /// Queue one frame (called from the BOA receive thread).
    fn push(&self, frame: CanFrame) {
        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        {
            let mut queue = match self.queue.lock() {
                Ok(queue) => queue,
                Err(poisoned) => poisoned.into_inner(),
            };
            if queue.frames.len() >= RX_QUEUE_CAPACITY {
                queue.frames.pop_front();
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
            queue.frames.push_back(frame);
        }
        self.ready.notify_one();
    }

    /// Wait up to `timeout` for a frame. `None` waits [`RX_WAIT_SLICE`].
    fn wait(&self, timeout: Option<Duration>) -> Option<CanFrame> {
        let deadline = Instant::now() + timeout.unwrap_or(RX_WAIT_SLICE);
        let mut queue = match self.queue.lock() {
            Ok(queue) => queue,
            Err(poisoned) => poisoned.into_inner(),
        };
        loop {
            if self.closed.load(Ordering::SeqCst) {
                return None;
            }
            if let Some(frame) = queue.frames.pop_front() {
                return Some(frame);
            }
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            let (guard, _) = match self.ready.wait_timeout(queue, deadline - now) {
                Ok(result) => result,
                Err(poisoned) => poisoned.into_inner(),
            };
            queue = guard;
        }
    }

    /// Wake every waiter and make the callback a no-op.
    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.ready.notify_all();
    }

    fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// `OCI_CANRxCallbackFunctionSingleMsgEx` implementation.
///
/// Runs on a BOA-internal thread: it must not block, must not call back into
/// OCI and must not unwind, so the body only copies the frame into the local
/// queue. The message pointer is only valid for the duration of the call.
unsafe extern "system" fn es582_rx_callback(user_data: *mut c_void, message: *mut u8) {
    if user_data.is_null() || message.is_null() {
        return;
    }
    let context = &*(user_data as *const RxContext);
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if context.closed.load(Ordering::SeqCst) {
            return;
        }
        let buffer = std::slice::from_raw_parts(message, CAN_MESSAGE_EX_SIZE);
        let tick_hz = context.tick_hz.load(Ordering::Relaxed);
        if let Some(frame) = decode_frame(buffer, tick_hz) {
            context.push(frame);
        }
    }));
}

/// Convert controller ticks into microseconds.
fn ticks_to_micros(ticks: i64, tick_hz: u32) -> Option<u64> {
    if tick_hz == 0 || ticks < 0 {
        return None;
    }
    Some((ticks as u128 * 1_000_000 / tick_hz as u128) as u64)
}

/// Decode one `OCI_CANMessage` / `OCI_CANMessageEx` buffer.
///
/// Classic CAN and CAN-FD messages share the same prefix, so one decoder
/// handles both; error frames and bus events are surfaced with an empty payload
/// (the XCP layer ignores them, but they show up in the trace log).
fn decode_frame(buffer: &[u8], tick_hz: u32) -> Option<CanFrame> {
    let message_type = get_u32(buffer, msg::TYPE);
    match message_type {
        CAN_RX_MESSAGE => {
            let id = get_u32(buffer, msg::RX_FRAME_ID);
            let flags = get_u16(buffer, msg::RX_FLAGS);
            let dlc = get_u8(buffer, msg::RX_LEN) as usize;
            let len = dlc.min(8).min(msg::RX_DATA_MAX);
            let timestamp_us = ticks_to_micros(get_i64(buffer, msg::RX_TIMESTAMP), tick_hz);
            Some(CanFrame {
                id,
                extended: flags & CAN_MSG_FLAG_EXTENDED != 0,
                data: buffer[msg::RX_DATA..msg::RX_DATA + len].to_vec(),
                timestamp_us,
            })
        }
        CANFDRX_MESSAGE => {
            let id = get_u32(buffer, msg::RX_FRAME_ID);
            let flags = get_u16(buffer, msg::RX_FLAGS);
            let size = get_u8(buffer, msg::RX_LEN) as usize;
            let len = size.min(msg::RX_DATA_MAX);
            let timestamp_us = ticks_to_micros(get_i64(buffer, msg::RX_TIMESTAMP), tick_hz);
            Some(CanFrame {
                id,
                extended: flags & CAN_MSG_FLAG_EXTENDED != 0,
                data: buffer[msg::RX_DATA..msg::RX_DATA + len].to_vec(),
                timestamp_us,
            })
        }
        CAN_ERROR_FRAME | CAN_BUS_EVENT => {
            let id = get_u32(buffer, msg::RX_FRAME_ID);
            let event = get_u32(buffer, msg::EVENT_CODE);
            let timestamp_us = ticks_to_micros(get_i64(buffer, msg::EVENT_TIMESTAMP), tick_hz);
            tracing::debug!(
                "ES582: message type {} id=0x{:X} code=0x{:X}",
                message_type,
                id,
                event
            );
            Some(CanFrame {
                id,
                extended: false,
                data: Vec::new(),
                timestamp_us,
            })
        }
        other => {
            tracing::trace!("ES582: ignoring message type {}", other);
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Device
// ---------------------------------------------------------------------------

/// One opened ES582 CAN channel.
pub struct Es582Device {
    api: Arc<BoaApi>,
    /// OCI URI of the channel.
    uri: String,
    /// Controller handle.
    controller: i32,
    /// Transmit queue handle.
    tx_queue: i32,
    /// Receive queue handle.
    rx_queue: i32,
    /// Negotiated OCI_CAN API version.
    version: BoaVersion,
    /// Channel description reported by the driver.
    info: CanDeviceInfo,
    /// True when CAN-FD framing is active.
    fd: bool,
    /// Human readable bit rate.
    bitrate: String,
    /// Controller clock in ticks per second.
    tick_hz: u32,
    /// How frames are taken off the OCI receive queue.
    rx_mode: RxMode,
    /// Callback receive path state (shared with the OCI `onFrame` callback).
    rx_context: Arc<RxContext>,
    /// Frames read in one batch but not yet handed to the caller (poll mode).
    pending: Mutex<VecDeque<CanFrame>>,
    /// Closed flag (guards double destroy).
    closed: AtomicBool,
}

impl Es582Device {
    /// Open the controller, create the queues and register an accept-all filter.
    fn configure(&mut self, config: &CanDeviceConfig) -> Result<(), CanDeviceError> {
        let nominal = config.baudrate.unwrap_or(DEFAULT_BAUDRATE);
        let fd = config.data_baudrate.is_some();
        let self_reception = if env_flag(ES582_SELF_RECEPTION_ENV) {
            SELF_RECEPTION_ON
        } else {
            SELF_RECEPTION_OFF
        };

        let nominal_timings = CanTiming::candidates(false);
        // Classic CAN has a single "data phase" placeholder so the loops below
        // stay uniform.
        let data_timings = if fd {
            CanTiming::candidates(true)
        } else {
            vec![nominal_timings[0]]
        };

        let mut opened = false;
        let mut last_error = String::new();

        'attempts: for (nominal_index, nominal_timing) in nominal_timings.iter().enumerate() {
            for (data_index, data_timing) in data_timings.iter().enumerate() {
                if nominal_index > 0 || data_index > 0 {
                    // A failed open must not leak controller state, so bind a
                    // fresh controller for every timing attempt.
                    unsafe {
                        (self.api.destroy_can_controller)(self.controller);
                    }
                    self.controller = OCI_NO_HANDLE;
                    let (handle, version) = self.api.create_controller(&self.uri)?;
                    self.controller = handle;
                    self.version = version;
                }

                let mut can_config = self.build_can_config(
                    config,
                    nominal,
                    self_reception,
                    *nominal_timing,
                    *data_timing,
                );
                let mut properties = vec![0u8; STRUCT_BUF];
                set_u32(&mut properties, 0, CONTROLLER_MODE_RUNNING);

                let status = unsafe {
                    (self.api.open_can_controller)(
                        self.controller,
                        can_config.as_mut_ptr(),
                        properties.as_mut_ptr(),
                    )
                };
                if status & BOA_RESULT_FLAG_ERROR == 0 {
                    tracing::debug!(
                        "ES582 {}: opened with nominal [{}] data [{}]",
                        self.uri,
                        nominal_timing.describe(),
                        if fd {
                            data_timing.describe()
                        } else {
                            "-".to_string()
                        }
                    );
                    opened = true;
                    break 'attempts;
                }
                last_error = self.api.error_text(self.controller, status);
                tracing::debug!(
                    "ES582 {}: open rejected for nominal [{}] data [{}]: {}",
                    self.uri,
                    nominal_timing.describe(),
                    if fd {
                        data_timing.describe()
                    } else {
                        "-".to_string()
                    },
                    last_error
                );
            }
        }

        if !opened {
            unsafe {
                (self.api.destroy_can_controller)(self.controller);
            }
            self.controller = OCI_NO_HANDLE;
            return Err(CanDeviceError::OpenFailed(format!(
                "OCI_OpenCANController({} @ {} bit/s{}) failed for every timing \
                 candidate: {}",
                self.uri,
                nominal,
                config
                    .data_baudrate
                    .map(|d| format!("/{}", d))
                    .unwrap_or_default(),
                last_error
            )));
        }

        // Rx queue with NULL callbacks: frames are queued and drained by
        // OCI_ReadCANData* instead of a C callback.
        // Controller clock: needed to turn the hardware timestamps into
        // microseconds. Query it before the receive queue is created so the
        // callback never sees tick_hz == 0.
        self.tick_hz = self.api.timer_tick_hz(self.controller);
        self.rx_context
            .tick_hz
            .store(self.tick_hz, Ordering::Relaxed);

        // Receive queue. In callback mode BOA pushes every accepted frame from
        // its own receive thread into `self.rx_context`; in poll mode the
        // callbacks stay NULL and frames are drained with
        // `OCI_ReadCANDataEx`. `onEvent` stays NULL either way, which makes BOA
        // deliver events through the frame callback / data queue.
        let mut rx_conf = vec![0u8; STRUCT_BUF];
        if self.rx_mode == RxMode::Callback {
            let context = Arc::as_ptr(&self.rx_context) as *mut c_void;
            let callback: CanRxCallbackExFn = es582_rx_callback;
            set_u64(
                &mut rx_conf,
                rx_queue::ON_FRAME_EX,
                callback as usize as u64,
            );
            set_u64(
                &mut rx_conf,
                rx_queue::ON_FRAME_USER_DATA,
                context as usize as u64,
            );
        }
        set_u32(&mut rx_conf, rx_queue::SELF_RECEPTION_MODE, self_reception);
        let mut rx_queue: i32 = OCI_NO_HANDLE;
        let status = unsafe {
            (self.api.create_can_rx_queue)(self.controller, rx_conf.as_mut_ptr(), &mut rx_queue)
        };
        if status & BOA_RESULT_FLAG_ERROR != 0 {
            let text = self.api.error_text(self.controller, status);
            self.destroy();
            return Err(CanDeviceError::OpenFailed(format!(
                "OCI_CreateCANRxQueue failed: {}",
                text
            )));
        }
        self.rx_queue = rx_queue;
        tracing::debug!(
            "ES582 {}: receive queue created in {:?} mode",
            self.uri,
            self.rx_mode
        );

        // Frame filters. OCI OR-combines them, so one filter per accepted CAN ID
        // gives the union the XCP transport asked for. Without any accepted IDs
        // fall back to a single accept-all filter (mask 0).
        let mut filter = vec![0u8; STRUCT_BUF * config.accept_ids.len().max(1)];
        let filter_count = if config.accept_ids.is_empty() {
            set_u32(&mut filter, 0, 0); // frameIDValue
            set_u32(&mut filter, 4, 0); // frameIDMask
            set_u32(&mut filter, 8, 0); // tag
            1u32
        } else {
            for (index, id) in config.accept_ids.iter().enumerate() {
                let base = index * FILTER_SIZE;
                set_u32(&mut filter, base, *id);
                // Exact match on all 29 identifier bits.
                set_u32(&mut filter, base + 4, CAN_ID_MASK);
                set_u32(&mut filter, base + 8, index as u32);
            }
            config.accept_ids.len() as u32
        };
        let status = unsafe {
            (self.api.add_can_frame_filter)(self.rx_queue, filter.as_mut_ptr(), filter_count)
        };
        if status & BOA_RESULT_FLAG_ERROR != 0 {
            let text = self.api.error_text(self.rx_queue, status);
            self.destroy();
            return Err(CanDeviceError::OpenFailed(format!(
                "OCI_AddCANFrameFilter failed: {}",
                text
            )));
        }
        tracing::debug!(
            "ES582 {}: {} hardware frame filter(s) for {:?}",
            self.uri,
            filter_count,
            config.accept_ids
        );

        let mut tx_conf = vec![0u8; STRUCT_BUF];
        let mut tx_queue: i32 = OCI_NO_HANDLE;
        let status = unsafe {
            (self.api.create_can_tx_queue)(self.controller, tx_conf.as_mut_ptr(), &mut tx_queue)
        };
        if status & BOA_RESULT_FLAG_ERROR != 0 {
            let text = self.api.error_text(self.controller, status);
            self.destroy();
            return Err(CanDeviceError::OpenFailed(format!(
                "OCI_CreateCANTxQueue failed: {}",
                text
            )));
        }
        self.tx_queue = tx_queue;

        self.fd = fd;
        self.bitrate = if fd {
            format!(
                "CAN-FD {}k/{}k",
                nominal / 1000,
                config.data_baudrate.unwrap_or(0) / 1000
            )
        } else {
            format!("{} bit/s", nominal)
        };

        let (name, device_id, controller) = parse_uri(&self.uri);
        let caps = self.api.capabilities(self.controller);
        let fd_capable = caps
            .as_ref()
            .map(|c| get_u32(c, cap::CAN_FD_SUPPORT) & CANFD_SUPPORT != 0)
            .unwrap_or(false);
        self.info = CanDeviceInfo {
            driver: "es582".to_string(),
            device_id,
            name,
            device_type: "ES582".to_string(),
            controller,
            channel: self.uri.clone(),
            available: true,
            fd_capable: fd_capable || fd,
        };

        tracing::info!(
            "Opened ES582 {} channel {} (OCI_CAN v{}, {}, tick={} Hz, fd_capable={})",
            self.info.device_type,
            self.info.channel,
            self.version,
            self.bitrate,
            self.tick_hz,
            self.info.fd_capable
        );

        Ok(())
    }

    /// Build the `OCI_CANConfiguration` buffer for one timing candidate.
    fn build_can_config(
        &self,
        config: &CanDeviceConfig,
        nominal: u32,
        self_reception: u32,
        nominal_timing: CanTiming,
        data_timing: CanTiming,
    ) -> Vec<u8> {
        let mut can_config = vec![0u8; STRUCT_BUF];
        set_u32(&mut can_config, cfg::BAUDRATE, nominal);
        set_u32(
            &mut can_config,
            cfg::SAMPLE_POINT,
            nominal_timing.sample_point,
        );
        set_u32(
            &mut can_config,
            cfg::SAMPLES_PER_BIT,
            CAN_ONE_SAMPLE_PER_BIT,
        );
        set_u32(&mut can_config, cfg::BTL_CYCLES, nominal_timing.btl_cycles);
        set_u32(&mut can_config, cfg::SJW, nominal_timing.sjw);
        set_u32(&mut can_config, cfg::SYNC_EDGE, CAN_SINGLE_SYNC_EDGE);
        set_u32(&mut can_config, cfg::PHYSICAL_MEDIA, CAN_MEDIA_HIGH_SPEED);
        set_u32(&mut can_config, cfg::SELF_RECEPTION_MODE, self_reception);
        set_u32(&mut can_config, cfg::BUS_PARTICIPATION_MODE, BUSMODE_ACTIVE);

        // v1.6 gates CAN 2.0 traffic on this flag; earlier versions ignore it.
        set_u32(&mut can_config, cfg::CAN_20_ENABLED, 1);

        if let Some(data_baudrate) = config.data_baudrate {
            set_u32(&mut can_config, cfg::CAN_FD_ENABLED, 1);
            set_u32(&mut can_config, cfg::FD_DATA_BIT_RATE, data_baudrate);
            set_u32(
                &mut can_config,
                cfg::FD_DATA_SAMPLE_POINT,
                data_timing.sample_point,
            );
            set_u32(
                &mut can_config,
                cfg::FD_DATA_BTL_CYCLES,
                data_timing.btl_cycles,
            );
            set_u32(&mut can_config, cfg::FD_DATA_SJW, data_timing.sjw);
            set_u32(&mut can_config, cfg::FD_FLAGS, 0);
            set_u32(&mut can_config, cfg::FD_TX_SSP_OFFSET, 0);
            // Pre-v1.6 CAN-FD receive/transmit selection. Classic CAN frames
            // stay visible (an XCP-on-CAN-FD slave may still answer on a CAN 2.0
            // CTO identifier), and both frame types may be transmitted.
            set_u32(
                &mut can_config,
                cfg::FD_RX_CAN_MODE,
                CANFD_RX_CAN_FRAMES_USING_CAN_MESSAGE,
            );
            set_u32(
                &mut can_config,
                cfg::FD_RX_FD_MODE,
                CANFD_RX_FRAMES_USING_CANFD_MESSAGE,
            );
            set_u32(
                &mut can_config,
                cfg::FD_TX_CONFIG,
                CANFD_TX_CAN_AND_CANFD_FRAMES,
            );
        }

        can_config
    }

    /// Convert a millisecond timeout into controller ticks.
    fn millis_to_ticks(&self, millis: u64) -> i64 {
        if self.tick_hz == 0 {
            // Fall back to a conservative tick rate so we do not block forever.
            return (millis as i64) * 1000;
        }
        ((millis as u128 * self.tick_hz as u128) / 1000) as i64
    }

    /// Read one batch of frames from the receive queue (poll mode only).
    fn read_frames(&self, timeout_ms: u64) -> Result<Vec<CanFrame>, CanDeviceError> {
        if self.rx_queue == OCI_NO_HANDLE {
            return Err(CanDeviceError::RecvFailed(
                "ES582 receive queue is not open".to_string(),
            ));
        }
        let timeout = if timeout_ms == 0 {
            0
        } else {
            self.millis_to_ticks(timeout_ms)
        };

        // The v1.3+ Ex API is required for CAN-FD; older sessions use the
        // classic entry point (which only carries CAN 2.0 frames).
        let use_ex = self.version.at_least(1, 3) && self.api.read_can_data_ex.is_some();
        let mut buffers: Vec<Vec<u8>> = (0..RX_BATCH).map(|_| vec![0u8; STRUCT_BUF]).collect();
        let mut count: u32 = 0;
        let mut remaining: u32 = 0;

        let status = unsafe {
            if use_ex {
                let read = self.api.read_can_data_ex.expect("checked above");
                let mut pointers: Vec<*mut u8> =
                    buffers.iter_mut().map(|b| b.as_mut_ptr()).collect();
                read(
                    self.rx_queue,
                    timeout,
                    pointers.as_mut_ptr(),
                    RX_BATCH as u32,
                    &mut count,
                    &mut remaining,
                )
            } else {
                let read = self.api.read_can_data.ok_or_else(|| {
                    CanDeviceError::RecvFailed(
                        "OCI_ReadCANData is not exported by this BOA version".to_string(),
                    )
                })?;
                // `OCI_ReadCANData` fills a contiguous array of
                // `OCI_CANMessage` (stride 40), so read into a flat buffer and
                // spread the entries over our per-message buffers.
                let mut flat = vec![0u8; RX_BATCH * CAN_MESSAGE_SIZE];
                let status = read(
                    self.rx_queue,
                    timeout,
                    flat.as_mut_ptr(),
                    RX_BATCH as u32,
                    &mut count,
                    &mut remaining,
                );
                if status & BOA_RESULT_FLAG_ERROR == 0 || count > 0 {
                    for (index, buffer) in buffers.iter_mut().enumerate() {
                        let start = index * CAN_MESSAGE_SIZE;
                        let end = start + CAN_MESSAGE_SIZE;
                        if end <= flat.len() {
                            buffer[..CAN_MESSAGE_SIZE].copy_from_slice(&flat[start..end]);
                        }
                    }
                }
                status
            }
        };

        // A timeout with no data is reported as an error code by some OCDs;
        // treat "nothing available" as an empty batch.
        if status & BOA_RESULT_FLAG_ERROR != 0 {
            if count == 0 {
                tracing::trace!(
                    "OCI_ReadCANData returned {} with no data",
                    self.api.error_text(self.rx_queue, status)
                );
                return Ok(Vec::new());
            }
            tracing::debug!(
                "OCI_ReadCANData reported {} after {} frames",
                self.api.error_text(self.rx_queue, status),
                count
            );
        }

        tracing::trace!(
            "ES582 {}: read {} frame(s), {} remaining",
            self.uri,
            count,
            remaining
        );

        let mut frames = Vec::with_capacity(count as usize);
        for buffer in buffers.iter().take(count as usize) {
            if let Some(frame) = decode_frame(buffer, self.tick_hz) {
                frames.push(frame);
            }
        }
        Ok(frames)
    }

    /// Polling receive path (`ES582_RX_MODE=poll`).
    fn recv_frame_polled(
        &self,
        timeout: Option<Duration>,
    ) -> Result<Option<CanFrame>, CanDeviceError> {
        if let Some(frame) = self.pending.lock().expect("ES582 rx mutex").pop_front() {
            return Ok(Some(frame));
        }

        match timeout {
            None => {
                let frames = self.read_frames(RX_POLL_TIMEOUT_MS)?;
                let mut pending = self.pending.lock().expect("ES582 rx mutex");
                pending.extend(frames);
                Ok(pending.pop_front())
            }
            Some(limit) => {
                let deadline = Instant::now() + limit;
                loop {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    let wait_ms = remaining.as_millis().min(100) as u64;
                    let frames = self.read_frames(wait_ms.max(1))?;
                    if !frames.is_empty() {
                        let mut pending = self.pending.lock().expect("ES582 rx mutex");
                        pending.extend(frames);
                        return Ok(pending.pop_front());
                    }
                    if Instant::now() >= deadline {
                        return Ok(None);
                    }
                }
            }
        }
    }

    /// Write one frame to the transmit queue.
    fn write_frame(&self, frame: &CanFrame) -> Result<(), CanDeviceError> {
        if self.tx_queue == OCI_NO_HANDLE {
            return Err(CanDeviceError::SendFailed(
                "ES582 transmit queue is not open".to_string(),
            ));
        }
        let fd = self.fd && frame.data.len() > 8;
        let max_len = if fd { msg::TX_DATA_MAX } else { 8 };
        if frame.data.len() > max_len {
            return Err(CanDeviceError::SendFailed(format!(
                "frame of {} bytes does not fit in a {} frame",
                frame.data.len(),
                if fd { "CAN-FD" } else { "classic CAN" }
            )));
        }

        let mut buffer = vec![0u8; STRUCT_BUF];
        let mut flags = 0u16;
        if frame.extended {
            flags |= CAN_MSG_FLAG_EXTENDED;
        }
        if fd {
            flags |= CAN_MSG_FLAG_FD_DATA | CAN_MSG_FLAG_FD_DATA_BIT_RATE;
        }
        set_u32(
            &mut buffer,
            msg::TYPE,
            if fd { CANFDTX_MESSAGE } else { CAN_TX_MESSAGE },
        );
        set_u32(&mut buffer, msg::TX_FRAME_ID, frame.id);
        set_u16(&mut buffer, msg::TX_FLAGS, flags);
        set_u8(&mut buffer, msg::TX_LEN, frame.data.len() as u8);
        buffer[msg::TX_DATA..msg::TX_DATA + frame.data.len()].copy_from_slice(&frame.data);

        let use_ex = self.version.at_least(1, 3) && self.api.write_can_data_ex.is_some();
        let mut remaining: u32 = 0;
        let status = unsafe {
            if use_ex {
                let write = self.api.write_can_data_ex.expect("checked above");
                let mut pointers: [*mut u8; 1] = [buffer.as_mut_ptr()];
                write(
                    self.tx_queue,
                    TX_TIMEOUT,
                    pointers.as_mut_ptr(),
                    1,
                    &mut remaining,
                )
            } else {
                let write = self.api.write_can_data.ok_or_else(|| {
                    CanDeviceError::SendFailed(
                        "OCI_WriteCANData is not exported by this BOA version".to_string(),
                    )
                })?;
                write(
                    self.tx_queue,
                    TX_TIMEOUT,
                    buffer.as_mut_ptr(),
                    1,
                    &mut remaining,
                )
            }
        };

        if status & BOA_RESULT_FLAG_ERROR != 0 {
            return Err(CanDeviceError::SendFailed(
                self.api.error_text(self.tx_queue, status),
            ));
        }
        Ok(())
    }

    /// Release queues and controller (idempotent).
    fn destroy(&mut self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }

        // Stop the callback before the queue goes away: BOA may still invoke it
        // once after `OCI_DestroyCANController` returns, so the context is
        // intentionally never freed (see `RxContext`).
        self.rx_context.close();
        let dropped = self.rx_context.dropped();
        if dropped > 0 {
            tracing::warn!(
                "ES582 {}: dropped {} frame(s) because the receive queue was full",
                self.uri,
                dropped
            );
        }
        // Leak one reference: the allocation must outlive any late callback.
        let leaked = Arc::clone(&self.rx_context);
        std::mem::forget(leaked);

        // Destroying the controller also releases its queues and filters.
        if self.controller != OCI_NO_HANDLE {
            unsafe {
                (self.api.destroy_can_controller)(self.controller);
            }
            self.controller = OCI_NO_HANDLE;
        }
        self.tx_queue = OCI_NO_HANDLE;
        self.rx_queue = OCI_NO_HANDLE;
    }

    /// Number of frames discarded because the receive queue was full.
    pub fn dropped_frames(&self) -> u64 {
        self.rx_context.dropped()
    }

    /// Receive path in use (callback or polling).
    pub fn rx_mode(&self) -> &'static str {
        match self.rx_mode {
            RxMode::Callback => "callback",
            RxMode::Poll => "poll",
        }
    }

    /// OCI URI of this channel.
    pub fn uri(&self) -> &str {
        &self.uri
    }

    /// Negotiated OCI_CAN API version.
    pub fn api_version(&self) -> String {
        self.version.to_string()
    }

    /// Controller clock in ticks per second.
    pub fn timer_tick_hz(&self) -> u32 {
        self.tick_hz
    }

    /// Current controller state bits (`OCI_CAN_STATE_*`).
    pub fn state_code(&self) -> Option<u32> {
        self.api.controller_state(self.controller)
    }
}

impl CanDevice for Es582Device {
    fn send_frame(&self, frame: &CanFrame) -> Result<(), CanDeviceError> {
        self.write_frame(frame)
    }

    fn recv_frame(&self, timeout: Option<Duration>) -> Result<Option<CanFrame>, CanDeviceError> {
        match self.rx_mode {
            // BOA already pushed the frames from its own receive thread; the
            // only work left is draining the local queue (no USB round trip).
            RxMode::Callback => Ok(self.rx_context.wait(timeout)),
            RxMode::Poll => self.recv_frame_polled(timeout),
        }
    }

    fn close(&mut self) -> Result<(), CanDeviceError> {
        self.destroy();
        Ok(())
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

impl Drop for Es582Device {
    fn drop(&mut self) {
        self.destroy();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_uri_es582() {
        let (name, id, controller) = parse_uri("ETAS:/ES582/CAN:1");
        assert_eq!(name, "ES582");
        assert_eq!(id, "ES582");
        assert_eq!(controller, 1);

        let (name, id, controller) = parse_uri("ETAS:/ES582/1234567/CAN:2");
        assert_eq!(name, "1234567");
        assert_eq!(id, "ES582/1234567");
        assert_eq!(controller, 2);
    }

    #[test]
    fn test_parse_uri_without_port_segment() {
        let (name, id, controller) = parse_uri("ETAS:/ES582");
        assert_eq!(name, "ES582");
        assert_eq!(id, "ES582");
        assert_eq!(controller, 0);
    }

    #[test]
    fn test_sanitize_roundtrip() {
        let id = sanitize_id("ETAS:/ES582/CAN:1");
        assert!(!id.contains(':'));
        assert_eq!(unsanitize_id(&id), "ETAS:/ES582/CAN:1");
    }

    #[test]
    fn test_buffer_helpers() {
        let mut buf = vec![0u8; 64];
        set_u32(&mut buf, 4, 0x1234_5678);
        assert_eq!(get_u32(&buf, 4), 0x1234_5678);
        set_u16(&mut buf, 8, 0xBEEF);
        assert_eq!(get_u16(&buf, 8), 0xBEEF);
        set_u8(&mut buf, 10, 0x42);
        assert_eq!(get_u8(&buf, 10), 0x42);
        set_i64(&mut buf, 16, -12345);
        assert_eq!(get_i64(&buf, 16), -12345);
        // Out-of-range access is tolerated and reads as zero.
        assert_eq!(get_u32(&buf, 4096), 0);
        assert_eq!(get_u8(&buf, 4096), 0);
    }

    #[test]
    fn test_version_at_least() {
        assert!(BoaVersion {
            major: 1,
            minor: 3,
            bugfix: 0,
            build: 0
        }
        .at_least(1, 3));
        assert!(BoaVersion {
            major: 1,
            minor: 6,
            bugfix: 2,
            build: 0
        }
        .at_least(1, 3));
        assert!(!BoaVersion {
            major: 1,
            minor: 2,
            bugfix: 9,
            build: 9
        }
        .at_least(1, 3));
        assert!(!BoaVersion {
            major: 0,
            minor: 9,
            bugfix: 0,
            build: 0
        }
        .at_least(1, 0));
    }

    #[test]
    fn test_uri_reconstruction_matches_parse_uri() {
        // The device id + controller must rebuild the same channel the scan
        // reports, otherwise the fast connect path would bind the wrong port.
        // BOA always emits `ETAS://...`, which is also the canonical form we
        // rebuild; a hand-written `ETAS:/...` channel is used verbatim instead
        // (see `resolve_uri_candidates`), so only the parsed identity has to
        // match here.
        for uri in [
            "ETAS://USB/ES582.1:114913/CAN:1",
            "ETAS://USB/ES582.1:114913/CAN:2",
            "ETAS:/ES582/CAN:1",
        ] {
            let (_, device_id, controller) = parse_uri(uri);
            let wanted = unsanitize_id(&device_id);
            let rebuilt = format!(
                "ETAS://{}/CAN:{}",
                wanted
                    .trim_start_matches("ETAS://")
                    .trim_start_matches("ETAS:/"),
                controller
            );

            let (_, rebuilt_id, rebuilt_controller) = parse_uri(&rebuilt);
            assert_eq!(rebuilt_controller, controller, "controller for {}", uri);
            assert_eq!(
                unsanitize_id(&rebuilt_id),
                wanted,
                "device path for {}",
                uri
            );
            if uri.starts_with("ETAS://") {
                assert_eq!(rebuilt, uri, "canonical URI must round-trip exactly");
            }
        }
    }

    #[test]
    fn test_selection_matches() {
        let uri = "ETAS://USB/ES582.1:114913/CAN:1";

        // Structured selection: device id + controller.
        let config = CanDeviceConfig {
            channel: "auto".into(),
            device_id: Some("USB/ES582.1;114913".into()),
            controller: Some(1),
            ..Default::default()
        };
        assert!(selection_matches(uri, &config));

        // Wrong controller.
        let wrong = CanDeviceConfig {
            controller: Some(2),
            ..config.clone()
        };
        assert!(!selection_matches(uri, &wrong));

        // Wrong device id.
        let wrong = CanDeviceConfig {
            device_id: Some("USB/ES582.1;999999".into()),
            ..config.clone()
        };
        assert!(!selection_matches(uri, &wrong));

        // Channel given as a controller index.
        let by_index = CanDeviceConfig {
            channel: "CAN:1".into(),
            ..Default::default()
        };
        assert!(selection_matches(uri, &by_index));
        let by_index = CanDeviceConfig {
            channel: "2".into(),
            ..Default::default()
        };
        assert!(!selection_matches(uri, &by_index));

        // Auto accepts anything.
        assert!(selection_matches(
            uri,
            &CanDeviceConfig {
                channel: "auto".into(),
                ..Default::default()
            }
        ));
    }

    #[test]
    fn test_api_versions_are_descending() {
        for pair in API_VERSIONS.windows(2) {
            let (newer, older) = (pair[0], pair[1]);
            assert!(
                (newer.major, newer.minor) > (older.major, older.minor),
                "{} should be newer than {}",
                newer,
                older
            );
        }
    }

    /// Structural sanity check of the field offsets we rely on: they must stay
    /// inside the buffer and follow the documented order. The numbers come from
    /// `offsetof` on the ETAS headers (MSVC x64).
    #[test]
    fn test_structure_offsets_are_consistent() {
        assert_eq!(cfg::BAUDRATE, 0);
        assert_eq!(cfg::CAN_FD_CONFIG, 40);
        assert_eq!(cfg::CAN_TX_POLICY, 80);
        assert_eq!(cfg::CAN_XL_CONFIG, 88);
        assert_eq!(cfg::CAN_20_ENABLED, 112);
        assert_eq!(cfg::CAN_20_ENABLED + 4, 116); // sizeof(OCI_CANConfiguration)
        assert_eq!(cfg::FD_TX_CONFIG, 72);
        assert_eq!(cfg::FD_TX_SSP_FILTER_WINDOW + 2, 78); // end of canFDConfig
        assert!(cfg::FD_TX_SSP_FILTER_WINDOW + 4 <= cfg::CAN_TX_POLICY);

        assert_eq!(rx_queue::SELF_RECEPTION_MODE, 32);

        assert_eq!(cap::CAN_FD_SUPPORT, 28);
        assert_eq!(cap::CAN_FD_RX_CAN_MODE, 44);
        assert_eq!(cap::CAN_TX_POLICIES, 60);
        assert_eq!(cap::CAN_TX_POLICIES + 4, 64);
        assert!(cap::CAN_TX_POLICIES + 4 <= STRUCT_BUF);

        assert_eq!(status::STATE_CODE, 4);
        assert_eq!(timer::TICK_FREQUENCY, 44);

        // OCI_CANMessage: type(0) + reserved(4) + union at 8, sizeof 40.
        assert_eq!(msg::PAYLOAD, 8);
        assert_eq!(msg::RX_TIMESTAMP, 8);
        assert_eq!(msg::RX_TAG, 16);
        assert_eq!(msg::RX_FRAME_ID, 20);
        assert_eq!(msg::RX_FLAGS, 24);
        assert_eq!(msg::RX_LEN, 27);
        assert_eq!(msg::RX_DATA, 32);
        assert_eq!(msg::RX_DATA + msg::RX_DATA_MAX, 96);

        assert_eq!(msg::TX_FRAME_ID, 8);
        assert_eq!(msg::TX_FLAGS, 12);
        assert_eq!(msg::TX_LEN, 15);
        assert_eq!(msg::TX_DATA, 16);
        assert_eq!(msg::TX_DATA + msg::TX_DATA_MAX, 80);

        assert_eq!(CAN_MESSAGE_SIZE, 40);
        assert!(msg::RX_DATA + msg::RX_DATA_MAX <= STRUCT_BUF);
        assert!(msg::TX_DATA + msg::TX_DATA_MAX <= STRUCT_BUF);

        // The receive callback is handed an `OCI_CANMessageEx`; the decoder
        // reads exactly this many bytes.
        assert_eq!(CAN_MESSAGE_EX_SIZE, 96);
        assert_eq!(msg::RX_DATA + msg::RX_DATA_MAX, CAN_MESSAGE_EX_SIZE);

        // `OCI_CANRxQueueConfiguration`: two callback slots of 16 bytes, then
        // the self-reception mode at offset 32 (sizeof 40).
        assert_eq!(rx_queue::ON_FRAME_EX, 0);
        assert_eq!(rx_queue::ON_FRAME_USER_DATA, 8);
        assert_eq!(rx_queue::ON_EVENT_EX, 16);
        assert_eq!(rx_queue::ON_EVENT_USER_DATA, 24);
        assert_eq!(rx_queue::SELF_RECEPTION_MODE, 32);
        assert!(rx_queue::SELF_RECEPTION_MODE + 4 <= STRUCT_BUF);
    }

    /// Build a synthetic `OCI_CANMessageEx` holding a classic CAN frame.
    fn classic_rx_buffer(id: u32, flags: u16, data: &[u8], ticks: i64) -> Vec<u8> {
        let mut buffer = vec![0u8; CAN_MESSAGE_EX_SIZE];
        set_u32(&mut buffer, msg::TYPE, CAN_RX_MESSAGE);
        set_i64(&mut buffer, msg::RX_TIMESTAMP, ticks);
        set_u32(&mut buffer, msg::RX_FRAME_ID, id);
        set_u16(&mut buffer, msg::RX_FLAGS, flags);
        set_u8(&mut buffer, msg::RX_LEN, data.len() as u8);
        buffer[msg::RX_DATA..msg::RX_DATA + data.len()].copy_from_slice(data);
        buffer
    }

    #[test]
    fn test_decode_classic_frame() {
        let buffer = classic_rx_buffer(0x551, 0, &[0xFF, 0x01, 0x02], 2_000_000);
        let frame = decode_frame(&buffer, 2_000_000).expect("frame");
        assert_eq!(frame.id, 0x551);
        assert!(!frame.extended);
        assert_eq!(frame.data, vec![0xFF, 0x01, 0x02]);
        // 2 MHz ticks -> 500 ns per tick.
        assert_eq!(frame.timestamp_us, Some(1_000_000));
    }

    #[test]
    fn test_decode_extended_and_fd_frames() {
        let buffer = classic_rx_buffer(
            0x18DAF110,
            CAN_MSG_FLAG_EXTENDED,
            &[1, 2, 3, 4, 5, 6, 7, 8],
            0,
        );
        let frame = decode_frame(&buffer, 0).expect("frame");
        assert_eq!(frame.id, 0x18DAF110);
        assert!(frame.extended);
        assert_eq!(frame.data.len(), 8);
        assert_eq!(
            frame.timestamp_us, None,
            "tick rate unknown -> no timestamp"
        );

        let mut buffer = vec![0u8; CAN_MESSAGE_EX_SIZE];
        set_u32(&mut buffer, msg::TYPE, CANFDRX_MESSAGE);
        set_u32(&mut buffer, msg::RX_FRAME_ID, 0x700);
        set_u8(&mut buffer, msg::RX_LEN, 16);
        for (index, slot) in buffer[msg::RX_DATA..msg::RX_DATA + 16]
            .iter_mut()
            .enumerate()
        {
            *slot = index as u8;
        }
        let frame = decode_frame(&buffer, 0).expect("fd frame");
        assert_eq!(frame.data.len(), 16);
        assert_eq!(frame.data[15], 15);
    }

    #[test]
    fn test_decode_ignores_unknown_message_type() {
        let mut buffer = vec![0u8; CAN_MESSAGE_EX_SIZE];
        set_u32(&mut buffer, msg::TYPE, 42);
        assert!(decode_frame(&buffer, 0).is_none());
    }

    #[test]
    fn test_rx_context_push_wait_and_close() {
        let context = RxContext::new();
        context.tick_hz.store(2_000_000, Ordering::Relaxed);

        // Nothing queued: `None` only waits the short slice.
        let start = Instant::now();
        assert!(context.wait(None).is_none());
        assert!(start.elapsed() >= RX_WAIT_SLICE);

        context.push(CanFrame::new(0x551, vec![0xFF]));
        context.push(CanFrame::new(0x552, vec![0xFE]));
        let first = context.wait(None).expect("first frame");
        assert_eq!(first.id, 0x551);
        let second = context
            .wait(Some(Duration::from_millis(10)))
            .expect("second frame");
        assert_eq!(second.id, 0x552);

        // After close, waiters return immediately and the callback no-ops.
        context.close();
        let start = Instant::now();
        assert!(context.wait(Some(Duration::from_secs(5))).is_none());
        assert!(start.elapsed() < Duration::from_secs(1));
        context.push(CanFrame::new(0x553, vec![0x00]));
        assert!(context.wait(None).is_none());
    }

    #[test]
    fn test_rx_context_drops_oldest_when_full() {
        let context = RxContext::new();
        for index in 0..(RX_QUEUE_CAPACITY + 5) {
            context.push(CanFrame::new(index as u32, vec![0u8]));
        }
        assert_eq!(context.dropped(), 5);
        // The five oldest frames were dropped.
        let first = context.wait(None).expect("frame");
        assert_eq!(first.id, 5);
    }

    #[test]
    fn test_rx_mode_from_env_defaults_to_callback() {
        // The environment is process-wide, so only assert the default mapping
        // (an unset variable) and the explicit poll aliases.
        std::env::remove_var(ES582_RX_MODE_ENV);
        assert_eq!(RxMode::from_env(), RxMode::Callback);
        std::env::set_var(ES582_RX_MODE_ENV, "poll");
        assert_eq!(RxMode::from_env(), RxMode::Poll);
        std::env::set_var(ES582_RX_MODE_ENV, "callback");
        assert_eq!(RxMode::from_env(), RxMode::Callback);
        std::env::remove_var(ES582_RX_MODE_ENV);
    }
}
