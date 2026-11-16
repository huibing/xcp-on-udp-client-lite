# ETAS ES582 CAN driver (BOA V2 / OCI)

`src/transport/can/es582.rs` adds ETAS ES582 (and other BOA-capable ETAS
interfaces) as an XCP-on-CAN device, next to the PEAK-System PCAN driver.

## How it works

The driver does not link against the ETAS SDK. Everything is resolved at
runtime from the BOA V2 installation:

| Layer | DLL | Purpose |
|-------|-----|---------|
| Search-for-service | `dll-csiBind.dll` | `CSI_CreateProtocolTree`, `CSI_GetUriForUuid`, `CSI_DestroyProtocolTree` |
| Controller proxy | `dll-ocdProxy.dll` | `OCI_CreateCANControllerVersion`, `OCI_OpenCANController`, `OCI_CreateCANRxQueue`, `OCI_AddCANFrameFilter`, `OCI_ReadCANDataEx`, `OCI_WriteCANDataEx`, … |
| Hardware OCD | `ES582_1_OCD_CAN.dll` | Loaded by BOA itself from its `OCDs` directory |

1. **Enumerate** – a CSI protocol tree is created for the whole node range and
   queried for `UUID_OCICAN` once per supported API version. Every returned URI
   looks like `ETAS://USB/ES582.1:114913/CAN:1`.
   For each URI a controller is bound briefly to read
   `OCI_CANControllerCapabilities` (CAN-FD support, max data size/rate, media,
   bus events, FD rx/tx modes) and `OCI_TimerCapabilities` (timestamp tick
   frequency); the controller is destroyed again. A channel that is already
   open elsewhere reports `available = false`.
2. **Open** – `OCI_CreateCANControllerVersion` is called with the newest
   supported API version (1.6 → 1.0), then `OCI_OpenCANController` with the bit
   rate and timing, an Rx queue carrying the `onFrame` callback, the hardware
   frame filters and a Tx queue.
3. **Receive** – the OCI `onFrame` callback (`OCI_CANRxCallbackFunctionSingleMsgEx`)
   is registered on the Rx queue, so BOA pushes every frame that passes the
   hardware filters from its own receive thread. The callback copies the frame
   into a bounded local queue (`RxContext`) and `recv_frame` only drains that
   queue — no USB round trip per frame. Classic CAN (`OCI_CAN_RX_MESSAGE`) and
   CAN-FD (`OCI_CANFDRX_MESSAGE`) frames share one decoder; error frames/bus
   events are surfaced with an empty payload. Timestamps are converted from
   controller ticks to microseconds using the timer tick frequency (2 MHz on an
   ES582).
4. **Transmit** – classic CAN for payloads ≤ 8 bytes, CAN-FD (`FD_DATA` +
   `FD_DATA_BIT_RATE`) for longer payloads when the channel was opened in FD
   mode. The write uses a zero timeout: `OCI_WriteCANData*` blocks while the Tx
   queue is full, and `OCI_NO_TIME` would hang forever on a bus that stops
   acknowledging frames.

The XCP framing itself (one packet per frame, PID filtering, `MAX_DLC_REQUIRED`
padding) is unchanged and lives in `CanTransport`.

## Why the callback, not `OCI_ReadCANDataEx`

Polling was the first implementation and it does not work on a real bus.
Measured on an ES582.1 (500 kbit/s, 150 frames/s of periodic traffic):

| Path | `OCI_ReadCANDataEx` (poll) | `onFrame` callback (default) |
|------|---------------------------|------------------------------|
| Frames per call | **1** (even with `size = 16`) | whole batch, pushed by BOA |
| Cost per call | ~7.5 ms busy / ~15-20 ms idle (USB round trip) | ~0 (a mutex push) |
| Capture of 150 frames/s | 354 of 750 frames in 5 s (~80 % lost) | **750 of 750** |
| DAQ, 10 ms event | 65-88 samples/s (expected 100), jittery | **100.0 samples/s**, steady |

A poll loop therefore falls behind the bus, the device queue grows, and every
XCP response / DAQ sample is delayed by the backlog — in the desktop app this
showed up as variable updates lagging by minutes.

The callback runs on a BOA-internal thread, so the implementation keeps the
contract tight:

- the body never blocks, never calls back into OCI, and is wrapped in
  `catch_unwind` (a panic must not unwind across the FFI boundary);
- the `OCI_CANMessageEx*` is only valid during the call, so the frame is copied
  immediately (fixed 96-byte view, `sizeof(OCI_CANMessageEx)`);
- the queue is capped (`RX_QUEUE_CAPACITY`); on overflow the oldest frame is
  dropped and counted (`dropped_frames()`);
- BOA may invoke the callback once **after** `OCI_DestroyCANController` returns,
  so `RxContext` is never freed: teardown sets `closed` (the callback then
  no-ops), wakes waiters and leaks one reference.

`ES582_RX_MODE=poll` switches back to the polling path for comparison/debugging;
`callback` is the default.

## Hardware filtering still matters

Even with the callback path, unrelated traffic is pointless work. `CanTransport`
passes `CAN_ID_SLAVE` + the DAQ DTO IDs to the driver in
`CanDeviceConfig::accept_ids`, and the driver programs one
`OCI_AddCANFrameFilter` per ID (exact 29-bit match), so only XCP frames are
queued.

When `accept_ids` is empty (e.g. `examples/es582_verify.rs`, which monitors the
raw bus) the driver falls back to a single accept-all filter.

## FFI layout notes

The OCI structures are versioned and grow at the end, so the driver allocates
zeroed 512-byte buffers and writes only documented field offsets instead of
mirroring every `#[repr(C)]` struct. The offsets are pinned by
`test_structure_offsets_are_consistent` and were verified against the ETAS
headers with `offsetof` (MSVC x64):

```
OCI_CANConfiguration        sizeof 116  (canFDConfig@40, canTxPolicy@80, can20Enabled@112)
OCI_CANFDConfiguration      sizeof  40  (canFdRxConfig@24 is a struct of two enums, canFdTxConfig@32)
OCI_CANRxQueueConfiguration sizeof  40  (selfReceptionMode@32)
OCI_CANMessage              sizeof  40  (union @8, rx dlc@27, data@32)
OCI_CANMessageEx            sizeof  96  (fd size@27, data@32)
```

Two traps worth remembering:

* `OCI_CANRxQueueConfiguration::selfReceptionMode` is at offset **32**, not 16 —
  the two callback members are structs of two function pointers each. Writing a
  self-reception flag at offset 16 installs a bogus callback pointer.
* `OCI_CANFDConfiguration::canFdTxConfig` is at offset **32**, not 28, because
  `canFdRxConfig` holds two enums (`canRxMode`, `canFdRxMode`). Leaving
  `canFdTxConfig` at 0 makes `OCI_OpenCANController` fail with
  `BOA_ERR_INVALID_PARAMETER` as soon as CAN-FD is enabled.

## Bit timing

`OCI_CANConfiguration` takes `BTL_Cycles` (time quanta per bit), `samplePoint`
(%) and `SJW`; the OCD derives the controller registers. A combination is only
realisable when `controller_clock / (bitrate × BTL_Cycles)` is an integer, and
the ES582 oscillator is not published. The driver therefore tries a list of
prescaler-friendly candidates (10, 16, 20, 8, 25, 40, 80 — plus 12/60 %/SJW 3
for the FD data phase) and keeps the first one the OCD accepts. Each attempt
rebinds the controller so a rejected configuration cannot leak state.

Unusual bit rates can pin the timing explicitly:

```
ES582_BTL_CYCLES=20 ES582_SAMPLE_POINT=80 ES582_SJW=1
ES582_FD_BTL_CYCLES=12 ES582_FD_SAMPLE_POINT=60 ES582_FD_SJW=3
```

## Environment variables

| Variable | Meaning |
|----------|---------|
| `BOA_DLL_DIR` | directory with `dll-ocdProxy.dll` / `dll-csiBind.dll` (default `%ProgramFiles%\ETAS\BOA_V2\Bin\x64\Dll\Framework`) |
| `BOA_OCD_DIR` | OCD plugin directory (default `%ProgramFiles%\ETAS\ED-EBI_V2\Bin\x64\Dll\OCDs`) |
| `ES582_URI` | channel URI to use when the config pins none |
| `ES582_RX_MODE` | `callback` (default) or `poll` (`OCI_ReadCANDataEx`, diagnostics only) |
| `ES582_SELF_RECEPTION` | `1` also delivers self-transmitted frames (loopback bring-up) |
| `ES582_BTL_CYCLES`, `ES582_SAMPLE_POINT`, `ES582_SJW` | nominal phase timing override |
| `ES582_FD_BTL_CYCLES`, `ES582_FD_SAMPLE_POINT`, `ES582_FD_SJW` | CAN-FD data phase timing override |

All BOA directories (framework, `vNetOCDs`, the OCD folders) are prepended to
`PATH` before the DLLs are loaded, because the framework resolves its own
dependencies and OCD plugins by name.

## Usage

```bash
# Device/channel info + 5 s of bus traffic
cargo run --example es582_verify

# CAN-FD, 3 s, plus one test frame
ES582_VERIFY_FD_BITRATE=2000000 ES582_VERIFY_READ_SECS=3 ES582_VERIFY_TX=1 \
  cargo run --example es582_verify

# Full XCP session (CONNECT, pages, SET_MTA+UPLOAD, DISCONNECT)
cargo run --example es582_xcp_test -- pms-a.a2l

# From the CLI / config file
cargo run --release -- verify --a2l hello_xcp.a2l --transport can \
  --can-driver es582 --can-channel "ETAS://USB/ES582.1:114913/CAN:1" \
  --can-id 0x550 --slave-id 0x551 --baudrate 500000
```

In a TOML config file (`daq-record`):

```toml
[connection]
type = "can"
driver = "es582"
channel = "auto"          # or the full URI, or "1" for controller 1
can_id = 0x550
slave_id = 0x551
baudrate = 500000
# data_baudrate = 2000000   # enable CAN-FD
```

## Troubleshooting

| Symptom | Cause / fix |
|---------|-------------|
| `BOA V2 framework not found` | BOA/EBI-IP not installed, or set `BOA_DLL_DIR` |
| `CSI_CreateProtocolTree returned no tree` | ESS providers not found — check that `BOA_V2\Bin\x64\Dll\vNetOCDs` exists, run with `RUST_LOG=debug` |
| No channel listed | ES582 not plugged in, or its OCD (`ES582_1_OCD_CAN.dll`) missing from the OCD folder; check the ETAS log directory |
| `available = false` | the channel is already open in another process (e.g. INCA) |
| `OCI_OpenCANController … invalid parameter` for every candidate | bit rate/timing not representable; pin `ES582_BTL_CYCLES`/`ES582_SAMPLE_POINT` |
| Frames received but state goes `error-passive`/`bus-off` after transmit | no other node acknowledges — wrong bus or wrong bit rate |
| No frames at all | bus is idle or the bit rate differs; verify with `ES582_SELF_RECEPTION=1` plus a self-transmitted frame |
| XCP commands time out although the slave answers | bus traffic floods the device queue — check that `accept_ids` reached the driver (`RUST_LOG=debug` logs `N hardware frame filter(s)`) and that the receive path is the callback (`RUST_LOG=info` logs `rx=callback` / `receive queue created in Callback mode`), and raise the CTO timeout for slow slaves (`XcpClient::set_command_timeout`) |
| DAQ/变量更新滞后、丢帧 | the polling path was in use (`ES582_RX_MODE=poll`) — it loses ~80 % of frames on a 150 frames/s bus; drop the variable and restart, `dropped_frames()` must stay 0 |