//! A2L File Parser and COMPU_METHOD Converter
//!
//! This module provides:
//! - A2L file parsing (ASAM MCD-2 MC standard)
//! - COMPU_METHOD conversion between raw and physical values
//! - Support for IDENTICAL, LINEAR, TAB_VERB, and COMPU_VTAB conversion types

pub mod compute_method;
pub mod reader;
pub mod types;

mod typedef_flatten;

// Re-export commonly used types
pub use compute_method::{CompuMethod, CompuMethodFactory, CompuVtabEntry};
pub use reader::{
    A2lCharacteristic, A2lMeasurement, A2lMemorySegment, A2lReader, A2lTransportConfig, AxisInfo,
    AxisPointSource, XcpChecksum, XcpPage, XcpSegmentInfo,
};
pub use types::{A2lDataType, CharacteristicType, DaqEvent, StorageOrder};
