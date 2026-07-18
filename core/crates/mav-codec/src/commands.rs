//! Outbound device-control command builders (WHOOP-P9, `[WRS]`): alarm and haptic. Each is a pure
//! function that returns the ready-to-write frame bytes for one generation — the WHOOP command inner
//! `[COMMAND, seq, opcode] + body`, wrapped by [`mav_frame::frame::build_frame`] for that wire. These
//! are builders only: the runtime has no outbound-command send lane yet, so nothing here writes to a
//! device. When that lane lands, it enqueues these bytes as a `TransportAction::Write`.
//!
//! Confidence: the haptic buzz opcodes and the alarm command bodies are the upstream's validated
//! layouts (`tanarchytan/whoop-rs`). The **alarm is EXPERIMENTAL/UNCONFIRMED** — the upstream flags
//! that its `SET_ALARM_TIME` body has not been confirmed to actually wake a strap — so a caller must
//! surface it as experimental, never as a guaranteed wake.

use mav_frame::frame::{build_frame, WireFormat};
use mav_model::error::Result;

/// The COMMAND packet type on both WHOOP wires. A command inner is `[COMMAND, seq, opcode] + body`.
const COMMAND_PACKET_TYPE: u8 = 35;

/// Wake-alarm opcodes.
const OP_SET_ALARM_TIME: u8 = 66;
const OP_DISABLE_ALARM: u8 = 69;

/// Haptic opcodes. gen4 drives the generic pattern runner; gen5/MG uses the one-shot "maverick" preset.
const OP_RUN_HAPTIC_PATTERN_MAVERICK: u8 = 19;
const OP_RUN_HAPTICS_PATTERN: u8 = 79;

/// Wrap `[COMMAND, seq, opcode] + body` into a complete outbound frame for `wire`.
fn command_frame(wire: WireFormat, seq: u8, opcode: u8, body: &[u8]) -> Result<Vec<u8>> {
    let mut inner = Vec::with_capacity(3 + body.len());
    inner.push(COMMAND_PACKET_TYPE);
    inner.push(seq);
    inner.push(opcode);
    inner.extend_from_slice(body);
    build_frame(wire, &inner)
}

// ---- Haptics -------------------------------------------------------------------------------------

/// The gen5/MG one-shot buzz body: RUN_HAPTIC_PATTERN_MAVERICK with the notification preset (the same
/// 47/152 waveform-effect pair the wake alarm uses).
const GEN5_BUZZ_BODY: [u8; 12] = [0x01, 47, 152, 0, 0, 0, 0, 0, 0, 0, 0, 0];

/// A ready-to-write gen5/MG one-shot haptic buzz.
pub fn haptic_buzz_gen5(seq: u8) -> Result<Vec<u8>> {
    command_frame(
        WireFormat::Gen5,
        seq,
        OP_RUN_HAPTIC_PATTERN_MAVERICK,
        &GEN5_BUZZ_BODY,
    )
}

/// The gen4 RUN_HAPTICS_PATTERN body (5 bytes): `[pattern_id, loops, 0, 0, 0]`. Pattern 2 is the
/// graduated alarm buzz; `loops` is how many times it repeats.
pub fn run_haptics_pattern_body(pattern_id: u8, loops: u8) -> [u8; 5] {
    [pattern_id, loops, 0, 0, 0]
}

/// A ready-to-write gen4 haptic-pattern command.
pub fn haptic_pattern_gen4(seq: u8, pattern_id: u8, loops: u8) -> Result<Vec<u8>> {
    command_frame(
        WireFormat::Gen4,
        seq,
        OP_RUN_HAPTICS_PATTERN,
        &run_haptics_pattern_body(pattern_id, loops),
    )
}

// ---- Wake alarm (EXPERIMENTAL/UNCONFIRMED) -------------------------------------------------------

/// gen5/MG SET_ALARM_TIME REVISION_4 body (20 bytes), all multi-byte fields little-endian:
///   `[0]=0x04 [1]=alarm_id [2..6]=u32 epoch s [6..8]=u16 subseconds(ms*32768/1000)`
///   `[8..16]=waveform effects [16..18]=loop control(0) [18]=overall loop(7) [19]=duration(30 s)`.
const ALARM_OVERALL_LOOP: u8 = 7;
const ALARM_DURATION_SECONDS: u8 = 30;
const ALARM_WAVEFORM_EFFECTS: [u8; 8] = [47, 152, 0, 0, 0, 0, 0, 0];

fn set_alarm_body_gen5(wake_epoch_ms: u64, alarm_id: u8) -> [u8; 20] {
    let seconds = (wake_epoch_ms / 1000) as u32;
    let subseconds = (((wake_epoch_ms % 1000) * 32768) / 1000) as u16;
    let mut out = [0u8; 20];
    out[0] = 4;
    out[1] = alarm_id;
    out[2..6].copy_from_slice(&seconds.to_le_bytes());
    out[6..8].copy_from_slice(&subseconds.to_le_bytes());
    out[8..16].copy_from_slice(&ALARM_WAVEFORM_EFFECTS);
    // out[16..18] loop control stays 0.
    out[18] = ALARM_OVERALL_LOOP;
    out[19] = ALARM_DURATION_SECONDS;
    out
}

/// A ready-to-write gen5/MG wake-alarm set. EXPERIMENTAL/UNCONFIRMED — not confirmed to wake a strap.
pub fn set_alarm_gen5(seq: u8, wake_epoch_ms: u64, alarm_id: u8) -> Result<Vec<u8>> {
    command_frame(
        WireFormat::Gen5,
        seq,
        OP_SET_ALARM_TIME,
        &set_alarm_body_gen5(wake_epoch_ms, alarm_id),
    )
}

/// gen4 SET_ALARM_TIME body (9 bytes): `[0x01][u32 LE epoch s][2 zero subsec][2 zero haptic-mode]`.
/// Minute-precision, so subseconds are always zero.
fn set_alarm_body_gen4(epoch_secs: u32) -> [u8; 9] {
    let mut out = [0u8; 9];
    out[0] = 0x01;
    out[1..5].copy_from_slice(&epoch_secs.to_le_bytes());
    out
}

/// A ready-to-write gen4 wake-alarm set. EXPERIMENTAL/UNCONFIRMED.
pub fn set_alarm_gen4(seq: u8, wake_epoch_secs: u32) -> Result<Vec<u8>> {
    command_frame(
        WireFormat::Gen4,
        seq,
        OP_SET_ALARM_TIME,
        &set_alarm_body_gen4(wake_epoch_secs),
    )
}

/// A ready-to-write gen5/MG DISABLE_ALARM (REVISION_2 body `[0x02, 0xFF]`). EXPERIMENTAL.
pub fn disable_alarm_gen5(seq: u8) -> Result<Vec<u8>> {
    command_frame(WireFormat::Gen5, seq, OP_DISABLE_ALARM, &[0x02, 0xFF])
}

// ---- Haptic clock (pure buzz schedule) ----------------------------------------------------------

// Haptic-Clock pulse/gap timing (ms). Long = a "tens" pulse, short = a "units" pulse.
const HC_LONG_MS: u32 = 550;
const HC_SHORT_MS: u32 = 200;
const HC_INTRA_GAP_MS: u32 = 450;
const HC_GROUP_GAP_MS: u32 = 900;
const HC_BLOCK_GAP_MS: u32 = 1500;

/// One buzz instruction: buzz for `duration_ms`, then stay silent for `gap_ms`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pulse {
    pub duration_ms: u32,
    pub gap_ms: u32,
}

impl Pulse {
    /// A long ("tens") pulse rather than a short ("units") one.
    pub fn is_long(&self) -> bool {
        self.duration_ms >= HC_LONG_MS
    }
}

/// 24h hour → 12-hour dial reading (0 → 12, 13 → 1 … 23 → 11).
pub fn twelve_hour(h24: u32) -> u32 {
    let h = h24 % 12;
    if h == 0 {
        12
    } else {
        h
    }
}

/// Encode `hour:minute` into a Haptic-Clock buzz schedule (order: hour-tens, hour-units, minute-tens,
/// minute-units; long pulses count tens, short pulses count units). The schedule ends on a buzz. This
/// is a pure schedule — sequencing the buzzes over time is a caller's job.
pub fn haptic_clock_pulses(hour: u32, minute: u32, is_24h: bool) -> Vec<Pulse> {
    let h24 = hour.min(23);
    let m = minute.min(59);
    let display_hour = if is_24h { h24 } else { twelve_hour(h24) };

    let mut out = Vec::new();
    append_group(&mut out, display_hour / 10, HC_LONG_MS);
    close_group(&mut out, HC_GROUP_GAP_MS);
    append_group(&mut out, display_hour % 10, HC_SHORT_MS);
    close_group(&mut out, HC_BLOCK_GAP_MS);
    append_group(&mut out, m / 10, HC_LONG_MS);
    close_group(&mut out, HC_GROUP_GAP_MS);
    append_group(&mut out, m % 10, HC_SHORT_MS);

    if let Some(last) = out.last_mut() {
        last.gap_ms = 0; // end on a buzz
    }
    out
}

fn append_group(out: &mut Vec<Pulse>, count: u32, duration_ms: u32) {
    for _ in 0..count {
        out.push(Pulse {
            duration_ms,
            gap_ms: HC_INTRA_GAP_MS,
        });
    }
}

fn close_group(out: &mut [Pulse], gap_ms: u32) {
    if let Some(last) = out.last_mut() {
        last.gap_ms = last.gap_ms.max(gap_ms);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use mav_frame::reassembler::{Reassembler, ReassemblyEvent};
    use mav_frame::spec::FrameSpec;

    /// Reassemble a built command frame back to its inner `[COMMAND, seq, opcode] + body` bytes, so a
    /// test pins the opcode and body independently of the framing internals.
    fn inner_of(frame: &[u8], spec: FrameSpec) -> Vec<u8> {
        let mut re = Reassembler::with_spec(spec);
        for event in re.push(frame) {
            if let ReassemblyEvent::Frame(f) = event {
                return f.payload;
            }
        }
        panic!("frame did not reassemble");
    }

    #[test]
    fn gen5_buzz_carries_the_maverick_opcode_and_preset() {
        let frame = haptic_buzz_gen5(7).unwrap();
        let inner = inner_of(&frame, FrameSpec::gen5());
        assert_eq!(inner[0], COMMAND_PACKET_TYPE);
        assert_eq!(inner[1], 7); // seq
        assert_eq!(inner[2], OP_RUN_HAPTIC_PATTERN_MAVERICK); // 19
        assert_eq!(&inner[3..15], &GEN5_BUZZ_BODY);
    }

    #[test]
    fn gen4_haptic_pattern_carries_pattern_and_loops() {
        let frame = haptic_pattern_gen4(2, 2, 3).unwrap();
        let inner = inner_of(&frame, FrameSpec::gen4());
        assert_eq!(inner[0], COMMAND_PACKET_TYPE);
        assert_eq!(inner[1], 2); // seq
        assert_eq!(inner[2], OP_RUN_HAPTICS_PATTERN); // 79
        assert_eq!(&inner[3..8], &[2, 3, 0, 0, 0]);
    }

    #[test]
    fn haptics_pattern_body_layout() {
        assert_eq!(run_haptics_pattern_body(2, 3), [2, 3, 0, 0, 0]);
    }

    #[test]
    fn gen5_set_alarm_rev4_body_layout() {
        // 1 s past the epoch, alarm 1, subseconds 0.
        let frame = set_alarm_gen5(4, 1_784_000_000_000, 1).unwrap();
        let inner = inner_of(&frame, FrameSpec::gen5());
        assert_eq!(inner[2], OP_SET_ALARM_TIME); // 66
        let body = &inner[3..23];
        assert_eq!(body[0], 4);
        assert_eq!(body[1], 1);
        assert_eq!(&body[2..6], &1_784_000_000u32.to_le_bytes());
        assert_eq!(&body[6..8], &0u16.to_le_bytes());
        assert_eq!(&body[8..16], &ALARM_WAVEFORM_EFFECTS);
        assert_eq!(body[18], ALARM_OVERALL_LOOP);
        assert_eq!(body[19], ALARM_DURATION_SECONDS);
    }

    #[test]
    fn gen5_set_alarm_encodes_subseconds() {
        // 500 ms past a whole second → subseconds = 500*32768/1000 = 16384.
        let frame = set_alarm_gen5(0, 1_784_000_000_500, 2).unwrap();
        let inner = inner_of(&frame, FrameSpec::gen5());
        let body = &inner[3..23];
        assert_eq!(&body[6..8], &16_384u16.to_le_bytes());
    }

    #[test]
    fn gen4_set_alarm_body_layout() {
        let frame = set_alarm_gen4(0, 1_784_000_000).unwrap();
        let inner = inner_of(&frame, FrameSpec::gen4());
        assert_eq!(inner[2], OP_SET_ALARM_TIME);
        let body = &inner[3..12];
        assert_eq!(body[0], 0x01);
        assert_eq!(&body[1..5], &1_784_000_000u32.to_le_bytes());
        assert_eq!(&body[5..], &[0, 0, 0, 0]);
    }

    #[test]
    fn gen5_disable_alarm_rev2_body() {
        let frame = disable_alarm_gen5(9).unwrap();
        let inner = inner_of(&frame, FrameSpec::gen5());
        assert_eq!(inner[2], OP_DISABLE_ALARM); // 69
        assert_eq!(&inner[3..5], &[0x02, 0xFF]);
    }

    #[test]
    fn haptic_clock_three_twentyfive_24h() {
        // 3:25 → hour-units 3 (short×3), minute-tens 2 (long×2), minute-units 5 (short×5) = 10 pulses.
        let p = haptic_clock_pulses(3, 25, true);
        assert_eq!(p.len(), 3 + 2 + 5);
        assert_eq!(p.last().unwrap().gap_ms, 0);
        // The two minute-tens pulses are long ("tens"); the units pulses are short.
        assert_eq!(p.iter().filter(|p| p.is_long()).count(), 2);
    }

    #[test]
    fn haptic_clock_midnight_24h_is_silent() {
        assert!(haptic_clock_pulses(0, 0, true).is_empty());
    }

    #[test]
    fn twelve_hour_dial_reading() {
        assert_eq!(twelve_hour(0), 12);
        assert_eq!(twelve_hour(13), 1);
        assert_eq!(twelve_hour(23), 11);
        assert_eq!(twelve_hour(12), 12);
    }
}
