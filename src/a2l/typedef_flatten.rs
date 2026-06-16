//! Flattens A2L INSTANCE + TYPEDEF_STRUCTURE + TYPEDEF_MEASUREMENT/CHARACTERISTIC
//! into synthetic A2lMeasurement / A2lCharacteristic entries.
//!
//! See `docs/superpowers/specs/2026-06-15-structural-measurements-design.md`.

use crate::a2l::reader::{A2lCharacteristic, A2lMeasurement};
use crate::a2l::types::{A2lDataType, CharacteristicType, StorageOrder};
use std::collections::HashMap;

/// Maximum nesting depth before we declare a cycle and stop recursing.
const MAX_DEPTH: usize = 8;

/// Builds a dotted path from an instance name and a chain of component names.
pub(super) fn build_path(instance: &str, components: &[&str]) -> String {
    let mut out = String::with_capacity(
        instance.len() + components.iter().map(|c| c.len() + 1).sum::<usize>(),
    );
    out.push_str(instance);
    for c in components {
        out.push('.');
        out.push_str(c);
    }
    out
}

/// Base scalar data type of a TYPEDEF_MEASUREMENT/TYPEDEF_CHARACTERISTIC.
/// Mirrors what `A2lReader::parse_datatype` produces — kept as a tiny enum so
/// this module's tests don't need a2lfile types.
///
/// Note: `A2lDataType::Bool` is intentionally NOT represented here. Bool is
/// produced by `A2lReader` from RECORD_LAYOUT name heuristics ("BOOLEAN")
/// in the flat MEASUREMENT/CHARACTERISTIC path, but `a2lfile::DataType` (the
/// only source feeding `BaseDt`) has no Bool variant — TYPEDEF_*s carry a
/// raw integer/float DataType, never a synthetic Bool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BaseDt {
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
}

impl BaseDt {
    /// Map an `a2lfile::DataType` to our internal `BaseDt`.
    ///
    /// Returns `None` for any variant we don't support (currently only
    /// `Float16Ieee`). The match is **exhaustive** — if `a2lfile` ever adds a
    /// new variant, this will fail to compile and force an explicit decision
    /// here rather than silently dropping the component.
    pub(super) fn from_a2lfile(dt: &a2lfile::DataType) -> Option<Self> {
        match dt {
            a2lfile::DataType::Ubyte => Some(BaseDt::U8),
            a2lfile::DataType::Uword => Some(BaseDt::U16),
            a2lfile::DataType::Ulong => Some(BaseDt::U32),
            a2lfile::DataType::AUint64 => Some(BaseDt::U64),
            a2lfile::DataType::Sbyte => Some(BaseDt::I8),
            a2lfile::DataType::Sword => Some(BaseDt::I16),
            a2lfile::DataType::Slong => Some(BaseDt::I32),
            a2lfile::DataType::AInt64 => Some(BaseDt::I64),
            a2lfile::DataType::Float32Ieee => Some(BaseDt::F32),
            a2lfile::DataType::Float64Ieee => Some(BaseDt::F64),
            // Float16 has no IEEE 754 binary16 path in our pipeline; skip it.
            a2lfile::DataType::Float16Ieee => None,
        }
    }
}

/// Combines a base scalar type with `matrix_dim` to produce an `A2lDataType`.
///
/// Rule (consistent with how `A2lDataType` itself is shaped — scalar / 1D /
/// 2D only):
///
/// - Any dim equal to `0` → `None` (caller should warn and skip)
/// - `matrix_dim.len() > 2` → `None` (3+ dims not representable)
/// - `[]` or all-`1` dims (`[1]`, `[1, 1]`) → scalar
/// - `[N]` with `N > 1` → 1D `Array(N)`
/// - `[N, 1]` or `[1, N]` with `N > 1` → 1D `Array(N)` (singleton dim squeezed)
/// - `[R, C]` with `R > 1` AND `C > 1` → 2D `Matrix(R, C)`
pub(super) fn component_datatype(base: BaseDt, matrix_dim: &[u16]) -> Option<A2lDataType> {
    // Reject zero dims and >2D shapes outright.
    if matrix_dim.contains(&0) {
        return None;
    }
    if matrix_dim.len() > 2 {
        return None;
    }

    match matrix_dim {
        // Scalar cases.
        [] | [1] | [1, 1] => Some(scalar(base)),
        // Pure 1D array.
        [n] => Some(array(base, *n as usize)),
        // 2D shape with a squeezable singleton becomes 1D.
        [n, 1] | [1, n] => Some(array(base, *n as usize)),
        // True 2D matrix.
        [r, c] => Some(matrix(base, *r as usize, *c as usize)),
        // Unreachable: covered by the length guard above, but keep exhaustive.
        _ => None,
    }
}

fn scalar(b: BaseDt) -> A2lDataType {
    match b {
        BaseDt::U8 => A2lDataType::U8,
        BaseDt::U16 => A2lDataType::U16,
        BaseDt::U32 => A2lDataType::U32,
        BaseDt::U64 => A2lDataType::U64,
        BaseDt::I8 => A2lDataType::I8,
        BaseDt::I16 => A2lDataType::I16,
        BaseDt::I32 => A2lDataType::I32,
        BaseDt::I64 => A2lDataType::I64,
        BaseDt::F32 => A2lDataType::F32,
        BaseDt::F64 => A2lDataType::F64,
    }
}

fn array(b: BaseDt, n: usize) -> A2lDataType {
    match b {
        BaseDt::U8 => A2lDataType::U8Array(n),
        BaseDt::U16 => A2lDataType::U16Array(n),
        BaseDt::U32 => A2lDataType::U32Array(n),
        BaseDt::U64 => A2lDataType::U64Array(n),
        BaseDt::I8 => A2lDataType::I8Array(n),
        BaseDt::I16 => A2lDataType::I16Array(n),
        BaseDt::I32 => A2lDataType::I32Array(n),
        BaseDt::I64 => A2lDataType::I64Array(n),
        BaseDt::F32 => A2lDataType::F32Array(n),
        BaseDt::F64 => A2lDataType::F64Array(n),
    }
}

fn matrix(b: BaseDt, r: usize, c: usize) -> A2lDataType {
    match b {
        BaseDt::U8 => A2lDataType::U8Matrix(r, c),
        BaseDt::U16 => A2lDataType::U16Matrix(r, c),
        BaseDt::U32 => A2lDataType::U32Matrix(r, c),
        BaseDt::U64 => A2lDataType::U64Matrix(r, c),
        BaseDt::I8 => A2lDataType::I8Matrix(r, c),
        BaseDt::I16 => A2lDataType::I16Matrix(r, c),
        BaseDt::I32 => A2lDataType::I32Matrix(r, c),
        BaseDt::I64 => A2lDataType::I64Matrix(r, c),
        BaseDt::F32 => A2lDataType::F32Matrix(r, c),
        BaseDt::F64 => A2lDataType::F64Matrix(r, c),
    }
}

/// Lookup tables built once per flatten call.
pub(super) struct TypedefTables<'a> {
    pub(super) structures: HashMap<&'a str, &'a a2lfile::TypedefStructure>,
    pub(super) measurements: HashMap<&'a str, &'a a2lfile::TypedefMeasurement>,
    pub(super) characteristics: HashMap<&'a str, &'a a2lfile::TypedefCharacteristic>,
}

impl<'a> TypedefTables<'a> {
    pub(super) fn build(module: &'a a2lfile::Module) -> Self {
        use a2lfile::A2lObjectName;
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
        Self {
            structures,
            measurements,
            characteristics,
        }
    }
}

/// Flatten all INSTANCE blocks in the module whose type resolves to a
/// TYPEDEF_STRUCTURE composed of TYPEDEF_MEASUREMENT components.
///
/// Returns synthetic A2lMeasurement entries with dotted names like
/// `instance.component[.subcomponent...]`.
///
/// Components whose type is not a TYPEDEF_MEASUREMENT or TYPEDEF_STRUCTURE
/// are skipped with a warning. Cycles are guarded at depth `MAX_DEPTH`.
pub(super) fn flatten_instance_measurements(module: &a2lfile::Module) -> Vec<A2lMeasurement> {
    use a2lfile::A2lObjectName;
    let tables = TypedefTables::build(module);
    let mut out = Vec::new();
    for inst in &module.instance {
        // Array-of-structs (INSTANCE with MATRIX_DIM) is a non-goal in this
        // iteration. An absent MATRIX_DIM block (or one with empty dim_list)
        // is treated as scalar.
        if inst
            .matrix_dim
            .as_ref()
            .is_some_and(|md| !md.dim_list.is_empty())
        {
            tracing::warn!(
                instance = %inst.get_name(),
                "INSTANCE with MATRIX_DIM (array-of-structs) is not supported, skipped",
            );
            continue;
        }
        if !inst.overwrite.is_empty() {
            tracing::warn!(
                instance = %inst.get_name(),
                count = inst.overwrite.len(),
                "INSTANCE OVERWRITE blocks ignored",
            );
        }
        let ext = inst
            .ecu_address_extension
            .as_ref()
            .map(|e| e.extension as u8)
            .unwrap_or(0);
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

#[allow(clippy::too_many_arguments)]
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
        tracing::warn!(
            instance,
            type_ref = type_name,
            depth,
            "max nesting depth reached - possible TYPEDEF_STRUCTURE cycle, stopping recursion",
        );
        return;
    }
    let Some(structure) = tables.structures.get(type_name) else {
        if depth == 0 {
            tracing::warn!(
                instance,
                type_ref = type_name,
                "INSTANCE references unknown TYPEDEF_STRUCTURE",
            );
        } else {
            tracing::warn!(
                instance,
                type_ref = type_name,
                depth,
                "nested STRUCTURE_COMPONENT references unknown TYPEDEF_STRUCTURE",
            );
        }
        return;
    };
    for comp in &structure.structure_component {
        let Some(comp_addr) = base_addr.checked_add(comp.address_offset) else {
            tracing::warn!(
                instance,
                component = %comp.component_name,
                base_addr,
                offset = comp.address_offset,
                "STRUCTURE_COMPONENT address overflow, skipped"
            );
            continue;
        };
        let comp_type = comp.component_type.as_str();

        // Try as nested struct first.
        if tables.structures.contains_key(comp_type) {
            path.push(comp.component_name.clone());
            flatten_struct(
                tables,
                comp_type,
                instance,
                path,
                comp_addr,
                ext,
                depth + 1,
                out,
            );
            path.pop();
            continue;
        }

        // Then as a TYPEDEF_MEASUREMENT leaf.
        let Some(typedef_meas) = tables.measurements.get(comp_type) else {
            tracing::warn!(
                instance,
                component = %comp.component_name,
                type_ref = comp_type,
                "STRUCTURE_COMPONENT type is not a known TYPEDEF_STRUCTURE or TYPEDEF_MEASUREMENT, skipped",
            );
            continue;
        };

        let Some(base) = BaseDt::from_a2lfile(&typedef_meas.datatype) else {
            tracing::warn!(
                instance,
                component = %comp.component_name,
                "unsupported A2L datatype on TYPEDEF_MEASUREMENT (e.g. FLOAT16_IEEE), skipped",
            );
            continue;
        };

        let matrix = matrix_dim_slice(&comp.matrix_dim);
        let Some(data_type) = component_datatype(base, matrix) else {
            tracing::warn!(
                instance,
                component = %comp.component_name,
                ?matrix,
                "STRUCTURE_COMPONENT MATRIX_DIM not representable (3+ dims or zero dim), skipped",
            );
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
            // unit is filled in by reader.rs (Task 6) - it has access to the
            // COMPU_METHOD table to resolve `conversion_name` -> unit string.
            unit: None,
            min: typedef_meas.lower_limit,
            max: typedef_meas.upper_limit,
            conversion_name: typedef_meas.conversion.clone(),
        });
    }
}

/// Convert `Option<a2lfile::MatrixDim>` to a slice of `u16` dims.
/// `None` and `Some` with empty `dim_list` both mean "no MATRIX_DIM" -> scalar.
fn matrix_dim_slice(md: &Option<a2lfile::MatrixDim>) -> &[u16] {
    md.as_ref().map(|m| m.dim_list.as_slice()).unwrap_or(&[])
}

/// Resolve a TYPEDEF_CHARACTERISTIC's `record_layout` (deposit) name to its
/// FNC_VALUES base datatype, mapped to our internal `BaseDt`.
///
/// Returns `None` if the record layout is missing, has no FNC_VALUES, or the
/// datatype is unsupported (e.g. FLOAT16_IEEE).
fn lookup_deposit_basetype(
    typedef_char: &a2lfile::TypedefCharacteristic,
    module: &a2lfile::Module,
) -> Option<BaseDt> {
    use a2lfile::A2lObjectName;
    let rl = module
        .record_layout
        .iter()
        .find(|rl| rl.get_name() == typedef_char.record_layout.as_str())?;
    let fnc = rl.fnc_values.as_ref()?;
    BaseDt::from_a2lfile(&fnc.datatype)
}

/// Flatten all INSTANCE blocks in the module whose type resolves to a
/// TYPEDEF_STRUCTURE composed of TYPEDEF_CHARACTERISTIC components.
///
/// Returns synthetic A2lCharacteristic entries with dotted names like
/// `instance.component[.subcomponent...]`.
///
/// Only VALUE-shaped components are supported in this iteration. CURVE/MAP
/// inside structs are warned and skipped. Components whose type is not a
/// TYPEDEF_CHARACTERISTIC or TYPEDEF_STRUCTURE are skipped with a warning.
/// Cycles are guarded at depth `MAX_DEPTH`.
pub(super) fn flatten_instance_characteristics(module: &a2lfile::Module) -> Vec<A2lCharacteristic> {
    use a2lfile::A2lObjectName;
    let tables = TypedefTables::build(module);
    let mut out = Vec::new();
    for inst in &module.instance {
        // Array-of-structs (INSTANCE with MATRIX_DIM) is a non-goal; the
        // measurement path already warns, so just skip silently here to avoid
        // duplicate noise.
        if inst
            .matrix_dim
            .as_ref()
            .is_some_and(|md| !md.dim_list.is_empty())
        {
            continue;
        }
        let ext = inst
            .ecu_address_extension
            .as_ref()
            .map(|e| e.extension as u8)
            .unwrap_or(0);
        let inst_name = inst.get_name();
        flatten_struct_chars(
            &tables,
            inst.type_ref.as_str(),
            inst_name,
            &mut Vec::new(),
            inst.start_address,
            ext,
            0,
            module,
            &mut out,
        );
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn flatten_struct_chars(
    tables: &TypedefTables<'_>,
    type_name: &str,
    instance: &str,
    path: &mut Vec<String>,
    base_addr: u32,
    ext: u8,
    depth: usize,
    module: &a2lfile::Module,
    out: &mut Vec<A2lCharacteristic>,
) {
    if depth >= MAX_DEPTH {
        tracing::warn!(
            instance,
            type_ref = type_name,
            depth,
            "max nesting depth reached - possible TYPEDEF_STRUCTURE cycle, stopping recursion (calibration path)",
        );
        return;
    }
    let Some(structure) = tables.structures.get(type_name) else {
        // Only emit the top-level "INSTANCE references unknown TYPEDEF_STRUCTURE"
        // warning at depth 0; the measurement path will already have warned for
        // the same instance, so stay quiet for nested misses to avoid duplicate
        // log spam.
        return;
    };
    for comp in &structure.structure_component {
        let Some(comp_addr) = base_addr.checked_add(comp.address_offset) else {
            tracing::warn!(
                instance,
                component = %comp.component_name,
                base_addr,
                offset = comp.address_offset,
                "STRUCTURE_COMPONENT address overflow, skipped (calibration path)"
            );
            continue;
        };
        let comp_type = comp.component_type.as_str();

        // Try as nested struct first.
        if tables.structures.contains_key(comp_type) {
            path.push(comp.component_name.clone());
            flatten_struct_chars(
                tables,
                comp_type,
                instance,
                path,
                comp_addr,
                ext,
                depth + 1,
                module,
                out,
            );
            path.pop();
            continue;
        }

        // Then as a TYPEDEF_CHARACTERISTIC leaf.
        let Some(typedef_char) = tables.characteristics.get(comp_type) else {
            // Could be a TYPEDEF_MEASUREMENT (handled by the measurement path)
            // or genuinely unknown. The measurement flattener already warns for
            // the unknown case; emit a debug-level note here only.
            tracing::debug!(
                instance,
                component = %comp.component_name,
                type_ref = comp_type,
                "STRUCTURE_COMPONENT type is not a TYPEDEF_CHARACTERISTIC or nested TYPEDEF_STRUCTURE in calibration flattener, skipped",
            );
            continue;
        };

        // Only VALUE-shaped TYPEDEF_CHARACTERISTICs are supported in this
        // iteration. CURVE/MAP/VAL_BLK/ASCII inside structs require axis
        // descriptions and storage-order plumbing we haven't designed yet.
        if !matches!(
            typedef_char.characteristic_type,
            a2lfile::CharacteristicType::Value
        ) {
            tracing::warn!(
                instance,
                component = %comp.component_name,
                type_ref = comp_type,
                kind = ?typedef_char.characteristic_type,
                "non-VALUE TYPEDEF_CHARACTERISTIC inside structure not supported, skipped",
            );
            continue;
        }

        let Some(base) = lookup_deposit_basetype(typedef_char, module) else {
            tracing::warn!(
                instance,
                component = %comp.component_name,
                deposit = %typedef_char.record_layout,
                "unable to resolve deposit RECORD_LAYOUT or unsupported FNC_VALUES datatype, skipped",
            );
            continue;
        };

        let matrix = matrix_dim_slice(&comp.matrix_dim);
        let Some(data_type) = component_datatype(base, matrix) else {
            tracing::warn!(
                instance,
                component = %comp.component_name,
                ?matrix,
                "STRUCTURE_COMPONENT MATRIX_DIM not representable (3+ dims or zero dim), skipped",
            );
            continue;
        };

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
            // unit is filled in by reader.rs via get_unit_for_conversion.
            unit: None,
            min: typedef_char.lower_limit,
            max: typedef_char.upper_limit,
            characteristic_type: CharacteristicType::Value,
            axis_info: Vec::new(),
            storage_order: StorageOrder::default(),
            conversion_name: typedef_char.conversion.clone(),
        });
    }
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

    #[test]
    fn datatype_scalar_empty() {
        assert!(matches!(
            component_datatype(BaseDt::F32, &[]),
            Some(A2lDataType::F32)
        ));
    }

    #[test]
    fn datatype_scalar_single_one() {
        assert!(matches!(
            component_datatype(BaseDt::U16, &[1]),
            Some(A2lDataType::U16)
        ));
    }

    #[test]
    fn datatype_scalar_one_one() {
        // [1, 1] squeezes to scalar.
        assert!(matches!(
            component_datatype(BaseDt::I32, &[1, 1]),
            Some(A2lDataType::I32)
        ));
    }

    #[test]
    fn datatype_array_1d() {
        assert!(matches!(
            component_datatype(BaseDt::F32, &[4]),
            Some(A2lDataType::F32Array(4))
        ));
        assert!(matches!(
            component_datatype(BaseDt::U8, &[10]),
            Some(A2lDataType::U8Array(10))
        ));
    }

    #[test]
    fn datatype_n_by_one_squeezes_to_array() {
        // [5, 1] squeezes the trailing singleton → 1D Array(5).
        assert!(matches!(
            component_datatype(BaseDt::U8, &[5, 1]),
            Some(A2lDataType::U8Array(5))
        ));
    }

    #[test]
    fn datatype_one_by_n_squeezes_to_array() {
        // [1, 5] squeezes the leading singleton → 1D Array(5).
        assert!(matches!(
            component_datatype(BaseDt::U8, &[1, 5]),
            Some(A2lDataType::U8Array(5))
        ));
    }

    #[test]
    fn datatype_matrix_2d() {
        assert!(matches!(
            component_datatype(BaseDt::F32, &[3, 2]),
            Some(A2lDataType::F32Matrix(3, 2))
        ));
    }

    #[test]
    fn datatype_3d_unsupported() {
        assert!(component_datatype(BaseDt::F32, &[2, 2, 2]).is_none());
    }

    #[test]
    fn datatype_three_dim_with_singleton_still_unsupported() {
        // [2, 3, 1] has only 2 effective dims, but len() > 2 so we reject.
        // The flatten pipeline never sees real 3D shapes; rejecting keeps the
        // contract simple.
        assert!(component_datatype(BaseDt::F32, &[2, 3, 1]).is_none());
    }

    #[test]
    fn datatype_zero_dim_rejected_1d() {
        assert!(component_datatype(BaseDt::F32, &[0]).is_none());
    }

    #[test]
    fn datatype_zero_dim_rejected_2d() {
        // [3, 0] has a zero dim — nonsensical, reject.
        assert!(component_datatype(BaseDt::F32, &[3, 0]).is_none());
        assert!(component_datatype(BaseDt::F32, &[0, 3]).is_none());
    }

    #[test]
    fn from_a2lfile_maps_known_types() {
        assert_eq!(
            BaseDt::from_a2lfile(&a2lfile::DataType::Ubyte),
            Some(BaseDt::U8)
        );
        assert_eq!(
            BaseDt::from_a2lfile(&a2lfile::DataType::Float64Ieee),
            Some(BaseDt::F64)
        );
        assert_eq!(
            BaseDt::from_a2lfile(&a2lfile::DataType::AInt64),
            Some(BaseDt::I64)
        );
    }

    #[test]
    fn from_a2lfile_rejects_float16() {
        assert_eq!(BaseDt::from_a2lfile(&a2lfile::DataType::Float16Ieee), None);
    }

    /// Ensure `MAX_DEPTH` keeps its documented value — the recursion guard in
    /// Task 4/5 will rely on this.
    #[test]
    fn max_depth_constant() {
        assert_eq!(MAX_DEPTH, 8);
    }

    // -------------------- Task 4: end-to-end flattening --------------------

    /// Minimal A2L exercising the user's Lookup struct example: one INSTANCE
    /// of a TYPEDEF_STRUCTURE with three FLOAT32 TYPEDEF_MEASUREMENT
    /// components.
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

    fn parse_a2l(src: &str) -> a2lfile::A2lFile {
        let mut errors = Vec::new();
        // strict_parsing=false: minimal fixtures may omit blocks the strict
        // parser otherwise expects (e.g. HEADER). The non-strict path still
        // accepts the structural elements we care about here.
        a2lfile::load_from_string(src, None, &mut errors, false).unwrap_or_else(|e| {
            panic!("a2l parse failed: {e:?}; log: {errors:?}");
        })
    }

    #[test]
    fn flatten_simple_struct_measurements() {
        let f = parse_a2l(SIMPLE_A2L);
        let module = &f.project.module[0];
        let measurements = flatten_instance_measurements(module);
        assert_eq!(measurements.len(), 3, "expected 3 leaf measurements");

        let summary: Vec<_> = measurements
            .iter()
            .map(|m| (m.name.as_str(), m.address, m.address_ext))
            .collect();
        assert!(
            summary.contains(&("lookup.input", 0xFFF4, 2)),
            "missing lookup.input @ 0xFFF4 ext 2; got {summary:?}",
        );
        assert!(
            summary.contains(&("lookup.output_linear", 0xFFF8, 2)),
            "missing lookup.output_linear @ 0xFFF8 ext 2; got {summary:?}",
        );
        assert!(
            summary.contains(&("lookup.output_spline", 0xFFFC, 2)),
            "missing lookup.output_spline @ 0xFFFC ext 2; got {summary:?}",
        );

        for meas in &measurements {
            assert!(
                matches!(meas.data_type, A2lDataType::F32),
                "expected F32, got {:?} for {}",
                meas.data_type,
                meas.name,
            );
            assert_eq!(meas.conversion_name, "NO_COMPU_METHOD");
            assert_eq!(meas.unit, None, "unit is filled later by reader.rs");
        }
    }

    // -------------------- Task 5: edge-case coverage --------------------

    /// Nested TYPEDEF_STRUCTURE: Outer { foo: Inner @16, bar: f32val @32 } and
    /// Inner { x: u16val @0, y: u16val @2 }. Asserts dotted paths and address
    /// arithmetic compose correctly across nesting.
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
    /begin STRUCTURE_COMPONENT foo Inner  16 /end STRUCTURE_COMPONENT
    /begin STRUCTURE_COMPONENT bar f32val 32 /end STRUCTURE_COMPONENT
  /end TYPEDEF_STRUCTURE
  /begin INSTANCE outer "" Outer 0x1000 /end INSTANCE
/end MODULE /end PROJECT
"#;

    #[test]
    fn flatten_nested_struct() {
        let f = parse_a2l(NESTED_A2L);
        let module = &f.project.module[0];
        let measurements = flatten_instance_measurements(module);
        assert_eq!(
            measurements.len(),
            3,
            "expected 3 leaf measurements, got {measurements:?}",
        );

        let by_name: HashMap<&str, &A2lMeasurement> =
            measurements.iter().map(|m| (m.name.as_str(), m)).collect();

        let foo_x = by_name.get("outer.foo.x").expect("outer.foo.x missing");
        assert_eq!(foo_x.address, 0x1010);
        assert!(matches!(foo_x.data_type, A2lDataType::U16));

        let foo_y = by_name.get("outer.foo.y").expect("outer.foo.y missing");
        assert_eq!(foo_y.address, 0x1012);
        assert!(matches!(foo_y.data_type, A2lDataType::U16));

        let bar = by_name.get("outer.bar").expect("outer.bar missing");
        assert_eq!(bar.address, 0x1020);
        assert!(matches!(bar.data_type, A2lDataType::F32));
    }

    /// STRUCTURE_COMPONENT with MATRIX_DIM should produce a 1D array leaf via
    /// `component_datatype`.
    const ARRAY_A2L: &str = r#"
ASAP2_VERSION 1 71
/begin PROJECT P "" /begin MODULE M ""
  /begin TYPEDEF_MEASUREMENT f32val "" FLOAT32_IEEE NO_COMPU_METHOD 0 0 -1e30 1e30 /end TYPEDEF_MEASUREMENT
  /begin TYPEDEF_STRUCTURE Vec4 "" 16
    /begin STRUCTURE_COMPONENT v f32val 0
      MATRIX_DIM 4
    /end STRUCTURE_COMPONENT
  /end TYPEDEF_STRUCTURE
  /begin INSTANCE vec4_inst "" Vec4 0x2000 /end INSTANCE
/end MODULE /end PROJECT
"#;

    #[test]
    fn flatten_array_component() {
        let f = parse_a2l(ARRAY_A2L);
        let module = &f.project.module[0];
        let measurements = flatten_instance_measurements(module);
        assert_eq!(measurements.len(), 1, "expected 1 leaf measurement");

        let m = &measurements[0];
        assert_eq!(m.name, "vec4_inst.v");
        assert_eq!(m.address, 0x2000);
        assert!(
            matches!(m.data_type, A2lDataType::F32Array(4)),
            "expected F32Array(4), got {:?}",
            m.data_type,
        );
    }

    /// Mixed struct: one good component + one bogus type reference. The bad
    /// component is skipped with a warning; the good one is emitted.
    const UNKNOWN_TYPE_A2L: &str = r#"
ASAP2_VERSION 1 71
/begin PROJECT P "" /begin MODULE M ""
  /begin TYPEDEF_MEASUREMENT f32val "" FLOAT32_IEEE NO_COMPU_METHOD 0 0 -1e30 1e30 /end TYPEDEF_MEASUREMENT
  /begin TYPEDEF_STRUCTURE Mixed "" 8
    /begin STRUCTURE_COMPONENT good f32val     0 /end STRUCTURE_COMPONENT
    /begin STRUCTURE_COMPONENT bad  Nonexistent 4 /end STRUCTURE_COMPONENT
  /end TYPEDEF_STRUCTURE
  /begin INSTANCE mixed "" Mixed 0x3000 /end INSTANCE
/end MODULE /end PROJECT
"#;

    #[test]
    fn flatten_unknown_type_skipped() {
        let f = parse_a2l(UNKNOWN_TYPE_A2L);
        let module = &f.project.module[0];
        let measurements = flatten_instance_measurements(module);
        assert_eq!(
            measurements.len(),
            1,
            "expected only the good component to survive: {measurements:?}",
        );
        assert_eq!(measurements[0].name, "mixed.good");
    }

    /// INSTANCE with a TYPE name that doesn't resolve to any TYPEDEF_STRUCTURE.
    /// The flattener must warn and produce no leaves.
    const ORPHAN_INSTANCE_A2L: &str = r#"
ASAP2_VERSION 1 71
/begin PROJECT P "" /begin MODULE M ""
  /begin INSTANCE orphan "" NoSuchType 0x4000 /end INSTANCE
/end MODULE /end PROJECT
"#;

    #[test]
    fn flatten_orphan_instance_skipped() {
        let f = parse_a2l(ORPHAN_INSTANCE_A2L);
        let module = &f.project.module[0];
        let measurements = flatten_instance_measurements(module);
        assert!(
            measurements.is_empty(),
            "expected no leaves for orphan INSTANCE: {measurements:?}",
        );
    }

    /// Self-referential cycle: A.b is type B, B.a is type A. The cycle guard
    /// must stop recursion at MAX_DEPTH and the test must complete promptly.
    /// No leaves are produced because there's no TYPEDEF_MEASUREMENT in the
    /// cycle; only nested structures.
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
        let f = parse_a2l(CYCLE_A2L);
        let module = &f.project.module[0];
        let measurements = flatten_instance_measurements(module);
        assert!(
            measurements.is_empty(),
            "cycle without leaf measurements should yield empty: {measurements:?}",
        );
    }

    // -------------------- Task 8: structural calibrations --------------------

    /// Minimal A2L exercising structural calibrations: one INSTANCE of a
    /// TYPEDEF_STRUCTURE composed of two FLOAT32 TYPEDEF_CHARACTERISTIC
    /// components.
    const CAL_A2L: &str = r#"
ASAP2_VERSION 1 71
/begin PROJECT P "" /begin MODULE M ""
  /begin RECORD_LAYOUT __F32 FNC_VALUES 1 FLOAT32_IEEE ROW_DIR DIRECT /end RECORD_LAYOUT
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
        let f = parse_a2l(CAL_A2L);
        let module = &f.project.module[0];
        let cs = flatten_instance_characteristics(module);
        let names: Vec<_> = cs.iter().map(|c| (c.name.as_str(), c.address)).collect();
        assert_eq!(cs.len(), 2, "expected 2 leaf characteristics, got {cs:?}");
        assert!(
            names.contains(&("tuning.gain", 0x6000)),
            "missing tuning.gain @ 0x6000; got {names:?}",
        );
        assert!(
            names.contains(&("tuning.offset", 0x6004)),
            "missing tuning.offset @ 0x6004; got {names:?}",
        );
        for c in &cs {
            assert!(
                matches!(
                    c.characteristic_type,
                    crate::a2l::types::CharacteristicType::Value
                ),
                "expected VALUE, got {:?} for {}",
                c.characteristic_type,
                c.name,
            );
            assert!(
                c.axis_info.is_empty(),
                "axis_info should be empty for VALUE"
            );
            assert!(
                matches!(c.data_type, A2lDataType::F32),
                "expected F32, got {:?} for {}",
                c.data_type,
                c.name,
            );
            assert_eq!(c.conversion_name, "NO_COMPU_METHOD");
            assert_eq!(c.unit, None, "unit is filled later by reader.rs");
        }
    }
}
