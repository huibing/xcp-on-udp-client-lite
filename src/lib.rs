//! XCP Client Library
//!
//! A Rust implementation of XCP Master for calibration and DAQ.

pub mod protocol;
pub mod transport;
pub mod client;
pub mod calibration;
pub mod daq;

pub use client::XcpClient;
pub use calibration::{CalibrationParam, CalibrationValue, CalibrationType, CalibrationManager};
pub use daq::{DaqManager, Measurement};
