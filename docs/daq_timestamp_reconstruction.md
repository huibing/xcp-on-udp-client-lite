# DAQ Timestamp Reconstruction Design

## Problem Statement

XCP slaves embed a hardware clock counter in every DTO (Data Transfer Object) packet. This counter is a raw tick value that:

1. **Counts in hardware ticks**, not human-readable time — the unit (1ns, 10µs, 1ms, etc.) varies per slave
2. **Wraps around** (saturates to max then rolls back to 0) — e.g., a 16-bit counter at 1ms/tick wraps every ~65.5 seconds
3. **Has no absolute epoch** — the zero point is slave-specific (e.g., power-on time), not UTC

The goal is to reconstruct a **continuous, monotonic, relative timeline in seconds** from these raw tick values, where `t=0.000s` corresponds to the moment the user clicks "Start Recording".

### Concrete Example (u16 timestamp, 1ms/tick)

Suppose the slave uses a **16-bit counter** with **1ms per tick**. The counter runs from 0 to 65535, then wraps to 0.

```
Slave clock at power-on:         0
User starts recording at tick:   60000  ← t0_ticks (from GET_DAQ_CLOCK)
```

**DTO stream (simplified):**

| Packet # | Raw timestamp (u16) | Elapsed since t0 | Reconstructed time |
|----------|--------------------:|------------------:|-------------------:|
| 1        | 60100               | 100 ticks         | 0.100 s            |
| 2        | 60200               | 200 ticks         | 0.200 s            |
| ...      | ...                 | ...               | ...                |
| 54       | 65400               | 5400 ticks        | 5.400 s            |
| 55       | 65535               | 5535 ticks        | 5.535 s            |
| 56       | 100 ← **WRAP!**     | 5636 ticks        | 5.636 s            |
| 57       | 200                 | 5736 ticks        | 5.736 s            |

**Before the wrap** (packet 54):
```
relative_ticks = 65400_u16.wrapping_sub(60000) = 5400
time_s = 5400 * 0.001 = 5.400 s  ✅
```

**After the wrap** (packet 56):
```
relative_ticks = 100_u16.wrapping_sub(60000) = 5636   // (100 - 60000) mod 65536
time_s = 5636 * 0.001 = 5.636 s  ✅
```

`wrapping_sub` gives the correct result because unsigned modular arithmetic naturally computes the forward distance, even across a wrap — as long as the total elapsed time doesn't exceed one full counter cycle (65535 ticks = 65.535s for this example).

## Current State

| Component | Status | Notes |
|-----------|--------|-------|
| DTO timestamp parsing (1/2/4 bytes) | ✅ Done | `parse_dto_packet()` in `daq.rs` |
| `GET_DAQ_RESOLUTION_INFO` (unit & size) | ✅ Done | `get_timestamp_mode()` in `daq.rs` |
| `GET_DAQ_CLOCK` (base clock read) | ✅ Done | Called in `start()`, stored as `base_timestamp` |
| `TimestampUnit::to_seconds()` | ✅ Done | Converts tick count to seconds multiplier |
| Raw tick storage in `DaqRecord` | ✅ Done | `record.timestamp: u32` (raw ticks) |
| CSV recording | ⚠️ Partial | Writes raw ticks, not converted seconds |
| Web UI display | ❌ Missing | Timestamp not sent to frontend |
| Wraparound handling | ❌ Missing | No wrap detection or correction |
| Relative time (t=0 at start) | ❌ Missing | `base_timestamp` used as offset, but not subtracted to produce relative time |

### What `base_timestamp` does today

```
current code:   record.timestamp = base_timestamp.wrapping_add(relative_ts)
```

This produces an **absolute slave tick value** (base + relative). It does **not** produce a relative "time since start" value. Also, no conversion to seconds is performed.

## Proposed Solution

### Step 1: Capture `t0` at recording start

When the user starts DAQ, call `GET_DAQ_CLOCK` and store the result as `t0_ticks`. This is already done (`base_timestamp`), but the semantic needs to change: `t0_ticks` represents the **reference origin**, not an additive offset.

Note: `GET_DAQ_CLOCK` always returns a `u32` regardless of DTO timestamp size. The DTO timestamp may be 1, 2, or 4 bytes — but the slave truncates the same underlying clock into the DTO field. For `wrapping_sub` to work correctly, the `t0` value must be truncated to match the DTO timestamp width.

### Step 2: For each DTO timestamp, compute relative seconds

```
DTO arrives with raw timestamp: ts_raw

// Truncate t0 to match DTO timestamp width
t0_truncated = t0_ticks & mask   // mask = 0xFF (1 byte), 0xFFFF (2 bytes), 0xFFFFFFFF (4 bytes)

// Compute relative ticks using unsigned modular subtraction at the DTO width
relative_ticks = ts_raw.wrapping_sub(t0_truncated)

// Convert to seconds
relative_seconds = (relative_ticks as f64) * timestamp_unit.to_seconds()
```

**Worked example (u16, 1ms/tick, continued from above):**
```
t0_ticks from GET_DAQ_CLOCK = 0x0000_EA60  (60000 as u32)
t0_truncated = 0xEA60 & 0xFFFF = 0xEA60    (60000 as u16)

Packet 56: ts_raw = 0x0064  (100 as u16)
relative_ticks = 0x0064_u16.wrapping_sub(0xEA60_u16)
              = (0x10064 - 0xEA60) mod 0x10000
              = 0x1604
              = 5636 ticks
time_s = 5636 * 0.001 = 5.636 s  ✅
```

### Step 3: Handle extended recording (multi-wrap)

For long recordings that exceed one full counter cycle, a single `wrapping_sub` becomes ambiguous. We need to track wrap events:

```rust
struct TimestampTracker {
    t0_ticks: u32,               // GET_DAQ_CLOCK at start (truncated to DTO width)
    prev_raw: u32,               // last seen raw timestamp
    wrap_count: u64,             // number of detected wraps
    max_value: u64,              // 0xFF, 0xFFFF, or 0xFFFF_FFFF depending on DTO size
    timestamp_unit: TimestampUnit,
}
```

**Wrap detection logic:**
```
if ts_raw < prev_raw && (prev_raw - ts_raw) > (max_value / 2) {
    wrap_count += 1;   // counter rolled over
}
prev_raw = ts_raw;

total_ticks: u64 = (wrap_count * (max_value + 1)) + ts_raw.wrapping_sub(t0_ticks) as u64
relative_seconds: f64 = (total_ticks as f64) * timestamp_unit.to_seconds()
```

**Example: u16 counter, second wrap (packet arrives after ~131s):**
```
wrap_count = 2
ts_raw = 500
t0_truncated = 60000

total_ticks = 2 * 65536 + 500_u16.wrapping_sub(60000_u16) as u64
            = 131072 + 6036
            = 137108
time_s = 137108 * 0.001 = 137.108 s  ✅
```

The threshold `max_value / 2` distinguishes genuine wraps from out-of-order packets.

### Step 4: Multi-event timestamp handling

With per-signal event configuration (different events at different rates), each event group generates independent DTO streams. Each stream has its own timestamp sequence, but all share the same slave clock. Therefore:

- **One `t0_ticks`** is sufficient (single `GET_DAQ_CLOCK` call)
- **One `TimestampTracker`** per DAQ list is needed (each list may wrap independently in terms of delivery order, but should use the same t0)
- When merging `latest_sample` across events, each event updates the shared record's timestamp to its own latest — the UI should display the **most recent** timestamp

### Step 5: Output

#### CSV file
```
time_s,signal_1,signal_2,...
0.000000,12.5,100.0,...
0.005000,12.6,100.1,...
0.010000,12.7,100.2,...
```
- First column: `time_s` — elapsed seconds since recording start (f64, 6 decimal places)
- Replaces the current raw tick value

#### DaqRecord
```rust
pub struct DaqRecord {
    pub timestamp_s: f64,       // Elapsed seconds since start (converted from ticks)
    pub timestamp_raw: u32,     // Raw tick value (for debugging)
    pub values: HashMap<String, f64>,
    pub raw_values: HashMap<String, Vec<u8>>,
}
```

## Timestamp Size & Wrap Intervals

| Timestamp Size | Max Value | At 1ms/tick | At 1µs/tick | At 10µs/tick |
|:--------------:|----------:|------------:|------------:|-------------:|
| 1 byte (u8)    | 255       | 0.255 s     | 0.255 ms    | 2.55 ms      |
| 2 bytes (u16)  | 65,535    | 65.5 s      | 65.5 ms     | 655 ms       |
| 4 bytes (u32)  | 4.29×10⁹  | 49.7 days   | 71.6 min    | 11.9 hours   |

**Important:** For u8 and u16 timestamps, wrapping happens frequently. Multi-wrap tracking (Step 3) is essential for u8/u16 to maintain correct timelines beyond one cycle.

## Edge Cases

1. **Slave doesn't support timestamps** — `timestamp_size = 0`. Use the timestamp from transport layer as the time stamp, e.g. timestamp when master gets the DTO message. This timestamp should also starts from 0.
2. **GET_DAQ_CLOCK fails** — Use first DTO timestamp as `t0_ticks` (graceful fallback, already partially implemented).
3. **Out-of-order DTOs** — Possible with UDP transport. Timestamps should still be computed via `wrapping_sub(t0)`, so they'll show correct relative time even if packets arrive out of order. However, this can trigger false wrap detection with multi-wrap tracking — use the threshold heuristic to mitigate.
4. **Multiple slaves** — Each slave has its own clock domain. `TimestampTracker` is per-slave (per-`DaqManager`), so no cross-contamination.
5. **Slave reset during recording** — Clock resets to 0. This will appear as a sudden jump. Cannot be handled transparently without an explicit reconnect.
6. **u8 timestamp at high rate** — A 1-byte counter at 1µs/tick wraps every 0.255ms. At a 100µs event rate, you get ~2.5 samples per cycle — wrap detection is critical. Consider whether u8 timestamps are practical for such use cases.

## Files to Modify

| File | Changes |
|------|---------|
| `src/daq.rs` | Add `TimestampTracker`, change `DaqRecord.timestamp` to `f64` seconds (keep raw for debug), update DTO reception loop to compute relative seconds, update CSV writing |
| `src/web/routes.rs` | Include `timestamp_s` in `/api/daq/data` response |
| `src/web/templates/daq.hbs` | Display elapsed time in live data panel |

## Summary

The core change is: **replace `base.wrapping_add(rel)` with `(raw.wrapping_sub(t0) + wrap_count * cycle) * unit_seconds`**. The current infrastructure (GET_DAQ_CLOCK, timestamp parsing, unit conversion) is already in place — what's missing is the final conversion step, width-aware truncation, and wrap tracking.
