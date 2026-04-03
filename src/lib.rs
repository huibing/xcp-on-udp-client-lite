//! XCP Client Library
//!
//! A Rust implementation of XCP Master for calibration and DAQ.

pub mod protocol;
pub mod transport;
pub mod client;
pub mod calibration;
pub mod daq;
pub mod a2l;  // NEW: A2L module with COMPU_METHOD support
pub mod a2l_reader;  // DEPRECATED: Will be removed, use `a2l` module instead
pub mod web;
pub mod slave;
pub mod variable;

pub use client::XcpClient;
pub use calibration::{CalibrationParam, CalibrationValue, CalibrationType, CalibrationManager};
pub use daq::{DaqManager, Measurement, DaqRecord, DaqConfiguration, OdtEntryConfig};
// NEW: Use CompuMethod from a2l module
pub use a2l::{CompuMethod, CompuVtabEntry};
// Keep backward compatibility with a2l_reader
pub use a2l_reader::{A2lReader, A2lMeasurement, A2lCharacteristic, A2lDataType};
pub use slave::{XcpSlave, SlaveId, SlaveInfo, SlaveEvent, SlaveError, SegmentPageState, CalibrationWriteError, PageMode};
pub use variable::{DeviceVariable, VariableType, VariableFilter, VariableSearchManager, VariableSummary};

// Re-export for TUI use
pub use calibration::CalibrationParam as CalParam;
pub use daq::Measurement as DaqMeasurement;
