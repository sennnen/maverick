//! The analytic spine: stored samples in, one `DailySnapshot` per local day out.
//!
//! It reads only the window it is asked about. A day's heart rate and beats come from two indexed
//! range scans, the streams a day holds come from one census query, and the longitudinal look-back
//! reads the nightly memo rather than re-deriving two months of beats. Nothing here loads a whole
//! stream: doing that made opening the app read every sample the device had ever produced.
//!
//! Two rules shape it. Availability is negotiated from the streams the day actually holds, so a
//! metric with no evidence is reported unavailable with its reason rather than computed from
//! nothing (ADR-005, ADR-024). And every derived row is rebuildable by construction: dropping them
//! all and recomputing reproduces them, which is what makes an algorithm change a recompute rather
//! than a migration.

use mav_analytic::capability::{negotiate, AnalyticAvailability};
use mav_analytic::frequency::{band_powers, FrequencyDomainHrv};
use mav_analytic::hrv::{time_domain, TimeDomainHrv, HRV_ALGORITHM, HRV_VERSION};
use mav_analytic::intervals::BeatSeries;
use mav_analytic::readiness::{HrvReadiness, HrvReadinessResult};
use mav_feature::hr::{hr_summary, HrSummary, HR_FEATURE_ALGORITHM, HR_FEATURE_VERSION};
use mav_model::error::Result;
use mav_model::ids::{DeviceId, MetadataId};
use mav_model::raw::RawValue;
use mav_model::stream::{Placement, Quality, Sample, StreamKind};
use mav_model::time::{DeviceTime, WallTime};
use mav_model::version::Version;
use mav_store::Store;
use serde::{Deserialize, Serialize};

use crate::recompute::{LocalDay, Timezone};

/// How many trailing days of nightly variability the readiness reading looks back over. The
/// analytic decides how many valid nights it needs; this only bounds how far back we read.
const READINESS_WINDOW_DAYS: i64 = 60;

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
    /// Time-domain variability, labelled HRV or PRV by the stream that timed the beats. Absent
    /// when the day held too few trustworthy intervals; `availability` says which.
    pub hrv: Option<TimeDomainHrv>,
    /// Task Force band powers over the longest uninterrupted run of the same beats. Absent when no
    /// run is long enough for the bands to be defined.
    pub hrv_spectrum: Option<FrequencyDomainHrv>,
    /// The longitudinal readout over the trailing nights. Absent while calibrating.
    pub readiness: Option<HrvReadinessResult>,
    /// One entry per known analytic, carrying `UnavailableReason` for everything not served.
    pub availability: Vec<AnalyticAvailability>,
    pub algorithms: Vec<AlgorithmStamp>,
}

/// The recompute engine over one device's stored samples.
pub struct Spine {
    timezone: Timezone,
}

impl Spine {
    pub fn new(timezone: Timezone) -> Self {
        Self { timezone }
    }

    pub fn timezone(&self) -> &Timezone {
        &self.timezone
    }

    /// Replace the offset spans. The platforms own the zone database; a change here moves day
    /// boundaries, so every derived row is discarded rather than reinterpreted.
    pub fn set_timezone(&mut self, store: &Store, timezone: Timezone) -> Result<()> {
        self.timezone = timezone;
        store.clear_derived(None)?;
        Ok(())
    }

    /// The local day a wall-clock instant falls in, under the current spans.
    pub fn day_of(&self, at: WallTime) -> LocalDay {
        LocalDay::of(at, &self.timezone)
    }

    /// Forget the remembered nights a sync touched, so the next read re-derives them.
    pub fn invalidate(
        &self,
        store: &Store,
        device: DeviceId,
        first: LocalDay,
        last: LocalDay,
    ) -> Result<()> {
        store.forget_nightly_variability(device, first.index(), last.index())
    }

    /// The snapshot for one day, persisted when it differs from the stored one. Recomputing is
    /// cheap enough that there is no in-memory cache to go stale behind a sync.
    pub fn snapshot(
        &self,
        store: &Store,
        device: DeviceId,
        day: LocalDay,
        computed_ns: i64,
    ) -> Result<DailySnapshot> {
        let snapshot = self.compute(store, device, day)?;
        let json = to_json(&snapshot)?;
        if store.daily_snapshot(device, day.index())?.as_deref() != Some(json.as_str()) {
            let algorithms = to_json(&snapshot.algorithms)?;
            store.upsert_daily_snapshot(device, day.index(), &json, &algorithms, computed_ns)?;
        }
        Ok(snapshot)
    }

    /// Compute one day from stored samples. A pure function of the samples and the offset spans:
    /// the nightly memo it fills in is derived from exactly the same beats.
    pub fn compute(&self, store: &Store, device: DeviceId, day: LocalDay) -> Result<DailySnapshot> {
        let (from, until) = self.span(day);
        let heart_rate = hr_summary(
            &store.samples_between(device, StreamKind::HeartRate, from, until)?,
            provenance(day),
        );
        let present = store.streams_between(device, from, until)?;

        let beats = self
            .interval_kind(&present)
            .map(|kind| {
                Ok::<_, mav_model::error::MavError>((
                    kind,
                    self.beats(store, device, kind, from, until)?,
                ))
            })
            .transpose()?;
        let hrv = beats
            .as_ref()
            .and_then(|(kind, beats)| time_domain(beats, *kind, provenance(day)));
        let hrv_spectrum = beats
            .as_ref()
            .and_then(|(kind, beats)| spectrum(beats, *kind));

        let readiness = match &hrv {
            Some(today) => {
                HrvReadiness::evaluate(&self.trailing_nights(store, device, today.source, day)?)
            }
            None => None,
        };

        Ok(DailySnapshot {
            day: day.to_string(),
            day_index: day.index(),
            heart_rate,
            hrv,
            hrv_spectrum,
            readiness,
            availability: negotiate(&present),
            algorithms: stamps(),
        })
    }

    /// The trailing nightly RMSSD series, oldest first, one slot per day. Remembered nights come
    /// from the memo; the rest are derived from that night's beats and remembered.
    fn trailing_nights(
        &self,
        store: &Store,
        device: DeviceId,
        kind: StreamKind,
        day: LocalDay,
    ) -> Result<Vec<Option<f64>>> {
        let first = day.offset(1 - READINESS_WINDOW_DAYS);
        let remembered = store.nightly_variability(device, kind, first.index(), day.index())?;
        let mut nights = Vec::with_capacity(READINESS_WINDOW_DAYS as usize);
        let mut remembered = remembered.into_iter().peekable();
        for index in first.index()..=day.index() {
            if let Some((_, remembered)) = remembered.next_if(|(at, _)| *at == index) {
                nights.push(remembered);
                continue;
            }
            let past = LocalDay::from_index(index);
            let (from, until) = self.span(past);
            let beats = self.beats(store, device, kind, from, until)?;
            let (ordered, _) = mav_analytic::intervals::ordered_intervals(&beats, |sample| {
                sample.kind == kind && sample.quality.is_usable()
            });
            let rmssd = BeatSeries::from_ordered(&ordered).rmssd_ms();
            store.upsert_nightly_variability(device, kind, index, rmssd)?;
            nights.push(rmssd);
        }
        Ok(nights)
    }

    /// Which interval stream a day's variability should be computed over, or `None` when the day
    /// has no beats at all. Electrical wins, including when it has to be detected from a waveform,
    /// because only electrical beats may be called heart-rate variability.
    fn interval_kind(&self, present: &[StreamKind]) -> Option<StreamKind> {
        let held = |kind| present.contains(&kind);
        if held(StreamKind::RrInterval) || held(StreamKind::Ecg) {
            Some(StreamKind::RrInterval)
        } else {
            held(StreamKind::PulseInterval).then_some(StreamKind::PulseInterval)
        }
    }

    /// The interval samples of one kind inside a window. A device that streams the electrical
    /// waveform instead of the intervals has them detected here, which is the only route from a
    /// raw ECG to genuine heart-rate variability.
    fn beats(
        &self,
        store: &Store,
        device: DeviceId,
        kind: StreamKind,
        from: WallTime,
        until: WallTime,
    ) -> Result<Vec<Sample<RawValue>>> {
        let stored = store.samples_between(device, kind, from, until)?;
        if !stored.is_empty() || kind != StreamKind::RrInterval {
            return Ok(stored);
        }
        let waveform = store.samples_between(device, StreamKind::Ecg, from, until)?;
        Ok(detected_beats(&waveform))
    }

    fn span(&self, day: LocalDay) -> (WallTime, WallTime) {
        (
            day.start(&self.timezone),
            day.offset(1).start(&self.timezone),
        )
    }
}

/// Turn a stored ECG waveform into interval samples. Each carries the placement of the waveform
/// sample its closing beat landed on, so a detected interval is exactly as well-placed in time as
/// the signal it came from and no better.
fn detected_beats(waveform: &[Sample<RawValue>]) -> Vec<Sample<RawValue>> {
    let at_ms = |sample: &Sample<RawValue>| sample.device_time.as_nanos().div_euclid(1_000_000);
    let timed: Vec<(i64, f64)> = waveform
        .iter()
        .filter(|sample| sample.quality.is_usable())
        .map(|sample| (at_ms(sample), sample.value.as_f64()))
        .collect();

    mav_analytic::ecg::intervals_from_timed(&timed)
        .into_iter()
        .enumerate()
        .map(|(index, (closing_ms, interval_ms))| Sample {
            kind: StreamKind::RrInterval,
            device_time: DeviceTime::from_nanos(closing_ms.saturating_mul(1_000_000)),
            placement: waveform
                .binary_search_by_key(&closing_ms, at_ms)
                .map_or(Placement::Unplaced, |index| waveform[index].placement),
            seq: index as u16,
            value: RawValue::Converted(interval_ms),
            quality: Quality::exact(),
            provenance: MetadataId::new(0),
        })
        .collect()
}

/// Band powers over the longest uninterrupted run of beats in the window. A spectrum is a
/// statement about one continuous stretch of time, so runs are not pooled the way pooled
/// successive differences are.
fn spectrum(beats: &[Sample<RawValue>], kind: StreamKind) -> Option<FrequencyDomainHrv> {
    let (ordered, _) = mav_analytic::intervals::ordered_intervals(beats, |sample| {
        sample.kind == kind && sample.quality.is_usable()
    });
    let mut longest: &[(i64, f64)] = &[];
    let mut start = 0usize;
    for index in 0..ordered.len() {
        let breaks = index + 1 == ordered.len()
            || ordered[index + 1].0 - ordered[index].0 > mav_analytic::intervals::RUN_GAP_MS;
        if breaks {
            let run = &ordered[start..=index];
            if run.len() > longest.len() {
                longest = run;
            }
            start = index + 1;
        }
    }
    band_powers(longest)
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

fn to_json<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|source| {
        mav_model::error::MavError::new(
            mav_model::error::codes::INTERNAL_INVARIANT,
            format!("a derived value did not serialize: {source}"),
        )
    })
}

#[cfg(test)]
mod tests;
