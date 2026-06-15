//! Flattens A2L INSTANCE + TYPEDEF_STRUCTURE + TYPEDEF_MEASUREMENT/CHARACTERISTIC
//! into synthetic A2lMeasurement / A2lCharacteristic entries.
//!
//! See `docs/superpowers/specs/2026-06-15-structural-measurements-design.md`.

/// Maximum nesting depth before we declare a cycle and stop recursing.
#[allow(dead_code)]
const MAX_DEPTH: usize = 8;

/// Builds a dotted path from an instance name and a chain of component names.
#[allow(dead_code)]
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

use crate::a2l::types::A2lDataType;

/// Base scalar data type of a TYPEDEF_MEASUREMENT/TYPEDEF_CHARACTERISTIC.
/// Mirrors what `A2lReader::parse_datatype` produces — kept as a tiny enum so
/// this module's tests don't need a2lfile types.
#[allow(dead_code)]
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
    Bool,
}

impl BaseDt {
    #[allow(dead_code)]
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
#[allow(dead_code)]
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
        BaseDt::Bool => A2lDataType::Bool,
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
        BaseDt::Bool => A2lDataType::BoolArray(n),
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
        BaseDt::Bool => A2lDataType::BoolMatrix(r, c),
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
    fn datatype_scalar() {
        assert!(matches!(
            component_datatype(BaseDt::F32, &[]),
            Some(A2lDataType::F32)
        ));
        assert!(matches!(
            component_datatype(BaseDt::U16, &[1]),
            Some(A2lDataType::U16)
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
    fn datatype_zero_dim_treated_as_scalar() {
        assert!(matches!(
            component_datatype(BaseDt::F32, &[0]),
            Some(A2lDataType::F32)
        ));
    }
}
