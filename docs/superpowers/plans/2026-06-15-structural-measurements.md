# Structural Measurements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add A2L `TYPEDEF_STRUCTURE` + `TYPEDEF_MEASUREMENT` + `TYPEDEF_CHARACTERISTIC` + `INSTANCE` parsing so each leaf component appears as an ordinary measurement or calibration parameter, named `instance.component`.

**Architecture:** Approach A from the spec — flatten in the A2L reader. A new private module `src/a2l/typedef_flatten.rs` resolves instances recursively into synthetic `A2lMeasurement` / `A2lCharacteristic` entries, which `A2lReader::get_measurements()` and `get_characteristics()` append after the parsed flat entries. Downstream code (`DaqManager`, `CalibrationManager`, UI surfaces) is unchanged.

**Tech Stack:** Rust, `a2lfile = "=2.3.0"` crate, `tracing` for warnings.

**Spec:** `docs/superpowers/specs/2026-06-15-structural-measurements-design.md`

---

## File Structure

| Path | Status | Responsibility |
|---|---|---|
| `src/a2l/typedef_flatten.rs` | **Create** | Recursive flattening of INSTANCE → synthetic A2lMeasurement / A2lCharacteristic entries. Cycle guard, validation warnings, dot-joined name building. Pure functions over `&a2lfile::Module`. Unit tests live here. |
| `src/a2l.rs` | Modify | Add `mod typedef_flatten;` (private — no re-export). |
| `src/a2l/reader.rs` | Modify | `get_measurements()` and `get_characteristics()`: append synthetic entries from `typedef_flatten` after the existing flat-parsed entries. ~10 lines added per method. |
| `tests/fixtures/structural_measurement.a2l` | **Create** | Minimal A2L mirroring the user's example (Lookup typedef, lookup instance, plus one plain `counter` measurement). |
| `tests/structural_measurement_a2l.rs` | **Create** | Integration test that loads the fixture via `A2lReader::load()` and asserts the flattened measurement list. |

**No changes** to `src/daq.rs`, `src/calibration.rs`, `src/web/`, `src-tauri/`, or `src/variable.rs`. That is the contract of Approach A — verify by running their existing tests at the end.

---

## Coding conventions (read once before starting)

- Project formatter: `cargo fmt --workspace`. Run after every code change before committing.
- Lints: `cargo clippy --workspace --all-targets --all-features -- -D warnings`. Treat warnings as errors.
- Logging: `tracing::warn!` and `tracing::info!` with structured fields, e.g. `tracing::warn!(instance = %name, component = %comp, "skipped: unknown type");`. Do not `println!`.
- Tests: inline `#[cfg(test)] mod tests { ... }` at the bottom of the source file (matches existing convention — see `src/a2l/compute_method.rs:376` and `src/a2l/reader.rs:2134`).
- Visibility: keep all flattening helpers `pub(super)` or private. Only re-export through `A2lReader::get_measurements` / `get_characteristics`.
- The repo uses CRLF line endings on Windows; `git config` is already correct — do not change it.

---

## Task 0: Probe the a2lfile 2.3.0 typedef API

**Why first:** the spec is written assuming a 2.2.0-shaped API (`Module.typedef_structure: Vec<TypedefStructure>`, `Module.instance: Vec<Instance>`, etc.). 2.3.0 *should* be the same but we have not compiled against it yet. A 5-minute probe avoids wasted work.

**Files:**
- Modify (temporarily): `src/a2l.rs` — add a tiny inline probe that reads field types

- [ ] **Step 1: Add a probe expression**

In `src/a2l.rs`, temporarily add at the bottom (delete in step 4):

```rust
#[cfg(test)]
#[test]
fn probe_a2lfile_api() {
    // Compile-time probe: confirm field names. Body is irrelevant.
    fn _probe(m: &a2lfile::Module) {
        let _: &Vec<a2lfile::TypedefStructure> = &m.typedef_structure;
        let _: &Vec<a2lfile::TypedefMeasurement> = &m.typedef_measurement;
        let _: &Vec<a2lfile::TypedefCharacteristic> = &m.typedef_characteristic;
        let _: &Vec<a2lfile::Instance> = &m.instance;

        for s in &m.typedef_structure {
            let _: &str = s.get_name();
            let _: &u32 = &s.total_size;
            let _: &Vec<a2lfile::StructureComponent> = &s.structure_component;
        }
        for c in m.typedef_structure.first().iter().flat_map(|s| &s.structure_component) {
            let _: &str = &c.component_name;
            let _: &str = &c.component_type;
            let _: &u32 = &c.address_offset;
            // matrix_dim shape must be checked
            let _ = &c.matrix_dim;
        }
        for inst in &m.instance {
            let _: &str = inst.get_name();
            let _: &str = &inst.type_ref;
            let _: &u32 = &inst.start_address;
            let _ = &inst.ecu_address_extension;
            let _ = &inst.matrix_dim;
        }
        for tm in &m.typedef_measurement {
            let _: &str = tm.get_name();
            let _: &a2lfile::DataType = &tm.datatype;
            let _: &str = &tm.conversion;
            let _: &f64 = &tm.lower_limit;
            let _: &f64 = &tm.upper_limit;
        }
    }
    let _ = _probe;
}
```

- [ ] **Step 2: Compile and run**

Run: `cargo test --lib probe_a2lfile_api -- --exact`
Expected: PASS, with no warnings beyond unused-function lints.

If a field has a different name or type, **stop and report**. The spec's address-math and field-mapping tables depend on the names used here. Do not silently adapt — the user needs to know.

- [ ] **Step 3: Note any deviations**

If the probe required any field-name fixups, write them down here in this task before deleting the probe. Subsequent tasks must use the corrected names.

- [ ] **Step 4: Remove the probe**

Delete the probe test from `src/a2l.rs`. Do NOT commit it. The probe is a throwaway.

- [ ] **Step 5: No commit**

Skip — there are no source changes after deletion.

---

## Task 1: Skeleton module + dot-joined name helper (TDD)

**Files:**
- Create: `src/a2l/typedef_flatten.rs`
- Modify: `src/a2l.rs` (add `mod typedef_flatten;`)

- [ ] **Step 1: Add module declaration**

Edit `src/a2l.rs`, line 8-10 area. Add a private module (no `pub`, no re-export):

```rust
pub mod compute_method;
pub mod reader;
pub mod types;
mod typedef_flatten;  // ADD THIS LINE
```

- [ ] **Step 2: Create the module file with the failing test**

Create `src/a2l/typedef_flatten.rs`:

```rust
//! Flattens A2L INSTANCE + TYPEDEF_STRUCTURE + TYPEDEF_MEASUREMENT/CHARACTERISTIC
//! into synthetic A2lMeasurement / A2lCharacteristic entries.
//!
//! See `docs/superpowers/specs/2026-06-15-structural-measurements-design.md`.

/// Maximum nesting depth before we declare a cycle and stop recursing.
const MAX_DEPTH: usize = 8;

/// Builds a dotted path from an instance name and a chain of component names.
pub(super) fn build_path(instance: &str, components: &[&str]) -> String {
    let mut out = String::with_capacity(instance.len() + components.iter().map(|c| c.len() + 1).sum::<usize>());
    out.push_str(instance);
    for c in components {
        out.push('.');
        out.push_str(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_path_flat() {
        assert_eq!(build_path("lookup", &["input"]), "lookup.input");
    }

    #[test]
    fn build_path_nested() {
        assert_eq!(build_path("outer", &["foo", "x"]), "outer.foo.x");
    }

    #[test]
    fn build_path_no_components() {
        assert_eq!(build_path("solo", &[]), "solo");
    }
}
```

- [ ] **Step 3: Run the tests, verify they fail to compile, then pass**

Run: `cargo test --lib a2l::typedef_flatten`
Expected: PASS (3 tests). If the module declaration is missing in `a2l.rs`, you'll get a "file not found for module" error first — fix it.

- [ ] **Step 4: Format and lint**

Run: `cargo fmt --workspace && cargo clippy --workspace --all-targets --all-features -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add src/a2l.rs src/a2l/typedef_flatten.rs
git commit -m "feat(a2l): scaffold typedef_flatten module with name-builder"
```

---

## Task 2: Component datatype derivation (TDD)

Maps a `TYPEDEF_MEASUREMENT.datatype` plus a `STRUCTURE_COMPONENT.matrix_dim` into an `A2lDataType`. Pure function; no a2lfile types in tests.

**Files:**
- Modify: `src/a2l/typedef_flatten.rs`

- [ ] **Step 1: Write the failing tests**

Append to `src/a2l/typedef_flatten.rs` inside the `mod tests` block:

```rust
use crate::a2l::types::A2lDataType;

#[test]
fn datatype_scalar() {
    assert!(matches!(component_datatype(BaseDt::F32, &[]), Some(A2lDataType::F32)));
    assert!(matches!(component_datatype(BaseDt::U16, &[1]), Some(A2lDataType::U16)));
}

#[test]
fn datatype_array_1d() {
    assert!(matches!(component_datatype(BaseDt::F32, &[4]), Some(A2lDataType::F32Array(4))));
    assert!(matches!(component_datatype(BaseDt::U8, &[10]), Some(A2lDataType::U8Array(10))));
}

#[test]
fn datatype_matrix_2d() {
    assert!(matches!(component_datatype(BaseDt::F32, &[3, 2]), Some(A2lDataType::F32Matrix(3, 2))));
}

#[test]
fn datatype_3d_unsupported() {
    assert!(component_datatype(BaseDt::F32, &[2, 2, 2]).is_none());
}

#[test]
fn datatype_zero_dim_treated_as_scalar() {
    assert!(matches!(component_datatype(BaseDt::F32, &[0]), Some(A2lDataType::F32)));
}
```

- [ ] **Step 2: Run, expect failure**

Run: `cargo test --lib a2l::typedef_flatten`
Expected: FAIL — `component_datatype` and `BaseDt` not found.

- [ ] **Step 3: Implement**

Add above the `#[cfg(test)]` block in `src/a2l/typedef_flatten.rs`:

```rust
use crate::a2l::types::A2lDataType;

/// Base scalar data type of a TYPEDEF_MEASUREMENT/TYPEDEF_CHARACTERISTIC.
/// Mirrors what `A2lReader::parse_datatype` already produces — kept as a tiny
/// enum so this module's tests don't need a2lfile types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BaseDt {
    U8, U16, U32, U64,
    I8, I16, I32, I64,
    F32, F64,
    Bool,
}

impl BaseDt {
    pub(super) fn from_a2lfile(dt: &a2lfile::DataType) -> Option<Self> {
        Some(match dt {
            a2lfile::DataType::Ubyte => BaseDt::U8,
            a2lfile::DataType::Uword => BaseDt::U16,
            a2lfile::DataType::Ulong => BaseDt::U32,
            a2lfile::DataType::AUint64 => BaseDt::U64,
            a2lfile::DataType::Sbyte => BaseDt::I8,
            a2lfile::DataType::Sword => BaseDt::I16,
            a2lfile::DataType::Slong => BaseDt::I32,
            a2lfile::DataType::AInt64 => BaseDt::I64,
            a2lfile::DataType::Float32Ieee => BaseDt::F32,
            a2lfile::DataType::Float64Ieee => BaseDt::F64,
            _ => return None,
        })
    }
}

/// Combines a base scalar type with `matrix_dim` to produce an `A2lDataType`.
///
/// - `[]`, `[0]`, `[1]` → scalar
/// - `[N]` (N > 1) → 1D array
/// - `[R, C]` → 2D matrix (row-major, matches existing convention)
/// - 3+ dims → `None` (caller should warn and skip)
pub(super) fn component_datatype(base: BaseDt, matrix_dim: &[u16]) -> Option<A2lDataType> {
    let n = matrix_dim.iter().filter(|&&d| d > 1).count();
    match n {
        0 => Some(scalar(base)),
        1 => {
            let len = matrix_dim.iter().copied().find(|&d| d > 1).unwrap() as usize;
            Some(array(base, len))
        }
        2 if matrix_dim.len() == 2 => {
            let r = matrix_dim[0] as usize;
            let c = matrix_dim[1] as usize;
            Some(matrix(base, r, c))
        }
        _ => None,
    }
}

fn scalar(b: BaseDt) -> A2lDataType {
    match b {
        BaseDt::U8 => A2lDataType::U8,   BaseDt::U16 => A2lDataType::U16,
        BaseDt::U32 => A2lDataType::U32, BaseDt::U64 => A2lDataType::U64,
        BaseDt::I8 => A2lDataType::I8,   BaseDt::I16 => A2lDataType::I16,
        BaseDt::I32 => A2lDataType::I32, BaseDt::I64 => A2lDataType::I64,
        BaseDt::F32 => A2lDataType::F32, BaseDt::F64 => A2lDataType::F64,
        BaseDt::Bool => A2lDataType::Bool,
    }
}

fn array(b: BaseDt, n: usize) -> A2lDataType {
    match b {
        BaseDt::U8 => A2lDataType::U8Array(n),   BaseDt::U16 => A2lDataType::U16Array(n),
        BaseDt::U32 => A2lDataType::U32Array(n), BaseDt::U64 => A2lDataType::U64Array(n),
        BaseDt::I8 => A2lDataType::I8Array(n),   BaseDt::I16 => A2lDataType::I16Array(n),
        BaseDt::I32 => A2lDataType::I32Array(n), BaseDt::I64 => A2lDataType::I64Array(n),
        BaseDt::F32 => A2lDataType::F32Array(n), BaseDt::F64 => A2lDataType::F64Array(n),
        BaseDt::Bool => A2lDataType::BoolArray(n),
    }
}

fn matrix(b: BaseDt, r: usize, c: usize) -> A2lDataType {
    match b {
        BaseDt::U8 => A2lDataType::U8Matrix(r, c),   BaseDt::U16 => A2lDataType::U16Matrix(r, c),
        BaseDt::U32 => A2lDataType::U32Matrix(r, c), BaseDt::U64 => A2lDataType::U64Matrix(r, c),
        BaseDt::I8 => A2lDataType::I8Matrix(r, c),   BaseDt::I16 => A2lDataType::I16Matrix(r, c),
        BaseDt::I32 => A2lDataType::I32Matrix(r, c), BaseDt::I64 => A2lDataType::I64Matrix(r, c),
        BaseDt::F32 => A2lDataType::F32Matrix(r, c), BaseDt::F64 => A2lDataType::F64Matrix(r, c),
        BaseDt::Bool => A2lDataType::BoolMatrix(r, c),
    }
}
```

**Note on `matrix_dim` element type:** the probe in Task 0 will tell you whether `a2lfile` uses `Vec<u16>` (likely) or `Vec<u32>`. If the probe revealed a different element type, change the `&[u16]` parameter and the `as usize` casts accordingly. The cast is always safe (matrix_dim values are bounded).

- [ ] **Step 4: Run tests**

Run: `cargo test --lib a2l::typedef_flatten`
Expected: PASS — 8 tests (3 from Task 1 + 5 new).

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
git add src/a2l/typedef_flatten.rs
git commit -m "feat(a2l): derive A2lDataType from TYPEDEF base + matrix_dim"
```

---

## Task 3: Build typedef lookup tables (TDD)

A small helper that, given an `&a2lfile::Module`, returns three `HashMap`s for O(1) name resolution. Keeps the recursive flattener simple.

**Files:**
- Modify: `src/a2l/typedef_flatten.rs`

- [ ] **Step 1: Add a helper that takes `&a2lfile::Module`**

Append to `src/a2l/typedef_flatten.rs`:

```rust
use std::collections::HashMap;

/// Lookup tables built once per flatten call.
pub(super) struct TypedefTables<'a> {
    pub structures: HashMap<&'a str, &'a a2lfile::TypedefStructure>,
    pub measurements: HashMap<&'a str, &'a a2lfile::TypedefMeasurement>,
    pub characteristics: HashMap<&'a str, &'a a2lfile::TypedefCharacteristic>,
}

impl<'a> TypedefTables<'a> {
    pub(super) fn build(module: &'a a2lfile::Module) -> Self {
        let mut structures = HashMap::with_capacity(module.typedef_structure.len());
        for s in &module.typedef_structure {
            structures.insert(s.get_name(), s);
        }
        let mut measurements = HashMap::with_capacity(module.typedef_measurement.len());
        for m in &module.typedef_measurement {
            measurements.insert(m.get_name(), m);
        }
        let mut characteristics = HashMap::with_capacity(module.typedef_characteristic.len());
        for c in &module.typedef_characteristic {
            characteristics.insert(c.get_name(), c);
        }
        Self { structures, measurements, characteristics }
    }
}
```

- [ ] **Step 2: No unit test for this helper directly**

The lookup table is a thin wrapper around `HashMap::insert`. Coverage comes from Task 4 and beyond, where tests construct a module and verify flatten output. **Do not** add a contrived test for this struct — that's testing `HashMap`.

- [ ] **Step 3: Compile**

Run: `cargo build --lib`
Expected: clean compile.

- [ ] **Step 4: Format, lint, commit**

```bash
cargo fmt --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
git add src/a2l/typedef_flatten.rs
git commit -m "feat(a2l): add typedef name-lookup tables"
```

---

## Task 4: Flatten one INSTANCE → measurements (TDD, end-to-end through real a2lfile parsing)

This is the meat. We test by feeding a small A2L source string through `a2lfile::load_from_string` (or equivalent) and asserting the flattened output. **Do not** hand-construct `a2lfile` structs in tests — they have many fields and the constructors change between versions.

**First**, confirm the loader API:

- [ ] **Step 0: Confirm a2lfile string-loading function**

Run: `cargo doc -p a2lfile --open` is overkill; instead grep:

```bash
cargo metadata --format-version 1 | grep -o '"a2lfile-[0-9.]*"' | head -1
```

Then grep the cached source for a function that loads from a `&str`:

```bash
find ~/.cargo/registry/src -path "*a2lfile-*/src/lib.rs" | head -1 | xargs grep -n "pub fn load"
```

Expected names: `load_from_string`, `load_fragment`, or similar. Use whichever the version exposes. If it only has `load(path)`, the test must write a temp file via `tempfile::NamedTempFile` (already in dev-deps if used elsewhere; otherwise the integration test in Task 7 covers this and we can leave Task 4 as compile-checked stubs and rely on the integration test).

If the only available loader is path-based:
- Skip writing a `mod tests` for `flatten_instance_measurements` here.
- Move the asserts into the integration test (Task 7).
- Still implement the function in this task; mark its unit tests as a stretch goal.

If a string loader exists, proceed with the tests below.

- [ ] **Step 1: Write the failing test (string-loader available case)**

Append to `src/a2l/typedef_flatten.rs` test module:

```rust
const SIMPLE_A2L: &str = r#"
ASAP2_VERSION 1 71
/begin PROJECT P "" /begin MODULE M ""
  /begin TYPEDEF_MEASUREMENT input "" FLOAT32_IEEE NO_COMPU_METHOD 0 0 -1e30 1e30 /end TYPEDEF_MEASUREMENT
  /begin TYPEDEF_MEASUREMENT output_linear "" FLOAT32_IEEE NO_COMPU_METHOD 0 0 -1e30 1e30 /end TYPEDEF_MEASUREMENT
  /begin TYPEDEF_MEASUREMENT output_spline "" FLOAT32_IEEE NO_COMPU_METHOD 0 0 -1e30 1e30 /end TYPEDEF_MEASUREMENT
  /begin TYPEDEF_STRUCTURE Lookup "" 12
    /begin STRUCTURE_COMPONENT input         input         0 /end STRUCTURE_COMPONENT
    /begin STRUCTURE_COMPONENT output_linear output_linear 4 /end STRUCTURE_COMPONENT
    /begin STRUCTURE_COMPONENT output_spline output_spline 8 /end STRUCTURE_COMPONENT
  /end TYPEDEF_STRUCTURE
  /begin INSTANCE lookup "" Lookup 0xFFF4 ECU_ADDRESS_EXTENSION 2 /end INSTANCE
/end MODULE /end PROJECT
"#;

fn parse_module(src: &str) -> a2lfile::A2lFile {
    let mut errors = Vec::new();
    // Adjust to the actual loader name discovered in Step 0.
    // `strict = false` is recommended for inline fixtures — strict parsing may
    // reject minimal A2L over missing optional blocks. If the call fails with
    // a parse error, try `false` first before debugging the fixture itself.
    a2lfile::load_from_string(src, None, &mut errors, false)
        .expect("a2l parse failed")
}

#[test]
fn flatten_simple_struct_measurements() {
    let f = parse_module(SIMPLE_A2L);
    let module = &f.project.module[0];
    let measurements = flatten_instance_measurements(module);
    assert_eq!(measurements.len(), 3);

    let m: Vec<_> = measurements.iter().map(|m| (m.name.as_str(), m.address, m.address_ext)).collect();
    assert!(m.contains(&("lookup.input", 0xFFF4, 2)));
    assert!(m.contains(&("lookup.output_linear", 0xFFF8, 2)));
    assert!(m.contains(&("lookup.output_spline", 0xFFFC, 2)));

    for meas in &measurements {
        assert!(matches!(meas.data_type, A2lDataType::F32));
        assert_eq!(meas.conversion_name, "NO_COMPU_METHOD");
    }
}
```

- [ ] **Step 2: Run, expect failure**

Run: `cargo test --lib a2l::typedef_flatten::tests::flatten_simple_struct_measurements`
Expected: FAIL — `flatten_instance_measurements` not defined.

- [ ] **Step 3: Implement the flattener**

Append to `src/a2l/typedef_flatten.rs` (above the `#[cfg(test)]` block):

```rust
use crate::a2l::reader::A2lMeasurement;

/// Flatten all INSTANCE blocks in the module whose type resolves to a
/// TYPEDEF_STRUCTURE composed of TYPEDEF_MEASUREMENT components.
///
/// Returns synthetic A2lMeasurement entries with dotted names like
/// `instance.component[.subcomponent...]`.
///
/// Components whose type is not a TYPEDEF_MEASUREMENT or TYPEDEF_STRUCTURE
/// are skipped with a warning. Cycles are guarded at depth `MAX_DEPTH`.
pub(super) fn flatten_instance_measurements(module: &a2lfile::Module) -> Vec<A2lMeasurement> {
    let tables = TypedefTables::build(module);
    let mut out = Vec::new();
    for inst in &module.instance {
        if inst.matrix_dim.as_ref().map_or(false, |md| !md.dim_list.is_empty()) {
            tracing::warn!(instance = %inst.get_name(), "array-of-structs not supported, skipped");
            continue;
        }
        if !inst.overwrite.is_empty() {
            tracing::warn!(instance = %inst.get_name(), "OVERWRITE blocks ignored");
        }
        let ext = inst.ecu_address_extension.as_ref().map(|e| e.extension as u8).unwrap_or(0);
        let inst_name = inst.get_name();
        flatten_struct(
            &tables,
            inst.type_ref.as_str(),
            inst_name,
            &mut Vec::new(),
            inst.start_address,
            ext,
            0,
            &mut out,
        );
    }
    out
}

fn flatten_struct(
    tables: &TypedefTables<'_>,
    type_name: &str,
    instance: &str,
    path: &mut Vec<String>,
    base_addr: u32,
    ext: u8,
    depth: usize,
    out: &mut Vec<A2lMeasurement>,
) {
    if depth >= MAX_DEPTH {
        tracing::warn!(instance, type_ref = type_name, depth, "max nesting depth reached");
        return;
    }
    let Some(structure) = tables.structures.get(type_name) else {
        if depth == 0 {
            tracing::warn!(instance, type_ref = type_name, "INSTANCE references unknown type");
        } else {
            tracing::warn!(instance, type_ref = type_name, "nested struct type unknown");
        }
        return;
    };
    for comp in &structure.structure_component {
        let comp_addr = base_addr.wrapping_add(comp.address_offset);
        let comp_type = comp.component_type.as_str();

        // Try as nested struct first.
        if tables.structures.contains_key(comp_type) {
            path.push(comp.component_name.clone());
            flatten_struct(tables, comp_type, instance, path, comp_addr, ext, depth + 1, out);
            path.pop();
            continue;
        }

        // Then as a TYPEDEF_MEASUREMENT leaf.
        let Some(typedef_meas) = tables.measurements.get(comp_type) else {
            tracing::warn!(instance, component = %comp.component_name, type_ref = comp_type,
                "structure component type is not a known TYPEDEF_STRUCTURE or TYPEDEF_MEASUREMENT");
            continue;
        };

        let Some(base) = BaseDt::from_a2lfile(&typedef_meas.datatype) else {
            tracing::warn!(instance, component = %comp.component_name,
                "unsupported A2L datatype on TYPEDEF_MEASUREMENT");
            continue;
        };

        let matrix = matrix_dim_slice(&comp.matrix_dim);
        let Some(data_type) = component_datatype(base, &matrix) else {
            tracing::warn!(instance, component = %comp.component_name,
                "matrix_dim with 3+ dimensions, skipped");
            continue;
        };

        path.push(comp.component_name.clone());
        let leaf_path: Vec<&str> = path.iter().map(|s| s.as_str()).collect();
        let name = build_path(instance, &leaf_path);
        path.pop();

        out.push(A2lMeasurement {
            name,
            description: typedef_meas.long_identifier.clone(),
            address: comp_addr,
            address_ext: ext,
            data_type,
            unit: None, // Filled in by reader.rs which has access to compu_method tables (Task 6).
            min: typedef_meas.lower_limit,
            max: typedef_meas.upper_limit,
            conversion_name: typedef_meas.conversion.clone(),
        });
    }
}

/// Convert a2lfile MatrixDim to a slice. Returns empty if absent.
/// (Adjust to the actual MatrixDim shape revealed by the Task 0 probe.)
fn matrix_dim_slice(md: &Option<a2lfile::MatrixDim>) -> Vec<u16> {
    match md {
        Some(m) => m.dim_list.clone(),
        None => Vec::new(),
    }
}
```

**Important — adjust to actual a2lfile shape from Task 0 probe:**
- `inst.matrix_dim` may be `Vec<u16>` directly or `Option<MatrixDim>`. The conditional in `flatten_instance_measurements` and `matrix_dim_slice` must match.
- `inst.overwrite` may be `Vec<Overwrite>` or `Option<...>`. Adjust the `is_empty()` check.
- `MatrixDim.dim_list` may be a different field name. The probe should have made this clear.

If the shape doesn't match, fix this code first; do not adapt the test. The test asserts behavior — the implementation must match a2lfile's API.

- [ ] **Step 4: Run, expect pass**

Run: `cargo test --lib a2l::typedef_flatten`
Expected: PASS.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
git add src/a2l/typedef_flatten.rs
git commit -m "feat(a2l): flatten INSTANCE+TYPEDEF_STRUCTURE into A2lMeasurements"
```

---

## Task 5: Edge cases — nested, arrays, unknown, cycles, collisions (TDD)

Add tests one at a time. Each gets a small A2L fragment and asserts behavior.

**Files:**
- Modify: `src/a2l/typedef_flatten.rs`

- [ ] **Step 1: Nested struct test**

Add to test module:

```rust
const NESTED_A2L: &str = r#"
ASAP2_VERSION 1 71
/begin PROJECT P "" /begin MODULE M ""
  /begin TYPEDEF_MEASUREMENT u16val "" UWORD NO_COMPU_METHOD 0 0 0 65535 /end TYPEDEF_MEASUREMENT
  /begin TYPEDEF_MEASUREMENT f32val "" FLOAT32_IEEE NO_COMPU_METHOD 0 0 -1e30 1e30 /end TYPEDEF_MEASUREMENT
  /begin TYPEDEF_STRUCTURE Inner "" 4
    /begin STRUCTURE_COMPONENT x u16val 0 /end STRUCTURE_COMPONENT
    /begin STRUCTURE_COMPONENT y u16val 2 /end STRUCTURE_COMPONENT
  /end TYPEDEF_STRUCTURE
  /begin TYPEDEF_STRUCTURE Outer "" 36
    /begin STRUCTURE_COMPONENT foo Inner 16 /end STRUCTURE_COMPONENT
    /begin STRUCTURE_COMPONENT bar f32val 32 /end STRUCTURE_COMPONENT
  /end TYPEDEF_STRUCTURE
  /begin INSTANCE outer "" Outer 0x1000 /end INSTANCE
/end MODULE /end PROJECT
"#;

#[test]
fn flatten_nested_struct() {
    let f = parse_module(NESTED_A2L);
    let m = flatten_instance_measurements(&f.project.module[0]);
    let names: Vec<_> = m.iter().map(|x| (x.name.as_str(), x.address)).collect();
    assert_eq!(names.len(), 3);
    assert!(names.contains(&("outer.foo.x", 0x1010)));
    assert!(names.contains(&("outer.foo.y", 0x1012)));
    assert!(names.contains(&("outer.bar",   0x1020)));
    assert!(matches!(m.iter().find(|x| x.name == "outer.foo.x").unwrap().data_type, A2lDataType::U16));
    assert!(matches!(m.iter().find(|x| x.name == "outer.bar").unwrap().data_type, A2lDataType::F32));
}
```

Run: `cargo test --lib flatten_nested_struct`
Expected: PASS (the implementation in Task 4 already supports this).

- [ ] **Step 2: Array component test**

```rust
const ARRAY_A2L: &str = r#"
ASAP2_VERSION 1 71
/begin PROJECT P "" /begin MODULE M ""
  /begin TYPEDEF_MEASUREMENT f32val "" FLOAT32_IEEE NO_COMPU_METHOD 0 0 -1e30 1e30 /end TYPEDEF_MEASUREMENT
  /begin TYPEDEF_STRUCTURE Vec4 "" 16
    /begin STRUCTURE_COMPONENT v f32val 0 MATRIX_DIM 4 /end STRUCTURE_COMPONENT
  /end TYPEDEF_STRUCTURE
  /begin INSTANCE vec4_inst "" Vec4 0x2000 /end INSTANCE
/end MODULE /end PROJECT
"#;

#[test]
fn flatten_array_component() {
    let f = parse_module(ARRAY_A2L);
    let m = flatten_instance_measurements(&f.project.module[0]);
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].name, "vec4_inst.v");
    assert_eq!(m[0].address, 0x2000);
    assert!(matches!(m[0].data_type, A2lDataType::F32Array(4)));
}
```

Run: `cargo test --lib flatten_array_component`
Expected: PASS.

- [ ] **Step 3: Unknown type skipped**

```rust
const UNKNOWN_A2L: &str = r#"
ASAP2_VERSION 1 71
/begin PROJECT P "" /begin MODULE M ""
  /begin TYPEDEF_MEASUREMENT f32val "" FLOAT32_IEEE NO_COMPU_METHOD 0 0 -1e30 1e30 /end TYPEDEF_MEASUREMENT
  /begin TYPEDEF_STRUCTURE Mixed "" 8
    /begin STRUCTURE_COMPONENT good f32val 0 /end STRUCTURE_COMPONENT
    /begin STRUCTURE_COMPONENT bad  Nonexistent 4 /end STRUCTURE_COMPONENT
  /end TYPEDEF_STRUCTURE
  /begin INSTANCE mixed "" Mixed 0x3000 /end INSTANCE
/end MODULE /end PROJECT
"#;

#[test]
fn flatten_unknown_type_skipped() {
    let f = parse_module(UNKNOWN_A2L);
    let m = flatten_instance_measurements(&f.project.module[0]);
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].name, "mixed.good");
}
```

Run: PASS.

- [ ] **Step 4: Orphan instance**

```rust
const ORPHAN_A2L: &str = r#"
ASAP2_VERSION 1 71
/begin PROJECT P "" /begin MODULE M ""
  /begin INSTANCE orphan "" NoSuchType 0x4000 /end INSTANCE
/end MODULE /end PROJECT
"#;

#[test]
fn flatten_orphan_instance_skipped() {
    let f = parse_module(ORPHAN_A2L);
    let m = flatten_instance_measurements(&f.project.module[0]);
    assert!(m.is_empty());
}
```

Run: PASS.

- [ ] **Step 5: Cycle guard (does not hang)**

```rust
const CYCLE_A2L: &str = r#"
ASAP2_VERSION 1 71
/begin PROJECT P "" /begin MODULE M ""
  /begin TYPEDEF_STRUCTURE A "" 8
    /begin STRUCTURE_COMPONENT b B 0 /end STRUCTURE_COMPONENT
  /end TYPEDEF_STRUCTURE
  /begin TYPEDEF_STRUCTURE B "" 8
    /begin STRUCTURE_COMPONENT a A 0 /end STRUCTURE_COMPONENT
  /end TYPEDEF_STRUCTURE
  /begin INSTANCE root "" A 0x5000 /end INSTANCE
/end MODULE /end PROJECT
"#;

#[test]
fn flatten_cycle_guard_does_not_hang() {
    let f = parse_module(CYCLE_A2L);
    let m = flatten_instance_measurements(&f.project.module[0]);
    // No leaves anywhere, so output is empty. The point of the test is that it returns.
    assert!(m.is_empty());
}
```

Run: PASS. If this hangs, the depth guard is broken.

- [ ] **Step 6: Format, lint, commit**

```bash
cargo fmt --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
git add src/a2l/typedef_flatten.rs
git commit -m "test(a2l): cover nesting, arrays, unknowns, orphans, cycles"
```

---

## Task 6: Wire into A2lReader::get_measurements (TDD)

Now hook the flattener into the reader so `get_measurements()` returns flat + synthetic. Also resolve `unit` here (the reader has the COMPU_METHOD tables; the typedef_flatten module does not).

**Files:**
- Modify: `src/a2l/reader.rs:433-445`

- [ ] **Step 1: Write the failing test**

In `src/a2l/reader.rs`, append to the existing `mod tests` block at the bottom of the file (around line 2134). If a small fixture A2L parser helper exists, reuse it; otherwise add:

```rust
#[test]
fn reader_appends_structural_measurements() {
    // Mirrors the spec example. Save to a temp file and load via A2lReader::load.
    let src = r#"
ASAP2_VERSION 1 71
/begin PROJECT P "" /begin MODULE M ""
  /begin MEASUREMENT counter "" ULONG NO_COMPU_METHOD 0 0 0 4294967295
    ECU_ADDRESS 0xFFF0 ECU_ADDRESS_EXTENSION 2 READ_WRITE
  /end MEASUREMENT
  /begin TYPEDEF_MEASUREMENT input "" FLOAT32_IEEE NO_COMPU_METHOD 0 0 -1e30 1e30 /end TYPEDEF_MEASUREMENT
  /begin TYPEDEF_MEASUREMENT output_linear "" FLOAT32_IEEE NO_COMPU_METHOD 0 0 -1e30 1e30 /end TYPEDEF_MEASUREMENT
  /begin TYPEDEF_MEASUREMENT output_spline "" FLOAT32_IEEE NO_COMPU_METHOD 0 0 -1e30 1e30 /end TYPEDEF_MEASUREMENT
  /begin TYPEDEF_STRUCTURE Lookup "" 12
    /begin STRUCTURE_COMPONENT input         input         0 /end STRUCTURE_COMPONENT
    /begin STRUCTURE_COMPONENT output_linear output_linear 4 /end STRUCTURE_COMPONENT
    /begin STRUCTURE_COMPONENT output_spline output_spline 8 /end STRUCTURE_COMPONENT
  /end TYPEDEF_STRUCTURE
  /begin INSTANCE lookup "" Lookup 0xFFF4 ECU_ADDRESS_EXTENSION 2 /end INSTANCE
/end MODULE /end PROJECT
"#;
    let tmp = std::env::temp_dir().join("xcp_struct_meas_test.a2l");
    std::fs::write(&tmp, src).unwrap();
    let reader = super::A2lReader::load(&tmp).unwrap();
    let measurements = reader.get_measurements();
    let names: std::collections::HashSet<_> = measurements.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains("counter"), "flat measurement preserved");
    assert!(names.contains("lookup.input"));
    assert!(names.contains("lookup.output_linear"));
    assert!(names.contains("lookup.output_spline"));
    // Flat parsed first, synthetic appended.
    assert_eq!(measurements[0].name, "counter");
}
```

- [ ] **Step 2: Run, expect failure**

Run: `cargo test --lib reader_appends_structural_measurements`
Expected: FAIL — only `counter` is returned.

- [ ] **Step 3: Modify `A2lReader::get_measurements`**

In `src/a2l/reader.rs`, find `pub fn get_measurements(&self) -> Vec<A2lMeasurement>` at line 433. Modify to:

```rust
pub fn get_measurements(&self) -> Vec<A2lMeasurement> {
    let mut measurements = Vec::new();

    for module in &self.file.project.module {
        // Existing: parse flat MEASUREMENT blocks.
        for meas in &module.measurement {
            if let Some(info) = self.parse_measurement(meas) {
                measurements.push(info);
            }
        }
        // NEW: append synthetic entries from INSTANCE+TYPEDEF_STRUCTURE flattening.
        // Insertion order is load-bearing: flat-parsed entries come first so they
        // win on name collisions in any downstream lookup-by-name code.
        let mut flattened = super::typedef_flatten::flatten_instance_measurements(module);
        // Resolve unit via existing COMPU_METHOD lookup (typedef_flatten doesn't have the tables).
        for m in &mut flattened {
            m.unit = self.get_unit_for_conversion(&m.conversion_name);
        }
        // Drop any synthetic name that collides with a flat name; warn.
        let flat_names: std::collections::HashSet<&str> =
            measurements.iter().map(|m| m.name.as_str()).collect();
        flattened.retain(|m| {
            if flat_names.contains(m.name.as_str()) {
                tracing::warn!(name = %m.name, "structural measurement collides with flat MEASUREMENT, dropped");
                false
            } else {
                true
            }
        });
        measurements.extend(flattened);
    }

    measurements
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib reader_appends_structural_measurements`
Expected: PASS.

Run: `cargo test --lib` (all library tests)
Expected: PASS, no regressions.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
git add src/a2l/reader.rs
git commit -m "feat(a2l): expose flattened structural measurements via get_measurements"
```

---

## Task 7: Integration test with file fixture

End-to-end smoke that the on-disk loading path works (string-loading in unit tests is convenient but doesn't hit `A2lReader::load`).

**Files:**
- Create: `tests/fixtures/structural_measurement.a2l`
- Create: `tests/structural_measurement_a2l.rs`

- [ ] **Step 1: Create the fixture**

`tests/fixtures/structural_measurement.a2l`:

```a2l
ASAP2_VERSION 1 71
/begin PROJECT structural ""
  /begin MODULE M ""
    /begin MEASUREMENT counter "" ULONG NO_COMPU_METHOD 0 0 0 4294967295
      ECU_ADDRESS 0xFFF0 ECU_ADDRESS_EXTENSION 2 READ_WRITE
    /end MEASUREMENT
    /begin TYPEDEF_MEASUREMENT input "" FLOAT32_IEEE NO_COMPU_METHOD 0 0 -1e30 1e30 /end TYPEDEF_MEASUREMENT
    /begin TYPEDEF_MEASUREMENT output_linear "" FLOAT32_IEEE NO_COMPU_METHOD 0 0 -1e30 1e30 /end TYPEDEF_MEASUREMENT
    /begin TYPEDEF_MEASUREMENT output_spline "" FLOAT32_IEEE NO_COMPU_METHOD 0 0 -1e30 1e30 /end TYPEDEF_MEASUREMENT
    /begin TYPEDEF_STRUCTURE Lookup "" 12
      /begin STRUCTURE_COMPONENT input         input         0 /end STRUCTURE_COMPONENT
      /begin STRUCTURE_COMPONENT output_linear output_linear 4 /end STRUCTURE_COMPONENT
      /begin STRUCTURE_COMPONENT output_spline output_spline 8 /end STRUCTURE_COMPONENT
    /end TYPEDEF_STRUCTURE
    /begin INSTANCE lookup "" Lookup 0xFFF4 ECU_ADDRESS_EXTENSION 2 /end INSTANCE
  /end MODULE
/end PROJECT
```

- [ ] **Step 2: Create the integration test**

`tests/structural_measurement_a2l.rs`:

```rust
//! End-to-end test for structural measurement flattening via A2lReader::load.

use xcp_client::{A2lDataType, A2lReader};

#[test]
fn loads_and_flattens_structural_measurement_fixture() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("structural_measurement.a2l");
    let reader = A2lReader::load(&path).expect("load fixture");
    let measurements = reader.get_measurements();
    let by_name: std::collections::HashMap<_, _> =
        measurements.iter().map(|m| (m.name.clone(), m)).collect();

    // Plain MEASUREMENT preserved.
    let counter = by_name.get("counter").expect("counter measurement present");
    assert_eq!(counter.address, 0xFFF0);
    assert_eq!(counter.address_ext, 2);
    assert!(matches!(counter.data_type, A2lDataType::U32));

    // Three flattened components.
    let lin = by_name.get("lookup.input").expect("lookup.input present");
    assert_eq!(lin.address, 0xFFF4);
    assert_eq!(lin.address_ext, 2);
    assert!(matches!(lin.data_type, A2lDataType::F32));

    let lo = by_name.get("lookup.output_linear").expect("lookup.output_linear present");
    assert_eq!(lo.address, 0xFFF8);

    let sp = by_name.get("lookup.output_spline").expect("lookup.output_spline present");
    assert_eq!(sp.address, 0xFFFC);
}
```

**Note on package name:** if `xcp_client` is not the actual crate name, check `Cargo.toml`'s `[package].name` and adjust the `use` line. Run `cargo metadata --format-version 1 --no-deps | grep '"name"' | head -1` to confirm.

- [ ] **Step 3: Run the test**

Run: `cargo test --test structural_measurement_a2l`
Expected: PASS.

- [ ] **Step 4: Format, lint, commit**

```bash
cargo fmt --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
git add tests/fixtures/structural_measurement.a2l tests/structural_measurement_a2l.rs
git commit -m "test(a2l): integration fixture for structural measurements"
```

---

## Task 8: Wire into A2lReader::get_characteristics (TDD)

Same pattern for calibration parameters: flatten `INSTANCE` of `TYPEDEF_STRUCTURE` whose components are `TYPEDEF_CHARACTERISTIC`.

**Files:**
- Modify: `src/a2l/typedef_flatten.rs` — add `flatten_instance_characteristics`
- Modify: `src/a2l/reader.rs:448` — call it from `get_characteristics`

- [ ] **Step 1: Write the failing test in typedef_flatten.rs**

```rust
const CAL_A2L: &str = r#"
ASAP2_VERSION 1 71
/begin PROJECT P "" /begin MODULE M ""
  /begin TYPEDEF_CHARACTERISTIC gain_t "" VALUE __F32 0 NO_COMPU_METHOD -1e30 1e30 /end TYPEDEF_CHARACTERISTIC
  /begin TYPEDEF_CHARACTERISTIC offset_t "" VALUE __F32 0 NO_COMPU_METHOD -1e30 1e30 /end TYPEDEF_CHARACTERISTIC
  /begin TYPEDEF_STRUCTURE Tuning "" 8
    /begin STRUCTURE_COMPONENT gain   gain_t   0 /end STRUCTURE_COMPONENT
    /begin STRUCTURE_COMPONENT offset offset_t 4 /end STRUCTURE_COMPONENT
  /end TYPEDEF_STRUCTURE
  /begin INSTANCE tuning "" Tuning 0x6000 /end INSTANCE
/end MODULE /end PROJECT
"#;

#[test]
fn flatten_simple_struct_characteristics() {
    let f = parse_module(CAL_A2L);
    let cs = flatten_instance_characteristics(&f.project.module[0]);
    let names: Vec<_> = cs.iter().map(|c| (c.name.as_str(), c.address)).collect();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&("tuning.gain", 0x6000)));
    assert!(names.contains(&("tuning.offset", 0x6004)));
    use crate::a2l::reader::CharacteristicType;
    for c in &cs {
        assert!(matches!(c.characteristic_type, CharacteristicType::Value));
        assert!(c.axis_info.is_empty());
    }
}
```

- [ ] **Step 2: Run, expect failure**

Run: `cargo test --lib flatten_simple_struct_characteristics`
Expected: FAIL — `flatten_instance_characteristics` not defined.

**Note on `__F32`:** A2L uses *deposit names* (record-layout names) for typedef characteristics. `__F32` is a placeholder — the actual fixture may need an explicit `RECORD_LAYOUT __F32 /begin RECORD_LAYOUT FNC_VALUES 1 FLOAT32_IEEE ROW_DIR DIRECT /end RECORD_LAYOUT` block at module level. If the parser rejects the test A2L, add the missing record layout:

```a2l
/begin RECORD_LAYOUT __F32 FNC_VALUES 1 FLOAT32_IEEE ROW_DIR DIRECT /end RECORD_LAYOUT
```

- [ ] **Step 3: Implement `flatten_instance_characteristics`**

In `src/a2l/typedef_flatten.rs`, append:

```rust
use crate::a2l::reader::{A2lCharacteristic, CharacteristicType};
use crate::a2l::types::StorageOrder;

pub(super) fn flatten_instance_characteristics(module: &a2lfile::Module) -> Vec<A2lCharacteristic> {
    let tables = TypedefTables::build(module);
    let mut out = Vec::new();
    for inst in &module.instance {
        if inst.matrix_dim.as_ref().map_or(false, |md| !md.dim_list.is_empty()) {
            continue; // Already warned in measurement path.
        }
        let ext = inst.ecu_address_extension.as_ref().map(|e| e.extension as u8).unwrap_or(0);
        flatten_struct_chars(
            &tables, inst.type_ref.as_str(), inst.get_name(),
            &mut Vec::new(), inst.start_address, ext, 0, &mut out,
        );
    }
    out
}

fn flatten_struct_chars(
    tables: &TypedefTables<'_>,
    type_name: &str,
    instance: &str,
    path: &mut Vec<String>,
    base_addr: u32,
    ext: u8,
    depth: usize,
    out: &mut Vec<A2lCharacteristic>,
) {
    if depth >= MAX_DEPTH { return; }
    let Some(structure) = tables.structures.get(type_name) else {
        return; // Already warned in measurement pass.
    };
    for comp in &structure.structure_component {
        let comp_addr = base_addr.wrapping_add(comp.address_offset);
        let comp_type = comp.component_type.as_str();

        if tables.structures.contains_key(comp_type) {
            path.push(comp.component_name.clone());
            flatten_struct_chars(tables, comp_type, instance, path, comp_addr, ext, depth + 1, out);
            path.pop();
            continue;
        }

        let Some(typedef_char) = tables.characteristics.get(comp_type) else {
            // Not a calibration component — measurement pass handles it (or already warned).
            continue;
        };

        // Only VALUE-shaped components in this iteration.
        if !matches!(typedef_char.characteristic_type, a2lfile::CharacteristicType::Value) {
            tracing::warn!(instance, component = %comp.component_name,
                "non-VALUE TYPEDEF_CHARACTERISTIC components inside structs not supported, skipped");
            continue;
        }

        // Determine base data type from the deposit (record layout).
        let Some(base) = lookup_deposit_basetype(typedef_char, &tables) else {
            tracing::warn!(instance, component = %comp.component_name, "unsupported deposit datatype");
            continue;
        };

        let matrix = matrix_dim_slice(&comp.matrix_dim);
        let Some(data_type) = component_datatype(base, &matrix) else { continue; };

        path.push(comp.component_name.clone());
        let leaf_path: Vec<&str> = path.iter().map(|s| s.as_str()).collect();
        let name = build_path(instance, &leaf_path);
        path.pop();

        out.push(A2lCharacteristic {
            name,
            description: typedef_char.long_identifier.clone(),
            address: comp_addr,
            address_ext: ext,
            data_type,
            unit: None, // Filled by reader.rs.
            min: typedef_char.lower_limit,
            max: typedef_char.upper_limit,
            characteristic_type: CharacteristicType::Value,
            axis_info: Vec::new(),
            storage_order: StorageOrder::default(),
            conversion_name: typedef_char.conversion.clone(),
        });
    }
}

/// Resolve the base scalar type of a TYPEDEF_CHARACTERISTIC by looking up its
/// deposit (record layout) and reading the FNC_VALUES datatype.
fn lookup_deposit_basetype(
    _typedef_char: &a2lfile::TypedefCharacteristic,
    _tables: &TypedefTables<'_>,
) -> Option<BaseDt> {
    // The TypedefTables struct doesn't carry record layouts — we need the module too.
    // OPTION 1: extend TypedefTables to carry &Module, look up by name here.
    // OPTION 2: Pass the module separately.
    //
    // Implementer: choose the smaller diff. Recommended: add `module: &'a a2lfile::Module`
    // to TypedefTables. The implementation reads:
    //   let rl = self.module.record_layout.iter().find(|r| r.get_name() == typedef_char.deposit)?;
    //   let fnc = rl.fnc_values.as_ref()?;
    //   BaseDt::from_a2lfile(&fnc.datatype)
    //
    // That mirrors the exact pattern used by `parse_characteristic_datatype` at
    // src/a2l/reader.rs:1220-1230. Read that function before writing this one.
    todo!("see comment above")
}
```

This deliberately leaves a `todo!()`. Implementing it properly requires either extending `TypedefTables` to carry the `&Module` reference, or passing the module through the call chain. Pick the smaller change. Look at the existing `parse_characteristic_datatype` in `src/a2l/reader.rs:1220` for the canonical lookup pattern.

`StorageOrder::default()` may not exist — check `src/a2l/types.rs`. If not, use the same default the rest of the code uses (likely `StorageOrder::RowDir`).

- [ ] **Step 4: Resolve the deposit lookup, then test**

Implement `lookup_deposit_basetype` per the comment, run:
`cargo test --lib flatten_simple_struct_characteristics`
Expected: PASS.

- [ ] **Step 5: Wire into A2lReader::get_characteristics**

In `src/a2l/reader.rs`, find `pub fn get_characteristics(&self) -> Vec<A2lCharacteristic>` at line 448. Apply the same append-and-dedupe pattern as Task 6 step 3, but for characteristics:

```rust
let mut flattened = super::typedef_flatten::flatten_instance_characteristics(module);
for c in &mut flattened {
    c.unit = self.get_unit_for_conversion(&c.conversion_name);
}
let flat_names: std::collections::HashSet<&str> =
    characteristics.iter().map(|c| c.name.as_str()).collect();
flattened.retain(|c| {
    if flat_names.contains(c.name.as_str()) {
        tracing::warn!(name = %c.name, "structural calibration collides with flat CHARACTERISTIC, dropped");
        false
    } else {
        true
    }
});
characteristics.extend(flattened);
```

(Append after the existing characteristic-parsing loop in the `for module in ...` body.)

- [ ] **Step 6: Add a reader-level test for characteristics**

Mirror the `reader_appends_structural_measurements` test in `mod tests` of `reader.rs`, but for characteristics. Write a fixture in temp, load it, assert `get_characteristics()` returns the expected flat + flattened entries.

- [ ] **Step 7: Run the whole library test suite**

Run: `cargo test --lib`
Expected: PASS.

- [ ] **Step 8: Format, lint, commit**

```bash
cargo fmt --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
git add src/a2l/typedef_flatten.rs src/a2l/reader.rs
git commit -m "feat(a2l): flatten structural calibrations via get_characteristics"
```

---

## Task 9: Smoke-check downstream consumers (no code changes expected)

The contract of Approach A is that downstream modules don't need to change. Verify that.

- [ ] **Step 1: Run integration tests**

Run: `cargo test --test mock_xcp_integration -- --test-threads=1`
Expected: PASS, identical to baseline.

- [ ] **Step 2: Run all tests**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 3: Build release**

Run: `cargo build --release`
Expected: clean build.

- [ ] **Step 4: Manual sanity (optional but recommended)**

Launch the Web UI against the structural fixture if convenient, or just run `cargo run --release -- web` and load the fixture A2L through the upload route. Confirm `lookup.input`, `lookup.output_linear`, `lookup.output_spline` appear in the measurement list. If they don't, debug — but the unit + integration tests should already have caught any issue.

- [ ] **Step 5: No commit (verification only)**

---

## Task 10: Update CHANGELOG / commit summary

- [ ] **Step 1: Final review pass**

Re-read `git log --oneline main..HEAD` (or the equivalent vs. the merge base). Verify each commit message is conventional-commit style (`feat:`, `test:`, `docs:`).

- [ ] **Step 2: If a CHANGELOG.md exists, add an entry**

Check: `ls CHANGELOG.md` — if present, add under "Unreleased":

```markdown
- A2L: support structural measurements and structural calibrations via TYPEDEF_STRUCTURE + INSTANCE; leaf components appear as `instance.component[.subcomponent...]` entries in the measurement and calibration lists.
```

If no CHANGELOG.md exists, skip this step. Don't create one.

- [ ] **Step 3: Commit if changed**

```bash
git add CHANGELOG.md
git commit -m "docs: note structural measurement support"
```

---

## Done criteria

All of these must hold before declaring complete:

- [ ] `cargo test --workspace` passes.
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean.
- [ ] `cargo fmt --workspace -- --check` clean.
- [ ] `cargo build --release` succeeds.
- [ ] The integration test in `tests/structural_measurement_a2l.rs` passes.
- [ ] No changes to `src/daq.rs`, `src/calibration.rs`, `src/web/`, `src-tauri/`, `src/variable.rs`. (Run `git diff --stat main..HEAD` and confirm.)
- [ ] Spec at `docs/superpowers/specs/2026-06-15-structural-measurements-design.md` accurately reflects what was built (or has been amended if reality forced changes).
