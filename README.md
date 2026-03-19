# XCP Client

A Rust implementation of XCP (Universal Measurement and Calibration Protocol) Master client for automotive ECU development. This client communicates with XCP slaves (ECUs) over UDP.

## Features

- **Connection Management**: CONNECT, DISCONNECT, GET_ID commands
- **Memory Access**: UPLOAD, DOWNLOAD, SHORT_UPLOAD, SHORT_DOWNLOAD
- **Calibration**: Read/write calibration parameters with multiple data types
- **DAQ (Data Acquisition)**: Configure and run DAQ lists for measurement
- **A2L Integration**: Support for calibration parameters defined in A2L files

## Supported Data Types

| Category | Types |
|----------|-------|
| Unsigned Integers | U8, U16, U32, U64 |
| Signed Integers | I8, I16, I32, I64 |
| Floating Point | F32, F64 |
| Other | Bool, Arrays, Matrices |

## Architecture

```
main.rs (CLI Application)
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
    └── daq.rs             # DAQ management
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

### Run Interactive CLI

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

## XCP Protocol Details

### Transport Layer Header (XCP on Ethernet)

```
+--------+--------+--------+--------+------------------+
| LEN_L  | LEN_H  | CNT_L  | CNT_H  | XCP Packet...    |
+--------+--------+--------+--------+------------------+
   0-1      2-3      4-5      6-7         8+
```

- **LEN**: Length of XCP packet (little-endian)
- **COUNTER**: Packet sequence counter (0-65535, wraps)

### Supported Commands

| Command | Code | Description |
|---------|------|-------------|
| CONNECT | 0xFF | Establish connection to slave |
| DISCONNECT | 0xFE | Terminate connection |
| GET_ID | 0xFA | Get slave identification |
| SET_MTA | 0xF6 | Set Memory Transfer Address |
| UPLOAD | 0xF5 | Upload data from slave |
| SHORT_UPLOAD | 0xF4 | Upload up to 7 bytes |
| DOWNLOAD | 0xF0 | Download data to slave |
| SHORT_DOWNLOAD | 0xEF | Download up to 7 bytes |
| FREE_DAQ | 0xD6 | Free DAQ lists |
| ALLOC_DAQ | 0xD5 | Allocate DAQ list |
| ALLOC_ODT | 0xD4 | Allocate ODT |
| ALLOC_ODT_ENTRY | 0xD3 | Allocate ODT entry |
| SET_DAQ_PTR | 0xE2 | Set DAQ pointer |
| WRITE_DAQ | 0xDA | Write DAQ entry |
| SET_DAQ_LIST_MODE | 0xDE | Set DAQ list mode |
| START_STOP_DAQ_LIST | 0xDD | Start/stop DAQ list |

### Byte Order

- **A2L File**: `BYTE_ORDER MSB_LAST` (big-endian)
- **XCP Commands**: Little-endian for addresses
- Data conversion handled automatically

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

## Reference Documentation

- [ASAM MCD-1 XCP Protocol Layer Specification](https://www.asam.net/standards/detail/mcd-1-xcp/)
- `ASAM_AE_MCD-1-XCP_BS_Protocol-Layer_V1-5-0.pdf` - Protocol specification
- `ASAM_AE_MCD-1-XCP_AS_Ethernet-Transport-Layer_V1-5-0.pdf` - Ethernet transport specification

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
