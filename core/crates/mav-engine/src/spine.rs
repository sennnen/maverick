//! The analytic spine: stored samples in, one `DailySnapshot` per local day out.
//!
//! This is the path `docs/pipeline.md` always described and nothing walked. It reads samples for a
//! device, buckets them into local days through the platform-supplied `Timezone`, computes the
//! `mav-feature` primitives and the admitted `mav-analytic` values over each day's window, and
//! persists the result in the derived `daily_snapshot` table.
//!
//! Two rules shape it. Availability is negotiated from the streams the day actually holds, so a
//! metric with no evidence is reported unavailable with its reason rather than computed from
//! nothing (ADR-005, ADR-024). And the derived table is rebuildable by construction: dropping every
//! row and recomputing reproduces it, which is what makes an algorithm change a recompute rather
//! than a migration.

use mav_analytic::capability::{negotiate, AnalyticAvailability};
use mav_analytic::hrv::{IntervalSource, TimeDomainHrv, HRV_ALGORITHM, HRV_VERSION};
use mav_analytic::readiness::{HrvReadiness, HrvReadinessResult};
use mav_analytic::time_domain;
use mav_feature::hr::{hr_summary, HrSummary, HR_FEATURE_ALGORITHM, HR_FEATURE_VERSION};
use mav_model::error::Result;
use mav_model::ids::{DeviceId, MetadataId};
use mav_model::raw::RawValue;
use mav_model::stream::{Sample, StreamKind};
use mav_model::time::WallTime;
use mav_model::version::Version;
use mav_store::Store;
use serde::{Deserialize, Serialize};

use crate::recompute::{AffectedDays, CacheKey, LocalDay, RecomputeCache, Timezone};

/// How many trailing days of nightly RMSSD the readiness reading looks back over. The analytic
/// itself decides how many valid nights it needs; this only bounds how far back we read.
const READINESS_WINDOW_DAYS: i64 = 60;

/// The longest wall-clock gap between two interval samples that can still be the same run of
/// beats. Real straps deliver RR in short bursts minutes apart; within a burst the samples are a
/// second or less apart, and across bursts they are tens of seconds. Differencing across a burst
/// boundary is differencing two beats that never followed one another.
const BEAT_RUN_GAP_MS: i64 = 3_000;

/// Every stream the spine reads. A day holding none of them produces a snapshot that says so.
const READ_STREAMS: [StreamKind; 2] = [StreamKind::HeartRate, StreamKind::RrInterval];

/// One algorithm that contributed to a snapshot, stamped so a stored row can be told from one an
/// older build produced.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct AlgorithmStamp {
    pub id: String,
    pub version: String,
}

/// What one local day produced. Every field is either a value the core computed or an explicit
/// statement that it could not; the `availability` list carries the reason for each absence.
/// Frozen in ADR-024.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct DailySnapshot {
    /// The local day, ISO-formatted, as the platforms display it.
    pub day: String,
    /// The same day as the engine's index, so a caller can request the neighbouring one.
    pub day_index: i64,
    pub heart_rate: HrSummary,
    /// Time-domain variability, labelled HRV or PRV by its interval source. Absent when the day
    /// held too few trustworthy intervals; `availability` says which.
    pub hrv: Option<TimeDomainHrv>,
    /// The longitudinal readout over the trailing nights. Absent while calibrating.
    pub readiness: Option<HrvReadinessResult>,
    /// One entry per known analytic, carrying `UnavailableReason` for everything not served.
    pub availability: Vec<AnalyticAvailability>,
    pub algorithms: Vec<AlgorithmStamp>,
}

impl DailySnapshot {
    /// An honest empty day: no samples, so nothing computed and every analytic unavailable for the
    /// reason that is actually true.
    fn empty(day: LocalDay) -> Self {
        Self {
            day: day.to_string(),
            day_index: day.index(),
            heart_rate: hr_summary(&[], MetadataId::new(0)),
            hrv: None,
            readiness: None,
            availability: negotiate(&[]),
            algorithms: Vec::new(),
        }
    }
}

/// The recompute engine over one device's stored samples.
pub struct Spine {
    timezone: Timezone,
    cache: RecomputeCache<DailySnapshot>,
}

impl Spine {
    pub fn new(timezone: Timezone) -> Self {
        Self {
            timezone,
            cache: RecomputeCache::new(),
        }
    }

    pub fn timezone(&self) -> &Timezone {
        &self.timezone
    }

    /// Replace the offset spans. The platforms own the zone database; a change here dirties every
    /// cached day, because the day a sample belongs to may have moved.
    pub fn set_timezone(&mut self, timezone: Timezone) {
        self.timezone = timezone;
        self.cache = RecomputeCache::new();
    }

    /// The local day a wall-clock instant falls in, under the current spans.
    pub fn day_of(&self, at: WallTime) -> LocalDay {
        LocalDay::of(at, &self.timezone)
    }

    /// Drop the cached snapshots for days a sync touched. The stored rows stay: they are still the
    /// last computed answer, and `snapshot` recomputes over them on the next read.
    pub fn invalidate(&mut self, days: &AffectedDays) -> Vec<CacheKey> {
        self.cache.invalidate(days)
    }

    /// The snapshot for one day, from cache if it is there and from a recomputation if not. A
    /// recomputation persists its result.
    pub fn snapshot(
        &mut self,
        store: &Store,
        device: DeviceId,
        day: LocalDay,
        computed_ns: i64,
    ) -> Result<DailySnapshot> {
        let key = cache_key(day);
        if let Some(cached) = self.cache.get(&key) {
            return Ok(cached.clone());
        }
        let snapshot = self.compute(store, device, day)?;
        persist(store, device, day, &snapshot, computed_ns)?;
        self.cache.put(key, snapshot.clone());
        Ok(snapshot)
    }

    /// Compute one day from stored samples, reading and writing no cache. This is the function the
    /// rebuildability property is about: it is a pure function of the stored samples and the spans.
    pub fn compute(&self, store: &Store, device: DeviceId, day: LocalDay) -> Result<DailySnapshot> {
        let mut by_day = DayIndex::default();
        for kind in READ_STREAMS {
            for sample in store.samples(device, kind)? {
                if let Some(at) = sample.wall_time {
                    by_day.push(LocalDay::of(at, &self.timezone), sample);
                }
            }
        }

        let Some(today) = by_day.get(day) else {
            return Ok(DailySnapshot::empty(day));
        };

        let heart_rate = hr_summary(&of_kind(today, StreamKind::HeartRate), provenance(day));

        // Time-domain variability is a statement about successive beats, so it is computed over the
        // longest run of intervals that actually followed one another — not over a day of bursts,
        // where the difference between the last beat of one burst and the first of the next is not
        // a beat-to-beat change at all. On real hardware that mistake inflates RMSSD roughly
        // tenfold.
        //
        // Optical intervals, so the analytic labels the result PRV rather than HRV. Nothing on the
        // supported straps produces ECG intervals on this path; when something does, the source
        // becomes a property of the stream rather than an assumption here.
        let runs = beat_runs(&of_kind(today, StreamKind::RrInterval));
        let hrv = runs
            .iter()
            .max_by_key(|run| run.len())
            .and_then(|run| time_domain(run, IntervalSource::Ppg, provenance(day)));

        // Readiness reads the trailing nights, oldest first, with a gap for every day that has no
        // usable series. The analytic decides how many valid nights it needs before answering.
        let nightly: Vec<Option<f64>> = ((day.index() - READINESS_WINDOW_DAYS + 1)..=day.index())
            .map(|index| {
                let past = LocalDay::from_index(index);
                by_day
                    .get(past)
                    .map(|samples| of_kind(samples, StreamKind::RrInterval))
                    .and_then(|rr| {
                        // Gap-aware: squared successive differences pool within each run and never
                        // across the break between runs.
                        let runs = beat_runs(&rr);
                        let beats: Vec<Vec<u16>> = runs
                            .iter()
                            .map(|run| {
                                run.iter()
                                    .map(|sample| sample.value.as_f64().round() as u16)
                                    .collect()
                            })
                            .collect();
                        HrvReadiness::rmssd_runs(beats.iter().map(Vec::as_slice))
                    })
            })
            .collect();
        let readiness = HrvReadiness::evaluate(&nightly);

        let mut present: Vec<StreamKind> = today.iter().map(|sample| sample.kind).collect();
        present.sort_by_key(|kind| *kind as u8);
        present.dedup();

        Ok(DailySnapshot {
            day: day.to_string(),
            day_index: day.index(),
            heart_rate,
            hrv,
            readiness,
            availability: negotiate(&present),
            algorithms: stamps(),
        })
    }
}

/// Samples bucketed by the local day they fall in.
#[derive(Default)]
struct DayIndex {
    days: std::collections::BTreeMap<i64, Vec<Sample<RawValue>>>,
}

impl DayIndex {
    fn push(&mut self, day: LocalDay, sample: Sample<RawValue>) {
        self.days.entry(day.index()).or_default().push(sample);
    }

    fn get(&self, day: LocalDay) -> Option<&[Sample<RawValue>]> {
        self.days.get(&day.index()).map(Vec::as_slice)
    }
}

/// Split interval samples into runs of beats that genuinely followed one another, in device-time
/// order. A gap wider than [`BEAT_RUN_GAP_MS`] starts a new run.
fn beat_runs(intervals: &[Sample<RawValue>]) -> Vec<Vec<Sample<RawValue>>> {
    let mut ordered = intervals.to_vec();
    ordered.sort_by_key(|sample| (sample.device_time.as_nanos(), sample.seq));

    let mut runs: Vec<Vec<Sample<RawValue>>> = Vec::new();
    let mut previous_ms: Option<i64> = None;
    for sample in ordered {
        let at_ms = sample.device_time.as_nanos().div_euclid(1_000_000);
        let continues =
            previous_ms.is_some_and(|last| at_ms.saturating_sub(last) <= BEAT_RUN_GAP_MS);
        if continues {
            if let Some(run) = runs.last_mut() {
                run.push(sample);
            }
        } else {
            runs.push(vec![sample]);
        }
        previous_ms = Some(at_ms);
    }
    runs
}

fn of_kind(samples: &[Sample<RawValue>], kind: StreamKind) -> Vec<Sample<RawValue>> {
    samples
        .iter()
        .filter(|sample| sample.kind == kind)
        .copied()
        .collect()
}

/// A snapshot's provenance is the day it describes: derived values point at the recomputation that
/// produced them, and that recomputation is identified by its day.
fn provenance(day: LocalDay) -> MetadataId {
    MetadataId::new(day.index().unsigned_abs())
}

fn stamps() -> Vec<AlgorithmStamp> {
    [
        (HR_FEATURE_ALGORITHM, HR_FEATURE_VERSION),
        (HRV_ALGORITHM, HRV_VERSION),
    ]
    .into_iter()
    .map(|(id, version): (&str, Version)| AlgorithmStamp {
        id: id.to_owned(),
        version: version.to_string(),
    })
    .collect()
}

fn cache_key(day: LocalDay) -> CacheKey {
    CacheKey {
        metric: "daily_snapshot".to_owned(),
        algorithm_version: HRV_VERSION,
        first_day: day,
        last_day: day,
    }
}

fn persist(
    store: &Store,
    device: DeviceId,
    day: LocalDay,
    snapshot: &DailySnapshot,
    computed_ns: i64,
) -> Result<()> {
    let json = serde_json::to_string(snapshot).map_err(|source| {
        mav_model::error::MavError::new(
            mav_model::error::codes::INTERNAL_INVARIANT,
            format!("daily snapshot did not serialize: {source}"),
        )
    })?;
    let algorithms = serde_json::to_string(&snapshot.algorithms).map_err(|source| {
        mav_model::error::MavError::new(
            mav_model::error::codes::INTERNAL_INVARIANT,
            format!("algorithm stamps did not serialize: {source}"),
        )
    })?;
    store.upsert_daily_snapshot(device, day.index(), &json, &algorithms, computed_ns)
}

#[cfg(test)]
mod tests;
