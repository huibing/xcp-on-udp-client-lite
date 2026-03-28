//! XCP Client Library
//!
//! A Rust implementation of XCP Master for calibration and DAQ.

pub mod protocol;
pub mod transport;
pub mod client;
pub mod calibration;
pub mod daq;
pub mod a2l_reader;
pub mod web;

pub use client::XcpClient;
pub use calibration::{CalibrationParam, CalibrationValue, CalibrationType, CalibrationManager};
pub use daq::{DaqManager, Measurement, DaqRecord, DaqConfiguration, OdtEntryConfig, CompuMethod};
pub use a2l_reader::{A2lReader, A2lMeasurement, A2lCharacteristic, A2lDataType};

// Re-export for TUI use
pub use calibration::CalibrationParam as CalParam;
pub use daq::Measurement as DaqMeasurement;
