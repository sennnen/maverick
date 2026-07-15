//! Property tests: whatever fragmentation the radio produces and whatever garbage sits between
//! frames, every valid frame is recovered exactly and in order, and single-bit corruption is
//! always caught by the payload CRC.
// Tests are allowed to panic; the workspace-level denies apply to library code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use mav_frame::frame::{build_frame, WireFormat};
use mav_frame::reassembler::{Reassembler, ReassemblyEvent};
use proptest::prelude::*;

fn padded_gen5(payload: &[u8]) -> Vec<u8> {
    let mut expected = payload.to_vec();
    expected.resize(payload.len().div_ceil(4) * 4, 0);
    expected
}

fn recovered(events: &[ReassemblyEvent]) -> Vec<Vec<u8>> {
    events
        .iter()
        .filter_map(|e| match e {
            ReassemblyEvent::Frame(f) => Some(f.payload.clone()),
            _ => None,
        })
        .collect()
}

fn format_strategy() -> impl Strategy<Value = WireFormat> {
    prop_oneof![Just(WireFormat::Gen4), Just(WireFormat::Gen5)]
}

proptest! {
    #[test]
    fn frames_survive_arbitrary_fragmentation(
        format in format_strategy(),
        payloads in prop::collection::vec(prop::collection::vec(any::<u8>(), 1..300), 1..6),
        cut_seed in any::<u64>(),
    ) {
        let mut wire = Vec::new();
        for p in &payloads {
            wire.extend(build_frame(format, p).unwrap());
        }

        let mut r = Reassembler::new(format);
        let mut events = Vec::new();
        let mut state = cut_seed;
        let mut rest = wire.as_slice();
        while !rest.is_empty() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let take = 1 + (state >> 33) as usize % rest.len().min(23);
            let (chunk, tail) = rest.split_at(take.min(rest.len()));
            events.extend(r.push(chunk));
            rest = tail;
        }

        let expected: Vec<Vec<u8>> = payloads
            .iter()
            .map(|p| match format {
                WireFormat::Gen4 => p.clone(),
                WireFormat::Gen5 => padded_gen5(p),
            })
            .collect();
        prop_assert_eq!(recovered(&events), expected);
        prop_assert_eq!(r.pending(), 0);
    }

    #[test]
    fn garbage_between_frames_never_loses_a_frame(
        format in format_strategy(),
        payloads in prop::collection::vec(prop::collection::vec(any::<u8>(), 1..100), 1..4),
        garbage in prop::collection::vec(
            prop::collection::vec(any::<u8>().prop_filter("not SOF", |&b| b != 0xAA), 0..30),
            1..5,
        ),
    ) {
        let mut wire = Vec::new();
        for (i, p) in payloads.iter().enumerate() {
            wire.extend(&garbage[i % garbage.len()]);
            wire.extend(build_frame(format, p).unwrap());
        }

        let mut r = Reassembler::new(format);
        let events = r.push(&wire);
        let expected: Vec<Vec<u8>> = payloads
            .iter()
            .map(|p| match format {
                WireFormat::Gen4 => p.clone(),
                WireFormat::Gen5 => padded_gen5(p),
            })
            .collect();
        prop_assert_eq!(recovered(&events), expected);
    }

    #[test]
    fn single_bit_flip_in_payload_is_always_detected(
        format in format_strategy(),
        payload in prop::collection::vec(any::<u8>(), 4..200),
        bit in any::<u32>(),
    ) {
        let mut wire = build_frame(format, &payload).unwrap();
        // Flip one bit somewhere in the payload region only, then expect zero recovered frames.
        let payload_start = format.header_len();
        let payload_end = wire.len() - 4;
        let span_bits = (payload_end - payload_start) * 8;
        let target = bit as usize % span_bits;
        wire[payload_start + target / 8] ^= 1 << (target % 8);

        let mut r = Reassembler::new(format);
        let events = r.push(&wire);
        prop_assert!(recovered(&events).is_empty(), "corrupted frame was accepted: {events:?}");
    }
}
