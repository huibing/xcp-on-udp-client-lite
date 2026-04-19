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
- **TUI**: Interactive terminal UI for single-device debugging
- **Transport Abstraction**: UDP and TCP support with CAN reserved for future extension

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
    │       └── can.rs         # CAN transport (reserved)
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

The transport abstraction currently supports:

- **UDP**: datagram-based XCP-on-Ethernet transport
- **TCP**: stream-based XCP-on-Ethernet transport with the same 16-bit `LEN` and 16-bit packet counter header as UDP
- **CAN**: reserved skeleton only

Library code can select the transport explicitly:

```rust
use xcp_client::transport::TransportConfig;

let udp = TransportConfig::Udp {
    addr: "127.0.0.1:5555".to_string(),
};

let tcp = TransportConfig::Tcp {
    addr: "127.0.0.1:5555".to_string(),
};
```

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

- **Transport Layer**: UDP transport, TCP transport, message routing
- **Protocol Layer**: Command builders, response parsers
- **Slave Management**: Multi-slave connection, state management
- **Calibration**: Parameter read/write, A2L parsing
- **DAQ**: DAQ list configuration, data acquisition
- **Web UI**: Authentication, routes, state management

## Project Structure

```
.
├── Cargo.toml           # Project configuration
├── README.md            # This file
├── CLAUDE.md            # Development guide
├── docs/                # Documentation
│   ├── multi-slave-refactor-plan.md
│   ├── build_checksum_design.md
│   └── specs/
├── src/
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
│   ├── transport/       # Transport abstraction
│   ├── tui/             # Terminal UI (single-device)
│   └── web/             # Web UI (multi-device)
└── hello_xcp.a2l        # Example A2L file
```

## License

This project is provided as-is for educational and development purposes.

## Contributing

Contributions are welcome! Please feel free to submit pull requests.

## Recent Changes

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
