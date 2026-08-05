//! `astd_event_detection 0.1.0` — sustained stress and recovery events from daytime stress bins.
//!
//! The input is a day of fifteen-minute bins, each holding a stress score in `[-1, +1]`:
//! negative is stressed, positive is restored. The output is the stretches long enough and
//! clean enough to call an event, with their durations.
//!
//! An event starts as a *window*: four consecutive bins that between them cover 55 to 65
//! minutes. Both endpoints must be present, at most one bin inside may be missing, every
//! present bin must belong to the event's own band, and at least one must be past the extreme
//! threshold rather than merely borderline. Both directions are scanned independently over
//! every starting position, so overlapping windows are normal and expected.
//!
//! Overlapping windows of the same kind are then merged when they are no more than thirty
//! minutes apart, which is what turns a run of four-bin windows back into one event of the
//! length a person would recognise. Two events of *opposite* kind may never overlap; the
//! archive raises on that rather than resolving it, and so does this.
//!
//! The duration is the span between the first and last bin *plus one bin width*, because a
//! bin is a quarter hour of elapsed time rather than an instant. A single four-bin window is
//! 45 minutes of span and a 60-minute event.

/// The stress score at or beyond which a bin is unambiguous.
const EXTREME_THRESHOLD: f64 = 0.5;

/// Between this and [`EXTREME_THRESHOLD`] a bin counts, but only alongside an extreme one.
const RELAXED_THRESHOLD: f64 = 0.4;

/// One bin, in milliseconds.
const BIN_WIDTH_MS: i64 = 900_000;

/// One bin, in minutes — what a window's span is extended by to become a duration.
const BIN_WIDTH_MINUTES: f64 = 15.0;

/// How many consecutive bins make a window.
const WINDOW_BINS: usize = 4;

/// A window's covered span must fall inside these, so a window with a missing bin either side
/// of it is not silently stretched into a longer stretch of time than it measured.
const MIN_WINDOW_SPAN_MS: i64 = 3_300_000;
const MAX_WINDOW_SPAN_MS: i64 = 3_900_000;

/// Two windows of the same kind no further apart than this become one event.
const MERGE_GAP_MS: i64 = 1_800_000;

/// At most this many bins inside a window may be missing.
const MAX_MISSING_PER_WINDOW: usize = 1;

/// Fewer bins than this and there is nothing to segment.
const MIN_BINS: usize = 4;

/// Which way a bin leans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// Sustained stress. The archive's type id is -1.
    Stressed,
    /// Sustained recovery. The archive's type id is +1.
    Restored,
}

impl EventKind {
    /// The archive's own type id.
    pub fn id(self) -> i32 {
        match self {
            Self::Stressed => -1,
            Self::Restored => 1,
        }
    }
}

/// Why the archive refused the input, with the code it refuses under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventDetectionError {
    /// 1 — fewer bins than a single window needs.
    NotEnoughBins,
    /// 2 — the values and the timestamps are different lengths.
    LengthMismatch,
    /// 3 — the timestamps do not strictly increase.
    TimestampsNotIncreasing,
    /// 4 — a present value falls outside `[-1, +1]`.
    ValueOutOfRange,
    /// 5 — two events of opposite kind overlap on the time axis.
    OppositeEventsOverlap,
}

impl EventDetectionError {
    /// The archive's own code for this refusal.
    pub fn code(self) -> u8 {
        match self {
            Self::NotEnoughBins => 1,
            Self::LengthMismatch => 2,
            Self::TimestampsNotIncreasing => 3,
            Self::ValueOutOfRange => 4,
            Self::OppositeEventsOverlap => 5,
        }
    }
}

/// One detected event.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Event {
    /// Which way it leans.
    pub kind: EventKind,
    /// Timestamp of the first bin, in milliseconds.
    pub start_ms: i64,
    /// Timestamp of the last bin, in milliseconds.
    pub end_ms: i64,
    /// Elapsed minutes, the span plus one bin width.
    pub duration_minutes: f64,
}

/// Everything the archive returns for one day.
#[derive(Debug, Clone, PartialEq)]
pub struct EventSummary {
    /// The events, ordered by start time.
    pub events: Vec<Event>,
    /// How many of them are stressed.
    pub stressed_count: i32,
    /// How many of them are restored.
    pub restored_count: i32,
    /// Total stressed minutes across the day.
    pub stressed_minutes: f64,
    /// Total restored minutes across the day.
    pub restored_minutes: f64,
}

/// How one bin sits relative to an event kind's thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Band {
    Missing,
    Extreme,
    Borderline,
    Other,
}

fn band(value: f64, kind: EventKind) -> Band {
    if value.is_nan() {
        return Band::Missing;
    }
    let signed = match kind {
        // Reading the stressed side through a sign flip keeps the two passes literally the
        // same comparison, which is what the archive does with two mirrored expressions.
        EventKind::Stressed => -value,
        EventKind::Restored => value,
    };
    if signed >= EXTREME_THRESHOLD {
        Band::Extreme
    } else if signed >= RELAXED_THRESHOLD {
        Band::Borderline
    } else {
        Band::Other
    }
}

fn validate(values: &[f64], timestamps: &[i64]) -> Result<(), EventDetectionError> {
    if values.len() < MIN_BINS {
        return Err(EventDetectionError::NotEnoughBins);
    }
    if values.len() != timestamps.len() {
        return Err(EventDetectionError::LengthMismatch);
    }
    if timestamps.windows(2).any(|pair| pair[1] <= pair[0]) {
        return Err(EventDetectionError::TimestampsNotIncreasing);
    }
    if values
        .iter()
        .any(|value| !value.is_nan() && (*value < -1.0 || *value > 1.0))
    {
        return Err(EventDetectionError::ValueOutOfRange);
    }
    Ok(())
}

/// Every accepted window of one kind, in start order.
fn collect(values: &[f64], timestamps: &[i64], kind: EventKind) -> Vec<(i64, i64)> {
    let mut windows = Vec::new();
    if values.len() < WINDOW_BINS {
        return windows;
    }
    for first in 0..=values.len() - WINDOW_BINS {
        let last = first + WINDOW_BINS - 1;
        // Both ends must be present: a window anchored on a missing bin does not know when it
        // started or stopped, whatever is inside it.
        if values[first].is_nan() || values[last].is_nan() {
            continue;
        }
        let span = timestamps[last] - timestamps[first] + BIN_WIDTH_MS;
        if !(MIN_WINDOW_SPAN_MS..=MAX_WINDOW_SPAN_MS).contains(&span) {
            continue;
        }
        let mut missing = 0;
        let mut extreme = 0;
        let mut other = 0;
        for value in &values[first..=last] {
            match band(*value, kind) {
                Band::Missing => missing += 1,
                Band::Extreme => extreme += 1,
                Band::Borderline => {}
                Band::Other => other += 1,
            }
        }
        // Nothing outside the band, at most one gap, and at least one bin that is not merely
        // borderline — a whole window of borderline readings is not an event.
        if other == 0 && missing <= MAX_MISSING_PER_WINDOW && extreme > 0 {
            windows.push((timestamps[first], timestamps[last]));
        }
    }
    windows
}

/// Detect the day's sustained stress and recovery events.
pub fn detect_events(
    values: &[f64],
    timestamps: &[i64],
) -> Result<EventSummary, EventDetectionError> {
    validate(values, timestamps)?;

    let mut windows: Vec<(EventKind, i64, i64)> = Vec::new();
    for kind in [EventKind::Stressed, EventKind::Restored] {
        windows.extend(
            collect(values, timestamps, kind)
                .into_iter()
                .map(|(start, end)| (kind, start, end)),
        );
    }
    // Stable by start: the stressed pass is collected first, so a stressed and a restored
    // window beginning on the same bin keep that order, as the archive's insertion sort does.
    windows.sort_by_key(|(_, start, _)| *start);

    let mut events: Vec<Event> = Vec::new();
    for (kind, start, end) in windows {
        match events.last_mut() {
            Some(last) if last.kind == kind && start - last.end_ms <= MERGE_GAP_MS => {
                last.end_ms = last.end_ms.max(end);
            }
            _ => events.push(Event {
                kind,
                start_ms: start,
                end_ms: end,
                duration_minutes: 0.0,
            }),
        }
    }

    for pair in events.windows(2) {
        if pair[0].kind != pair[1].kind && pair[1].start_ms <= pair[0].end_ms {
            return Err(EventDetectionError::OppositeEventsOverlap);
        }
    }

    let mut summary = EventSummary {
        events: Vec::new(),
        stressed_count: 0,
        restored_count: 0,
        stressed_minutes: 0.0,
        restored_minutes: 0.0,
    };
    for mut event in events {
        // A bin is a quarter hour of elapsed time, not an instant, so the last bin contributes
        // its own width beyond its timestamp.
        event.duration_minutes =
            (event.end_ms - event.start_ms) as f64 / 60_000.0 + BIN_WIDTH_MINUTES;
        match event.kind {
            EventKind::Stressed => {
                summary.stressed_count += 1;
                summary.stressed_minutes += event.duration_minutes;
            }
            EventKind::Restored => {
                summary.restored_count += 1;
                summary.restored_minutes += event.duration_minutes;
            }
        }
        summary.events.push(event);
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: i64 = 1_700_000_000_000;
    const TOLERANCE: f64 = 1e-9;

    fn grid(count: usize) -> Vec<i64> {
        (0..count).map(|i| BASE + i as i64 * BIN_WIDTH_MS).collect()
    }

    #[test]
    fn a_run_of_overlapping_windows_becomes_one_event() {
        let values = [-0.8, -0.7, -0.6, -0.9, -0.45];
        let got = detect_events(&values, &grid(values.len())).expect("valid day");
        assert_eq!(got.events.len(), 1);
        assert_eq!(got.stressed_count, 1);
        // Five bins: four of span, plus the last bin's own width.
        assert!((got.events[0].duration_minutes - 75.0).abs() < TOLERANCE);
    }

    #[test]
    fn one_missing_bin_is_tolerated_and_two_are_not() {
        let mut values = [-0.8, -0.7, -0.6, -0.9, -0.8, -0.7];
        values[2] = f64::NAN;
        assert_eq!(
            detect_events(&values, &grid(values.len()))
                .expect("valid day")
                .stressed_count,
            1
        );
        values[1] = f64::NAN;
        assert_eq!(
            detect_events(&values, &grid(values.len()))
                .expect("valid day")
                .stressed_count,
            0
        );
    }

    #[test]
    fn a_window_of_only_borderline_bins_is_not_an_event() {
        let values = [-0.45, -0.42, -0.48, -0.41];
        assert!(detect_events(&values, &grid(4))
            .expect("valid day")
            .events
            .is_empty());
        // One extreme bin among them is enough.
        let values = [-0.45, -0.42, -0.55, -0.41];
        assert_eq!(
            detect_events(&values, &grid(4))
                .expect("valid day")
                .stressed_count,
            1
        );
    }

    #[test]
    fn a_gap_in_the_timestamps_stretches_the_window_past_its_limit() {
        let values = [-0.8, -0.7, -0.6, -0.9];
        let stretched = vec![
            BASE,
            BASE + BIN_WIDTH_MS,
            BASE + 2 * BIN_WIDTH_MS,
            BASE + 4 * BIN_WIDTH_MS,
        ];
        assert!(detect_events(&values, &stretched)
            .expect("valid day")
            .events
            .is_empty());
    }

    #[test]
    fn runs_further_apart_than_the_merge_gap_stay_separate() {
        let values = [
            -0.8, -0.7, -0.6, -0.9, 0.0, 0.0, 0.0, -0.8, -0.7, -0.6, -0.9,
        ];
        let got = detect_events(&values, &grid(values.len())).expect("valid day");
        assert_eq!(got.stressed_count, 2);
        assert!((got.stressed_minutes - 120.0).abs() < TOLERANCE);
    }

    #[test]
    fn refuses_the_inputs_the_archive_refuses() {
        assert_eq!(
            detect_events(&[-0.8, -0.7, -0.6], &grid(3)),
            Err(EventDetectionError::NotEnoughBins)
        );
        assert_eq!(
            detect_events(&[-1.4, -0.7, -0.6, -0.9], &grid(4)),
            Err(EventDetectionError::ValueOutOfRange)
        );
        assert_eq!(
            detect_events(
                &[-0.8, -0.7, -0.6, -0.9],
                &[BASE, BASE + 1, BASE + 1, BASE + 2]
            ),
            Err(EventDetectionError::TimestampsNotIncreasing)
        );
        assert_eq!(
            detect_events(&[-0.8, -0.7, -0.6, -0.9], &grid(5)),
            Err(EventDetectionError::LengthMismatch)
        );
    }

    #[test]
    fn the_band_edges_are_where_the_archive_puts_them() {
        assert_eq!(band(-0.5, EventKind::Stressed), Band::Extreme);
        assert_eq!(band(-0.4999, EventKind::Stressed), Band::Borderline);
        assert_eq!(band(-0.4, EventKind::Stressed), Band::Borderline);
        assert_eq!(band(-0.3999, EventKind::Stressed), Band::Other);
        assert_eq!(band(0.5, EventKind::Restored), Band::Extreme);
        assert_eq!(band(-0.8, EventKind::Restored), Band::Other);
    }

    /// Vectors generated by `tools/ml/deterministic_vectors.py astd_event_detection_0_1_0`.
    #[test]
    fn matches_the_archive_on_generated_vectors() {
        let raw = include_str!(
            "../../../../../../artifacts/models/vectors/astd_event_detection_0_1_0.json"
        );
        let file: serde_json::Value =
            serde_json::from_str(raw).expect("the vector file should parse");
        let mut produced = 0;
        let mut refused = 0;
        for vector in file["vectors"]
            .as_array()
            .expect("vectors should be a list")
        {
            let inputs = &vector["inputs"];
            let values: Vec<f64> = inputs["dsa_values"]
                .as_array()
                .expect("values should be a list")
                .iter()
                // A missing bin is written as null, which is what NaN became on the way out.
                .map(|v| v.as_f64().unwrap_or(f64::NAN))
                .collect();
            let timestamps: Vec<i64> = inputs["dsa_timestamps_ms"]
                .as_array()
                .expect("timestamps should be a list")
                .iter()
                .map(|v| v.as_i64().expect("timestamp should be an integer"))
                .collect();
            let got = detect_events(&values, &timestamps);
            match vector.get("error").and_then(|e| e.as_str()) {
                Some(message) => {
                    let error = got.expect_err("the archive refused this input");
                    assert!(
                        message.starts_with(&format!("builtins.Exception: {}", error.code())),
                        "expected code {} for {message}",
                        error.code()
                    );
                    refused += 1;
                }
                None => {
                    let got = got.expect("the archive produced a summary");
                    let want = vector["outputs"].as_array().expect("outputs are a list");
                    let scalar = |index: usize| want[index][0].as_f64().expect("a number");
                    assert_eq!(got.stressed_count as f64, scalar(0), "stressed count");
                    assert_eq!(got.restored_count as f64, scalar(1), "restored count");
                    assert!(
                        (got.stressed_minutes - scalar(2)).abs() < TOLERANCE,
                        "stressed minutes {} vs {}",
                        got.stressed_minutes,
                        scalar(2)
                    );
                    assert!(
                        (got.restored_minutes - scalar(3)).abs() < TOLERANCE,
                        "restored minutes {} vs {}",
                        got.restored_minutes,
                        scalar(3)
                    );
                    let ids = want[4].as_array().expect("ids are a list");
                    let starts = want[5].as_array().expect("starts are a list");
                    let ends = want[6].as_array().expect("ends are a list");
                    assert_eq!(got.events.len(), ids.len(), "event count");
                    for (event, index) in got.events.iter().zip(0..) {
                        assert_eq!(
                            f64::from(event.kind.id()),
                            ids[index].as_f64().expect("an id"),
                            "event {index} kind"
                        );
                        assert_eq!(
                            event.start_ms,
                            starts[index].as_i64().expect("a start"),
                            "event {index} start"
                        );
                        assert_eq!(
                            event.end_ms,
                            ends[index].as_i64().expect("an end"),
                            "event {index} end"
                        );
                    }
                    produced += 1;
                }
            }
        }
        assert_eq!(
            (produced, refused),
            (6, 3),
            "six summaries and three refusals"
        );
    }
}
