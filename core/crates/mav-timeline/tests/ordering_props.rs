//! The ordering property from M1-P6: for any shuffled batch, the drained series is ordered by
//! time and the set of surviving samples does not depend on insertion order.
// Tests are allowed to panic; the workspace-level denies apply to library code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use mav_model::ids::MetadataId;
use mav_model::raw::RawValue;
use mav_model::stream::{Placement, Quality, Sample, StreamKind};
use mav_model::time::DeviceTime;
use mav_timeline::Timeline;
use proptest::prelude::*;

fn sample(nanos: i64, ms: u16, seq: u16) -> Sample<RawValue> {
    Sample {
        kind: StreamKind::RrInterval,
        device_time: DeviceTime::from_nanos(nanos),
        placement: Placement::Unplaced,
        seq,
        value: RawValue::U16(ms),
        quality: Quality::scored(1.0),
        provenance: MetadataId::new(0),
    }
}

proptest! {
    #[test]
    fn ordering_is_stable_and_input_order_free(
        raw in prop::collection::vec((0i64..10, 300u16..900, 0u16..3), 1..60),
        rotate_by in any::<usize>(),
    ) {
        let batch: Vec<_> = raw.iter().map(|&(t, ms, seq)| sample(t, ms, seq)).collect();

        let mut forward = Timeline::new();
        for s in &batch {
            forward.insert(*s);
        }

        let mut rotated_batch = batch.clone();
        rotated_batch.rotate_left(rotate_by % batch.len().max(1));
        let mut rotated = Timeline::new();
        for s in &rotated_batch {
            rotated.insert(*s);
        }

        let a = forward.drain_ordered();
        let b = rotated.drain_ordered();

        prop_assert_eq!(&a, &b, "surviving samples must not depend on insertion order");
        for pair in a.windows(2) {
            prop_assert!(
                pair[0].device_time <= pair[1].device_time,
                "series must be time-ordered"
            );
        }
    }
}
