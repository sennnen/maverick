//! The immutable read model the UI queries, and its canonical form. The snapshot is derived purely
//! from stored data, with no clock reading of its own, so the same capture always produces the same
//! snapshot and therefore the same hash. That determinism is the whole basis of the cross-platform
//! parity check: two platforms running the one shared core over the one fixture must return an
//! identical hash, and any difference is a binding bug (see docs/testing.md).

use mav_analytic::{
    AnalyticAvailability, IntervalSource, TimeDomainHrv, HRV_ALGORITHM, HRV_VERSION,
};
use mav_feature::hr::HrSummary;
use mav_model::error::{codes, MavError, Result};
use mav_model::ids::DeviceId;
use serde::{Deserialize, Serialize};

pub const SNAPSHOT_SCHEMA: &str = "snapshot/v1";
pub const ANALYTICS_SNAPSHOT_SCHEMA: &str = "analytics-snapshot/v1";

/// The Milestone 1 snapshot: current heart rate and the session summary. The mean is carried as an
/// integer of milli-bpm rather than a float so the canonical form has no floating-point formatting
/// to differ on across platforms.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema: String,
    pub device_id: u64,
    pub current_bpm: Option<u16>,
    pub mean_milli_bpm: Option<u32>,
    pub in_range_samples: u32,
    pub excluded_samples: u32,
    /// The provenance row behind the heart-rate figures, so a caller can walk back to the samples.
    pub provenance_id: u64,
}

impl Snapshot {
    pub fn from_hr(device: DeviceId, hr: &HrSummary) -> Self {
        Self {
            schema: SNAPSHOT_SCHEMA.to_owned(),
            device_id: device.get(),
            current_bpm: hr.current_bpm,
            mean_milli_bpm: hr.mean_bpm.map(|m| (m * 1000.0).round() as u32),
            in_range_samples: hr.sample_count,
            excluded_samples: hr.excluded_count,
            provenance_id: hr.provenance.get(),
        }
    }

    /// The deterministic serialisation the parity hash is taken over. Field order is fixed by the
    /// struct and there are no floats, so the bytes are reproducible.
    pub fn canonical_json(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|e| {
            MavError::new(codes::STORAGE_SERIALIZE, "could not serialise the snapshot")
                .context(e.to_string())
        })
    }

    /// A stable 64-bit hash of the canonical form, as lowercase hex. FNV-1a is used because it is
    /// tiny, dependency-free, and identical on every platform, which is all the parity check needs.
    pub fn canonical_hash(&self) -> Result<String> {
        Ok(fnv1a_64(self.canonical_json()?.as_bytes()))
    }
}

/// Deterministic analytic read model. Floats become fixed-point integers before crossing FFI or
/// entering a parity hash.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct AnalyticsSnapshot {
    pub schema: String,
    pub interval_source: String,
    pub variability_label: Option<String>,
    pub mean_interval_micros: Option<u64>,
    pub rmssd_micros: Option<u64>,
    pub sdnn_micros: Option<u64>,
    pub nn50_count: Option<u32>,
    pub pnn50_milli_percent: Option<u64>,
    pub interval_count: u32,
    pub excluded_interval_count: u32,
    pub algorithm: String,
    pub algorithm_version: String,
    pub availability: Vec<AnalyticAvailability>,
    pub provenance_id: Option<u64>,
}

impl AnalyticsSnapshot {
    pub fn new(
        source: IntervalSource,
        variability: Option<&TimeDomainHrv>,
        availability: Vec<AnalyticAvailability>,
    ) -> Self {
        Self {
            schema: ANALYTICS_SNAPSHOT_SCHEMA.to_owned(),
            interval_source: match source {
                IntervalSource::Ecg => "ecg",
                IntervalSource::Ppg => "ppg",
                IntervalSource::Unknown => "unknown",
            }
            .to_owned(),
            variability_label: variability.map(|value| value.label.clone()),
            mean_interval_micros: variability
                .map(|value| (value.mean_interval_ms * 1_000.0).round() as u64),
            rmssd_micros: variability.map(|value| (value.rmssd_ms * 1_000.0).round() as u64),
            sdnn_micros: variability.map(|value| (value.sdnn_ms * 1_000.0).round() as u64),
            nn50_count: variability.map(|value| value.nn50_count),
            pnn50_milli_percent: variability
                .map(|value| (value.pnn50_percent * 1_000.0).round() as u64),
            interval_count: variability.map_or(0, |value| value.interval_count),
            excluded_interval_count: variability.map_or(0, |value| value.excluded_count),
            algorithm: HRV_ALGORITHM.to_owned(),
            algorithm_version: HRV_VERSION.to_string(),
            availability,
            provenance_id: variability.map(|value| value.provenance.get()),
        }
    }

    pub fn canonical_json(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|e| {
            MavError::new(
                codes::STORAGE_SERIALIZE,
                "could not serialise the analytics snapshot",
            )
            .context(e.to_string())
        })
    }

    pub fn canonical_hash(&self) -> Result<String> {
        Ok(fnv1a_64(self.canonical_json()?.as_bytes()))
    }
}

fn fnv1a_64(bytes: &[u8]) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mav_feature::hr::HR_FEATURE_VERSION;
    use mav_model::ids::MetadataId;

    fn summary() -> HrSummary {
        HrSummary {
            current_bpm: Some(63),
            mean_bpm: Some(61.5),
            sample_count: 4,
            excluded_count: 1,
            provenance: MetadataId::new(2),
        }
    }

    #[test]
    fn mean_becomes_integer_milli_bpm() {
        let snap = Snapshot::from_hr(DeviceId::new(1), &summary());
        assert_eq!(snap.mean_milli_bpm, Some(61_500));
        assert_eq!(snap.current_bpm, Some(63));
    }

    #[test]
    fn canonical_hash_is_stable_across_calls() {
        let snap = Snapshot::from_hr(DeviceId::new(1), &summary());
        assert_eq!(
            snap.canonical_hash().unwrap(),
            snap.canonical_hash().unwrap()
        );
    }

    #[test]
    fn canonical_json_round_trips() {
        let snap = Snapshot::from_hr(DeviceId::new(9), &summary());
        let json = snap.canonical_json().unwrap();
        let back: Snapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back, snap);
        // The version const is exercised so a bump is a visible change here too.
        assert_eq!(
            HR_FEATURE_VERSION,
            mav_model::version::Version::new(1, 0, 0)
        );
    }

    #[test]
    fn a_changed_field_changes_the_hash() {
        let a = Snapshot::from_hr(DeviceId::new(1), &summary());
        let mut changed = summary();
        changed.current_bpm = Some(64);
        let b = Snapshot::from_hr(DeviceId::new(1), &changed);
        assert_ne!(a.canonical_hash().unwrap(), b.canonical_hash().unwrap());
    }

    #[test]
    fn analytics_snapshot_uses_fixed_point_numbers() {
        let variability = TimeDomainHrv {
            source: IntervalSource::Ppg,
            label: "pulse_rate_variability".to_owned(),
            mean_interval_ms: 800.125,
            rmssd_ms: 42.25,
            sdnn_ms: 51.5,
            nn50_count: 2,
            pnn50_percent: 40.0,
            interval_count: 6,
            excluded_count: 1,
            provenance: MetadataId::new(9),
        };
        let snapshot = AnalyticsSnapshot::new(IntervalSource::Ppg, Some(&variability), Vec::new());
        assert_eq!(snapshot.mean_interval_micros, Some(800_125));
        assert_eq!(snapshot.rmssd_micros, Some(42_250));
        assert_eq!(snapshot.pnn50_milli_percent, Some(40_000));
        assert_eq!(snapshot.interval_source, "ppg");
    }
}
