//! A2L Data Types
//!
//! Core data types for A2L characteristics, measurements, and conversion methods.

/// Data type information extracted from A2L
#[derive(Debug, Clone)]
pub enum A2lDataType {
    // Scalar types
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
    // Array types
    U8Array(usize),
    U16Array(usize),
    U32Array(usize),
    U64Array(usize),
    I8Array(usize),
    I16Array(usize),
    I32Array(usize),
    I64Array(usize),
    F32Array(usize),
    F64Array(usize),
    BoolArray(usize),
    // Matrix types (row-major order)
    U8Matrix(usize, usize),  // rows, cols
    U16Matrix(usize, usize),
    U32Matrix(usize, usize),
    U64Matrix(usize, usize),
    I8Matrix(usize, usize),
    I16Matrix(usize, usize),
    I32Matrix(usize, usize),
    I64Matrix(usize, usize),
    F32Matrix(usize, usize),
    F64Matrix(usize, usize),
    BoolMatrix(usize, usize),
}

impl A2lDataType {
    /// Get the size in bytes
    pub fn size(&self) -> usize {
        match self {
            // Scalar types
            A2lDataType::U8 | A2lDataType::I8 | A2lDataType::Bool => 1,
            A2lDataType::U16 | A2lDataType::I16 => 2,
            A2lDataType::U32 | A2lDataType::I32 | A2lDataType::F32 => 4,
            A2lDataType::U64 | A2lDataType::I64 | A2lDataType::F64 => 8,
            // Array types
            A2lDataType::U8Array(n) | A2lDataType::I8Array(n) | A2lDataType::BoolArray(n) => *n,
            A2lDataType::U16Array(n) | A2lDataType::I16Array(n) => *n * 2,
            A2lDataType::U32Array(n) | A2lDataType::I32Array(n) | A2lDataType::F32Array(n) => *n * 4,
            A2lDataType::U64Array(n) | A2lDataType::I64Array(n) | A2lDataType::F64Array(n) => *n * 8,
            // Matrix types
            A2lDataType::U8Matrix(r, c) | A2lDataType::I8Matrix(r, c) | A2lDataType::BoolMatrix(r, c) => r * c,
            A2lDataType::U16Matrix(r, c) | A2lDataType::I16Matrix(r, c) => r * c * 2,
            A2lDataType::U32Matrix(r, c) | A2lDataType::I32Matrix(r, c) | A2lDataType::F32Matrix(r, c) => r * c * 4,
            A2lDataType::U64Matrix(r, c) | A2lDataType::I64Matrix(r, c) | A2lDataType::F64Matrix(r, c) => r * c * 8,
        }
    }

    /// Get type name string
    pub fn type_name(&self) -> String {
        match self {
            // Scalar types
            A2lDataType::U8 => "U8".to_string(),
            A2lDataType::U16 => "U16".to_string(),
            A2lDataType::U32 => "U32".to_string(),
            A2lDataType::U64 => "U64".to_string(),
            A2lDataType::I8 => "I8".to_string(),
            A2lDataType::I16 => "I16".to_string(),
            A2lDataType::I32 => "I32".to_string(),
            A2lDataType::I64 => "I64".to_string(),
            A2lDataType::F32 => "F32".to_string(),
            A2lDataType::F64 => "F64".to_string(),
            A2lDataType::Bool => "BOOL".to_string(),
            // Array types
            A2lDataType::U8Array(n) => format!("U8[{}]", n),
            A2lDataType::U16Array(n) => format!("U16[{}]", n),
            A2lDataType::U32Array(n) => format!("U32[{}]", n),
            A2lDataType::U64Array(n) => format!("U64[{}]", n),
            A2lDataType::I8Array(n) => format!("I8[{}]", n),
            A2lDataType::I16Array(n) => format!("I16[{}]", n),
            A2lDataType::I32Array(n) => format!("I32[{}]", n),
            A2lDataType::I64Array(n) => format!("I64[{}]", n),
            A2lDataType::F32Array(n) => format!("F32[{}]", n),
            A2lDataType::F64Array(n) => format!("F64[{}]", n),
            A2lDataType::BoolArray(n) => format!("BOOL[{}]", n),
            // Matrix types
            A2lDataType::U8Matrix(r, c) => format!("U8[{}][{}]", r, c),
            A2lDataType::U16Matrix(r, c) => format!("U16[{}][{}]", r, c),
            A2lDataType::U32Matrix(r, c) => format!("U32[{}][{}]", r, c),
            A2lDataType::U64Matrix(r, c) => format!("U64[{}][{}]", r, c),
            A2lDataType::I8Matrix(r, c) => format!("I8[{}][{}]", r, c),
            A2lDataType::I16Matrix(r, c) => format!("I16[{}][{}]", r, c),
            A2lDataType::I32Matrix(r, c) => format!("I32[{}][{}]", r, c),
            A2lDataType::I64Matrix(r, c) => format!("I64[{}][{}]", r, c),
            A2lDataType::F32Matrix(r, c) => format!("F32[{}][{}]", r, c),
            A2lDataType::F64Matrix(r, c) => format!("F64[{}][{}]", r, c),
            A2lDataType::BoolMatrix(r, c) => format!("BOOL[{}][{}]", r, c),
        }
    }

    /// Check if this is a float type
    pub fn is_float(&self) -> bool {
        matches!(self, A2lDataType::F32 | A2lDataType::F64)
    }

    /// Check if this is an integer type
    pub fn is_integer(&self) -> bool {
        !self.is_float() && !matches!(self, A2lDataType::Bool)
    }
}

/// Characteristic type from A2L file
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacteristicType {
    /// Scalar value (VALUE)
    Value,
    /// 1D lookup table with axis (CURVE)
    Curve,
    /// 2D lookup table with row and column axes (MAP)
    Map,
    /// Value block - array/matrix without axis semantics (VAL_BLK)
    ValBlk,
}

impl CharacteristicType {
    /// Get the string representation for JSON serialization
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Value => "VALUE",
            Self::Curve => "CURVE",
            Self::Map => "MAP",
            Self::ValBlk => "VAL_BLK",
        }
    }
}

/// Storage order for matrix/map data in memory
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StorageOrder {
    /// Row-major order: data stored row by row
    #[default]
    RowDir,
    /// Column-major order: data stored column by column
    ColumnDir,
}

impl StorageOrder {
    /// Get the string representation for JSON serialization
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RowDir => "ROW_DIR",
            Self::ColumnDir => "COLUMN_DIR",
        }
    }
}
