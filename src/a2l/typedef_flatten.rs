//! Flattens A2L INSTANCE + TYPEDEF_STRUCTURE + TYPEDEF_MEASUREMENT/CHARACTERISTIC
//! into synthetic A2lMeasurement / A2lCharacteristic entries.
//!
//! See `docs/superpowers/specs/2026-06-15-structural-measurements-design.md`.

// All items here are wired into the flatten pipeline by Task 6; until then
// most symbols look unused. Keep a single module-level allow rather than
// scattering attributes on every item.
#![allow(dead_code)]

use crate::a2l::types::A2lDataType;
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
}
