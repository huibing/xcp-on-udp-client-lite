# Editable MAP/CURVE Axis Values in Tauri Experiment Page

**Date:** 2026-06-15
**Status:** Approved (pending spec review)

## Problem

In the Tauri desktop app's Experiment page, the matrix modal for MAP and CURVE
calibration variables currently lets the user modify the data cells (the
characteristic values) but renders the axis breakpoint values (column / row
headers) as read-only text. Users have no way to adjust axis points from the UI
without reloading a HEX file.

## Goal

Allow the user to directly modify axis breakpoint values for MAP/CURVE
variables in the Tauri Experiment page. Keep the change as small as possible.

## Constraints

- **XCP semantics**: only `AXIS_PTS_REF` (`AxisPointSource::Memory`) axes are
  physically backed by ECU memory and writable at runtime. The other three
  variants (`Calculated`, `CalculatedDist`, `Explicit` — corresponding to
  `FIX_AXIS_PAR`, `FIX_AXIS_PAR_DIST`, `FIX_AXIS_PAR_LIST`) are static A2L
  definitions and have no memory backing to write to.
- **Working page only**: writes (axis or cell) are blocked unless the device is
  on the working page, identical to existing `cal_write_element` behavior.
- **Working-hex mirroring**: every successful write must patch the working-page
  HEX image and persist it to disk, matching the existing pattern used by
  `cal_write_element` so checksum verification stays consistent.

## Non-Goals

- No new read path: axis values are already returned by `cal_detail`. We reuse it.
- No undo for axis edits in this pass. Axis edits append to the existing
  `calHistory` for visibility, but undo plumbing is not added.
- No changes to data-cell rendering or to `cal_read`/`cal_value`.
- No A2L re-parsing or compute-method changes for axis values. Axes in practice
  use the `Identical` compu method; we reuse `param.compu_method.to_raw` for
  uniformity with cell writes.

## Design

### Backend — `src-tauri/src/commands/calibration.rs`

**1. New Tauri command `cal_write_axis_point`.**

```text
fn cal_write_axis_point(
    state, device_id: String, idx: usize,
    axis: usize,    // 0 = X, 1 = Y
    point: usize,   // index along the axis
    value: String,  // user-entered display value
) -> Result<Value, String>
```

Behavior:
- Resolve device, slave, and `param` exactly like `cal_write_element` does.
- Read `param.axis_info[axis]`. If absent → error.
- If `point_source` is **not** `AxisPointSource::Memory`, return
  `{ success: false, error: "Axis is not writable (static A2L axis)" }`.
- Compute element address from the `Memory { address, address_ext, data_type, .. }`
  fields: `addr = address + point * scalar_data_type_size(data_type)`.
- Build a single-element fake `CalibrationParam` describing this scalar axis
  point (helper `axis_element_write_param`, parallel to existing
  `element_write_param`).
- Convert string → raw via `param.compu_method.to_raw(&value)`, then via
  `scalar_from_raw(raw, data_type)` to a `CalibrationValue`.
- Call `write_calibration_to_slave` + serialize via `calibration_value_to_bytes`.
- Mirror to working-hex (same block as `cal_write_element`, lines ~489–516):
  patch at the computed axis element address, persist if `working_hex_path`
  is non-empty.
- Return `{ success: true }`.

**2. Extend `cal_detail` axis JSON.**

Add one field per axis entry: `"writable": <bool>` (true iff
`AxisPointSource::Memory`). This is a two-line change inside the existing
`axis_json` builder.

**3. Register the new command** in `src-tauri/src/lib.rs` `generate_handler![]`.

### Frontend — `ui/src/components/Experiment.vue`

**1. State.** Add two refs alongside the existing matrix-modal state:
- `matrixXAxisWritable` (bool)
- `matrixYAxisWritable` (bool)

In `showMatrix`, after parsing `axes`, set both from `axes[i].writable` (default
`false`).

**2. Template.** In the matrix-modal table:
- Column headers (currently `{{ matrixXAxisValues[ci - 1] ?? (ci - 1) }}`)
  become an `<input>` when `matrixXAxisWritable` is true; otherwise unchanged.
- Row headers, only when both `matrixTransposed` and `matrixYAxisWritable` are
  true, become an `<input>`; otherwise unchanged.
- Inputs are `:disabled="pageMode !== 'working'"` and call
  `writeAxisPoint(axis, point, value)` on `@change`.

**3. New `writeAxisPoint(axis, point, value)` function.** Mirrors `writeCell`:
- `invoke('cal_write_axis_point', { deviceId, idx, axis, point, value })`.
- On success: update `matrixXAxisValues[point]` or `matrixYAxisValues[point]`
  in place, set `matrixMsg`, mark `changedParams[cpKey]`, and append a
  `kind: 'axis'` entry to `calHistory` with old/new values, axis name, and
  point index.
- On failure: set `matrixMsg`/`matrixErr`.

**4. CSS.** Add a small `.axis-cell` rule so editable axis inputs render with
slightly different (header-like) styling — bold + tinted background — to keep
the visual distinction between axes and data cells.

## Files Changed

| File | Change |
|---|---|
| `src-tauri/src/commands/calibration.rs` | New `cal_write_axis_point` command (~60 lines) + helper `axis_element_write_param` (~15 lines) + `writable` field in `cal_detail` (~2 lines). |
| `src-tauri/src/lib.rs` | One line in `generate_handler![]`. |
| `ui/src/components/Experiment.vue` | New refs (~2 lines), template swap on two header cells (~8 lines), `writeAxisPoint` function (~25 lines), one CSS rule (~5 lines). |

Total estimated diff: ~120 lines added, no deletions.

## Test Plan

Manual (no automated tests added — none of the affected files have integration
tests for the matrix modal today; the protocol/transport layers are unchanged):

1. Connect to mock XCP slave on the working page. Open MAP variable (`map`,
   F32[3][9]) — axes are typically `AXIS_PTS_REF` in `hello_xcp.a2l`.
   Verify column and row headers render as editable inputs.
2. Edit an X-axis breakpoint, press Enter / blur. Verify:
   - History panel shows the change with old → new value.
   - Re-opening the modal reads back the new value (round-trip via `cal_detail`).
   - Working-hex file on disk reflects the patched bytes.
3. Switch to reference page. Verify axis inputs become disabled.
4. Open a CURVE with a `FIX_AXIS_PAR` axis (if available) — verify axis
   headers stay as plain text (not editable).
5. Edit a Y-axis breakpoint on a 2D MAP. Verify the row header updates and
   that data cells remain unaffected.

## Risks

- **Min/max validation**: axis breakpoints are typically expected to be
  monotonic. We are not enforcing monotonicity on write — the user can
  produce a non-monotonic axis. This matches the current data-cell write
  behavior (no monotonicity check on cells either) and keeps the change
  minimal. Document as a known limitation.
- **Compu method on axes**: most ECUs use `Identical` for axis breakpoints.
  Reusing `param.compu_method` is technically the characteristic's compu
  method, not the axis's. In practice these are the same for the common
  case; if they ever differ, the axis edit will use the characteristic's
  conversion. Acceptable for v1; can be tightened later by reading
  `AXIS_PTS`'s own compu method.
