//! Streaming reassembly: BLE notifications arrive as arbitrary fragments, and this turns them
//! back into validated frames. Nothing is dropped silently; garbage and CRC failures come back as
//! events so the caller can log them with their error codes.

use crate::crc::{crc16_modbus, crc32, crc8};
use crate::frame::{RawFrame, WireFormat, START_OF_FRAME};
use mav_model::error::{codes, MavError};

const DEFAULT_MAX_FRAME_BYTES: usize = 8192;

#[derive(Clone, PartialEq, Debug)]
pub enum ReassemblyEvent {
    /// A frame whose header and payload CRCs both checked out.
    Frame(RawFrame),
    /// Bytes discarded while scanning for a start-of-frame marker.
    SkippedGarbage { bytes: usize },
    /// A start-of-frame that failed validation. One byte was consumed and scanning resumed, so a
    /// real frame that happened to contain 0xAA in its body is still recovered.
    InvalidFrame(MavError),
}

pub struct Reassembler {
    format: WireFormat,
    max_frame_bytes: usize,
    buf: Vec<u8>,
    pos: usize,
}

impl Reassembler {
    pub fn new(format: WireFormat) -> Self {
        Self::with_max_frame_bytes(format, DEFAULT_MAX_FRAME_BYTES)
    }

    pub fn with_max_frame_bytes(format: WireFormat, max_frame_bytes: usize) -> Self {
        Self {
            format,
            max_frame_bytes,
            buf: Vec::new(),
            pos: 0,
        }
    }

    /// Bytes buffered but not yet resolved into an event.
    pub fn pending(&self) -> usize {
        self.buf.len() - self.pos
    }

    /// Drop any partial frame, e.g. on disconnect. Returns how many bytes were discarded so the
    /// caller can log the loss rather than swallow it.
    pub fn reset(&mut self) -> usize {
        let discarded = self.pending();
        self.buf.clear();
        self.pos = 0;
        discarded
    }

    /// Feed one fragment and collect everything that resolves. Incomplete trailing frames stay
    /// buffered for the next call.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<ReassemblyEvent> {
        self.buf.extend_from_slice(chunk);
        let mut events = Vec::new();

        loop {
            let skipped = self.buf[self.pos..]
                .iter()
                .take_while(|&&b| b != START_OF_FRAME)
                .count();
            if skipped > 0 {
                self.pos += skipped;
                events.push(ReassemblyEvent::SkippedGarbage { bytes: skipped });
            }

            let avail = self.buf.len() - self.pos;
            if avail < self.format.header_len() {
                break;
            }

            let head = &self.buf[self.pos..];
            let (declared, header_ok) = match self.format {
                WireFormat::Gen4 => {
                    let declared = u16::from_le_bytes([head[1], head[2]]);
                    (declared, crc8(&head[1..3]) == head[3])
                }
                WireFormat::Gen5 => {
                    let declared = u16::from_le_bytes([head[2], head[3]]);
                    let stored = u16::from_le_bytes([head[6], head[7]]);
                    (declared, crc16_modbus(&head[0..6]) == stored)
                }
            };

            if !header_ok {
                events.push(ReassemblyEvent::InvalidFrame(
                    MavError::new(codes::FRAME_HEADER_CRC_MISMATCH, "header crc mismatch")
                        .context(format!("declared_len {declared}")),
                ));
                self.pos += 1;
                continue;
            }

            if usize::from(declared) < 4 {
                events.push(ReassemblyEvent::InvalidFrame(
                    MavError::new(
                        codes::FRAME_TRUNCATED,
                        "declared length shorter than its crc32",
                    )
                    .context(format!("declared_len {declared}")),
                ));
                self.pos += 1;
                continue;
            }

            let total = self.format.header_len() + usize::from(declared);
            if total > self.max_frame_bytes {
                events.push(ReassemblyEvent::InvalidFrame(
                    MavError::new(codes::FRAME_OVERSIZED, "frame exceeds maximum size")
                        .context(format!("total {total}, max {}", self.max_frame_bytes)),
                ));
                self.pos += 1;
                continue;
            }

            if avail < total {
                break;
            }

            let frame = &self.buf[self.pos..self.pos + total];
            let payload = &frame[self.format.header_len()..total - 4];
            let stored = u32::from_le_bytes([
                frame[total - 4],
                frame[total - 3],
                frame[total - 2],
                frame[total - 1],
            ]);
            if crc32(payload) != stored {
                events.push(ReassemblyEvent::InvalidFrame(
                    MavError::new(codes::FRAME_PAYLOAD_CRC_MISMATCH, "payload crc32 mismatch")
                        .context(format!("payload_len {}", payload.len())),
                ));
                self.pos += 1;
                continue;
            }

            events.push(ReassemblyEvent::Frame(RawFrame {
                payload: payload.to_vec(),
            }));
            self.pos += total;
        }

        if self.pos > 0 {
            self.buf.drain(..self.pos);
            self.pos = 0;
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::build_frame;

    fn frames_of(events: &[ReassemblyEvent]) -> Vec<Vec<u8>> {
        events
            .iter()
            .filter_map(|e| match e {
                ReassemblyEvent::Frame(f) => Some(f.payload.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn whole_frame_in_one_push() {
        for format in [WireFormat::Gen4, WireFormat::Gen5] {
            let wire = build_frame(format, &[0x28, 0x01, 0x00, 0x42]).unwrap();
            let mut r = Reassembler::new(format);
            let events = r.push(&wire);
            assert_eq!(frames_of(&events), vec![vec![0x28, 0x01, 0x00, 0x42]]);
            assert_eq!(r.pending(), 0);
        }
    }

    #[test]
    fn frame_split_across_single_byte_pushes() {
        let wire = build_frame(WireFormat::Gen5, &[0x23, 0x01, 0x91, 0x01]).unwrap();
        let mut r = Reassembler::new(WireFormat::Gen5);
        let mut collected = Vec::new();
        for &b in &wire {
            collected.extend(r.push(&[b]));
        }
        assert_eq!(frames_of(&collected), vec![vec![0x23, 0x01, 0x91, 0x01]]);
    }

    #[test]
    fn garbage_before_frame_is_reported_and_skipped() {
        let mut wire = vec![0x00, 0x13, 0x37];
        wire.extend(build_frame(WireFormat::Gen4, &[0x30, 0x02, 0x05]).unwrap());
        let mut r = Reassembler::new(WireFormat::Gen4);
        let events = r.push(&wire);
        assert_eq!(events[0], ReassemblyEvent::SkippedGarbage { bytes: 3 });
        assert_eq!(frames_of(&events), vec![vec![0x30, 0x02, 0x05]]);
    }

    #[test]
    fn corrupted_payload_is_rejected_and_next_frame_recovered() {
        let mut bad = build_frame(WireFormat::Gen4, &[0x28, 0x01, 0x00, 0x42]).unwrap();
        let last = bad.len() - 5;
        bad[last] ^= 0x01;
        let good = build_frame(WireFormat::Gen4, &[0x28, 0x02, 0x00, 0x43]).unwrap();
        let mut wire = bad;
        wire.extend(&good);

        let mut r = Reassembler::new(WireFormat::Gen4);
        let events = r.push(&wire);
        let invalid: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                ReassemblyEvent::InvalidFrame(err) => Some(err.code),
                _ => None,
            })
            .collect();
        assert!(
            invalid.contains(&codes::FRAME_PAYLOAD_CRC_MISMATCH),
            "{events:?}"
        );
        assert_eq!(frames_of(&events), vec![vec![0x28, 0x02, 0x00, 0x43]]);
    }

    #[test]
    fn corrupted_header_is_rejected() {
        let mut wire = build_frame(WireFormat::Gen5, &[0x23, 0x01, 0x91, 0x01]).unwrap();
        wire[6] ^= 0xFF;
        let mut r = Reassembler::new(WireFormat::Gen5);
        let events = r.push(&wire);
        assert!(matches!(
            events.first(),
            Some(ReassemblyEvent::InvalidFrame(err)) if err.code == codes::FRAME_HEADER_CRC_MISMATCH
        ));
        assert!(frames_of(&events).is_empty());
    }

    #[test]
    fn oversized_declared_length_is_rejected_not_awaited() {
        let mut r = Reassembler::with_max_frame_bytes(WireFormat::Gen4, 64);
        let mut header = vec![START_OF_FRAME];
        let declared = 1000u16.to_le_bytes();
        header.extend_from_slice(&declared);
        header.push(crc8(&declared));
        let events = r.push(&header);
        assert!(matches!(
            events.first(),
            Some(ReassemblyEvent::InvalidFrame(err)) if err.code == codes::FRAME_OVERSIZED
        ));
    }

    #[test]
    fn reset_reports_discarded_bytes() {
        let wire = build_frame(WireFormat::Gen4, &[0x28, 0x01, 0x00, 0x42]).unwrap();
        let mut r = Reassembler::new(WireFormat::Gen4);
        r.push(&wire[..5]);
        assert_eq!(r.reset(), 5);
        assert_eq!(r.pending(), 0);
    }
}
