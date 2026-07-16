//! Capability negotiation for analytics. Requirements are data, not UI conditionals.

use mav_model::stream::StreamKind;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalyticId {
    TimeDomainHrv,
    Recovery,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AnalyticDescriptor {
    pub id: AnalyticId,
    pub required_streams: &'static [StreamKind],
    pub admitted: bool,
}

pub const ANALYTICS: &[AnalyticDescriptor] = &[
    AnalyticDescriptor {
        id: AnalyticId::TimeDomainHrv,
        required_streams: &[StreamKind::RrInterval],
        admitted: true,
    },
    AnalyticDescriptor {
        id: AnalyticId::Recovery,
        required_streams: &[StreamKind::RrInterval],
        admitted: false,
    },
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
            let missing: Vec<_> = descriptor
                .required_streams
                .iter()
                .copied()
                .filter(|stream| !streams.contains(stream))
                .collect();
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
