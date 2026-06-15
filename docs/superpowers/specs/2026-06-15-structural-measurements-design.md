# Structural Measurements Support — Design

**Date:** 2026-06-15
**Status:** Draft for review
**Scope:** A2L `TYPEDEF_STRUCTURE` + `TYPEDEF_MEASUREMENT` + `TYPEDEF_CHARACTERISTIC` + `INSTANCE` parsing in the Rust XCP client.

## Problem

The XCP client only consumes flat A2L `MEASUREMENT` and `CHARACTERISTIC` blocks today. ECU vendors increasingly publish structural variables — instances of typedef structs whose components are individual signals — for example:

```a2l
/begin TYPEDEF_MEASUREMENT input "" FLOAT32_IEEE NO_COMPU_METHOD 0 0 ... /end TYPEDEF_MEASUREMENT
/begin TYPEDEF_MEASUREMENT output_linear "" FLOAT32_IEEE NO_COMPU_METHOD 0 0 ... /end TYPEDEF_MEASUREMENT
/begin TYPEDEF_MEASUREMENT output_spline "" FLOAT32_IEEE NO_COMPU_METHOD 0 0 ... /end TYPEDEF_MEASUREMENT

/begin TYPEDEF_STRUCTURE Lookup "" 12
    /begin STRUCTURE_COMPONENT input         input         0 /end STRUCTURE_COMPONENT
    /begin STRUCTURE_COMPONENT output_linear output_linear 4 /end STRUCTURE_COMPONENT
    /begin STRUCTURE_COMPONENT output_spline output_spline 8 /end STRUCTURE_COMPONENT
/end TYPEDEF_STRUCTURE

/begin INSTANCE lookup "" Lookup 0xFFF4 ECU_ADDRESS_EXTENSION 2
    /begin IF_DATA XCP /begin DAQ_EVENT FIXED_EVENT_LIST EVENT 0 /end DAQ_EVENT /end IF_DATA
/end INSTANCE
```

Such variables are currently invisible to the client — they cannot be measured, calibrated, or displayed.

## Goals

1. Read `INSTANCE` blocks from A2L and expose their components in the existing measurement / calibration lists.
2. Each leaf scalar (or 1D/2D array) component becomes a fully-functional measurement or calibration parameter that goes through every existing code path: DAQ allocation, DTO decoding, calibration write, UI list, search, charts.
3. Zero impact on existing flat `MEASUREMENT` / `CHARACTERISTIC` behavior.

## Non-goals (this iteration)

- Array-of-structs (`INSTANCE` with `matrix_dim`).
- `OVERWRITE` blocks inside `INSTANCE`.
- `CURVE` / `MAP` components inside `TYPEDEF_STRUCTURE` (only `VALUE`-shaped scalars and arrays).
- Struct-aware UI (e.g. "write whole struct atomically", grouped tree view).
- Direct memory polling specific to structs (the existing flat-measurement polling path covers leaf components automatically).

## Decision: Approach A — flatten in the A2L reader

The reader resolves `INSTANCE` recursively and emits one synthetic `A2lMeasurement` (or `A2lCharacteristic`) per leaf component. Downstream code (`DaqManager`, `CalibrationManager`, `DeviceVariable`, all UI surfaces) is unchanged.

**Why:**
- One module changes; reuses every downstream path.
- Matches the chosen UX (flat list, dot-named, scalar-edit).
- Easy to test in isolation.
- Future struct-aware features can be added later by attaching an optional `parent_instance: Option<String>` to the existing structs without breaking anything.

Approach B (first-class `A2lStructure` / `A2lInstance` types stored on the reader, with flattening pushed to consumers) was considered and rejected as premature: it would touch `daq.rs`, `calibration.rs`, web routes, Tauri commands, and `DeviceVariable` for no current user-visible benefit.

## Architecture

```
src/a2l/
├── reader.rs                   # extended: get_measurements / get_characteristics
│                               #   call new flattening helper
├── typedef_flatten.rs   (NEW)  # private: resolves INSTANCE + TYPEDEF_STRUCTURE
│                               #   + TYPEDEF_MEASUREMENT/CHARACTERISTIC into
│                               #   synthetic A2lMeasurement / A2lCharacteristic
└── types.rs                    # unchanged
```

**Public API:** `A2lReader::get_measurements()` and `A2lReader::get_characteristics()` return the same `Vec<A2lMeasurement>` / `Vec<A2lCharacteristic>` types as today, with synthetic entries appended **after** the parsed flat entries. This insertion order is load-bearing — it is what makes the "flat MEASUREMENT/CHARACTERISTIC wins on name collision" rule work without an explicit precedence check. Do not reorder this merge.

**Build-once lookup tables per call:**
- `HashMap<&str, &TypedefStructure>` — keyed by structure name
- `HashMap<&str, &TypedefMeasurement>` — keyed by typedef name
- `HashMap<&str, &TypedefCharacteristic>` — keyed by typedef name (calibration path)

Lookups are O(1); build cost is O(n) over the module.

## Data model

No new public types. Synthetic entries reuse `A2lMeasurement` / `A2lCharacteristic` verbatim. Struct context lives in the `name` field as a dot-joined path.

### Synthetic `A2lMeasurement` field mapping

| Field | Source |
|---|---|
| `name` | `"<instance>.<comp1>[.<comp2>...]"` |
| `description` | `TYPEDEF_MEASUREMENT.long_identifier` |
| `address` | `INSTANCE.start_address + Σ STRUCTURE_COMPONENT.address_offset` along the path |
| `address_ext` | `INSTANCE.ecu_address_extension` (defaults 0; nested levels inherit) |
| `data_type` | `TYPEDEF_MEASUREMENT.datatype` combined with `STRUCTURE_COMPONENT.matrix_dim` |
| `unit` | resolved via existing `get_unit_for_conversion(TYPEDEF_MEASUREMENT.conversion)` |
| `min` / `max` | `TYPEDEF_MEASUREMENT.lower_limit` / `upper_limit` |
| `conversion_name` | `TYPEDEF_MEASUREMENT.conversion` |

### Synthetic `A2lCharacteristic` field mapping

Parallel to above, sourced from `TYPEDEF_CHARACTERISTIC`. Always:
- `characteristic_type = CharacteristicType::Value`
- `axis_info = vec![]`
- `storage_order` = default

CURVE/MAP-shaped components are warned and skipped.

### Address math examples

**Flat struct** — `INSTANCE lookup TYPE Lookup AT 0xFFF4 EXT 2`, `Lookup` has `input@0`, `output_linear@4`, `output_spline@8`, all `FLOAT32_IEEE`:

```
lookup.input         → 0xFFF4 + 0 = 0xFFF4, ext 2, F32
lookup.output_linear → 0xFFF4 + 4 = 0xFFF8, ext 2, F32
lookup.output_spline → 0xFFF4 + 8 = 0xFFFC, ext 2, F32
```

**Nested** — recursion with accumulated offset:

```
INSTANCE outer TYPE Outer AT 0x1000 EXT 0
  Outer { foo: Inner @ 16, bar: F32 @ 32 }
  Inner { x: U16 @ 0, y: U16 @ 2 }

→ outer.foo.x  @ 0x1010 ext 0  (U16)
→ outer.foo.y  @ 0x1012 ext 0  (U16)
→ outer.bar    @ 0x1020 ext 0  (F32)
```

**Array components** — `STRUCTURE_COMPONENT.matrix_dim`:
- `matrix_dim=[N]`     → existing `A2lDataType::*Array(N)` variant
- `matrix_dim=[R, C]`  → existing `A2lDataType::*Matrix(R, C)` variant
- `matrix_dim=[1]`     → treated as scalar
- 3+ dimensions        → warn, skip

## Validation rules at flatten time

| Condition | Action |
|---|---|
| `INSTANCE.type_ref` not in TYPEDEF_STRUCTURE table | Warn, skip the instance |
| `STRUCTURE_COMPONENT.component_type` unknown | Warn, skip that component, continue with siblings |
| `address_offset + component_size > TYPEDEF_STRUCTURE.total_size` | Warn (bounds), skip component |
| `TYPEDEF_STRUCTURE.total_size` ≠ sum of component sizes | Warn (informational), flatten anyway |
| Duplicate flattened name across instances | Warn, keep first occurrence |
| Recursion depth > 8 | Warn (cycle guard), stop recursion at that branch |
| `INSTANCE.matrix_dim` present (array of structs) | Warn, skip in this iteration |
| `OVERWRITE` blocks present in `INSTANCE` | Warn once per instance, ignore |
| `TYPEDEF_CHARACTERISTIC` component shaped as CURVE/MAP | Warn, skip |
| Flattened name collides with existing flat MEASUREMENT/CHARACTERISTIC | Flat one wins, warn |

All warnings use `tracing::warn!` with structured fields (`instance`, `component`, `reason`). After flattening completes, log at info level: `"flattened N components from M instances; K warnings"`.

## Test plan

### 1. Unit tests in `typedef_flatten.rs`

Each test constructs a small `a2lfile::Module` (or loads a small inline A2L string) and asserts the flattened output:

- `flatten_simple_struct` — the `Lookup` example, asserts 3 entries with expected addresses and names.
- `flatten_nested_struct` — 2-level nesting, asserts dot-joined names and accumulated offsets.
- `flatten_array_component` — `matrix_dim=[4]` → `F32Array(4)`.
- `flatten_2d_matrix_component` — `matrix_dim=[3, 2]` → `F32Matrix(3, 2)`.
- `flatten_unknown_type_skipped` — bad `component_type`, asserts skip + warning emitted.
- `flatten_orphan_instance_skipped` — INSTANCE with no matching TYPEDEF.
- `flatten_cycle_guard` — A→B→A, asserts depth-8 stop, no infinite loop.
- `flatten_collision_first_wins` — two instances producing same flattened name.
- `flatten_structural_characteristic` — calibration variant, asserts `CharacteristicType::Value`.

### 2. Integration test (new fixture)

`tests/fixtures/structural_measurement.a2l` — mirrors the example in the user's request (`Lookup` typedef, `lookup` instance, plus one plain `counter` measurement for comparison). The test loads via `A2lReader::open()` and asserts `get_measurements()` returns 4 entries (`counter`, `lookup.input`, `lookup.output_linear`, `lookup.output_spline`) with the expected addresses, extensions, and types.

### 3. Existing tests

All existing unit and integration tests must pass unchanged. That's the contract of Approach A — downstream code paths are not touched.

### 4. Mock-slave end-to-end (deferred)

Not part of this iteration. Downstream DAQ paths already work on flat measurements; once flattening produces the right entries, DAQ allocation, DTO decode, and calibration write all proceed unchanged. An end-to-end DAQ test against the mock slave can be added in a follow-up if regressions are suspected.

## Risks

| Risk | Mitigation |
|---|---|
| `a2lfile` 2.3.0 field shape differs from 2.2.0 (registry cache) | Build with the pinned version; verify field names match by compiling a quick probe before writing the flattening logic. |
| Real-world A2Ls use OVERWRITE / matrix_dim INSTANCE in ways that break the warn-and-skip path | Warnings are visible in logs; if observed in user data, lift them out of non-goals. |
| Name collisions silently hide measurements | "Keep first, warn" policy logs the collision; users can rename instances in the A2L. |

## UX confirmation (recap of brainstorming answers)

- **Naming:** `instance.component` dot-separated.
- **UI flattening:** flatten to leaf signals — no UI changes.
- **Calibration UX:** flatten to scalar params — each component is its own calibration entry, edited independently, written via the existing calibration write path.
- **Nested structs:** supported via recursion (depth limit 8).
- **Arrays inside structs:** supported (1D and 2D `matrix_dim`).
- **Address-extension:** inherited from `INSTANCE` (no `OVERWRITE` support yet).
- **Test fixture:** new separate file under `tests/fixtures/`.
