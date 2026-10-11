# XCP Client

A Rust implementation of XCP (Universal Measurement and Calibration Protocol) Master client for automotive ECU development. Supports multiple slave connections with independent A2L data management.

## Features

- **Multi-Slave Support**: Connect to multiple XCP slaves simultaneously
- **Connection Management**: CONNECT, DISCONNECT, GET_ID commands
- **Memory Access**: UPLOAD, DOWNLOAD, SHORT_UPLOAD, SHORT_DOWNLOAD
- **Calibration**: Read/write calibration parameters with device association
- **DAQ (Data Acquisition)**: Configure and run DAQ lists per slave
- **Checksum Verification**: BUILD_CHECKSUM with 9 XCP algorithms (ADD, CRC)
- **A2L Integration**: Each slave has independent A2L calibration/measurement data
- **HEX File Support**: Intel HEX parser for firmware reference images
- **CLI Subcommands**: Non-interactive `calibrate`, `verify`, and `daq-record` for CI/automation
- **Web UI**: Browser-based interface with multi-slave management
- **Desktop App (Tauri)**: Native cross-platform GUI with Experiment and Compare workflows
- **TUI**: Interactive terminal UI for single-device debugging
- **Transport Abstraction**: UDP, TCP and CAN (XCP-on-CAN over PEAK-System PCAN-Basic) with a pluggable CAN device-driver layer
- **CAN Device Discovery**: Enumerate CAN adapters/channels and pick one (by stable device id) from the desktop UI

## Supported Data Types

| Category | Types |
|----------|-------|
| Unsigned Integers | U8, U16, U32, U64 |
| Signed Integers | I8, I16, I32, I64 |
| Floating Point | F32, F64 |
| Other | Bool, Arrays, Matrices |
| Characteristic Types | VALUE, CURVE, MAP, VAL_BLK |

## Architecture

```
main.rs (Application Entry)
    │
lib.rs → client.rs (XcpClient - Multi-Slave Manager)
    │
    ├── protocol.rs        # XCP Protocol Layer entrypoint
    │   └── protocol/
    │       ├── types.rs       # Command codes, error codes, MTA, ConnectInfo
    │       ├── commands.rs    # Command builders
    │       └── responses.rs   # Response parsers
    │
    ├── transport.rs       # Transport Abstraction Layer entrypoint
    │   └── transport/
    │       ├── transport.rs   # Transport trait and factory
    │       ├── udp.rs         # UDP transport (XCP-on-UDP/IP)
    │       ├── tcp.rs         # TCP transport (XCP-on-TCP/IP)
    │       └── can/           # XCP-on-CAN transport + device drivers
    │           ├── mod.rs     #   CanTransport (framing, ID filtering, MAX_DLC)
    │           ├── device.rs  #   CanDevice/CanDriver traits + registry
    │           └── pcan.rs    #   PEAK-System PCAN-Basic driver
    │
    ├── slave.rs           # XcpSlave - Individual slave context
    ├── calibration.rs     # Calibration parameter management
    ├── daq.rs             # DAQ management
    ├── checksum.rs        # BUILD_CHECKSUM algorithms & verification
    ├── hex.rs             # Intel HEX file parser
    ├── variable.rs        # Device-associated variables
    ├── calibrate.rs       # CLI calibrate subcommand
    ├── verify.rs          # CLI verify subcommand
    ├── daq_record.rs      # CLI daq-record subcommand
    │
    ├── a2l.rs             # A2L description file support entrypoint
    │   └── a2l/
    │       ├── reader.rs      # A2L file parser
    │       ├── types.rs       # A2L data types
    │       └── compute_method.rs  # COMPU_METHOD conversion
    │
    ├── tui.rs             # Terminal UI entrypoint
    │   └── tui/
    │       └── app.rs         # Ratatui-based interactive CLI
    │
    └── web.rs             # Web UI entrypoint
        └── web/
            ├── server.rs      # Axum HTTP server
            ├── auth.rs        # Session authentication
            ├── routes.rs      # HTTP route handlers
            ├── state.rs       # Multi-slave state management
            └── templates.rs   # HTML templates

src-tauri/ (Tauri desktop backend)
    ├── Cargo.toml         # Tauri crate, depends on xcp_client
    ├── tauri.conf.json    # Window config, frontend dist path
    └── src/
        ├── lib.rs         # Tauri builder, invoke_handler registration
        ├── state.rs       # TauriAppState (mirrors web state, no Axum)
        └── commands/
            ├── device.rs       # Device CRUD & connection commands
            ├── a2l.rs          # A2L file loading
            ├── calibration.rs  # Read/write calibration parameters
            ├── daq.rs          # DAQ selection, start/stop, data polling
            ├── daq_live.rs     # Live DAQ streaming helpers
            ├── checksum.rs     # HEX load & BUILD_CHECKSUM verify
            ├── compare.rs      # Compare-session management
            ├── experiment.rs   # Combined calibration+DAQ experiment mode
            └── helpers.rs      # A2l ↔ CalibrationParam/Measurement conversion

ui/ (Vue 3 frontend)
    ├── package.json       # Vue 3, vue-router, @tauri-apps/api, vite
    ├── vite.config.js     # Fixed port 5173, relative base path
    └── src/
        ├── App.vue        # Root layout: sidebar nav + <router-view>
        ├── main.js        # Vue app bootstrap, router config
        └── components/
            ├── Dashboard.vue      # Device overview & connection panel
            ├── DeviceManager.vue  # Add/remove/configure devices
            ├── Experiment.vue     # Combined calibration + DAQ panel
            ├── Compare.vue        # Multi-session calibration comparison
            ├── Calibration.vue    # Calibration sub-component
            ├── DAQ.vue            # DAQ sub-component
            └── Checksum.vue       # Flash checksum verification
```

## Requirements

- Rust 1.70 or later
- One or more XCP slaves (ECU simulators) running on target addresses

## Build

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release
```

## Mock slave integration tests

The repository now includes a localhost mock XCP slave for integration and end-to-end testing.

- Test binary: `tests\mock_xcp_integration.rs`
- Mock server: `tests\support\mock_slave\...`
- Transport: **UDP only** for the first pass
- Fixed bind: `127.0.0.1:5555`

Run the mocked-slave suite serially:

```bash
cargo test --test mock_xcp_integration -- --test-threads=1
```

The suite covers:

- raw XCP-on-UDP framing
- client connect and `GET_ID`
- calibration-page reads and writes over real UDP transport
- chunked upload/download and `BUILD_CHECKSUM`
- DAQ metadata queries and live DTO streaming into `DaqManager`

The mock is an in-repo Rust test server, not a transport stub, so the repository exercises its real UDP transport, message router, slave API, and DAQ decoding path.

TCP support is intentionally left as a follow-up extension for the mock server.

## Usage

### Transport Configuration

The transport abstraction supports:

- **UDP**: datagram-based XCP-on-Ethernet transport (4-byte `LEN` + packet counter header)
- **TCP**: stream-based XCP-on-Ethernet transport with the same `LEN`/counter header as UDP
- **CAN**: XCP-on-CAN with a pluggable CAN device-driver layer. One XCP packet maps to
  one CAN frame; there is **no** `LEN`/`COUNTER` header (the CAN DLC is the length and the
  XCP packet starts at data byte 0). Classic CAN and CAN-FD are supported.

CAN configuration is normally derived from the A2L `XCP_ON_CAN` block (`BAUDRATE`,
`CAN_FD_DATA_TRANSFER_BAUDRATE`, `CAN_ID_SLAVE`, `DAQ_LIST_CAN_ID`,
`EVENT_CAN_ID_LIST`, `MAX_DLC_REQUIRED`) and can be overridden from the desktop UI by
picking a specific adapter/channel.

```rust
use xcp_client::transport::{list_can_devices, TransportConfig};

// Ethernet
let udp = TransportConfig::Udp { addr: "127.0.0.1:5555".to_string() };
let tcp = TransportConfig::Tcp { addr: "127.0.0.1:5555".to_string() };

// CAN (XCP-on-CAN over PCAN)
let can = TransportConfig::Can {
    driver: Some("pcan".to_string()),   // None = default driver ("pcan")
    channel: "auto".to_string(),        // or e.g. "PCAN_USBBUS1"
    device_id: None,                    // Some("3") pins a stable device id
    controller: None,
    can_id: 0x7fb,                      // CAN_ID_MASTER
    baudrate: Some(500_000),
    data_baudrate: Some(2_000_000),     // Some = CAN-FD
    slave_id: Some(0x7fc),              // CAN_ID_SLAVE
    daq_ids: vec![0x7fd],               // DAQ/EVENT DTO CAN IDs
    max_dlc_required: true,
};

// Enumerate attached CAN channels (driver + device_id + controller + channel)
for dev in list_can_devices() {
    println!("{}", dev.display_label());
}
```

CAN environment variables:

- `PCAN_BASIC_DLL` — path to `PCANBasic.dll` (either the file or its directory)
- `PCAN_BITRATE` — fallback bit rate used only when the config declares none
- `PCAN_FD_CLOCK` — CAN-FD controller clock in Hz used to derive timing (default 80 MHz)

See [`PCAN_CAN_QA.md`](PCAN_CAN_QA.md) for the CAN/PCAN design Q&A and
[`src/transport/can/device.rs`](src/transport/can/device.rs) for how to add a new
CAN device driver.

### Run Web UI

```bash
# Run with default settings (localhost:8080)
cargo run --release -- web

# Run with custom host and port (allow external access)
cargo run --release -- web --host 0.0.0.0 --port 8080
```

The Web UI provides browser-based access with the following features:

- **Dashboard**: View all connected slaves and system overview
- **Connect**: Connect to multiple XCP slaves with custom names
- **Slaves**: Manage connected slaves (select/disconnect)
- **Calibration**: Browse, read, and write calibration parameters with device association
- **DAQ**: Select measurements per slave and monitor real-time data
- **A2L**: Load A2L files for each slave independently

#### Authentication

- **Username**: `admin`
- **Password**: `admin`

Note: The Web UI supports only a single concurrent user. If a user is already logged in, subsequent login attempts will be rejected until the current user logs out.

#### Multi-Slave Workflow

1. **Login**: Access http://localhost:8080/login
   - Username: `admin`, Password: `admin`

2. **Connect First Slave**:
   - Click "Connect New Slave"
   - Name: `ECU1`
   - Address: `127.0.0.1:5555`
   - Click Connect

3. **Load A2L**:
   - Navigate to `/a2l`
   - Enter A2L file path: `hello_xcp.a2l`
   - Click Load

4. **View Calibration Variables**:
   - Navigate to `/calibration`
   - Variables displayed as: `[ECU1] variable_name`

5. **Start DAQ**:
   - Navigate to `/daq`
   - Select measurements for current slave
   - Click Start

6. **Connect Additional Slaves**:
   - Repeat steps 2-5 for each slave
   - Each slave gets independent A2L data

7. **Switch Between Slaves**:
   - On Dashboard, click "Select" to switch current slave
   - Or use slave selector in DAQ page

8. **Disconnect Slaves**:
   - Click "Disconnect" to disconnect specific slave
   - Or use main Disconnect to disconnect current slave

### Calibrate ECU Parameters (CLI)

Write calibration parameters to an ECU non-interactively. Two modes are supported:

#### Single Variable Mode

Write one parameter at a time:

```bash
# Write a scalar value
cargo run --release -- calibrate --a2l ecu.a2l --var my_params.delay --value 100

# Write an array value (comma-separated)
cargo run --release -- calibrate --a2l ecu.a2l --var my_params.array --value "1,2,3,4"

# Override slave address (default is read from A2L)
cargo run --release -- calibrate --a2l ecu.a2l --var my_params.delay --value 100 --addr 192.168.1.10:5555
```

#### DCM File Mode

Write multiple parameters from a DCM (Data Conservation and Measurement) file:

```bash
# Apply all matching parameters from a DCM file
cargo run --release -- calibrate --a2l ecu.a2l --dcm calibration.DCM

# With address override
cargo run --release -- calibrate --a2l ecu.a2l --dcm calibration.DCM --addr 192.168.1.10:5555
```

DCM files contain physical values (display units). The tool automatically converts physical values to raw ECU representation using the COMPU_METHOD defined in the A2L file before writing.

Supported DCM parameter types: FESTWERT (scalar), FESTWERTEBLOCK (array), KENNLINIE (curve), GRUPPENKENNFELD (map).

### Record DAQ Measurements (CLI)

Record XCP DAQ measurements to MF4 and/or CSV files non-interactively. Runs until CTRL+C, then gracefully stops DAQ and saves the output files.

```bash
cargo run --release -- daq-record --config daq_record.config.toml
```

#### Config File Format (TOML)

Create a config file specifying the A2L, output files, connection, and measurements:

```toml
a2l = "ecu.a2l"                    # Path to A2L description file (required)

[connection]
addr = "192.168.1.100:5555"        # Optional: falls back to A2L slave address

[output]
mf4 = "recording.mf4"             # Optional (at least one of mf4/csv required)
csv = "recording.csv"              # Optional (at least one of mf4/csv required)

[daq]
event_channel = 0                  # Default event channel

# Group measurements by event channel (omit to record all from A2L)
[[events]]
channel = 0
measurements = ["vehicle_speed", "engine_rpm", "accel_pedal"]

[[events]]
channel = 1
measurements = ["coolant_temp", "oil_temp"]
```

- If `[connection].addr` is omitted, the slave address is read from the A2L file
- If `[[events]]` is omitted, all measurements from the A2L file are selected
- Both MF4 and CSV can be recorded simultaneously; at least one `[output]` path is required
- Invalid measurement names are warned and skipped (recording continues with valid ones)
- On CTRL+C: recording stops → DAQ stops → `FREE_DAQ` restores the slave → disconnect

### Verify ECU Flash Checksums (CLI)

Verify ECU flash memory integrity by comparing BUILD_CHECKSUM results against an Intel HEX reference file:

```bash
# Basic verification
cargo run --release -- verify --a2l ecu.a2l --hex firmware.hex

# With custom max block size (default: 16 KiB)
cargo run --release -- verify --a2l ecu.a2l --hex firmware.hex --max-block-size 4096

# With address override
cargo run --release -- verify --a2l ecu.a2l --hex firmware.hex --addr 192.168.1.10:5555
```

The verify workflow:
1. Loads A2L file to determine memory segments and checksum algorithm
2. Loads Intel HEX file as the reference image
3. Connects to the XCP slave
4. For each memory segment, computes the reference checksum from the HEX data and requests the slave's BUILD_CHECKSUM
5. Compares results and prints a per-segment pass/fail report

Example output:
```
=== Checksum Verification Report ===
Segment  Address Range              Size      Algorithm  Status
──────────────────────────────────────────────────────────────────
0        0x80000000..0x8001FFFF     128 KiB   CRC_32     ✓ PASS
1        0x80020000..0x8003FFFF     128 KiB   CRC_32     ✗ FAIL
  Reference: 0xA3B2C1D0  Slave: 0x12345678
```

### Run Interactive CLI (TUI)

```bash
cargo run
```

The CLI provides an interactive menu for single-device debugging:

```
XCP Client Menu:
1. Connect to slave
2. Disconnect from slave
3. List calibration parameters
4. Read parameter value
5. Write parameter value
6. Start DAQ
7. Stop DAQ
8. Exit
```

---

## Tauri Desktop App

The Tauri desktop app provides a native cross-platform GUI for XCP device management, calibration, and DAQ recording. It embeds the same `xcp_client` Rust library and exposes its full feature set through a Vue 3 frontend.

### Environment Setup

**Prerequisites**

| Tool | Version | Purpose |
|------|---------|---------|
| Rust | stable (1.70+) | Tauri backend |
| Node.js | 18+ | Frontend build |
| npm | 9+ | Package manager |
| Tauri CLI | 2.x | Dev server & bundler |

Install the Tauri CLI:

```bash
cargo install tauri-cli --version "^2"
```

Install frontend dependencies:

```bash
cd ui
npm install
cd ..
```

> **Windows users**: Also install [WebView2](https://developer.microsoft.com/en-us/microsoft-edge/webview2/) (usually pre-installed on Windows 10/11 or included in the Tauri installer).

### Development

Start the app in development mode with hot-reload:

```bash
cargo tauri dev
```

This automatically:
1. Starts the Vite dev server on `http://localhost:5173` (`ui/`)
2. Compiles the Rust Tauri backend (`src-tauri/`)
3. Opens the native window pointing to the dev server

You can also start the frontend and backend separately:

```bash
# Terminal 1: start the Vite dev server
cd ui && npm run dev

# Terminal 2: start Tauri (will connect to the running Vite server)
cargo tauri dev
```

### Release Build

Build a distributable installer:

```bash
cargo tauri build
```

Output installers are placed in `src-tauri/target/release/bundle/`:

| Platform | Format |
|----------|--------|
| Windows  | `.msi`, `.exe` (NSIS) |
| macOS    | `.dmg`, `.app` |
| Linux    | `.deb`, `.AppImage` |

### Configuration

Devices and their A2L/HEX paths are persisted in `config.toml` at the project root. The file is read on startup and written automatically when you add/remove devices from the UI.

```toml
[[devices]]
id = "device_0"
name = "ECU1"
address = "192.168.1.100:5555"
a2l_path = "ecu1.a2l"
hex_path = "firmware.hex"
max_cto = 255
```

`max_cto` is optional (default: 255). If omitted or `0`, the value from the A2L file is used.

### App Pages

The sidebar provides navigation to four main pages:

| Page | Route | Description |
|------|-------|-------------|
| **Dashboard** | `/` | Connected device overview; quick connect/disconnect |
| **Devices** | `/devices` | Add, remove, configure, and A2L/HEX-load devices |
| **Experiment** | `/experiment` | Combined calibration + DAQ panel for live tuning |
| **Compare** | `/compare` | Multi-session calibration comparison |

#### Dashboard

Shows all configured devices with connection status, A2L filename, and quick-action buttons. Clicking **Connect All** connects every device listed in `config.toml`.

#### Devices

Full device management:
- Add a new device (name + address + optional A2L/HEX paths)
- Load or reload an A2L file for any device (file dialog)
- Load an Intel HEX file for checksum verification
- Rename devices, remove devices
- Connect / disconnect individual devices
- Choose the transport protocol (Auto / UDP / TCP / CAN)
- When CAN is selected, pick a specific CAN adapter/channel from the enumerated list
  (defaults to the first available channel; selection is pinned by stable device id)

#### Experiment

The main working page. Combines calibration browsing and DAQ recording in a single split-panel view:

- **Left panel (Calibration)**: Browse parameters, read/write values, switch Working/Reference page mode, export changes to DCM
- **Right panel (DAQ)**: Select measurements, set event channels, start/stop recording; live values update in real time
- **Checksum tab**: Load HEX file and run `BUILD_CHECKSUM` verification per memory segment

#### Compare

Creates named comparison sessions. For each session:
1. Choose a source device and capture its current calibration values
2. Load a DCM file as the reference
3. View a side-by-side diff of physical values (ECU live vs. reference)

### Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                         Tauri Desktop App                           │
│                                                                     │
│  ┌──────────────────────┐          ┌───────────────────────────┐   │
│  │  Frontend (ui/)      │          │  Backend (src-tauri/)     │   │
│  │                      │  invoke  │                           │   │
│  │  Vue 3 + Vite        │ ──────►  │  Tauri 2 + Rust           │   │
│  │  @tauri-apps/api     │ ◄──────  │  TauriAppState            │   │
│  │  vue-router          │  result  │  Commands (device, cal,   │   │
│  │                      │          │  daq, compare, checksum,  │   │
│  │  Components:         │          │  experiment, a2l)         │   │
│  │  Dashboard           │          │         │                 │   │
│  │  DeviceManager       │          │         ▼                 │   │
│  │  Experiment          │          │  xcp_client (Rust lib)    │   │
│  │  Compare             │          │  XcpClient, CalManager,   │   │
│  │                      │          │  DaqManager, A2lReader,   │   │
│  └──────────────────────┘          │  HexImage, ChecksumVerify │   │
│                                    └───────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
```

- **Frontend → Backend**: calls go through `@tauri-apps/api` `invoke()` which dispatches to registered Tauri commands.
- **Backend**: `TauriAppState` (in `src-tauri/src/state.rs`) holds the `XcpClient` and per-device `CalibrationManager` / `DaqManager` behind `Arc<RwLock<_>>`. It mirrors the Web UI state but has no Axum dependency.
- **Library**: `src-tauri` depends on the `xcp_client` crate via a local path dependency (`path = ".."`). No network server is started; all communication is in-process.

### Tauri Commands Reference

All backend functions are registered with `tauri::generate_handler![]` in `src-tauri/src/lib.rs`. Each command is an `async fn` decorated with `#[tauri::command]`.

#### Pattern (Rust side)

```rust
// src-tauri/src/commands/device.rs
#[tauri::command]
pub async fn connect_device(
    state: State<'_, SharedState>,   // injected shared state
    device_id: String,               // deserialized from JS invoke args
    transport_protocol: Option<String>,
) -> Result<Value, String> {         // Ok(json!({...})) or Err(msg)
    let app = state.read().await;
    // ... do work with app.client, app.devices ...
    Ok(json!({ "success": true, "message": "Connected" }))
}
```

Commands that push live data to the frontend take an additional `AppHandle`:

```rust
#[tauri::command]
pub async fn daq_start(
    app: AppHandle,                  // used to emit Tauri events
    state: State<'_, SharedState>,
    device_id: String,
) -> Result<Value, String> {
    // spawns background task that calls app.emit("daq-data", payload)
    spawn_live_daq_event_pump(app, ...);
    Ok(json!({ "success": true }))
}
```

#### Pattern (Vue 3 side)

```js
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

// Request/response (calibration read, device connect, etc.)
const result = await invoke('connect_device', {
  deviceId: device.id,
  transportProtocol: 'udp',   // camelCase → snake_case auto-converted
})
if (result.success) { ... }

// Subscribe to live DAQ data pushed from Rust
const unlisten = await listen('daq-data', (event) => {
  const { device_id, values } = event.payload
  updateChart(device_id, values)
})
// Call unlisten() to stop listening
```

> **Naming convention**: JS `camelCase` argument names are automatically converted to Rust `snake_case` parameter names by the Tauri IPC layer (e.g. `deviceId` → `device_id`).

#### Full Command Table

| Module | Command | Rust signature summary |
|--------|---------|------------------------|
| **device** | `list_devices` | `() → { devices: [...] }` |
| | `list_can_devices` | `() → { drivers: [...], devices: [...] }` |
| | `add_device` | `(name, address?) → { device_id }` |
| | `connect_device` | `(device_id, transport_protocol?, can_device?) → { success }` |
| | `disconnect_device` | `(device_id) → { success }` |
| | `remove_device` | `(device_id) → { success }` |
| | `rename_device` | `(device_id, name) → { success }` |
| | `connect_all_devices` | `(transport_protocols?, can_devices?) → { connected }` |
| **a2l** | `load_a2l` | `(device_id, file_path) → { success, cal_count, meas_count }` |
| **calibration** | `cal_available` | `() → { params: [...] }` |
| | `cal_read` | `(device_id, idx) → { value, display }` |
| | `cal_read_all` | `(items: [{device_id, idx}]) → { results: [...] }` |
| | `cal_write` | `(device_id, idx, value) → { success }` |
| | `cal_write_element` | `(device_id, idx, row, col, value) → { success }` |
| | `cal_value` | `(device_id, idx) → cached value` |
| | `cal_detail` | `(device_id, idx) → full param info` |
| | `get_page_mode` | `() → "working" \| "reference"` |
| | `switch_page_mode` | `(device_id, mode) → { success }` |
| **daq** | `daq_available` | `() → { measurements: [...] }` |
| | `daq_start` | `(device_id) → { success }` |
| | `daq_stop` | `(device_id) → { success }` |
| | `daq_status` | `() → { running: bool }` |
| | `daq_toggle` | `(device_id, idx) → { success }` |
| | `daq_set_selection` | `(device_id, indices: [int]) → { success }` |
| | `daq_clear` | `(device_id) → { success }` |
| | `daq_data` | `(device_id) → latest sample` |
| | `daq_events` | `(device_id) → available events` |
| | `daq_set_event` | `(device_id, event_channel) → { success }` |
| | `daq_set_measurement_events` | `(device_id, mappings) → { success }` |
| | `daq_start_recording` | `(device_id, config) → { success }` |
| | `daq_stop_recording` | `(device_id) → { success }` |
| | `daq_download_csv` | `(device_id) → csv string` |
| **checksum** | `hex_load` | `(device_id, file_path) → { success }` |
| | `checksum_verify` | `(device_id) → { report: [...] }` |
| **compare** | `compare_sessions` | `() → { sessions: [...] }` |
| | `compare_create_session` | `() → { session_id }` |
| | `compare_rename_session` | `(session_id, name) → { success }` |
| | `compare_delete_session` | `(session_id) → { success }` |
| | `compare_load_source` | `(session_id, side, source) → { success }` |
| | `compare_clear_source` | `(session_id, side) → { success }` |
| | `compare_variables` | `(session_id, ...) → { variables }` |
| | `compare_select_variables` | `(session_id, names) → { success }` |
| | `compare_result` | `(session_id) → { diff: [...] }` |
| **experiment** | `experiment_variables` | `() → { measurements, calibrations }` |
| | `experiment_start` | `(config) → { success }` |
| | `experiment_stop` | `() → { success }` |
| | `experiment_status` | `() → { running }` |
| | `experiment_pagemode` | `(mode) → { success }` |
| | `experiment_rename_slave` | `(device_id, name) → { success }` |
| | `experiment_checksum` | `(config) → { report }` |
| | `experiment_cal_changed` | `(device_id, idx) → bool` |
| | `experiment_dcm_export` | `(device_id, path) → { success }` |
| | `experiment_cal_write` | `(device_id, idx, value) → { success }` |
| | `experiment_cal_original` | `(device_id, idx) → original value` |
| | `experiment_record_start` | `(config) → { success }` |
| | `experiment_record_stop` | `() → { success }` |
| | `write_text_file` | `(path, content) → { success }` |
| | `read_text_file` | `(path) → content string` |

#### Live Events (Rust → Frontend)

| Event name | Payload | Emitted by |
|------------|---------|------------|
| `daq-data` | `{ device_id, values: { name: f64 } }` | `daq_live::spawn_live_daq_event_pump` |
| `experiment-data` | `{ device_id, values: { name: f64 } }` | `experiment_start` background task |

#### Adding a New Command

1. Add the `async fn` with `#[tauri::command]` in the appropriate `src-tauri/src/commands/*.rs` file.
2. Register it in `src-tauri/src/lib.rs` inside `tauri::generate_handler![...]`.
3. Call it from Vue with `invoke('command_name', { argName: value })`.

---

### Library Usage

```rust
use xcp_client::{XcpClient, CalibrationManager, DaqManager};
use xcp_client::transport::TransportConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create multi-slave client
    let mut client = XcpClient::new();

    // Connect first slave
    let slave1 = client.connect_slave(
        "ECU1",
        TransportConfig::Udp { addr: "127.0.0.1:5555".to_string() }
    ).await?;

    // Connect second slave
    let slave2 = client.connect_slave(
        "ECU2",
        TransportConfig::Udp { addr: "127.0.0.1:5556".to_string() }
    ).await?;

    // Load A2L for first slave
    client.load_a2l_for_slave(slave1, "ecu1.a2l").await?;

    // Load A2L for second slave
    client.load_a2l_for_slave(slave2, "ecu2.a2l").await?;

    // Switch to first slave and read memory
    client.set_current_slave(slave1)?;
    let data = client.read_memory(mta, 4).await?;
    println!("Value: {:?}", data);

    // Disconnect specific slave
    client.disconnect_slave(slave1).await?;

    // Disconnect all
    client.disconnect_all().await?;

    Ok(())
}
```

## XCP Protocol Reference

For detailed XCP protocol specifications, please refer to the official ASAM documentation:

- [ASAM MCD-1 XCP Protocol Specification](https://www.asam.net/standards/detail/mcd-1-xcp/)

## Calibration Parameters

Example parameters from `hello_xcp.a2l`:

| Parameter | Address | Extension | Type | Characteristic Type |
|-----------|---------|-----------|------|---------------------|
| my_params.counter_max | 0x80010000 | 2 | U32 | VALUE |
| my_params.counter_on | 0x8001002C | 2 | Bool | VALUE |
| my_params.delay | 0x80010004 | 2 | U32 | VALUE |
| my_params.array | 0x80010008 | 2 | U8[4] | VAL_BLK |
| my_params.matrix | 0x8001000C | 2 | U8[8][4] | VAL_BLK |
| my_params.curve | 0x80010050 | 2 | U16[10] | CURVE |
| my_params.map | 0x80010060 | 2 | F32[3][9] | MAP |

## Measurements

| Measurement | Address | Extension |
|-------------|---------|-----------|
| counter | 0xFFF8 | 2 |
| const_data | 0xFFFC | 2 |

## Web UI Multi-Slave Features

### Device-Associated Variables

Variables are displayed with device prefix to distinguish between slaves:

```
[ECU1] engineSpeed      1500 rpm    (F32)
[ECU1] throttlePosition   45.2 %    (F32)
[ECU2] batteryVoltage     12.4 V    (F32)
[ECU2] coolantTemp        85.0 °C   (F32)
```

### Per-Slave A2L Data

Each slave maintains independent:
- Calibration parameters
- DAQ measurement definitions
- A2L file path reference

### Dashboard Overview

```
┌─ Slaves (2) ───────────────────────────────┐
│ ECU1 [Connected]                           │
│   5 calibration parameters                 │
│   8 measurements                           │
│   [Select] [Disconnect]                    │
├────────────────────────────────────────────┤
│ ECU2 [Connected]                           │
│   3 calibration parameters                 │
│   4 measurements                           │
│   [Select] [Disconnect]                    │
└────────────────────────────────────────────┘
```

## Dependencies

### Core Library (`xcp_client`)

| Crate | Version | Purpose |
|-------|---------|---------|
| tokio | 1 | Async runtime |
| bytes | 1 | Byte buffer utilities |
| thiserror | 1 | Error type definitions |
| tracing | 0.1 | Structured logging |
| tracing-subscriber | 0.3 | Logging configuration |
| csv | 1 | CSV file output for DAQ |
| chrono | 0.4 | Timestamp handling |
| serde | 1 | Serialization |
| axum | 0.7 | Web framework |
| tower-http | 0.5 | HTTP middleware |
| clap | 4 | CLI argument parsing |
| toml | 0.8 | TOML config file parsing |
| uuid | 1.6 | Slave ID generation |
| async-trait | 0.1 | Transport abstraction |
| libloading | 0.8 | Dynamic `PCANBasic.dll` loading for the PCAN CAN driver |

### Tauri Desktop App (`src-tauri`)

| Crate / Package | Version | Purpose |
|-----------------|---------|---------|
| tauri | 2 | Native window & IPC runtime |
| tauri-plugin-dialog | 2 | Native file-open/save dialogs |
| tauri-plugin-shell | 2 | Shell command helpers |
| vue | ^3.5 | Frontend UI framework |
| vue-router | ^4.5 | SPA routing |
| vite | ^6 | Frontend build tool |
| @tauri-apps/api | ^2 | `invoke()` IPC bridge |
| @vitejs/plugin-vue | ^5 | Vite plugin for Vue SFCs |

## Testing

```bash
# Run unit tests
cargo test

# Run with output
cargo test -- --nocapture

# Run specific test
cargo test test_name -- --nocapture
```

### Test Coverage

- **Transport Layer**: UDP transport, TCP transport, CAN transport (framing, ID filtering, DLC conversion), message routing
- **CAN Drivers**: PCAN channel/handle parsing, bit-rate/timing conversion, device enumeration, driver registry
- **Protocol Layer**: Command builders, response parsers
- **Slave Management**: Multi-slave connection, state management
- **Calibration**: Parameter read/write, A2L parsing
- **DAQ**: DAQ list configuration, data acquisition
- **Web UI**: Authentication, routes, state management

## Project Structure

```
.
├── Cargo.toml           # Core library & CLI workspace member
├── README.md            # This file
├── CLAUDE.md            # Development guide
├── config.toml          # Device config (auto-generated by Tauri app)
├── docs/                # Documentation
│   ├── multi-slave-refactor-plan.md
│   ├── build_checksum_design.md
│   └── specs/
├── src/                 # Core library + CLI entry
│   ├── main.rs          # Application entry & CLI args
│   ├── lib.rs           # Library exports
│   ├── client.rs        # Multi-slave XcpClient
│   ├── slave.rs         # Individual slave context
│   ├── calibration.rs   # Calibration management
│   ├── daq.rs           # DAQ management
│   ├── checksum.rs      # BUILD_CHECKSUM algorithms
│   ├── hex.rs           # Intel HEX file parser
│   ├── variable.rs      # Device-associated variables
│   ├── calibrate.rs     # CLI calibrate subcommand
│   ├── verify.rs        # CLI verify subcommand
│   ├── daq_record.rs    # CLI daq-record subcommand
│   ├── a2l/             # A2L file support
│   ├── protocol/        # XCP protocol implementation
│   ├── transport/       # Transport abstraction (UDP/TCP/CAN + CAN device drivers)
│   ├── tui/             # Terminal UI (single-device)
│   └── web/             # Web UI (multi-device)
├── src-tauri/           # Tauri desktop app backend
│   ├── Cargo.toml       # Tauri crate (depends on xcp_client)
│   ├── tauri.conf.json  # Window config, bundle targets
│   └── src/
│       ├── lib.rs        # Tauri builder, command registration
│       ├── state.rs      # TauriAppState, DeviceState, DeviceConfig
│       └── commands/     # Tauri command implementations
├── ui/                  # Vue 3 frontend (Tauri)
│   ├── package.json     # npm dependencies
│   ├── vite.config.js   # Vite config (port 5173, relative base)
│   └── src/
│       ├── App.vue       # Root layout + sidebar
│       ├── main.js       # Router & app bootstrap
│       └── components/   # Page components
└── hello_xcp.a2l        # Example A2L file
```

## License

This project is provided as-is for educational and development purposes.

## Contributing

Contributions are welcome! Please feel free to submit pull requests.

## Recent Changes

### v0.5.0 - CAN / PCAN Transport

- ✅ **XCP-on-CAN Transport**: One XCP packet per CAN frame, no `LEN`/`COUNTER` header (DLC = length)
  - CAN ID receive filtering: accepts `CAN_ID_SLAVE` plus DAQ DTO IDs from `DAQ_LIST_CAN_ID` / `EVENT_CAN_ID_LIST`, always rejects the master's own ID
  - `MAX_DLC_REQUIRED` padding; classic CAN and CAN-FD (timing derived from bit rates)
- ✅ **CAN Device Driver Abstraction**: `CanDevice`/`CanDriver` traits with a registry; `pcan.rs` implements PEAK-System PCAN-Basic (dynamic `PCANBasic.dll` loading via `libloading`)
- ✅ **A2L-driven CAN config**: bitrate, CAN-FD data bitrate, `CAN_ID_SLAVE`, `DAQ_LIST_CAN_ID`, `EVENT_CAN_ID_LIST`, `MAX_DLC_REQUIRED`
- ✅ **Device-level selection**: enumerate attached channels, choose by stable `device_id`/controller (resolved via `CAN_LookUpChannel`)
- ✅ **Tauri/Vue**: `list_can_devices` command and a CAN device/channel dropdown on the Devices page
- 📄 Design Q&A in [`PCAN_CAN_QA.md`](PCAN_CAN_QA.md)

### v0.4.0 - CLI Subcommands & Checksum Verification

- ✅ **Calibrate Subcommand**: Non-interactive ECU calibration from command line
  - Single variable mode: `--var name --value val`
  - DCM file mode: `--dcm calibration.DCM` for batch calibration
  - Physical → raw value conversion using A2L COMPU_METHOD
- ✅ **Verify Subcommand**: ECU flash memory checksum verification
  - Compares BUILD_CHECKSUM results against Intel HEX reference
  - Per-segment pass/fail report with checksum details
  - Configurable max block size for large segments
- ✅ **BUILD_CHECKSUM**: Full XCP checksum implementation
  - 9 algorithms: ADD_11/12/14/22/24/44, CRC_16, CRC_16_CITT, CRC_32
  - Multi-page segment verification with automatic page switching
  - Block splitting with additive sum / CRC-of-CRCs composition
- ✅ **Intel HEX Parser**: Sparse memory image from .hex files

### v0.3.0 - Matrix Editor & UI Improvements

- ✅ **Matrix/Table Editor**: Visual editor for arrays, matrices, CURVE, and MAP types
  - Table view with axis values display for CURVE/MAP
  - Cell-by-cell editing with immediate save to device
  - Support for X/Y axis labels from A2L AXIS_PTS
  - Transposed display for correct MAP orientation (X=columns, Y=rows)
- ✅ **Variable Info Modal**: Detailed parameter information popup
  - Shows address, type, unit, range, axis info
  - Copy to clipboard functionality
- ✅ **Improved Variable Selection**: Click row to toggle selection
  - Removed per-row checkboxes for cleaner UI
  - Selected rows highlighted with green background
- ✅ **Page Mode Support**: Working/Reference page mode indicator
  - Working page: editable values
  - Reference page: read-only display
- ✅ **Logging Improvements**: Cleaner log output
  - Suppressed a2lfile library warnings
  - Debug logs downgraded to trace level

### v0.2.0 - Multi-Slave Support

- ✅ Connect to multiple XCP slaves simultaneously
- ✅ Each slave has independent A2L data
- ✅ Device-associated variable display
- ✅ Web UI multi-slave management
- ✅ Transport abstraction layer (UDP/TCP/CAN)
- ✅ Backward-compatible single-device API
