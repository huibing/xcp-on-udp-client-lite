# GitHub Copilot Instructions

## Project Overview

Rust implementation of an XCP (Universal Measurement and Calibration Protocol) Master client for automotive ECU development. Supports multiple simultaneous XCP slave connections, each with independent A2L data management. Exposes both a TUI (single-device) and a Web UI (multi-device).

## Commands

```bash
cargo build                          # Debug build
cargo build --release                # Release (LTO enabled)
cargo check                          # Error check without build
cargo test                           # Run all tests
cargo test <test_name> -- --nocapture  # Run single test with output

cargo run                            # Interactive TUI (single-device)
cargo run --release -- web           # Web UI at localhost:8080
cargo run --release -- web --host 0.0.0.0 --port 8080  # Custom bind
```

Logs are written to `logs/xcp_client_<timestamp>.log`. The `a2lfile` crate's warnings are suppressed at startup (`a2lfile=error`).

## Architecture

```
main.rs
  ├── tui/app.rs          → Ratatui TUI for single-slave debugging
  └── web/                → Axum Web UI for multi-slave management
        ├── server.rs     → HTTP server + WebSocket
        ├── routes.rs     → HTTP handlers
        ├── state.rs      → AppState / DeviceState (shared state)
        ├── auth.rs       → Session auth (single concurrent user)
        └── templates.rs  → Handlebars HTML templates

lib.rs → client.rs (XcpClient)
  ├── slave.rs            → XcpSlave + SlaveId (UUID), SlaveError
  ├── calibration.rs      → CalibrationManager, CalibrationType, CalibrationParam
  ├── daq.rs              → DaqManager, Measurement, OdtEntryConfig
  ├── variable.rs         → DeviceVariable (variable + slave association)
  ├── a2l_reader.rs       → DEPRECATED wrapper; use `a2l` module instead
  ├── a2l/                → A2L parsing + COMPU_METHOD conversion
  │     ├── reader.rs     → A2lReader, A2lCharacteristic, A2lMeasurement
  │     ├── types.rs      → A2lDataType, CharacteristicType, StorageOrder
  │     └── compute_method.rs → CompuMethod (IDENTICAL, LINEAR, TAB_VERB)
  ├── protocol/           → XCP wire protocol (ASAM MCD-1 XCP V1.5.0)
  │     ├── types.rs      → CommandCode, ErrorCode, Mta, ConnectInfo
  │     ├── commands.rs   → Command packet builders
  │     └── responses.rs  → Response parsers
  └── transport/          → Async transport abstraction (async-trait)
        ├── transport.rs  → Transport trait + TransportFactory + TransportConfig
        ├── udp.rs        → UDP/IP implementation (production)
        └── can.rs        → CAN stub (NotImplemented)
```

## Key Conventions

### Multi-Slave Identity
- Every slave gets a `SlaveId(Uuid)` on connection via `XcpClient::connect_slave`.
- Variables are always wrapped in `DeviceVariable` which carries both the `SlaveId` and `slave_name` for display (`[ECU1] param_name`).
- `DeviceState` (in `web/state.rs`) holds per-device `CalibrationManager` and `DaqManager` behind `Arc<RwLock<_>>`.

### Error Handling
- All public errors use `thiserror` with `#[error(...)]` attributes.
- Transport errors: `TransportError` enum; protocol errors: `ErrorCode` enum (implements `std::error::Error`).
- `SlaveError` wraps both via `#[from]` conversions.

### XCP Transport Header
Packets are prefixed with a 4-byte header: `[length_L, length_H, reserved, reserved]` followed by the command payload. All multi-byte values use little-endian encoding.

### A2L Module Deprecation
`a2l_reader.rs` is deprecated. Use the `a2l` module (`a2l::A2lReader`, `a2l::CompuMethod`, etc.) for all new code. The `lib.rs` re-exports the `a2l` module types at the crate root.

### Transport Abstraction
New transports implement the `Transport` trait (from `transport/transport.rs`) using `#[async_trait]`. Add the variant to `TransportConfig` and handle it in `TransportFactory::create`.

### Web UI State
- `AppState` is the top-level Axum shared state (wrapped in `Arc`).
- Single-user constraint is enforced in `auth.rs` — concurrent logins are rejected.
- Default credentials: `admin` / `admin`.

### Calibration Types
`CalibrationType` covers scalars (U8–F64, Bool), fixed-size arrays (`U8Array(n)`), and row-major matrices (`U8Matrix(rows, cols)`). Always compute byte size via `CalibrationType::size()`.

### Logging
Uses `tracing` with `tracing-subscriber`. Log to file only (not stdout/stderr), so TUI rendering isn't corrupted. Use `tracing::info!`, `tracing::debug!`, `tracing::warn!`, `tracing::error!` macros.

## DAQ Workflow

Dynamic DAQ follows the XCP standard sequence. All individual DAQ commands are exposed on `XcpSlave` (and proxied through `XcpClient`) as async methods:

1. **Probe slave capabilities** — `get_daq_processor_info()` → `DAQProcessorInfo`, `get_daq_resolution_info()` → `DAQResolutionInfo`
2. **Free existing DAQ** — `free_daq()`
3. **Allocate structure** — `alloc_daq(daq_count)` → `alloc_odt(daq_list, odt_count)` → `alloc_odt_entry(daq_list, odt, entry_count)`
4. **Write entries** — `set_daq_ptr(daq_list, odt, entry)` then `write_daq(bit_offset, mta, data_size)` for each `OdtEntryConfig`
5. **Set mode** — `set_daq_list_mode(daq_list, mode, event_channel, prescaler, priority)`
6. **Start/Stop** — `start_stop_daq_list(mode, daq_list)` and `start_stop_synch()`

Higher-level orchestration lives in `DaqManager`:
- `selected_indices` tracks which measurements from `all_measurements` are active
- DTO packets arrive as `Response::Daq { daq_list, odt, data }` and are decoded into `DaqRecord` using `DaqConfiguration::get_odt_entries()`
- `DaqRecord.timestamp` is a **raw slave clock counter** (not UTC). Convert with `TimestampUnit::to_seconds()`
- Raw bytes are converted to physical `f64` values via each `Measurement`'s `CompuMethod`
- Decoded samples are written to CSV (`daq_data.csv` by default) and cached in `latest_sample` for the Web UI

### Key Types

| Type | Purpose |
|------|---------|
| `OdtEntryConfig` | Position of one measurement in the DAQ structure (daq_list, odt, entry, byte_offset) |
| `DaqConfiguration` | Complete decoded structure used to parse incoming DTO packets |
| `DaqRecord` | One timestamped sample: `HashMap<name, f64>` physical values |
| `TimestampUnit` | Enum mapping slave clock resolution (1ns … 1s) to `f64` seconds |

## COMPU_METHOD Conversion

`CompuMethod` (in `a2l/compute_method.rs`) converts raw ECU integer values to physical display values bidirectionally.

| Variant | Formula | Reverse supported |
|---------|---------|-------------------|
| `Identical` | display = raw | ✅ |
| `Linear { factor, offset }` | display = factor × raw + offset | ✅ |
| `Rational { a,b,c,d }` | display = (a×raw + b) / (c×raw + d) | ❌ |
| `TabVerb { pairs }` | linear interpolation over (raw, phys) pairs | ❌ |
| `CompuVtab { entries, ... }` | discrete enum lookup (raw ↔ text string) | ✅ (by text or numeric fallback) |

**Key rules:**
- Float data types (`F32`, `F64`) **skip COMPU_METHOD** in `bytes_to_display` — they are parsed directly from IEEE 754 little-endian bytes.
- For integer types, bytes are parsed as **signed** little-endian before conversion.
- `CompuVtab` stores two `HashMap`s: `raw_to_text` and `text_to_raw` for O(1) lookup in both directions.
- `CompuMethodFactory::from_a2l()` bridges from the `a2lfile` crate's parsed AST into this crate's `CompuMethod`.
- `CompuMethod::default()` is `Identical`.

**Usage pattern:**
```rust
// Forward: raw bytes → display
let phys = measurement.compu_method.bytes_to_display(&raw_bytes, is_float);

// Forward: raw integer → display string (for calibration UI)
let text = cal_param.compu_method.to_display(raw_i64);

// Reverse: user input → raw (for write-back)
let raw = cal_param.compu_method.to_raw(&user_input_str)?;
```

## Web UI Routes

All routes (except `/login`, `/api/login`) require a valid session cookie enforced by `auth_middleware`.

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/login` | Login page |
| POST | `/api/login` | Submit credentials |
| GET | `/logout` | Invalidate session |
| GET | `/` | Dashboard — all slaves overview |
| GET | `/slaves` | Slave list with select/disconnect |
| GET | `/api/slave/select/:id` | Switch current slave |
| GET | `/api/disconnect/:id` | Disconnect a specific slave |
| GET | `/connect` | Connect-new-slave form |
| POST | `/api/connect` | Connect via address |
| GET | `/api/disconnect` | Disconnect current slave |
| POST | `/api/devices/connect-all` | Connect all configured devices |
| POST | `/api/devices/connect` | Connect a device by config |
| POST | `/api/devices/disconnect` | Disconnect a device |
| POST | `/api/devices/remove` | Remove a device entry |
| GET | `/a2l` | A2L upload page |
| POST | `/api/a2l/load` | Upload A2L file (max 50 MB) |
| GET | `/calibration` | Calibration parameter browser |
| GET | `/api/cal/available` | List available parameters (JSON) |
| GET | `/api/cal/read/:device_id/:idx` | Read one parameter from ECU |
| GET | `/api/cal/value/:device_id/:idx` | Get cached parameter value |
| GET | `/api/cal/pagemode` | Get current page mode |
| POST | `/api/cal/pagemode/:device_id` | Switch Working/Reference page |
| GET | `/cal/write/:device_id/:idx` | Write form for a parameter |
| POST | `/api/cal/write/:device_id/:idx` | Write scalar/array parameter |
| POST | `/api/cal/write-element/:device_id/:idx` | Write single matrix element |
| GET | `/api/cal/detail/:device_id/:idx` | Parameter detail (JSON) |
| GET | `/daq` | DAQ measurement page |
| GET | `/api/daq/available` | List available measurements (JSON) |
| POST | `/api/daq/select` | Set selected measurements |
| GET | `/api/daq/start` | Start DAQ acquisition |
| GET | `/api/daq/stop` | Stop DAQ acquisition |
| GET | `/api/daq/status` | DAQ running status |
| POST | `/api/daq/toggle` | Toggle one measurement on/off |
| GET | `/api/daq/clear` | Clear selection |
| POST | `/api/daq/data` | Poll latest DAQ sample (JSON) |
| GET | `/api/daq/detail/:device_id/:idx` | Measurement detail (JSON) |

## TUI Module

The TUI is a single-slave, keyboard-driven interface built with **ratatui** + **crossterm** + **tui-input**.

### Architecture

```
tui/
  app.rs      → App struct (all state), render via ratatui Frame, async handle_key
  event.rs    → EventHandler: two tokio tasks (key poller + ticker), CancellationToken
  terminal.rs → Tui wrapper: enter/exit alternate screen, crossterm backend
  mod.rs      → re-exports
```

- `EventHandler::new(tick_rate)` spawns two background tasks: a crossterm key poller (50 ms poll) and a `Tick` interval generator. Only `KeyEventKind::Press` events are forwarded (Release/Repeat are discarded).
- The main loop calls `tui.draw(&mut app)` then `event_handler.next().await`, matching on `Event::Key | Event::Tick | Event::Resize`.
- `App::handle_key(key) -> Result<bool, _>` returns `false` to signal exit.
- `App::on_tick()` is called on `Event::Tick` for live DAQ data polling.

### App Modes (`AppMode`)

| Mode | Description |
|------|-------------|
| `Menu` | Main menu (context-sensitive: fewer items when disconnected) |
| `Connect` | Connection address input |
| `ParamList` | Scrollable calibration parameter list |
| `ParamDetail` | Detail/edit view for a single parameter |
| `CombinedView` | Split pane: calibration left, DAQ right |
| `Input` | Generic text input dialog (reused for several prompts) |
| `SelectA2L` | A2L file path entry |
| `SearchDialog` | Variable search/filter with keyboard-selectable results |

### CombinedView

The main working mode. Two focusable panels toggled with Tab:
- `CombinedFocus::Calibration` — scrollable list, reads values from cache (`cal_values: HashMap<String, CalibrationValue>`)
- `CombinedFocus::DaqSelection` — DAQ measurement selection; live values display in `DaqDisplayMode::Text` or chart mode

Color scheme: `MATRIX_GREEN` / `MATRIX_BRIGHT_GREEN` (green-on-black terminal aesthetic).

## Testing Approach

All tests are inline `#[cfg(test)] mod tests { ... }` within each source file — no separate test directory.

```bash
cargo test                           # Run all tests
cargo test test_name -- --nocapture  # Run one test with stdout
cargo test -- --test-threads=1       # Force serial (needed if tests share global state)
```

### Test pattern per module

| Module | What is tested |
|--------|---------------|
| `protocol/types.rs` | Command/error code byte values, `ConnectInfo::parse`, byte conversion helpers |
| `protocol/commands.rs` | Command packet byte layout (exact byte-level assertions) |
| `protocol/responses.rs` | Response packet parsing |
| `transport/transport.rs` | `TransportConfig`, `TransportType`, `TransportError` display |
| `transport/message_router.rs` | Message routing logic |
| `a2l/compute_method.rs` | All `CompuMethod` variants: forward, reverse, float bypass |
| `a2l/reader.rs` | A2L file parsing from string fixtures |
| `calibration.rs` | `CalibrationType::size()`, value encoding/decoding |
| `daq.rs` | `DaqConfiguration`, `DaqRecord`, byte decoding |
| `client.rs` | `XcpClient` construction, `SlaveId` uniqueness, state queries |
| `slave.rs` | `SlaveId` UUID format, `SlaveInfo` display name, `SlaveError` display |
| `variable.rs` | `DeviceVariable` construction, `VariableType` helpers |
| `web/auth.rs` | Credential check, single-user session lock, logout/re-login |
| `web/templates.rs` | Template rendering |

### Important caveats

- `web/auth.rs` tests share global `once_cell` session state. Use `clear_session()` in both setup and teardown, and run auth tests with `--test-threads=1` if they race.
- Tests that require an actual XCP slave over UDP are **integration tests** not included in the repo — `cargo test` only runs unit tests that mock the transport or test pure logic.
- Protocol tests use hard-coded byte arrays that match the ASAM MCD-1 XCP V1.5.0 spec. Keep these aligned with the spec when adding new commands.

## Protocol Reference

XCP command codes and error codes match ASAM MCD-1 XCP Protocol Layer V1.5.0 (spec PDF is in the repo root). `CommandCode` is `#[repr(u8)]`; `ErrorCode` implements `From<u8>` for deserialization.
