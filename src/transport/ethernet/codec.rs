//! XCP-on-Ethernet framing shared by UDP and TCP.
//!
//! Both transports use the same 4-byte header:
//! - LEN (2 bytes, little-endian): length of the XCP payload
//! - PACKET_COUNTER (2 bytes, little-endian): packet sequence counter (wraps at 65535)
//!
//! UDP delivers datagrams (possibly holding several frames); TCP delivers a byte stream (frames
//! may be split or coalesced). [`EthFrameParser`] serves both.

use std::collections::VecDeque;

/// Size of the XCP-on-Ethernet transport header.
pub const ETH_HEADER_SIZE: usize = 4;

/// Build one XCP-on-Ethernet frame.
pub fn build_frame(payload: &[u8], counter: u16) -> Vec<u8> {
    let len = payload.len() as u16;
    let mut frame = Vec::with_capacity(ETH_HEADER_SIZE + payload.len());
    frame.push((len & 0xFF) as u8);
    frame.push((len >> 8) as u8);
    frame.push((counter & 0xFF) as u8);
    frame.push((counter >> 8) as u8);
    frame.extend_from_slice(payload);
    frame
}

/// A gap in the peer's packet counter sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CounterGap {
    pub expected: u16,
    pub received: u16,
    pub gap: u16,
}

/// Tracks incoming packet counters without exposing them outside the transport.
#[derive(Debug, Default)]
pub struct ReceivedCounterTracker {
    last_received: Option<u16>,
}

impl ReceivedCounterTracker {
    /// Observe one counter value; returns a gap description when the sequence is discontinuous.
    pub fn observe(&mut self, received: u16) -> Option<CounterGap> {
        let gap = self.last_received.and_then(|previous| {
            let expected = previous.wrapping_add(1);
            let wrapped = previous == u16::MAX && received == 0;
            if received == expected || wrapped {
                None
            } else {
                Some(CounterGap {
                    expected,
                    received,
                    gap: received.wrapping_sub(expected),
                })
            }
        });

        self.last_received = Some(received);
        gap
    }
}

/// Incremental frame parser: feed bytes in, pop normalized payloads out.
#[derive(Debug, Default)]
pub struct EthFrameParser {
    buf: Vec<u8>,
    pending: VecDeque<Vec<u8>>,
    counter: ReceivedCounterTracker,
}

impl EthFrameParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pop the next complete payload, if any.
    pub fn pop(&mut self) -> Option<Vec<u8>> {
        self.pending.pop_front()
    }

    /// Feed one UDP datagram. A datagram may carry several frames; trailing bytes that do not
    /// form a complete frame are dropped (datagram boundaries are authoritative).
    pub fn push_datagram(&mut self, datagram: &[u8]) {
        self.buf.clear();
        self.buf.extend_from_slice(datagram);
        self.drain_frames();

        if !self.buf.is_empty() {
            tracing::warn!(
                "Discarding {} trailing byte(s) of an incomplete XCP-on-UDP frame",
                self.buf.len()
            );
            self.buf.clear();
        }
    }

    /// Feed a chunk of a TCP byte stream; emits every frame that became complete.
    pub fn push_stream(&mut self, chunk: &[u8]) {
        self.buf.extend_from_slice(chunk);
        self.drain_frames();
    }

    fn drain_frames(&mut self) {
        while let Some((counter, total_len)) = take_frame(&self.buf) {
            if let Some(gap) = self.counter.observe(counter) {
                tracing::warn!(
                    "Slave packet counter discontinuity: expected {}, received {} (gap: {})",
                    gap.expected,
                    gap.received,
                    gap.gap
                );
            }

            self.pending
                .push_back(self.buf[ETH_HEADER_SIZE..total_len].to_vec());
            self.buf.drain(..total_len);
        }
    }
}

/// Split one complete frame off the front of `buf`, returning `(counter, total frame length)`.
fn take_frame(buf: &[u8]) -> Option<(u16, usize)> {
    if buf.len() < ETH_HEADER_SIZE {
        return None;
    }

    let len = u16::from_le_bytes([buf[0], buf[1]]) as usize;
    let counter = u16::from_le_bytes([buf[2], buf[3]]);
    let total_len = ETH_HEADER_SIZE + len;

    if buf.len() < total_len {
        return None;
    }

    Some((counter, total_len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_frame_adds_header() {
        let payload = vec![0xFF, 0x01, 0x02];
        assert_eq!(
            build_frame(&payload, 0x1234),
            vec![0x03, 0x00, 0x34, 0x12, 0xFF, 0x01, 0x02]
        );
    }

    #[test]
    fn test_datagram_with_multiple_frames() {
        let datagram = vec![
            0x04, 0x00, 0x01, 0x00, 0xFF, 0xAA, 0xBB, 0xCC, // counter 1, 4 bytes
            0x03, 0x00, 0x02, 0x00, 0xC0, 0x10, 0x11, // counter 2, 3 bytes
        ];

        let mut parser = EthFrameParser::new();
        parser.push_datagram(&datagram);

        assert_eq!(parser.pop(), Some(vec![0xFF, 0xAA, 0xBB, 0xCC]));
        assert_eq!(parser.pop(), Some(vec![0xC0, 0x10, 0x11]));
        assert_eq!(parser.pop(), None);
    }

    #[test]
    fn test_datagram_drops_incomplete_trailing_frame() {
        let datagram = vec![
            0x01, 0x00, 0x01, 0x00, 0xFF, // counter 1, 1 byte
            0x04, 0x00, 0x02, 0x00, 0xC0, 0x10, // counter 2 declares 4 bytes, only 2 present
        ];

        let mut parser = EthFrameParser::new();
        parser.push_datagram(&datagram);

        assert_eq!(parser.pop(), Some(vec![0xFF]));
        assert_eq!(parser.pop(), None);
    }

    #[test]
    fn test_stream_handles_split_and_coalesced_frames() {
        let mut combined = build_frame(&[0xFF], 1);
        combined.extend_from_slice(&build_frame(&[0xC0, 0x11], 2));

        let mut parser = EthFrameParser::new();

        // Split inside the first frame's header.
        parser.push_stream(&combined[..3]);
        assert_eq!(parser.pop(), None);

        parser.push_stream(&combined[3..]);
        assert_eq!(parser.pop(), Some(vec![0xFF]));
        assert_eq!(parser.pop(), Some(vec![0xC0, 0x11]));
        assert_eq!(parser.pop(), None);
    }

    #[test]
    fn test_counter_gap_detection_and_wrap() {
        let mut tracker = ReceivedCounterTracker::default();
        assert_eq!(tracker.observe(1), None);
        assert_eq!(tracker.observe(2), None);

        let gap = tracker.observe(5).expect("gap detected");
        assert_eq!(gap.expected, 3);
        assert_eq!(gap.received, 5);
        assert_eq!(gap.gap, 2);

        // Wrap-around is not a gap.
        let mut tracker = ReceivedCounterTracker::default();
        tracker.observe(u16::MAX);
        assert_eq!(tracker.observe(0), None);
    }
}