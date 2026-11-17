//! Resources shared by both CAN halves: the device handle, the reader thread and the frame filter.
//!
//! The reader thread pulls frames off the device, applies [`FrameFilter`] and forwards accepted
//! frames over an unbounded channel. The channel is closed when the receive half is dropped, which
//! is how the thread learns that nobody is listening any more.

use super::device::{CanDevice, CanDeviceError, CanFrame};
use super::CanConfig;
use crate::transport::error::TransportError;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

/// Connection resources shared by [`super::CanSender`] and [`super::CanReceiver`].
///
/// Whichever half is dropped last releases the device and joins the reader thread, so a link stays
/// alive exactly as long as at least one half is alive.
pub struct CanShared {
    device: Arc<Mutex<Box<dyn CanDevice>>>,
    stop: CancellationToken,
    reader: Mutex<Option<JoinHandle<()>>>,
}

impl CanShared {
    pub(super) fn new(
        device: Arc<Mutex<Box<dyn CanDevice>>>,
        stop: CancellationToken,
        reader: JoinHandle<()>,
    ) -> Arc<Self> {
        Arc::new(Self {
            device,
            stop,
            reader: Mutex::new(Some(reader)),
        })
    }

    /// Cancel the reader thread, join it and close the device. Idempotent.
    pub fn shutdown(&self) {
        self.stop.cancel();

        let reader = self
            .reader
            .lock()
            .expect("CAN reader handle mutex poisoned")
            .take();
        if let Some(handle) = reader {
            let _ = handle.join();
        }

        if let Ok(mut device) = self.device.lock() {
            let _ = device.close();
        }
    }

    /// Send one frame through the device.
    pub(super) fn send_frame(&self, frame: &CanFrame, addr: &str) -> Result<(), TransportError> {
        self.device
            .lock()
            .expect("CAN device mutex poisoned")
            .send_frame(frame)
            .map_err(|e| TransportError::send(addr, e))
    }
}

impl Drop for CanShared {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Start the background thread that drains the CAN device receive queue and forwards accepted
/// frames to `tx` in arrival order.
pub(super) fn spawn_reader(
    device: Arc<Mutex<Box<dyn CanDevice>>>,
    filter: FrameFilter,
    tx: UnboundedSender<CanFrame>,
    stop: CancellationToken,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("can-reader".to_string())
        .spawn(move || {
            tracing::debug!("CAN reader thread started");
            'reader: while !stop.is_cancelled() {
                let mut received_any = false;
                loop {
                    let result = {
                        let device = device.lock().expect("CAN device mutex poisoned");
                        device.recv_frame(None)
                    };

                    match result {
                        Ok(Some(frame)) => {
                            received_any = true;
                            if frame.data.is_empty() {
                                // Error/status frame: ignore.
                                continue;
                            }
                            let pid = frame.data[0];
                            if !filter.accepts(frame.id, pid) {
                                tracing::trace!(
                                    "Ignoring CAN frame id=0x{:X} pid=0x{:02X} (filtered out)",
                                    frame.id,
                                    pid
                                );
                                continue;
                            }
                            // Unbounded send: it only fails once the receiver is gone, i.e. the
                            // link was dropped, so stop reading.
                            if tx.send(frame).is_err() {
                                tracing::debug!("CAN receive channel closed");
                                break 'reader;
                            }
                        }
                        Ok(None) => break,
                        Err(error) => {
                            tracing::warn!("CAN receive error: {}", error);
                            break;
                        }
                    }
                }

                // Poll fast when idle; yield briefly when frames are streaming.
                if !received_any {
                    std::thread::sleep(Duration::from_micros(200));
                }
            }
            tracing::debug!("CAN reader thread stopped");
        })
        .expect("failed to spawn CAN reader thread")
}

/// Frame acceptance rule for the reader thread.
///
/// Filtering is joint on CAN ID **and** PID so that command responses (CTO) are never polluted by
/// unrelated bus traffic, while DAQ (DTO) stays lenient enough to support VARIABLE/INCREMENTAL DAQ
/// IDs:
/// - The master's own transmit ID is always rejected (echo suppression).
/// - CTO-class frames (PID >= 0xFC: RES/ERR/EV/SERV) are accepted only from `slave_id` when it is
///   known. A random frame whose first byte happens to be >= 0xFC on any other ID is dropped
///   instead of being mistaken for a command response.
/// - DAQ-class frames (PID < 0xFC) are accepted from the fixed DAQ DTO IDs plus `slave_id`. Per
///   the XCP-on-CAN transport layer specification (§1.1 Addressing), DAQ packets share the
///   slave→master identifier (CAN_ID_SLAVE) unless `DAQ_LIST_CAN_ID` declares separate IDs — so
///   when no fixed DAQ IDs are declared, the allow-list defaults to `slave_id` instead of
///   accepting everything. Accepting all non-master IDs would let unrelated periodic bus traffic
///   (whose first data byte happens to be a valid PID) corrupt DAQ recordings with garbage/zero
///   samples. Only when the slave ID is unknown as well does the DAQ path stay lenient.
pub(super) struct FrameFilter {
    master_id: u32,
    slave_id: Option<u32>,
    /// DAQ DTO allow-list: `slave_id` ∪ fixed DAQ IDs; `None` only when both are unknown (accept
    /// all non-master IDs).
    daq_allowed: Option<HashSet<u32>>,
}

impl FrameFilter {
    pub(super) fn from_config(config: &CanConfig) -> Self {
        let daq_allowed = if config.daq_ids.is_empty() {
            config.slave_id.map(|slave_id| HashSet::from([slave_id]))
        } else {
            let mut set: HashSet<u32> = config.daq_ids.iter().copied().collect();
            if let Some(slave_id) = config.slave_id {
                set.insert(slave_id);
            }
            Some(set)
        };

        Self {
            master_id: config.can_id,
            slave_id: config.slave_id,
            daq_allowed,
        }
    }

    /// Decide whether a received frame should enter the XCP stream.
    ///
    /// `pid` is the first data byte (XCP packet ID). CTO-class frames (PID >= 0xFC) are only
    /// accepted from the declared slave response ID, so unrelated bus traffic can never be
    /// mistaken for a command response. DAQ-class frames (PID < 0xFC) follow the DAQ allow-list
    /// (lenient when DAQ IDs are unknown).
    pub(super) fn accepts(&self, can_id: u32, pid: u8) -> bool {
        if can_id == self.master_id {
            return false; // echo suppression
        }
        if pid >= 0xFC {
            // CTO (RES/ERR/EV/SERV): only from the slave response ID.
            match self.slave_id {
                Some(slave_id) => can_id == slave_id,
                None => true, // slave ID unknown -> cannot filter (lenient)
            }
        } else {
            // DAQ DTO.
            match &self.daq_allowed {
                Some(set) => set.contains(&can_id),
                None => true, // DAQ IDs unknown -> accept (VARIABLE DAQ)
            }
        }
    }

    pub(super) fn describe(&self) -> String {
        let cto = match self.slave_id {
            Some(id) => format!("CTO only from 0x{:X}", id),
            None => "CTO accept-all (CAN_ID_SLAVE unknown)".to_string(),
        };
        let dto = match &self.daq_allowed {
            Some(set) => {
                let mut ids: Vec<String> = set.iter().map(|id| format!("0x{:X}", id)).collect();
                ids.sort();
                format!("DAQ strict [{}]", ids.join(", "))
            }
            None => "DAQ accept-all (CAN_ID_SLAVE unknown)".to_string(),
        };
        format!("{}; {}", cto, dto)
    }
}

/// Map a driver error raised while opening the link.
pub(super) fn map_open_error(addr: &str, error: CanDeviceError) -> TransportError {
    match error {
        CanDeviceError::DriverNotFound(_)
        | CanDeviceError::NoChannel(_)
        | CanDeviceError::UnknownChannel(_)
        | CanDeviceError::Unsupported(_) => TransportError::InvalidAddress(error.to_string()),
        CanDeviceError::OpenFailed(_)
        | CanDeviceError::SendFailed(_)
        | CanDeviceError::RecvFailed(_)
        | CanDeviceError::Driver(_) => TransportError::open(addr, error),
    }
}