# XCP Client Architecture Diagram

## Overview

This is a Rust implementation of XCP (Universal Measurement and Calibration Protocol) Master client for automotive ECU development. It supports multiple slave connections with independent A2L data management.

## High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              XCP Client System                               │
│                                                                             │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │                        User Interfaces                               │  │
│  │                                                                      │  │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐              │  │
│  │  │   CLI/TUI    │  │   Web UI     │  │ Tauri Desktop│              │  │
│  │  │  (Ratatui)   │  │   (Axum)     │  │   (Vue 3)    │              │  │
│  │  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘              │  │
│  │         │                 │                 │                       │  │
│  └─────────┼─────────────────┼─────────────────┼───────────────────────┘  │
│            │                 │                 │                          │
│            ▼                 ▼                 ▼                          │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │                      Core Library (xcp_client)                        │  │
│  │                                                                      │  │
│  │  ┌──────────────────────────────────────────────────────────────┐  │  │
│  │  │                    XcpClient (Multi-Slave Manager)           │  │  │
│  │  │  ┌────────────────────────────────────────────────────────┐ │  │  │
│  │  │  │  HashMap<SlaveId, Arc<Mutex<XcpSlave>>>                │ │  │  │
│  │  │  │  - Manages multiple slave connections                   │ │  │  │
│  │  │  │  - Event broadcast channel for state changes            │ │  │  │
│  │  │  │  - Current slave selection for backward compatibility    │ │  │  │
│  │  │  └────────────────────────────────────────────────────────┘ │  │  │
│  │  └──────────────────────────────────────────────────────────────┘  │  │
│  │                              │                                         │  │
│  │                              ▼                                         │  │
│  │  ┌──────────────────────────────────────────────────────────────┐  │  │
│  │  │                      XcpSlave (Per-Slave Context)            │  │  │
│  │  │  ┌────────────────────────────────────────────────────────┐  │  │  │
│  │  │  │  - SlaveId (UUID)                                     │  │  │  │
│  │  │  │  - Transport (UDP/TCP/CAN)                            │  │  │  │
│  │  │  │  - MessageRouter (request/response handling)           │  │  │  │
│  │  │  │  - CalibrationManager (A2L + calibration params)        │  │  │  │
│  │  │  │  - DaqManager (DAQ lists, ODTs, measurements)          │  │  │  │
│  │  │  │  - HexImage (Intel HEX reference for checksum)         │  │  │  │
│  │  │  │  - A2L file path & parsed data                         │  │  │  │
│  │  │  └────────────────────────────────────────────────────────┘  │  │  │
│  │  └──────────────────────────────────────────────────────────────┘  │  │
│  │                              │                                         │  │
│  │                              ▼                                         │  │
│  │  ┌──────────────────────────────────────────────────────────────┐  │  │
│  │  │                    Protocol Layer                               │  │  │
│  │  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐        │  │  │
│  │  │  │  commands.rs  │  │ responses.rs │  │   types.rs   │        │  │  │
│  │  │  │  Command      │  │  Response    │  │  CommandCode │        │  │  │
│  │  │  │  Builders     │  │  Parsers     │  │  ErrorCode   │        │  │  │
│  │  │  │              │  │              │  │  MTA         │        │  │  │
│  │  │  └──────────────┘  └──────────────┘  └──────────────┘        │  │  │
│  │  └──────────────────────────────────────────────────────────────┘  │  │
│  │                              │                                         │  │
│  │                              ▼                                         │  │
│  │  ┌──────────────────────────────────────────────────────────────┐  │  │
│  │                   Transport Abstraction Layer                     │  │  │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐        │  │  │
│  │  │   udp.rs     │  │   tcp.rs     │  │   can.rs     │        │  │  │
│  │  │  XCP-on-UDP  │  │  XCP-on-TCP  │  │  (reserved)  │        │  │  │
│  │  │  Datagram     │  │  Stream      │  │              │        │  │  │
│  │  └──────────────┘  └──────────────┘  └──────────────┘        │  │  │
│  │  └──────────────────────────────────────────────────────────────┘  │  │
│  │                              │                                         │  │
│  │                              ▼                                         │  │
│  │  ┌──────────────────────────────────────────────────────────────┐  │  │
│  │                      Network Layer                                │  │  │
│  │                    (UDP/TCP Sockets)                               │  │  │
│  └──────────────────────────────────────────────────────────────┘  │  │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Component Details

### 1. User Interfaces

#### CLI/TUI (Terminal UI)
- **Entry Point**: `src/main.rs` → `src/tui.rs`
- **Framework**: Ratatui (terminal UI library)
- **Features**: Interactive menu for single-device debugging
- **Commands**: Connect, disconnect, read/write parameters, start/stop DAQ

#### Web UI
- **Entry Point**: `src/main.rs` → `src/web.rs`
- **Framework**: Axum (web framework) + HTML templates
- **Features**: Browser-based multi-slave management
- **Authentication**: Single-user session (admin/admin)
- **Routes**: Dashboard, Calibration, DAQ, A2L management

#### Tauri Desktop App
- **Entry Point**: `src-tauri/src/lib.rs`
- **Frontend**: Vue 3 + Vite (in `ui/` directory)
- **Backend**: Tauri 2 + Rust (in `src-tauri/` directory)
- **Features**: Native cross-platform GUI with Experiment and Compare workflows
- **IPC**: Tauri commands invoke Rust functions via `@tauri-apps/api`

### 2. Core Library (xcp_client)

#### XcpClient (Multi-Slave Manager)
- **File**: `src/client.rs`
- **Responsibilities**:
  - Manage multiple XCP slave connections
  - Provide backward-compatible single-slave API
  - Broadcast slave events (connected, disconnected, A2L loaded, DAQ started/stopped)
  - Switch between active slaves
- **Key Methods**:
  - `connect_slave()` - Connect to new slave with transport config
  - `disconnect_slave()` - Disconnect specific slave
  - `load_a2l_for_slave()` - Load A2L file for specific slave
  - `subscribe_events()` - Subscribe to slave state changes

#### XcpSlave (Per-Slave Context)
- **File**: `src/slave.rs`
- **Responsibilities**:
  - Individual slave connection state
  - Transport layer management
  - Message routing (request/response correlation)
  - Calibration and DAQ management
- **Components**:
  - `SlaveId` - UUID-based unique identifier
  - `Transport` - UDP/TCP/CAN transport instance
  - `MessageRouter` - Correlates requests with responses
  - `CalibrationManager` - Manages calibration parameters
  - `DaqManager` - Manages DAQ lists and measurements
  - `HexImage` - Intel HEX reference for checksum verification

### 3. Protocol Layer

#### Commands (`src/protocol/commands.rs`)
- **Purpose**: Build XCP command packets
- **Key Commands**:
  - `CONNECT` - Establish connection
  - `DISCONNECT` - Terminate connection
  - `GET_ID` - Get slave identification
  - `SET_MTA` - Set memory transfer address
  - `UPLOAD/DOWNLOAD` - Memory access
  - `BUILD_CHECKSUM` - Compute memory checksum
  - DAQ commands: `FREE_DAQ`, `ALLOC_DAQ`, `SET_DAQ_PTR`, `WRITE_DAQ`, etc.

#### Responses (`src/protocol/responses.rs`)
- **Purpose**: Parse XCP response packets
- **Key Parsers**:
  - `parse_connect_response()` - Parse CONNECT response
  - `parse_get_id_response()` - Parse GET_ID response
  - `parse_checksum()` - Parse BUILD_CHECKSUM response
  - `parse_error_response()` - Parse error responses

#### Types (`src/protocol/types.rs`)
- **Purpose**: Define XCP protocol data types
- **Key Types**:
  - `CommandCode` - Enum of all XCP commands
  - `ErrorCode` - XCP error codes
  - `MTA` - Memory Transfer Address
  - `ConnectInfo` - Connection information from slave

### 4. Transport Layer

#### Transport Abstraction (`src/transport/transport.rs`)
- **Purpose**: Unified interface for different transport protocols
- **Trait**: `Transport` - Defines common interface
  - `send_command()` - Send command and wait for response
  - `recv_packet()` - Receive normalized XCP packet
  - `close()` - Close connection
- **Factory**: `TransportFactory` - Creates transport instances

#### UDP Transport (`src/transport/udp.rs`)
- **Protocol**: XCP-on-UDP/IP
- **Characteristics**: Datagram-based, connectionless
- **Header**: 16-bit LEN + 16-bit packet counter + XCP payload

#### TCP Transport (`src/transport/tcp.rs`)
- **Protocol**: XCP-on-TCP/IP
- **Characteristics**: Stream-based, connection-oriented
- **Header**: Same as UDP (16-bit LEN + 16-bit packet counter + XCP payload)

#### CAN Transport (`src/transport/can.rs`)
- **Status**: Reserved skeleton (not yet implemented)
- **Purpose**: Future support for XCP-on-CAN

#### Message Router (`src/transport/message_router.rs`)
- **Purpose**: Correlate requests with responses
- **Mechanism**: Uses packet counter to match responses
- **Timeout**: Configurable timeout for response waiting

### 5. Calibration Management

#### CalibrationManager (`src/calibration.rs`)
- **Purpose**: Manage calibration parameters from A2L file
- **Key Types**:
  - `CalibrationParam` - Single calibration parameter
  - `CalibrationType` - Data type (U8, U16, F32, arrays, matrices, etc.)
  - `CalibrationValue` - Value representation (raw + physical)
- **Features**:
  - Read/write calibration parameters
  - Convert between raw and physical values using COMPU_METHOD
  - Support for VALUE, CURVE, MAP, VAL_BLK characteristic types
  - Page mode switching (Working/Reference)

#### A2L Integration (`src/a2l.rs`)
- **Purpose**: Parse A2L (ASAM MCD-2 MC) description files
- **Key Types**:
  - `A2lReader` - A2L file parser
  - `A2lCharacteristic` - Calibration parameter definition
  - `A2lMeasurement` - Measurement definition
  - `CompuMethod` - Conversion method (raw ↔ physical)
  - `XcpMemorySegment` - Memory segment for checksum verification
- **Features**:
  - Parse A2L files using `a2lfile` crate
  - Extract calibration parameters and measurements
  - Support COMPU_METHOD conversions
  - Memory segment information for checksum verification

### 6. DAQ Management

#### DaqManager (`src/daq.rs`)
- **Purpose**: Manage Data Acquisition (DAQ) lists and measurements
- **Key Types**:
  - `Measurement` - Measurement definition with type info
  - `DaqAllocation` - DAQ list, ODT, and ODT entry allocation
  - `OdtEntryConfig` - ODT entry configuration
- **Features**:
  - Configure DAQ lists, ODTs, and ODT entries
  - Start/stop DAQ lists
  - Receive and parse DTO (Data Transmission Object) packets
  - Record to MF4 and/or CSV files
  - Support for multiple event channels

#### DAQ Flow
```
1. Query DAQ capabilities (GET_DAQ_RESOLUTION_INFO, GET_DAQ_PROCESSOR_INFO)
2. Allocate DAQ lists (ALLOC_DAQ)
3. Allocate ODTs per DAQ list (ALLOC_ODT)
4. Allocate ODT entries (ALLOC_ODT_ENTRY)
5. Set DAQ pointer (SET_DAQ_PTR)
6. Write DAQ entries (WRITE_DAQ) - map measurements to ODT entries
7. Set DAQ list mode (SET_DAQ_LIST_MODE) - assign event channels
8. Start DAQ list (START_STOP_DAQ_LIST)
9. Receive DTO packets continuously
10. Stop DAQ list (START_STOP_DAQ_LIST)
11. Free DAQ resources (FREE_DAQ)
```

### 7. Checksum Verification

#### Checksum Module (`src/checksum.rs`)
- **Purpose**: Verify ECU flash memory integrity
- **Supported Algorithms**: ADD_11, ADD_12, ADD_14, ADD_16, CRC_8, CRC_16, CRC_16_CCITT, CRC_32
- **Workflow**:
  1. Load A2L file to get memory segments
  2. Load Intel HEX file as reference
  3. For each segment:
     - Compute reference checksum from HEX data
     - Request BUILD_CHECKSUM from slave
     - Compare results
  4. Generate pass/fail report

#### HEX Parser (`src/hex.rs`)
- **Purpose**: Parse Intel HEX format files
- **Features**:
  - Parse standard Intel HEX records
  - Extract data at specific addresses
  - Support for extended linear addressing

## Data Flow Examples

### Calibration Read/Write Flow
```
User Interface
    │
    ▼
XcpClient (select slave)
    │
    ▼
XcpSlave.CalibrationManager
    │
    ├─→ Read: SET_MTA → UPLOAD → parse → convert to physical
    │
    └─→ Write: convert to raw → SET_MTA → DOWNLOAD
```

### DAQ Setup Flow
```
User Interface
    │
    ▼
XcpClient (select slave)
    │
    ▼
XcpSlave.DaqManager
    │
    ├─→ Query capabilities
    ├─→ Allocate DAQ lists/ODTs/entries
    ├─→ Map measurements to ODT entries
    ├─→ Set event channels
    ├─→ Start DAQ
    │
    └─→ Receive DTO packets → parse → record/display
```

### Multi-Slave Workflow
```
User Interface
    │
    ├─→ Connect Slave 1 (UDP: 192.168.1.100:5555)
    │   └─→ Load A2L file for Slave 1
    │
    ├─→ Connect Slave 2 (UDP: 192.168.1.101:5555)
    │   └─→ Load A2L file for Slave 2
    │
    ├─→ Switch to Slave 1
    │   └─→ Read/write calibration parameters
    │
    ├─→ Switch to Slave 2
    │   └─→ Start DAQ recording
    │
    └─→ Disconnect all slaves
```

## Key Dependencies

### Core Dependencies
- **tokio** (1.43.0) - Async runtime
- **bytes** (1.9.0) - Byte buffer utilities
- **thiserror** (1.0.69) - Error handling
- **tracing** (0.1.41) - Structured logging
- **serde** (1) - Serialization
- **a2lfile** (2.3.0) - A2L file parsing
- **axum** (0.7.5) - Web framework
- **ratatui** (0.27.0) - Terminal UI
- **clap** (4) - CLI argument parsing

### MF4 Recording (Optional)
- **mf4_parse** (git) - Native MF4 writer (default)
- **mdf4-rs** (0.2) - Alternative MF4 backend

### Tauri Desktop App
- **tauri** (2) - Native window & IPC runtime
- **vue** (^3.5) - Frontend UI framework
- **vite** (^6) - Frontend build tool

## File Structure Summary

```
xcp_client/
├── src/
│   ├── main.rs              # CLI entry point
│   ├── lib.rs               # Library exports
│   ├── client.rs            # XcpClient (multi-slave manager)
│   ├── slave.rs             # XcpSlave (per-slave context)
│   ├── calibration.rs       # Calibration parameter management
│   ├── daq.rs               # DAQ management
│   ├── checksum.rs          # BUILD_CHECKSUM verification
│   ├── hex.rs               # Intel HEX parser
│   ├── variable.rs          # Device-associated variables
│   ├── a2l.rs               # A2L module entry
│   ├── a2l_reader.rs        # A2L file parser (deprecated)
│   ├── calibrate.rs         # CLI calibrate subcommand
│   ├── verify.rs            # CLI verify subcommand
│   ├── daq_record.rs        # CLI daq-record subcommand
│   ├── protocol/            # XCP protocol layer
│   │   ├── protocol.rs      # Module entry
│   │   ├── types.rs         # Command codes, error codes, MTA
│   │   ├── commands.rs      # Command builders
│   │   └── responses.rs     # Response parsers
│   ├── transport/           # Transport abstraction layer
│   │   ├── transport.rs     # Transport trait and factory
│   │   ├── udp.rs           # UDP transport
│   │   ├── tcp.rs           # TCP transport
│   │   ├── can.rs           # CAN transport (reserved)
│   │   └── message_router.rs # Request/response correlation
│   ├── web/                 # Web UI
│   │   ├── web.rs           # Module entry
│   │   ├── server.rs        # Axum HTTP server
│   │   ├── auth.rs          # Session authentication
│   │   ├── routes.rs        # HTTP route handlers
│   │   ├── state.rs         # Multi-slave state management
│   │   └── templates.rs     # HTML templates
│   └── tui/                 # Terminal UI
│       ├── tui.rs           # Module entry
│       └── app.rs           # Ratatui-based interactive CLI
├── src-tauri/               # Tauri desktop backend
│   ├── src/
│   │   ├── lib.rs           # Tauri builder, invoke_handler
│   │   ├── state.rs         # TauriAppState
│   │   └── commands/        # Tauri commands
│   │       ├── device.rs    # Device CRUD & connection
│   │       ├── a2l.rs       # A2L file loading
│   │       ├── calibration.rs # Calibration commands
│   │       ├── daq.rs       # DAQ commands
│   │       ├── checksum.rs  # Checksum verification
│   │       ├── compare.rs   # Compare-session management
│   │       └── experiment.rs # Experiment mode
│   └── tauri.conf.json      # Tauri configuration
├── ui/                      # Vue 3 frontend
│   ├── src/
│   │   ├── App.vue          # Root layout
│   │   └── components/      # Vue components
│   └── package.json         # Frontend dependencies
├── Cargo.toml               # Core library dependencies
├── README.md                # Project documentation
└── config.toml              # Device configuration (Tauri)
```

## Key Design Patterns

### 1. Multi-Slave Architecture
- Each slave is independent with its own transport, calibration manager, and DAQ manager
- Central `XcpClient` manages multiple slaves via `HashMap<SlaveId, Arc<Mutex<XcpSlave>>>`
- Event broadcast channel notifies UI of slave state changes

### 2. Transport Abstraction
- `Transport` trait provides unified interface for UDP, TCP, and future CAN
- `TransportFactory` creates transport instances based on configuration
- Message router handles request/response correlation

### 3. Backward Compatibility
- Single-slave API maintained for legacy code
- Current slave selection provides seamless migration path
- All multi-slave features are opt-in

### 4. Type-Safe Protocol
- Strongly-typed command codes and error codes
- Builder pattern for command construction
- Dedicated parsers for each response type

### 5. Async/Await
- Tokio async runtime throughout
- Non-blocking I/O for network operations
- Concurrent DAQ data reception

## Testing

### Unit Tests
- Transport layer tests (UDP, TCP)
- Protocol layer tests (command builders, response parsers)
- Slave management tests

### Integration Tests
- Mock XCP slave in `tests/mock_xcp_integration.rs`
- Tests real UDP transport, message routing, slave API
- Covers calibration, DAQ, and checksum verification

### Test Execution
```bash
# Unit tests
cargo test

# Integration tests (serial)
cargo test --test mock_xcp_integration -- --test-threads=1
```

## Summary

This XCP client implementation provides a comprehensive, multi-interface solution for automotive ECU calibration and data acquisition. The architecture is modular, with clear separation between:

1. **User interfaces** (CLI/TUI, Web, Desktop)
2. **Core library** (multi-slave management, protocol, transport)
3. **Data management** (calibration, DAQ, checksum verification)
4. **File parsing** (A2L, Intel HEX, MF4)

The transport abstraction layer allows easy addition of new protocols, while the multi-slave architecture enables simultaneous management of multiple ECUs with independent A2L data.
