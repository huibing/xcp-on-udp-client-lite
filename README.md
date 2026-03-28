# XCP Client

A Rust implementation of XCP (Universal Measurement and Calibration Protocol) Master client for automotive ECU development. This client communicates with XCP slaves (ECUs) over UDP.

## Features

- **Connection Management**: CONNECT, DISCONNECT, GET_ID commands
- **Memory Access**: UPLOAD, DOWNLOAD, SHORT_UPLOAD, SHORT_DOWNLOAD
- **Calibration**: Read/write calibration parameters with multiple data types
- **DAQ (Data Acquisition)**: Configure and run DAQ lists for measurement
- **A2L Integration**: Support for calibration parameters defined in A2L files
- **Web UI**: Browser-based interface for remote access (single-user)

## Supported Data Types

| Category | Types |
|----------|-------|
| Unsigned Integers | U8, U16, U32, U64 |
| Signed Integers | I8, I16, I32, I64 |
| Floating Point | F32, F64 |
| Other | Bool, Arrays, Matrices |

## Architecture

```
main.rs (Application Entry)
    │
lib.rs → client.rs (XcpClient - Main API)
    │
    ├── protocol/          # XCP Protocol Layer
    │   ├── types.rs       # Command codes, error codes, MTA, ConnectInfo
    │   ├── commands.rs    # Command builders
    │   └── responses.rs   # Response parsers
    │
    ├── transport/         # Transport Layer
    │   └── udp.rs         # UDP transport (XCP-on-UDP/IP)
    │
    ├── calibration.rs     # Calibration parameter management
    │
    ├── daq.rs             # DAQ management
    │
    └── web/               # Web UI Module
        ├── server.rs      # Axum HTTP server
        ├── auth.rs        # Session authentication
        ├── routes.rs      # HTTP route handlers
        └── templates.rs   # HTML templates
```

## Requirements

- Rust 1.70 or later
- XCP slave (ECU simulator) running on target address

## Build

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release
```

## Usage

### Run Interactive CLI (TUI)

```bash
cargo run
```

The CLI provides an interactive menu:

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

### Run Web UI

```bash
# Run with default settings (localhost:8080)
cargo run -- web

# Run with custom host and port (allow external access)
cargo run -- web --host 0.0.0.0 --port 8080
```

The Web UI provides browser-based access with the following features:

- **Dashboard**: View connection status and system overview
- **Connect**: Connect/disconnect to XCP slaves
- **Calibration**: Browse, read, and write calibration parameters
- **DAQ**: Select measurements and monitor real-time data
- **A2L**: Load A2L files for calibration and measurement definitions

#### Authentication

- **Username**: `admin`
- **Password**: `admin`

Note: The Web UI supports only a single concurrent user. If a user is already logged in, subsequent login attempts will be rejected until the current user logs out or the session expires (30 minutes).

### Default Configuration

- **Slave Address**: `127.0.0.1:5555` (localhost)
- **Timeout**: 1000ms (XCP T1 parameter)
- **Max DTO Size**: 1496 bytes

### Library Usage

```rust
use xcp_client::{XcpClient, CalibrationManager, DaqManager};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create and connect client
    let mut client = XcpClient::new("127.0.0.1:5555");
    client.connect().await?;

    // Read memory
    let data = client.read_memory(0xFFF8, 2, 4).await?;
    println!("Value: {:?}", data);

    // Write memory
    client.write_memory(0x80010000, 2, &[0x01, 0x02, 0x03, 0x04]).await?;

    // Disconnect
    client.disconnect().await?;

    Ok(())
}
```

## XCP Protocol Reference

For detailed XCP protocol specifications, please refer to the official ASAM documentation:

- [ASAM MCD-1 XCP Protocol Specification](https://www.asam.net/standards/detail/mcd-1-xcp/)

## Calibration Parameters

Defined in `hello_xcp.a2l`:

| Parameter | Address | Extension | Type |
|-----------|---------|-----------|------|
| my_params.counter_max | 0x80010000 | 2 | U32 |
| my_params.counter_on | 0x8001002C | 2 | Bool |
| my_params.delay | 0x80010004 | 2 | U32 |
| my_params.array | 0x80010008 | 2 | U8[4] |
| my_params.matrix | 0x8001000C | 2 | U8[8][4] |

## Measurements

| Measurement | Address | Extension |
|-------------|---------|-----------|
| counter | 0xFFF8 | 2 |
| const_data | 0xFFFC | 2 |

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

## Reference Documentation

- [ASAM XCP Protocol Specification](https://www.asam.net/standards/detail/xcp/)

## Testing

```bash
# Run unit tests
cargo test

# Run with output
cargo test -- --nocapture
```

## License

This project is provided as-is for educational and development purposes.

## Contributing

Contributions are welcome! Please feel free to submit pull requests.
