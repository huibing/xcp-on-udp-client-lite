//! XCP Client Library
//!
//! A Rust implementation of XCP Master for calibration and DAQ.

pub mod a2l;
pub mod calibration;
pub mod client;
pub mod daq;
pub mod protocol;
pub mod transport; // A2L module with COMPU_METHOD support
                   // pub mod a2l_reader;  // DEPRECATED: Will be removed, use `a2l` module instead
pub mod checksum;
pub mod hex;
pub mod slave;
pub mod variable;
pub mod web;

pub use calibration::{
    a2l_characteristic_to_param, a2l_type_to_calibration, matrix_to_flat, CalibrationManager,
    CalibrationParam, CalibrationType, CalibrationValue,
};
pub use client::XcpClient;
pub use daq::{DaqConfiguration, DaqManager, DaqRecord, Measurement, OdtEntryConfig};
// Use types from a2l module
pub use a2l::{
    A2lCharacteristic, A2lDataType, A2lMeasurement, A2lMemorySegment, A2lReader, AxisInfo,
    AxisPointSource, CharacteristicType, CompuMethod, CompuVtabEntry, DaqEvent, StorageOrder,
    XcpChecksum, XcpPage, XcpSegmentInfo,
};
pub use slave::{
    CalibrationWriteError, PageMode, SegmentPageState, SlaveError, SlaveEvent, SlaveId, SlaveInfo,
    XcpSlave,
};
pub use variable::{
    DeviceVariable, VariableFilter, VariableSearchManager, VariableSummary, VariableType,
};

// Re-export for TUI use
pub use calibration::CalibrationParam as CalParam;
pub use daq::Measurement as DaqMeasurement;
