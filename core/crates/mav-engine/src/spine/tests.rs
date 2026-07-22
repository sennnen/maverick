//! The spine's contract: pinned values from a golden fixture day, rebuildability after dropping
//! the derived table, cache short-circuiting, and honest availability on a day with no evidence.

use super::*;
use crate::recompute::OffsetSpan;
use mav_model::stream::Quality;
use mav_model::time::DeviceTime;

/// 2025-07-16T00:00:00Z, a Wednesday, chosen so the fixture day is unambiguous under UTC.
const MIDNIGHT_NS: i64 = 1_752_624_000 * 1_000_000_000;

fn utc() -> Timezone {
    Timezone::fixed("UTC", 0)
}

/// Five hours behind UTC. The fixture day runs 00:00–00:29 UTC, so under this zone the whole
/// window belongs to the *previous* local day — the clearest demonstration that the platform's
/// spans, not the sample's timestamp, decide which day a reading counts towards.
fn behind() -> Timezone {
    Timezone::new(
        "America/New_York",
        vec![OffsetSpan {
            start_unix_seconds: i64::MIN / 2,
            offset_seconds: -5 * 3_600,
        }],
    )
    .expect("one span is a valid zone")
}

fn sample(kind: StreamKind, wall_ns: i64, value: RawValue, seq: u16) -> Sample<RawValue> {
    Sample {
        kind,
        device_time: DeviceTime::from_nanos(wall_ns),
        wall_time: Some(WallTime::from_nanos(wall_ns)),
        seq,
        value,
        quality: Quality::scored(1.0),
        provenance: MetadataId::new(1),
    }
}

/// A day shaped like real hardware: heart rate once a minute, and intervals in bursts — a short
/// four-beat burst most minutes, plus one sustained sixty-beat run. Values alternate 900/950 ms so
/// every successive difference inside a run is exactly 50 ms.
///
/// The bursts are the point. On a real strap they arrive minutes apart, and differencing the last
/// beat of one against the first of the next is not a beat-to-beat change; a day-wide calculation
/// over these samples reports an RMSSD roughly ten times the truth.
fn seed_day(store: &Store, device: DeviceId, day_start_ns: i64) {
    let write_interval = |at: i64, index: i64| {
        let interval = if index % 2 == 0 { 900u16 } else { 950 };
        store
            .insert_sample(
                device,
                &sample(StreamKind::RrInterval, at, RawValue::U16(interval), 0),
            )
            .expect("interval");
    };

    for minute in 0..30i64 {
        let at = day_start_ns + minute * 60 * 1_000_000_000;
        store
            .insert_sample(
                device,
                &sample(
                    StreamKind::HeartRate,
                    at,
                    RawValue::U8(58 + (minute % 5) as u8),
                    minute as u16,
                ),
            )
            .expect("heart rate");
        for beat in 0..4i64 {
            write_interval(at + beat * 1_000_000_000, beat);
        }
    }

    // One sustained run, an hour into the day, one beat per second.
    let run_start = day_start_ns + 3_600 * 1_000_000_000;
    for beat in 0..60i64 {
        write_interval(run_start + beat * 1_000_000_000, beat);
    }
}

fn fixture() -> (Store, DeviceId, LocalDay) {
    let store = Store::open_in_memory().expect("store");
    let device = DeviceId::new(7);
    seed_day(&store, device, MIDNIGHT_NS);
    let day = LocalDay::of(WallTime::from_nanos(MIDNIGHT_NS), &utc());
    (store, device, day)
}

#[test]
fn a_golden_day_produces_pinned_values() {
    let (store, device, day) = fixture();
    let spine = Spine::new(utc());
    let snapshot = spine.compute(&store, device, day).expect("compute");

    assert_eq!(snapshot.day, "2025-07-16");
    assert_eq!(snapshot.day_index, day.index());

    // 30 heart-rate samples cycling 58..=62, so the mean is exactly 60.
    assert_eq!(snapshot.heart_rate.sample_count, 30);
    assert_eq!(snapshot.heart_rate.excluded_count, 0);
    assert_eq!(snapshot.heart_rate.mean_bpm, Some(60.0));
    // "Current" is the latest by device time: minute 29, so 58 + (29 % 5) = 62.
    assert_eq!(snapshot.heart_rate.current_bpm, Some(62));

    // Variability comes from the longest run of genuinely successive beats — the sixty-beat one,
    // not the day's 180 scattered intervals. Alternating 900/950 gives a mean of 925 and a
    // successive difference of exactly 50 ms every time.
    let hrv = snapshot
        .hrv
        .as_ref()
        .expect("a day with a sustained run has variability");
    assert_eq!(
        hrv.interval_count, 60,
        "the four-beat bursts must not be spliced into the sustained run"
    );
    assert_eq!(hrv.excluded_count, 0);
    assert_eq!(hrv.mean_interval_ms, 925.0);
    assert_eq!(hrv.rmssd_ms, 50.0);
    assert_eq!(hrv.nn50_count, 0);
    // Optical intervals: the analytic must label this PRV, never HRV.
    assert_eq!(hrv.source, IntervalSource::Ppg);
    assert_eq!(hrv.label, "pulse_rate_variability");

    // Both algorithms that ran are stamped, so a stored row can be told from an older build's.
    let ids: Vec<&str> = snapshot
        .algorithms
        .iter()
        .map(|stamp| stamp.id.as_str())
        .collect();
    assert!(ids.contains(&HR_FEATURE_ALGORITHM));
    assert!(ids.contains(&HRV_ALGORITHM));
}

/// The failure real hardware exposed: RR arrives in bursts minutes apart, and treating a day of
/// them as one series differences beats that never followed one another. Against a live WHOOP MG
/// capture that reported an RMSSD of 476 ms — roughly ten times any plausible value.
#[test]
fn intervals_from_separate_bursts_are_never_differenced_against_each_other() {
    let store = Store::open_in_memory().expect("store");
    let device = DeviceId::new(7);
    // Two beats a second apart, then a burst three minutes later. The within-burst difference is
    // 50 ms; the across-burst one would be 600 ms.
    for (offset_ms, interval) in [(0i64, 900u16), (1_000, 950), (180_000, 350), (181_000, 400)] {
        store
            .insert_sample(
                device,
                &sample(
                    StreamKind::RrInterval,
                    MIDNIGHT_NS + offset_ms * 1_000_000,
                    RawValue::U16(interval),
                    0,
                ),
            )
            .expect("interval");
    }

    let spine = Spine::new(utc());
    let day = LocalDay::of(WallTime::from_nanos(MIDNIGHT_NS), &utc());
    let snapshot = spine.compute(&store, device, day).expect("compute");

    // Two beats is not a variance estimate, and neither burst reaches the analytic's minimum, so
    // the honest answer is no reading at all — not a number assembled across the gap.
    assert!(
        snapshot.hrv.is_none(),
        "two-beat bursts must not be spliced into a four-interval series"
    );
}

/// One night is not a baseline. Readiness stays absent rather than reporting a tier from a single
/// night's RMSSD, which is the ADR-005 refusal applied to a longitudinal metric.
#[test]
fn readiness_is_absent_until_it_has_enough_nights() {
    let (store, device, day) = fixture();
    let spine = Spine::new(utc());
    assert_eq!(
        spine
            .compute(&store, device, day)
            .expect("compute")
            .readiness,
        None
    );
}

/// The property the derived table is defined by: drop every row, recompute, get the same answer.
/// Without it, an algorithm change would be a migration instead of a rebuild.
#[test]
fn dropping_the_derived_table_and_recomputing_reproduces_it() {
    let (store, device, day) = fixture();
    let mut spine = Spine::new(utc());

    let first = spine.snapshot(&store, device, day, 1_000).expect("first");
    let stored = store
        .daily_snapshot(device, day.index())
        .expect("read")
        .expect("a row was written");

    assert_eq!(store.clear_daily_snapshots(device).expect("clear"), 1);
    assert_eq!(
        store.daily_snapshot(device, day.index()).expect("read"),
        None
    );

    let mut rebuilt = Spine::new(utc());
    let second = rebuilt
        .snapshot(&store, device, day, 2_000)
        .expect("rebuild");
    assert_eq!(first, second);
    assert_eq!(
        store
            .daily_snapshot(device, day.index())
            .expect("read")
            .expect("rebuilt row"),
        stored,
        "the rebuilt row is byte-identical, so the table is derived and not authoritative"
    );
}

/// A cache hit must not touch the store. Proven by deleting every sample first: a recomputation
/// would report an empty day, and the cached answer does not.
#[test]
fn a_cache_hit_short_circuits_recomputation() {
    let (store, device, day) = fixture();
    let mut spine = Spine::new(utc());
    let first = spine.snapshot(&store, device, day, 1_000).expect("first");
    assert!(first.hrv.is_some());

    let empty = Store::open_in_memory().expect("empty store");
    let cached = spine.snapshot(&empty, device, day, 2_000).expect("cached");
    assert_eq!(cached, first, "the cache answered without reading samples");

    // Dirtying the day evicts it, and the next read sees the empty store honestly.
    let mut days = AffectedDays::default();
    days.insert(day);
    assert_eq!(spine.invalidate(&days).len(), 1);
    let recomputed = spine
        .snapshot(&empty, device, day, 3_000)
        .expect("recompute");
    assert_eq!(recomputed.hrv, None);
    assert_eq!(recomputed.heart_rate.sample_count, 0);
}

/// A day with no evidence reports every analytic unavailable with the reason that is actually
/// true, rather than reporting nothing at all.
#[test]
fn an_empty_day_reports_why_each_analytic_is_unavailable() {
    let store = Store::open_in_memory().expect("store");
    let spine = Spine::new(utc());
    let day = LocalDay::of(WallTime::from_nanos(MIDNIGHT_NS), &utc());
    let snapshot = spine
        .compute(&store, DeviceId::new(7), day)
        .expect("compute");

    assert!(snapshot.hrv.is_none());
    assert!(!snapshot.availability.is_empty());
    assert!(
        snapshot.availability.iter().all(|entry| !entry.available),
        "no evidence means nothing is available"
    );
    assert!(
        snapshot
            .availability
            .iter()
            .all(|entry| entry.reason.is_some()),
        "and every absence carries its reason"
    );
}

/// The zone is the platform's to supply, and it decides which day a reading counts towards. A
/// sample at 00:15 UTC belongs to the previous local day five hours behind.
#[test]
fn the_supplied_offset_decides_which_day_a_sample_belongs_to() {
    let store = Store::open_in_memory().expect("store");
    let device = DeviceId::new(7);
    let at = MIDNIGHT_NS + 900 * 1_000_000_000; // 00:15 UTC
    store
        .insert_sample(
            device,
            &sample(StreamKind::HeartRate, at, RawValue::U8(61), 0),
        )
        .expect("sample");

    let utc_day = LocalDay::of(WallTime::from_nanos(at), &utc());
    let behind_day = LocalDay::of(WallTime::from_nanos(at), &behind());
    assert_eq!(behind_day.index(), utc_day.index() - 1);

    let in_utc = Spine::new(utc());
    assert_eq!(
        in_utc
            .compute(&store, device, utc_day)
            .expect("utc")
            .heart_rate
            .sample_count,
        1
    );
    let shifted = Spine::new(behind());
    assert_eq!(
        shifted
            .compute(&store, device, utc_day)
            .expect("behind")
            .heart_rate
            .sample_count,
        0,
        "five hours behind, the reading belongs to the previous local day"
    );
    assert_eq!(
        shifted
            .compute(&store, device, behind_day)
            .expect("behind")
            .heart_rate
            .sample_count,
        1
    );
}

/// Replacing the spans must not leave a day cached under the old ones, because the day a sample
/// belongs to may have moved.
#[test]
fn changing_the_zone_drops_every_cached_day() {
    let (store, device, day) = fixture();
    let mut spine = Spine::new(utc());
    assert!(spine
        .snapshot(&store, device, day, 1_000)
        .expect("seed")
        .hrv
        .is_some());

    spine.set_timezone(behind());
    let after = spine.snapshot(&store, device, day, 2_000).expect("after");
    assert_eq!(
        after.heart_rate.sample_count, 0,
        "five hours behind, the whole fixture window moved to the previous local day"
    );
    assert_eq!(after.hrv, None);
}
