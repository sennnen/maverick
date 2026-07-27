//! Capability negotiation for analytics. Requirements are data, not UI conditionals.

use mav_model::stream::StreamKind;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalyticId {
    TimeDomainHrv,
    /// Task Force band powers over the same beats, by Lomb-Scargle periodogram.
    FrequencyDomainHrv,
    Recovery,
    /// A sleep-quality composite. Needs staged sleep, which no admitted analytic produces yet.
    SleepPerformance,
    /// The multi-signal pre-symptomatic pattern: resting HR, skin temperature, variability, and
    /// respiration moving together against the user's own baseline.
    IllnessRisk,
    /// Menstrual-cycle phase awareness from the nightly skin-temperature series. Awareness only,
    /// never contraception, fertility prediction, or a diagnosis.
    CyclePhase,
}

/// Beat-to-beat intervals from either physiological source. An analytic that only needs the
/// timing of beats is served by either; one that needs them to be electrical asks for
/// `StreamKind::RrInterval` alone.
const ANY_INTERVAL: &[StreamKind] = &[StreamKind::RrInterval, StreamKind::PulseInterval];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AnalyticDescriptor {
    pub id: AnalyticId,
    /// Streams that must all be present.
    pub requires_all: &'static [StreamKind],
    /// Groups within which any one stream will do. A group with nothing present is reported by
    /// its first member, the one the analytic would rather have.
    pub requires_any: &'static [&'static [StreamKind]],
    pub admitted: bool,
}

const fn descriptor(
    id: AnalyticId,
    requires_all: &'static [StreamKind],
    requires_any: &'static [&'static [StreamKind]],
    admitted: bool,
) -> AnalyticDescriptor {
    AnalyticDescriptor {
        id,
        requires_all,
        requires_any,
        admitted,
    }
}

pub const ANALYTICS: &[AnalyticDescriptor] = &[
    descriptor(AnalyticId::TimeDomainHrv, &[], &[ANY_INTERVAL], true),
    descriptor(AnalyticId::FrequencyDomainHrv, &[], &[ANY_INTERVAL], true),
    descriptor(AnalyticId::Recovery, &[], &[ANY_INTERVAL], false),
    descriptor(
        AnalyticId::SleepPerformance,
        &[StreamKind::SleepStateRaw, StreamKind::HeartRate],
        &[],
        false,
    ),
    descriptor(
        AnalyticId::IllnessRisk,
        &[
            StreamKind::HeartRate,
            StreamKind::SkinTemp,
            StreamKind::RespRaw,
        ],
        &[ANY_INTERVAL],
        false,
    ),
    descriptor(
        AnalyticId::CyclePhase,
        &[StreamKind::SkinTemp, StreamKind::HeartRate],
        &[ANY_INTERVAL],
        false,
    ),
];

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum UnavailableReason {
    MissingStreams { streams: Vec<StreamKind> },
    AlgorithmNotAdmitted,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct AnalyticAvailability {
    pub analytic: AnalyticId,
    pub available: bool,
    pub reason: Option<UnavailableReason>,
}

pub fn negotiate(streams: &[StreamKind]) -> Vec<AnalyticAvailability> {
    let streams: HashSet<_> = streams.iter().copied().collect();
    ANALYTICS
        .iter()
        .map(|descriptor| {
            let mut missing: Vec<_> = descriptor
                .requires_all
                .iter()
                .copied()
                .filter(|stream| !streams.contains(stream))
                .collect();
            missing.extend(
                descriptor
                    .requires_any
                    .iter()
                    .filter(|group| !group.iter().any(|stream| streams.contains(stream)))
                    .filter_map(|group| group.first().copied()),
            );
            let reason = if !missing.is_empty() {
                Some(UnavailableReason::MissingStreams { streams: missing })
            } else if !descriptor.admitted {
                Some(UnavailableReason::AlgorithmNotAdmitted)
            } else {
                None
            };
            AnalyticAvailability {
                analytic: descriptor.id,
                available: reason.is_none(),
                reason,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn availability(result: &[AnalyticAvailability], id: AnalyticId) -> &AnalyticAvailability {
        result
            .iter()
            .find(|item| item.analytic == id)
            .expect("descriptor must be present")
    }

    #[test]
    fn rr_makes_time_domain_hrv_available() {
        let result = negotiate(&[StreamKind::HeartRate, StreamKind::RrInterval]);
        assert!(availability(&result, AnalyticId::TimeDomainHrv).available);
    }

    #[test]
    fn missing_rr_reports_the_exact_missing_stream() {
        let result = negotiate(&[StreamKind::HeartRate]);
        assert_eq!(
            availability(&result, AnalyticId::TimeDomainHrv).reason,
            Some(UnavailableReason::MissingStreams {
                streams: vec![StreamKind::RrInterval]
            })
        );
    }

    #[test]
    fn recovery_refuses_to_fake_an_unadmitted_algorithm() {
        let result = negotiate(&[StreamKind::RrInterval]);
        assert_eq!(
            availability(&result, AnalyticId::Recovery).reason,
            Some(UnavailableReason::AlgorithmNotAdmitted)
        );
    }

    #[test]
    fn missing_input_takes_precedence_over_admission_state() {
        let result = negotiate(&[]);
        assert_eq!(
            availability(&result, AnalyticId::Recovery).reason,
            Some(UnavailableReason::MissingStreams {
                streams: vec![StreamKind::RrInterval]
            })
        );
    }
}
