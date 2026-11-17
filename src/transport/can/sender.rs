//! The send half of an XCP-on-CAN link.

use super::device::CanFrame;
use super::shared::CanShared;
use crate::transport::error::TransportError;
use crate::transport::link::TransportSender;
use crate::transport::TransportType;
use async_trait::async_trait;
use std::sync::Arc;

/// Sends XCP payloads as single CAN frames.
pub struct CanSender {
    shared: Arc<CanShared>,
    /// CAN ID used for master → slave commands.
    can_id: u32,
    /// True when `can_id` uses a 29-bit identifier.
    extended: bool,
    /// True when CAN-FD framing is used.
    fd: bool,
    /// Pad command frames to the maximum DLC (`MAX_DLC_REQUIRED`).
    max_dlc_required: bool,
    addr: String,
}

impl CanSender {
    pub(super) fn new(
        shared: Arc<CanShared>,
        can_id: u32,
        fd: bool,
        max_dlc_required: bool,
        addr: String,
    ) -> Self {
        Self {
            shared,
            can_id,
            extended: can_id > 0x7FF,
            fd,
            max_dlc_required,
            addr,
        }
    }

    /// Transmit a normalized XCP payload as a single CAN frame.
    ///
    /// Unlike XCP-on-Ethernet there is no length/counter header: the CAN DLC carries the length
    /// and the XCP packet starts at data byte 0. When `MAX_DLC_REQUIRED` is set the frame is
    /// padded to the maximum DLC.
    fn send_frame(&self, payload: &[u8]) -> Result<(), TransportError> {
        if payload.is_empty() {
            return Err(TransportError::send(
                &self.addr,
                "refusing to send an empty XCP payload",
            ));
        }
        if !self.fd && payload.len() > 8 {
            return Err(TransportError::send(
                &self.addr,
                format!(
                    "XCP payload of {} bytes does not fit in a classic CAN frame",
                    payload.len()
                ),
            ));
        }

        // `MAX_DLC_REQUIRED` requires fixed-size command frames; unused bytes are filled with
        // 0x00 (max DLC is 8 for classic CAN, 64 for CAN-FD).
        let padded = if self.max_dlc_required {
            let target = if self.fd { 64 } else { 8 };
            if payload.len() < target {
                let mut buf = payload.to_vec();
                buf.resize(target, 0x00);
                Some(buf)
            } else {
                None
            }
        } else {
            None
        };
        let payload = padded.as_deref().unwrap_or(payload);

        tracing::trace!(
            "Sending XCP-on-CAN frame: id=0x{:X}, fd={}, len={}, data=[{:02X?}]",
            self.can_id,
            self.fd,
            payload.len(),
            payload
        );

        let frame = CanFrame {
            id: self.can_id,
            extended: self.extended,
            data: payload.to_vec(),
            timestamp_us: None,
        };
        self.shared.send_frame(&frame, &self.addr)
    }
}

#[async_trait]
impl TransportSender for CanSender {
    async fn send(&self, payload: &[u8]) -> Result<(), TransportError> {
        self.send_frame(payload)
    }

    async fn close(&self) -> Result<(), TransportError> {
        tracing::trace!("Closing CAN link to {}", self.addr);
        self.shared.shutdown();
        Ok(())
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Can
    }

    fn target_addr(&self) -> String {
        self.addr.clone()
    }
}