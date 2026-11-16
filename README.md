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
- **CLI Subcommands**: Non-interactive `calibrate`, `verify`, `daq-record` and `daq-print` for CI/automation
- **Web UI**: Browser-based interface with multi-slave management
- **Desktop App (Tauri)**: Native cross-platform GUI with Experiment and Compare workflows
- **TUI**: Interactive terminal UI for single-device debugging
- **Transport Abstraction**: UDP, TCP and CAN (XCP-on-CAN over PEAK-System PCAN-Basic or ETAS ES582 via BOA/OCI) with a pluggable CAN device-driver layer
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
    │           ├── pcan.rs    #   PEAK-System PCAN-Basic driver
    │           └── es582.rs   #   ETAS ES582 driver (BOA V2 / OCI)
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
    ├── daq_print.rs       # CLI daq-print subcommand (live stdout)
    ├── cli_connection.rs  # Shared CLI/TOML connection settings
    ├── cli_slave.rs       # Shared slave labelling for config files
    ├── cli_signal.rs      # Shared CTRL+C handling for CLI subcommands
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

See [`docs/PCAN_CAN_QA.md`](docs/PCAN_CAN_QA.md) for the CAN/PCAN design Q&A,
[`docs/ES582_CAN.md`](docs/ES582_CAN.md) for the ETAS ES582 (BOA/OCI) driver and
[`src/transport/can/device.rs`](src/transport/can/device.rs) for how to add a new
CAN device driver.

#### ETAS ES582 (`--can-driver es582`)

The ES582 driver talks to ETAS hardware through the **BOA V2 / OCI** runtime that
ships with EBI-IP/INCA (`dll-ocdProxy.dll`, `dll-csiBind.dll` and the
`ES582_1_OCD_CAN.dll` Open Controller Driver). Nothing has to be compiled in —
the DLLs are loaded at runtime, exactly like `PCANBasic.dll`.

```bash
# Enumerate ES582 channels (and everything else the drivers report)
cargo run --example es582_verify

# Use it from the CLI
cargo run --release -- verify --a2l hello_xcp.a2l --transport can \
  --can-driver es582 --can-channel auto --can-id 0x550 --slave-id 0x551 --baudrate 500000
```

Channel URIs are discovered by BOA's search-for-service layer, e.g.
`ETAS://USB/ES582.1:114913/CAN:1`; they can be passed to `--can-channel`
directly, or a channel can be pinned with `--can-device`/`--can-controller`.

ES582 environment variables:

- `BOA_DLL_DIR` — directory containing `dll-ocdProxy.dll`/`dll-csiBind.dll`
  (default `%ProgramFiles%\ETAS\BOA_V2\Bin\x64\Dll\Framework`)
- `BOA_OCD_DIR` — OCD plugin directory (default
  `%ProgramFiles%\ETAS\ED-EBI_V2\Bin\x64\Dll\OCDs`)
- `ES582_URI` — channel URI used when the config does not pin one
- `ES582_SELF_RECEPTION=1` — also receive self-transmitted frames (bring-up tests)
- `ES582_BTL_CYCLES`, `ES582_SAMPLE_POINT`, `ES582_SJW` — nominal bit timing
  overrides (the driver otherwise probes a list of prescaler-friendly candidates)
- `ES582_FD_BTL_CYCLES`, `ES582_FD_SAMPLE_POINT`, `ES582_FD_SJW` — CAN-FD data
  phase timing overrides

The driver programs hardware acceptance filters from
`CanDeviceConfig::accept_ids` (the CAN transport fills it with `CAN_ID_SLAVE`
plus the DAQ DTO IDs). That matters on an ES582: polling costs a USB round trip
per frame, so unfiltered periodic bus traffic would push command responses out
of the device queue.

Frames are received through the OCI `onFrame` callback (BOA pushes them from its
own thread; `recv_frame` only drains a local queue). This is not an optimisation
detail: with `OCI_ReadCANDataEx` polling, one call returns a single frame and
costs a USB round trip, so a 150 frames/s bus lost ~80 % of its frames and DAQ
samples lagged behind by minutes. `ES582_RX_MODE=poll` restores the old polling
path for comparison.

#### XCP smoke test (connect, pages, upload)

`examples/es582_xcp_test.rs` runs a minimal XCP session against a real ECU
(default A2L: `pms-a.a2l`): `CONNECT`, `GET_ID`, `GET_CAL_PAGE`,
`SET_CAL_PAGE` (switch to the working page and back), `SET_MTA` + `UPLOAD` of the
calibration segment, `DISCONNECT`.

```bash
cargo run --example es582_xcp_test -- pms-a.a2l

# slow slaves (this demo ECU answers in 0.3-1.5 s, above the 1 s default)
XCP_TEST_TIMEOUT_MS=5000 cargo run --example es582_xcp_test

# talk to a PCAN channel instead
XCP_TEST_DRIVER=pcan XCP_TEST_CHANNEL=PCAN_USBBUS1 cargo run --example es582_xcp_test
```

The CTO response timeout is also available programmatically:
`XcpClient::set_command_timeout(Duration)` (applies to newly created slaves,
including their `CONNECT`) and `XcpSlave::set_command_timeout(Duration)`.

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

# Override the UDP/TCP slave address (default is read from A2L)
cargo run --release -- calibrate --a2l ecu.a2l --var my_params.delay --value 100 --addr 192.168.1.10:5555

# XCP-on-CAN ECU: transport is auto-detected from the A2L XCP_ON_CAN block
cargo run --release -- calibrate --a2l ecu.a2l --var my_params.delay --value 100

# XCP-on-CAN with an explicit adapter channel / CAN IDs
cargo run --release -- calibrate --a2l ecu.a2l --var my_params.delay --value 100 \
  --transport can --can-channel PCAN_USBBUS1 --can-id 0x550 --slave-id 0x551 --baudrate 500000
```

The transport is auto-selected from the A2L file when no connection flags are
given (priority: UDP > CAN > TCP), so an ECU described only by an
`XCP_ON_CAN` block works without extra flags. Any CAN setting (`--can-id`,
`--can-channel`, …) also selects CAN implicitly; `--transport udp|tcp|can`
forces a specific transport. See [CAN Transport](#can-transport) for the full
flag list.

#### DCM File Mode

Write multiple parameters from a DCM (Data Conservation and Measurement) file:

```bash
# Apply all matching parameters from a DCM file
cargo run --release -- calibrate --a2l ecu.a2l --dcm calibration.DCM

# With address override
cargo run --release -- calibrate --a2l ecu.a2l --dcm calibration.DCM --addr 192.168.1.10:5555

# Over XCP-on-CAN (transport auto-detected from the A2L)
cargo run --release -- calibrate --a2l ecu.a2l --dcm calibration.DCM --transport can --can-channel auto
```

DCM files contain physical values (display units). The tool automatically converts physical values to raw ECU representation using the COMPU_METHOD defined in the A2L file before writing.

Supported DCM parameter types: FESTWERT (scalar), FESTWERTEBLOCK (array), KENNLINIE (curve), GRUPPENKENNFELD (map).

### Record DAQ Measurements (CLI)

Record XCP DAQ measurements to MF4 and/or CSV files non-interactively, from one
or several slaves. Runs until CTRL+C (or `q` + Enter on stdin), then gracefully
stops DAQ and saves the output files.

```bash
cargo run --release -- daq-record --config daq_record.config.toml
```

#### Config File Format (TOML)

Single slave — top-level `a2l`, `[connection]`, `[output]`, `[daq]` and
`[[events]]`:

```toml
a2l = "ecu.a2l"                    # Path to A2L description file (required)

[connection]
type = "udp"                      # Optional: "udp" (default), "tcp" or "can"
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

- If `[connection]` is omitted, the transport is auto-selected from the A2L file
  (priority: UDP > CAN > TCP)
- If `[connection].addr` is omitted, the slave address is read from the A2L file
- If `[[events]]` is omitted, all measurements from the A2L file are selected
- Both MF4 and CSV can be recorded simultaneously; at least one `[output]` path is required
- Invalid measurement names are warned and skipped (recording continues with valid ones)
- On CTRL+C: recording stops → DAQ stops → `FREE_DAQ` restores the slave → disconnect
  (extra CTRL+C presses during that shutdown are ignored, so the process still exits cleanly)

#### Struct / enum / array measurements

| A2L kind | Handling |
|----------|----------|
| scalar | one DAQ entry / one CSV or MF4 channel |
| `COMPU_VTAB` enum | scalar value; MF4 gets a value-to-text block |
| `INSTANCE` + `TYPEDEF_STRUCTURE` | flattened by the A2L reader into one measurement per member (`lookup.input`, `lookup.output_linear`) |
| struct member with `MATRIX_DIM` (array/matrix) | expanded into one scalar entry per element: `DTC[0]`, `DTC[1]`, … / `MAP[r][c]` (row-major), so every element becomes its own DAQ entry, CSV column and MF4 channel |

Because blocks are expanded, `measurements = ["DTC"]` in the config selects **all**
`DTC[i]` elements — no need to list every index. Array-of-structs
(`INSTANCE` with `MATRIX_DIM`) is not supported by the A2L reader and is skipped
with a warning.

#### Multiple slaves

Replace the top-level `a2l`/`[connection]`/`[daq]`/`[[events]]` sections with
one `[[slaves]]` table per slave (the two forms are mutually exclusive). A
top-level `[output]` may stay and is shared by every slave that does not declare
its own `[slaves.output]`; `{name}` / `{index}` placeholders expand to the slave
label and its 0-based index:

```toml
[output]                          # shared default for all slaves
csv = "recording_{name}.csv"
mf4 = "recording_{name}.mf4"

[[slaves]]
name = "inverter"                 # optional label (default: A2L file stem)
a2l = "pms-a.a2l"

[slaves.connection]
type = "can"
can_id = 0x7fb
slave_id = 0x7fc
daq_ids = [0x7fd]

[slaves.output]                   # optional: replaces the shared [output]
csv = "inverter.csv"

[[slaves.events]]
channel = 0
measurements = ["speed", "rpm"]

[[slaves]]                        # second slave, independent config
name = "charger"
a2l = "charger.a2l"

[slaves.connection]
addr = "192.168.1.20:5555"

[[slaves.events]]
channel = 0
measurements = ["voltage", "current"]
```

- Output paths are expanded, checked for duplicates (case-insensitively) and
  refused if the file already exists — before any slave is connected
- If one slave fails to start, already started slaves are shut down cleanly
- The recording summary lists duration and file sizes per slave
- `daq_record.config.toml` contains a fully commented multi-slave block with a
  dummy second slave

> **CAN note:** every slave opens its own CAN device/channel, so multiple CAN
> slaves need distinct `channel` (or `device_id`/`controller`) settings.
> UDP/TCP slaves are independent and can be combined freely.

##### CAN Transport

Set `type = "can"` in `[connection]` to record over XCP-on-CAN (e.g. PCAN):

```toml
[connection]
type = "can"
channel = "auto"              # or e.g. "PCAN_USBBUS1"; "auto" picks the first free channel
can_id = 0x7fb                # CAN_ID_MASTER (command CAN ID)
baudrate = 500000             # Nominal bit rate in bit/s
data_baudrate = 2000000       # Optional: presence enables CAN-FD
slave_id = 0x7fc              # CAN_ID_SLAVE (response CAN ID)
daq_ids = [0x7fd]             # DAQ DTO CAN IDs
max_dlc_required = false      # Pad command frames to the maximum DLC
#driver = "pcan"              # Optional: CAN driver id (default: "pcan")
#device_id = "3"              # Optional: stable device id (overrides channel)
#controller = 0               # Optional: controller index within the device
```

- Undeclared CAN fields fall back to the A2L `XCP_ON_CAN` block
  (`BAUDRATE`, `CAN_FD_DATA_TRANSFER_BAUDRATE`, `CAN_ID_SLAVE`, `DAQ_LIST_CAN_ID`,
  `MAX_DLC_REQUIRED`); `can_id` (`CAN_ID_MASTER`) is required from the config or the A2L
- `channel`/`driver`/`device_id` select the local CAN adapter (no A2L equivalent);
  `channel = "auto"` picks the first free channel
- CAN environment variables (`PCAN_BASIC_DLL`, `PCAN_BITRATE`, `PCAN_FD_CLOCK`) apply
  as documented in [Transport Configuration](#transport-configuration)

#### CAN Flags for `calibrate` / `verify`

`calibrate` and `verify` accept the same CAN settings as CLI flags (all optional;
undeclared values fall back to the A2L `XCP_ON_CAN` block):

```bash
cargo run --release -- calibrate --a2l ecu.a2l --var my_param --value 1 \
  --transport can \
  --can-channel PCAN_USBBUS1 \   # or "auto" (default)
  --can-driver pcan \            # optional driver id
  --can-device 3 \               # optional stable device id
  --can-controller 0 \           # optional controller index
  --can-id 0x550 \               # CAN_ID_MASTER
  --slave-id 0x551 \             # CAN_ID_SLAVE
  --baudrate 500000 \            # nominal bit rate
  --data-baudrate 2000000 \      # optional: enables CAN-FD
  --daq-ids 0x7fd,0x7fe \        # optional DAQ DTO IDs
  --max-dlc-required             # optional: pad command frames
```

CAN IDs and bit rates accept decimal or `0x`-prefixed hexadecimal values.

### Print Live DAQ Values (CLI)

Print DAQ measurements to **stdout** instead of writing files — useful for quick
checks and shell capture (`grep`, `tee`, `awk`). One line is printed per event
group and per print tick:

```bash
cargo run --release -- daq-print --config daq_print.config.toml

# capture to a file (header lines start with '#')
cargo run --release -- daq-print --config daq_print.config.toml | tee capture.log
```

Example output:

```text
# xcp-client daq-print | 1 slave(s) | refresh 10ms | mode line | CTRL+C to stop
# [inverter] pms-a.a2l via CAN PCAN_USBBUS1 0x7FB
# [inverter] ev0 "Task_10ms" | 10ms | every 100 sample(s) (~1000ms) | 3 signal(s)
# [inverter]   CAN1_TX_PMS03_PMS_BATTMODEREQ, CAN1_TX_PMS02_PMS_MAXTEMP, CAN1_TX_PMS02_PMS_BUSVOLTAGE
[12:00:01.234] inverter ev0  CAN1_TX_PMS03_PMS_BATTMODEREQ=3  CAN1_TX_PMS02_PMS_MAXTEMP=25.5  CAN1_TX_PMS02_PMS_BUSVOLTAGE=48.2
```

- Values are **physical values** (A2L `COMPU_METHOD` applied); enumerations
  (`COMPU_VTAB`) print as text
- Struct measurements are flattened into `INSTANCE.MEMBER` by the A2L reader;
  arrays/matrices are expanded per element but printed as one compact field
  `NAME=[v0, v1, …]` (limited by `[print].max_block_items`, default 16, `0` = all)
- Header lines start with `#` so data lines are easy to filter
- Signals not yet received are printed as `NAME=-`
- Runs until CTRL+C (or `q` + Enter when stdin is a terminal); extra CTRL+C
  presses during the graceful shutdown are ignored so the process exits cleanly

#### Decimation (print rate)

Printing every sample of a 10 ms event is noisy. Each event can declare a
`decimation` — the number of samples between prints:

```toml
[[slaves.events]]
channel = 0
measurements = ["CAN1_TX_PMS02_PMS_MAXTEMP"]
decimation = 100        # 10 ms event → printed once per second
```

`[print].decimation` sets the default for events without their own value, and
`[print].refresh_ms` bounds the polling rate (default 10 ms).

#### Output modes

`[print].mode` selects how lines reach the terminal:

| mode | behaviour |
|------|-----------|
| `line` (default) | appends one line per event per print tick — every sample is kept, so the output can be captured |
| `single-line` | keeps a fixed block with one line per event and redraws it in place (ANSI cursor-up), so the terminal never scrolls |

```toml
[print]
mode = "single-line"     # live dashboard: latest values only
refresh_ms = 50
```

In `single-line` mode each event still honours its `decimation`, so a 10 ms
event with `decimation = 100` refreshes its line once per second while other
events keep updating. Lines longer than the terminal width are truncated, and
if there are more events than terminal rows the block is capped with a
`... (+N more event(s))` line.

> `single-line` needs a terminal. When stdout is redirected (e.g. `| tee`), the
> tool warns and falls back to `line` mode, because in-place updates would only
> leave the last frame in the file.

#### Multiple slaves

Each `[[slaves]]` table is an independent acquisition session with its own A2L,
transport, DAQ defaults and event groups. Slave labels default to the A2L file
stem.

```toml
[print]
mode = "line"          # or "single-line" for an in-place dashboard
decimation = 1
refresh_ms = 10

[[slaves]]
name = "inverter"
a2l = "pms-a.a2l"

[slaves.connection]
type = "can"
can_id = 0x7fb
slave_id = 0x7fc
daq_ids = [0x7fd]

[slaves.daq]
event_channel = 0

[[slaves.events]]
channel = 0
decimation = 100
measurements = ["CAN1_TX_PMS02_PMS_MAXTEMP"]

[[slaves]]
name = "charger"
a2l = "charger.a2l"

[slaves.connection]
addr = "192.168.1.20:5555"

[[slaves.events]]
channel = 2
decimation = 50
measurements = ["CHG_Voltage", "CHG_Current"]
```

A single-slave config may also use the top-level `a2l`, `[connection]`, `[daq]`
and `[[events]]` layout of `daq-record` (see `daq_print.config.toml`).

> **CAN note:** every slave opens its own CAN device/channel, so multiple CAN
> slaves need distinct `channel` (or `device_id`/`controller`) settings.
> UDP/TCP slaves are independent and can be combined freely.

See `daq_print.config.toml` for a fully commented example.

### Verify ECU Flash Checksums (CLI)

Verify ECU flash memory integrity by comparing BUILD_CHECKSUM results against an Intel HEX reference file:

```bash
# Basic verification
cargo run --release -- verify --a2l ecu.a2l --hex firmware.hex

# With custom max block size (default: 16 KiB)
cargo run --release -- verify --a2l ecu.a2l --hex firmware.hex --max-block-size 4096

# With address override
cargo run --release -- verify --a2l ecu.a2l --hex firmware.hex --addr 192.168.1.10:5555

# Over XCP-on-CAN (transport auto-detected from the A2L)
cargo run --release -- verify --a2l ecu.a2l --hex firmware.hex
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
- **Card display styles**: boolean variables can render as text, toggle, radio or bulb, and numeric ones as text, gauge or level bar (pick it with the ◈ button on the card). The ⚙ button tunes the renderer: display range (empty = A2L limits), gauge/bar colour, bar orientation and cell count. Calibration booleans toggle in place on the Working page; measurements stay read-only. Choices are stored per variable in the experiment JSON (`display_style` + `display_config`, file version 3) — see [docs/experiment-display-styles.md](docs/experiment-display-styles.md).

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
| libloading | 0.8 | Dynamic `PCANBasic.dll` / BOA (`dll-ocdProxy.dll`, `dll-csiBind.dll`) loading for the CAN drivers |

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
- **CAN Drivers**: PCAN channel/handle parsing, bit-rate/timing conversion, device enumeration, driver registry; ES582 URI parsing, BOA version negotiation, timing-candidate probing
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
│   ├── daq_print.rs     # CLI daq-print subcommand (live stdout)
│   ├── cli_connection.rs # Shared CLI/TOML connection settings
│   ├── cli_slave.rs     # Shared slave labelling for config files
│   ├── cli_signal.rs    # Shared CTRL+C handling for CLI subcommands
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

### v0.6.2 - ES582 callback receive path

- ✅ **ES582 receives through the OCI `onFrame` callback** instead of
  `OCI_ReadCANDataEx` polling. A poll call returns one frame and costs a USB
  round trip (~7.5 ms busy, ~15-20 ms idle), so the driver fell behind the bus:
  150 frames/s of traffic lost ~80 % of its frames, DAQ ran at 65-88 samples/s
  instead of 100, and the desktop app showed variable updates lagging by
  minutes. With the callback BOA pushes whole batches: 750/750 frames captured,
  100.0 DAQ samples/s, `dropped_frames()` stays 0.
  - callback safety: no blocking, no re-entrant OCI calls, `catch_unwind`
    around the body, frame copied out immediately (the message pointer is only
    valid during the call), bounded queue with drop-oldest accounting, and the
    callback context is leaked on teardown because BOA may fire it once after
    `OCI_DestroyCANController` returns
  - `ES582_RX_MODE=poll` keeps the old path for comparison; `recv_frame` now
    blocks on a condition variable instead of sleeping in a poll loop
- ✅ `Es582Device::rx_mode()` / `dropped_frames()` expose the receive-path state
  (printed by `examples/es582_verify.rs`)

### v0.6.1 - XCP-on-CAN bring-up fixes

- ✅ **Hardware acceptance filters**: `CanDeviceConfig::accept_ids` lets the CAN transport hand `CAN_ID_SLAVE` + DAQ DTO IDs to the driver; the ES582 driver programs `OCI_AddCANFrameFilter` from it. Without it, an ES582 (one frame per OCI read, one USB round trip per read) fell behind the bus and command responses were delayed by seconds or lost
- ✅ **Configurable CTO timeout**: `XcpClient::set_command_timeout` / `XcpSlave::set_command_timeout` (default stays 1 s) — slow ECUs that service XCP from a polled loop need more
- ✅ **Segment numbers from the A2L**: `update_segment_page_state` / `switch_all_pages` use the declared `SEGMENT` numbers instead of `0..N` (A2Ls where several `MEMORY_SEGMENT`s share one logical segment, or numbers don't start at 0, no longer query non-existent segments)
- ✅ **ES582 transmit no longer blocks**: `OCI_WriteCANData*` is called with a zero timeout, so a bus that stops acknowledging frames returns an error instead of hanging the caller
- ✅ `cargo run --example es582_xcp_test` — CONNECT / GET_ID / GET_CAL_PAGE / SET_CAL_PAGE / SET_MTA+UPLOAD / DISCONNECT against a real ECU

### v0.6.0 - ETAS ES582 (BOA/OCI) CAN Driver

- ✅ **ES582 Driver**: `src/transport/can/es582.rs` drives ETAS ES582 CAN/CAN-FD interfaces through the BOA V2 Open Controller Interface
  - Dynamic loading of `dll-ocdProxy.dll` (OCI) and `dll-csiBind.dll` (CSI) via `libloading`; no ETAS SDK needed at build time
  - Channel enumeration through the CSI search-for-service tree (`CSI_CreateProtocolTree` + `CSI_GetUriForUuid`, `UUID_OCICAN`)
  - OCI_CAN API version negotiation (1.6 → 1.0), `OCI_OpenCANController` with bit-rate/timing candidates, Tx/Rx queues with an accept-all frame filter
  - Classic CAN and CAN-FD frames, hardware timestamps converted from controller ticks to microseconds
  - `cargo run --example es582_verify` smoke test (device/channel info, frame receive, optional transmit)
- 📄 Driver notes in [`docs/ES582_CAN.md`](docs/ES582_CAN.md)

### v0.5.0 - CAN / PCAN Transport

- ✅ **XCP-on-CAN Transport**: One XCP packet per CAN frame, no `LEN`/`COUNTER` header (DLC = length)
  - CAN ID receive filtering: accepts `CAN_ID_SLAVE` plus DAQ DTO IDs from `DAQ_LIST_CAN_ID` / `EVENT_CAN_ID_LIST`, always rejects the master's own ID; when no DAQ IDs are declared, DAQ defaults to `CAN_ID_SLAVE` (per the XCP-on-CAN spec) so unrelated bus traffic never pollutes DAQ recordings
  - `MAX_DLC_REQUIRED` padding; classic CAN and CAN-FD (timing derived from bit rates)
- ✅ **CAN Device Driver Abstraction**: `CanDevice`/`CanDriver` traits with a registry; `pcan.rs` implements PEAK-System PCAN-Basic (dynamic `PCANBasic.dll` loading via `libloading`)
- ✅ **A2L-driven CAN config**: bitrate, CAN-FD data bitrate, `CAN_ID_SLAVE`, `DAQ_LIST_CAN_ID`, `EVENT_CAN_ID_LIST`, `MAX_DLC_REQUIRED`
- ✅ **Device-level selection**: enumerate attached channels, choose by stable `device_id`/controller (resolved via `CAN_LookUpChannel`)
- ✅ **Tauri/Vue**: `list_can_devices` command and a CAN device/channel dropdown on the Devices page
- 📄 Design Q&A in [`docs/PCAN_CAN_QA.md`](docs/PCAN_CAN_QA.md)

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
