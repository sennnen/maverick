//! The spine's contract: pinned values from a golden fixture day, rebuildability after dropping
//! the derived tables, honest availability on a day with no evidence, and a read window that stops
//! at the day boundary the platform's offsets define.

use super::*;
use crate::recompute::OffsetSpan;
use mav_model::raw::RawValue;
use mav_model::stream::{Placement, Quality, Sample};
use mav_model::time::DeviceTime;

/// 2025-07-16T00:00:00Z, a Wednesday, chosen so the fixture day is unambiguous under UTC.
const MIDNIGHT_NS: i64 = 1_752_624_000 * 1_000_000_000;

fn utc() -> Timezone {
    Timezone::fixed("UTC", 0)
}

/// Five hours behind UTC. The fixture day runs 00:00–01:01 UTC, so under this zone the whole
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
        placement: Placement::DeviceClock(WallTime::from_nanos(wall_ns)),
        seq,
        value,
        quality: Quality::exact(),
        provenance: MetadataId::new(1),
    }
}

fn write(store: &Store, device: DeviceId, kind: StreamKind, at_ns: i64, value: RawValue) {
    store
        .insert_sample(device, &sample(kind, at_ns, value, 0))
        .expect("insert");
}

/// A day shaped like real hardware: heart rate once a minute, and pulse intervals in bursts — a
/// short four-beat burst most minutes, plus one sustained sixty-beat run. Values alternate 900/950
/// ms so every successive difference inside a run is exactly 50 ms.
///
/// The bursts are the point. On a real strap they arrive minutes apart, and differencing the last
/// beat of one against the first of the next is not a beat-to-beat change; a day-wide calculation
/// over these samples reports an RMSSD roughly ten times the truth.
fn seed_day(store: &Store, device: DeviceId, day_start_ns: i64) {
    let beat = |at: i64, index: i64| {
        let interval = if index % 2 == 0 { 900u16 } else { 950 };
        write(
            store,
            device,
            StreamKind::PulseInterval,
            at,
            RawValue::U16(interval),
        );
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
        for index in 0..4i64 {
            beat(at + index * 1_000_000_000, index);
        }
    }

    // One sustained run, an hour into the day, one beat per second.
    let run_start = day_start_ns + 3_600 * 1_000_000_000;
    for index in 0..60i64 {
        beat(run_start + index * 1_000_000_000, index);
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
    let snapshot = Spine::new(utc())
        .compute(&store, device, day)
        .expect("compute");

    assert_eq!(snapshot.day, "2025-07-16");
    assert_eq!(snapshot.day_index, day.index());

    // 30 heart-rate samples cycling 58..=62, so the mean is exactly 60.
    assert_eq!(snapshot.heart_rate.sample_count, 30);
    assert_eq!(snapshot.heart_rate.excluded_count, 0);
    assert_eq!(snapshot.heart_rate.mean_bpm, Some(60.0));
    // "Current" is the latest by device time: minute 29, so 58 + (29 % 5) = 62.
    assert_eq!(snapshot.heart_rate.current_bpm, Some(62));

    let hrv = snapshot
        .hrv
        .as_ref()
        .expect("a day with sustained beats has variability");
    // Every burst contributes, because differences pool within runs and never across them. The
    // earlier design took only the longest run and threw away two thirds of the day's beats.
    assert_eq!(hrv.interval_count, 30 * 4 + 60);
    assert_eq!(hrv.excluded_count, 0);
    assert_eq!(hrv.mean_interval_ms, 925.0);
    assert_eq!(hrv.rmssd_ms, 50.0);
    assert_eq!(hrv.nn50_count, 0);
    // Optical intervals: the analytic must label this PRV, never HRV.
    assert_eq!(hrv.source, StreamKind::PulseInterval);
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

/// The failure real hardware exposed: beats arrive in bursts minutes apart, and treating a day of
/// them as one series differences beats that never followed one another. Against a live WHOOP MG
/// capture that reported an RMSSD of 476 ms — roughly ten times any plausible value.
#[test]
fn intervals_from_separate_bursts_are_never_differenced_against_each_other() {
    let store = Store::open_in_memory().expect("store");
    let device = DeviceId::new(7);
    // Two beats a second apart, then a burst three minutes later. Both within-burst differences
    // are 50 ms; the across-burst one would be 600 ms and would dominate the root-mean-square.
    for (offset_ms, interval) in [(0i64, 900u16), (1_000, 950), (180_000, 350), (181_000, 400)] {
        write(
            &store,
            device,
            StreamKind::PulseInterval,
            MIDNIGHT_NS + offset_ms * 1_000_000,
            RawValue::U16(interval),
        );
    }

    let day = LocalDay::of(WallTime::from_nanos(MIDNIGHT_NS), &utc());
    let hrv = Spine::new(utc())
        .compute(&store, device, day)
        .expect("compute")
        .hrv
        .expect("four intervals give two honest differences");
    assert_eq!(hrv.interval_count, 4);
    assert_eq!(hrv.rmssd_ms, 50.0, "the 600 ms gap must not appear");
}

/// Electrical beats win when a day holds both, because only they may be called HRV.
#[test]
fn an_electrical_series_outranks_an_optical_one_on_the_same_day() {
    let store = Store::open_in_memory().expect("store");
    let device = DeviceId::new(7);
    for index in 0..10i64 {
        let at = MIDNIGHT_NS + index * 1_000_000_000;
        write(
            &store,
            device,
            StreamKind::PulseInterval,
            at,
            RawValue::U16(if index % 2 == 0 { 900 } else { 950 }),
        );
        write(
            &store,
            device,
            StreamKind::RrInterval,
            at,
            RawValue::U16(if index % 2 == 0 { 800 } else { 820 }),
        );
    }

    let day = LocalDay::of(WallTime::from_nanos(MIDNIGHT_NS), &utc());
    let hrv = Spine::new(utc())
        .compute(&store, device, day)
        .expect("compute")
        .hrv
        .expect("both streams are present");
    assert_eq!(hrv.source, StreamKind::RrInterval);
    assert_eq!(hrv.label, "heart_rate_variability");
    assert_eq!(hrv.rmssd_ms, 20.0);
}

/// The headline gap the audit found: the ECG waveform was decoded on real hardware, modelled, and
/// documented, yet no path turned it into intervals — so the one source that may legitimately be
/// called heart-rate variability could never produce any.
#[test]
fn a_stored_ecg_waveform_becomes_genuine_heart_rate_variability() {
    let store = Store::open_in_memory().expect("store");
    let device = DeviceId::new(7);
    let rate_hz = 100.0;
    let interval_ms = 800.0;
    let beats = 40;

    // A minimal synthetic lead: a sharp deflection once per beat over a quiet baseline.
    let step_ns = (1_000_000_000.0 / rate_hz) as i64;
    let per_beat = (interval_ms * rate_hz / 1_000.0) as i64;
    for index in 0..(beats * per_beat) {
        let phase = (index % per_beat) as f64 - per_beat as f64 / 2.0;
        let counts = 2_048.0 + 900.0 * (-0.5 * (phase / 0.8).powi(2)).exp()
            - 180.0 * (-0.5 * ((phase - 2.5) / 1.0).powi(2)).exp()
            + 250.0 * (-0.5 * ((phase - 25.0) / 4.5).powi(2)).exp();
        write(
            &store,
            device,
            StreamKind::Ecg,
            MIDNIGHT_NS + index * step_ns,
            RawValue::Converted(counts),
        );
    }

    let day = LocalDay::of(WallTime::from_nanos(MIDNIGHT_NS), &utc());
    let hrv = Spine::new(utc())
        .compute(&store, device, day)
        .expect("compute")
        .hrv
        .expect("a detected beat series has variability");
    assert_eq!(hrv.source, StreamKind::RrInterval);
    assert_eq!(
        hrv.label, "heart_rate_variability",
        "electrical beats, so the honest label is HRV"
    );
    assert!(
        (hrv.mean_interval_ms - interval_ms).abs() < 20.0,
        "recovered {} ms, expected {interval_ms}",
        hrv.mean_interval_ms
    );
    assert!(hrv.interval_count as i64 > beats - 4);
}

/// One night is not a baseline. Readiness stays absent rather than reporting a tier from a single
/// night's RMSSD, which is the ADR-005 refusal applied to a longitudinal metric.
#[test]
fn readiness_is_absent_until_it_has_enough_nights() {
    let (store, device, day) = fixture();
    assert_eq!(
        Spine::new(utc())
            .compute(&store, device, day)
            .expect("compute")
            .readiness,
        None
    );
}

/// The look-back reads the memo rather than two months of beats, so the memo has to hold exactly
/// the value a fresh derivation produces — and re-reading must not change the answer.
#[test]
fn the_nightly_memo_records_what_the_day_actually_measured() {
    let (store, device, day) = fixture();
    let spine = Spine::new(utc());
    let first = spine.compute(&store, device, day).expect("compute");

    let remembered = store
        .nightly_variability(device, StreamKind::PulseInterval, day.index(), day.index())
        .expect("read");
    assert_eq!(remembered, vec![(day.index(), Some(50.0))]);
    assert_eq!(spine.compute(&store, device, day).expect("again"), first);
}

/// A sync that lands new beats must not be answered from a stale memo.
#[test]
fn invalidating_a_day_forgets_its_remembered_night() {
    let (store, device, day) = fixture();
    let spine = Spine::new(utc());
    spine.compute(&store, device, day).expect("seed");

    spine.invalidate(&store, device, day, day).expect("forget");
    assert!(store
        .nightly_variability(device, StreamKind::PulseInterval, day.index(), day.index())
        .expect("read")
        .is_empty());
}

/// The property the derived tables are defined by: drop every row, recompute, get the same answer.
/// Without it, an algorithm change would be a migration instead of a rebuild.
#[test]
fn dropping_the_derived_tables_and_recomputing_reproduces_them() {
    let (store, device, day) = fixture();
    let spine = Spine::new(utc());

    let first = spine.snapshot(&store, device, day, 1_000).expect("first");
    let stored = store
        .daily_snapshot(device, day.index())
        .expect("read")
        .expect("a row was written");

    assert!(store.clear_derived(Some(device)).expect("clear") > 0);
    assert_eq!(
        store.daily_snapshot(device, day.index()).expect("read"),
        None
    );

    let second = Spine::new(utc())
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

/// Every read stops at the day boundary. Yesterday's beats must not leak into today's variability,
/// which is the whole reason the read is a range and not a whole stream.
#[test]
fn a_days_computation_reads_only_that_day() {
    let (store, device, day) = fixture();
    // A neighbouring day of wildly different beats, either side.
    for offset_days in [-1i64, 1] {
        for index in 0..10i64 {
            write(
                &store,
                device,
                StreamKind::PulseInterval,
                MIDNIGHT_NS + offset_days * 86_400 * 1_000_000_000 + index * 1_000_000_000,
                RawValue::U16(if index % 2 == 0 { 400 } else { 1_900 }),
            );
        }
    }

    let hrv = Spine::new(utc())
        .compute(&store, device, day)
        .expect("compute")
        .hrv
        .expect("the fixture day still has beats");
    assert_eq!(hrv.interval_count, 30 * 4 + 60);
    assert_eq!(hrv.rmssd_ms, 50.0);
}

/// A day with no evidence reports every analytic unavailable with the reason that is actually
/// true, rather than reporting nothing at all.
#[test]
fn an_empty_day_reports_why_each_analytic_is_unavailable() {
    let store = Store::open_in_memory().expect("store");
    let day = LocalDay::of(WallTime::from_nanos(MIDNIGHT_NS), &utc());
    let snapshot = Spine::new(utc())
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

/// Availability is negotiated against every stream the day holds, not only the ones the snapshot
/// happens to read. Blaming a missing stream that is sitting in the database is a lie the app
/// then renders.
#[test]
fn availability_sees_streams_the_snapshot_does_not_itself_consume() {
    let store = Store::open_in_memory().expect("store");
    let device = DeviceId::new(7);
    for (kind, value) in [
        (StreamKind::HeartRate, RawValue::U8(60)),
        (StreamKind::SkinTemp, RawValue::Converted(33.2)),
        (StreamKind::RespRaw, RawValue::U16(1_200)),
        (StreamKind::PulseInterval, RawValue::U16(900)),
    ] {
        write(&store, device, kind, MIDNIGHT_NS, value);
    }

    let day = LocalDay::of(WallTime::from_nanos(MIDNIGHT_NS), &utc());
    let snapshot = Spine::new(utc())
        .compute(&store, device, day)
        .expect("compute");
    let illness = snapshot
        .availability
        .iter()
        .find(|entry| entry.analytic == mav_analytic::AnalyticId::IllnessRisk)
        .expect("declared");
    assert_eq!(
        illness.reason,
        Some(mav_analytic::UnavailableReason::AlgorithmNotAdmitted),
        "every input is present, so the honest reason is the missing algorithm"
    );
}

/// The zone is the platform's to supply, and it decides which day a reading counts towards. A
/// sample at 00:15 UTC belongs to the previous local day five hours behind.
#[test]
fn the_supplied_offset_decides_which_day_a_sample_belongs_to() {
    let store = Store::open_in_memory().expect("store");
    let device = DeviceId::new(7);
    let at = MIDNIGHT_NS + 900 * 1_000_000_000; // 00:15 UTC
    write(&store, device, StreamKind::HeartRate, at, RawValue::U8(61));

    let utc_day = LocalDay::of(WallTime::from_nanos(at), &utc());
    let behind_day = LocalDay::of(WallTime::from_nanos(at), &behind());
    assert_eq!(behind_day.index(), utc_day.index() - 1);

    let count = |spine: &Spine, day: LocalDay| {
        spine
            .compute(&store, device, day)
            .expect("compute")
            .heart_rate
            .sample_count
    };
    assert_eq!(count(&Spine::new(utc()), utc_day), 1);
    let shifted = Spine::new(behind());
    assert_eq!(
        count(&shifted, utc_day),
        0,
        "five hours behind, the reading belongs to the previous local day"
    );
    assert_eq!(count(&shifted, behind_day), 1);
}

/// Replacing the spans moves day boundaries, so every derived row computed under the old ones has
/// to go rather than be reinterpreted.
#[test]
fn changing_the_zone_discards_every_derived_row() {
    let (store, device, day) = fixture();
    let mut spine = Spine::new(utc());
    assert!(spine
        .snapshot(&store, device, day, 1_000)
        .expect("seed")
        .hrv
        .is_some());

    spine.set_timezone(&store, behind()).expect("rezone");
    assert_eq!(
        store.daily_snapshot(device, day.index()).expect("read"),
        None
    );
    let after = spine.snapshot(&store, device, day, 2_000).expect("after");
    assert_eq!(
        after.heart_rate.sample_count, 0,
        "five hours behind, the whole fixture window moved to the previous local day"
    );
    assert_eq!(after.hrv, None);
}
