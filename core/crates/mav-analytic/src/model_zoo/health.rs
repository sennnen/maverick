//! How much of a model's input was real, and what the archive's own gate made of it.
//!
//! These weights cannot be retrained. There is no labelled corpus to fit a replacement against,
//! and there is not going to be one — so the models ship as they are, fitted on a cohort and a
//! wear site that may not be the wearer's. That decision is made; this module is what makes it
//! survivable.
//!
//! The idea is narrow on purpose. **Accuracy needs labels and we have none. Input health needs
//! nothing.** Whether the numbers going *into* a graph are readings or substitutions is fully
//! knowable at inference time, costs almost nothing to record, and separates the two failures
//! that look identical from outside:
//!
//! - a model that ran on real data and was somewhat wrong, which is the price of using it;
//! - a model that ran on zeros and returned a confident number anyway, which is not a reading at
//!   all and must never be shown as one.
//!
//! The second is not hypothetical here. [`super::cycle`] rejects a temperature outside
//! `[35.5, 37.5]` °C to `NaN` and then fills `NaN` with zero, because that is what the archive
//! does. A wearer whose skin temperature sits outside that band — or a site that reads cooler
//! than the finger the model was fitted on — gets a forty-day series of zeros and an ovulation
//! probability computed from none of their data. Nothing in the output says so. This does.
//!
//! ## What the archives already tell us for free
//!
//! Two of the ported front-ends carry a validity signal the training pipeline itself defined, and
//! both were being computed and discarded:
//!
//! - `cva_pulse` sets `accepted` from the min-max normalised pulse: mean within
//!   `[52.35, 79.81]` and standard deviation at least `20.36`. Because min-max normalisation has
//!   already removed absolute amplitude, this is a **shape** gate rather than an amplitude one —
//!   a weaker signal does not fail it, but a differently *shaped* pulse does. That makes it a
//!   genuine out-of-distribution detector that the model's own authors calibrated, and it is
//!   free.
//! - `cycle_input` counts how many of its forty rows are real days.
//!
//! Neither needs a label. Both are worth more than a confidence number a model invented about
//! itself.

use serde::{Deserialize, Serialize};

/// Why a value in a prepared tensor is not a reading.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Substitution {
    /// The window was shorter than the contract's fixed length and was padded out.
    Padded,
    /// A reading existed and fell outside the range the archive accepts, so the archive
    /// discarded it. Distinct from [`Self::Missing`] in cause and identical in effect, which is
    /// exactly why both are named: a surface that says "no data" when the truth is "your readings
    /// are outside the band this model accepts" has told the wearer the wrong thing.
    OutOfRange,
    /// No reading was recorded for that position at all.
    Missing,
}

impl Substitution {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Padded => "padded",
            Self::OutOfRange => "out_of_range",
            Self::Missing => "missing",
        }
    }
}

/// How far an output may be trusted, given only what went in.
///
/// This is deliberately not a probability. It is a statement about the input, and turning it into
/// a number would invite averaging it with things that are not comparable.
///
/// Deliberately not `Ord` either. Three of the four sit on a severity axis and [`Self::Unmeasured`]
/// does not, so an ordering would be wrong exactly where someone reached for it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Applicability {
    /// The input is substantially real and any gate the archive defines passed.
    Sound,
    /// Enough of the input was substituted, or a gate failed, that the output should carry a
    /// qualification wherever it is shown.
    Degraded,
    /// So little of the input was real that the output is a function of the padding. It may be
    /// stored; it may not be presented as a reading.
    Unfounded,
    /// The core did not assemble these tensors, so it has nothing to say about them.
    ///
    /// This is the replay and test path, where a caller supplies tensors directly. Reporting it
    /// as [`Self::Sound`] would be a claim the core cannot support, and reporting it as
    /// [`Self::Unfounded`] would be a different false claim; it is genuinely a fourth thing.
    Unmeasured,
}

impl Applicability {
    /// The wire name. One source of truth, so a platform's string and this enum cannot drift.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Sound => "sound",
            Self::Degraded => "degraded",
            Self::Unfounded => "unfounded",
            Self::Unmeasured => "unmeasured",
        }
    }
}

/// Below this fraction of real input, an output is [`Applicability::Unfounded`].
///
/// Set where it is because the failure it guards against is total rather than gradual: the case
/// worth catching is a window that is nearly all zeros, not one that lost a few samples.
pub const UNFOUNDED_BELOW: f32 = 0.25;

/// Below this, [`Applicability::Degraded`].
pub const DEGRADED_BELOW: f32 = 0.80;

/// What a prepared input was actually made of.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InputHealth {
    /// Fraction of the contract's positions that carry a real reading, in `0.0..=1.0`.
    pub real_fraction: f32,
    /// The archive's own validity gate, where it defines one. `None` means the archive offers no
    /// opinion — which is not the same as passing, and is why this is an `Option` rather than
    /// defaulting to `true`.
    pub gate_passed: Option<bool>,
    /// Why anything was substituted, in the order the reasons were first met.
    pub substitutions: Vec<Substitution>,
}

impl InputHealth {
    /// Tensors the core did not build and cannot vouch for.
    pub fn unmeasured() -> Self {
        Self {
            real_fraction: f32::NAN,
            gate_passed: None,
            substitutions: Vec::new(),
        }
    }

    /// An input made entirely of readings, with no gate.
    pub fn sound() -> Self {
        Self {
            real_fraction: 1.0,
            gate_passed: None,
            substitutions: Vec::new(),
        }
    }

    /// `real` of `total` positions were readings.
    pub fn of(real: usize, total: usize) -> Self {
        Self {
            real_fraction: if total == 0 {
                0.0
            } else {
                real as f32 / total as f32
            },
            gate_passed: None,
            substitutions: Vec::new(),
        }
    }

    pub fn with_gate(mut self, passed: bool) -> Self {
        self.gate_passed = Some(passed);
        self
    }

    pub fn substituting(mut self, reason: Substitution) -> Self {
        if !self.substitutions.contains(&reason) {
            self.substitutions.push(reason);
        }
        self
    }

    /// The verdict.
    ///
    /// A failed gate caps the result at [`Applicability::Degraded`] however complete the input
    /// was: the archive's authors put that gate there because a shape it rejects is one the
    /// weights were not fitted against, and completeness does not answer that.
    pub fn applicability(&self) -> Applicability {
        if self.real_fraction.is_nan() {
            return Applicability::Unmeasured;
        }
        if self.real_fraction < UNFOUNDED_BELOW {
            return Applicability::Unfounded;
        }
        if self.gate_passed == Some(false) || self.real_fraction < DEGRADED_BELOW {
            return Applicability::Degraded;
        }
        Applicability::Sound
    }

    /// True when a surface may present this output as a reading.
    ///
    /// [`Applicability::Unmeasured`] counts as presentable: it is the replay path, where the
    /// operator supplied the tensors and knows what they are. It is not evidence of a problem,
    /// only of the core having no view.
    pub fn presentable(&self) -> bool {
        self.applicability() != Applicability::Unfounded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_verdict_has_a_distinct_wire_name() {
        let names = [
            Applicability::Sound.name(),
            Applicability::Degraded.name(),
            Applicability::Unfounded.name(),
            Applicability::Unmeasured.name(),
        ];
        let mut unique = names.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), names.len(), "two verdicts share a name");
    }

    #[test]
    fn a_complete_input_is_sound() {
        assert_eq!(InputHealth::sound().applicability(), Applicability::Sound);
        assert_eq!(
            InputHealth::of(40, 40).applicability(),
            Applicability::Sound
        );
    }

    #[test]
    fn a_mostly_padded_window_is_unfounded_rather_than_merely_degraded() {
        // Four real days of forty is the case this exists for: the model returns a number, and
        // thirty-six fortieths of it is the padding talking.
        let health = InputHealth::of(4, 40).substituting(Substitution::Missing);
        assert_eq!(health.applicability(), Applicability::Unfounded);
        assert!(!health.presentable());
    }

    #[test]
    fn a_partly_substituted_window_is_degraded_and_still_presentable() {
        let health = InputHealth::of(30, 40).substituting(Substitution::OutOfRange);
        assert_eq!(health.applicability(), Applicability::Degraded);
        assert!(health.presentable());
    }

    /// A failed gate is not outvoted by a complete input. This is the wrist-versus-finger case:
    /// every sample present, and the archive's own shape test says it is not the signal the
    /// weights were fitted on.
    #[test]
    fn a_failed_gate_degrades_an_otherwise_complete_input() {
        let health = InputHealth::of(40, 40).with_gate(false);
        assert_eq!(health.real_fraction, 1.0);
        assert_eq!(health.applicability(), Applicability::Degraded);
    }

    #[test]
    fn a_passing_gate_leaves_a_complete_input_sound() {
        assert_eq!(
            InputHealth::of(40, 40).with_gate(true).applicability(),
            Applicability::Sound
        );
    }

    /// No gate is not a passing gate. A model whose archive defines no validity test has not
    /// vouched for anything, and recording that as `true` would manufacture assurance.
    #[test]
    fn an_absent_gate_is_distinguishable_from_a_passing_one() {
        assert_eq!(InputHealth::sound().gate_passed, None);
        assert_ne!(InputHealth::sound().gate_passed, Some(true));
    }

    #[test]
    fn out_of_range_and_missing_are_reported_apart() {
        let health = InputHealth::of(20, 40)
            .substituting(Substitution::OutOfRange)
            .substituting(Substitution::Missing)
            .substituting(Substitution::OutOfRange);
        assert_eq!(
            health.substitutions,
            vec![Substitution::OutOfRange, Substitution::Missing],
            "a reason should be recorded once, in first-met order"
        );
    }

    /// Both thresholds are strictly-less-than, so a fraction sitting exactly on one takes the
    /// kinder verdict. Pinned because it is the first thing a reader will wonder about and the
    /// last thing a refactor will preserve by accident.
    #[test]
    fn a_fraction_exactly_on_a_threshold_takes_the_kinder_verdict() {
        assert_eq!(
            InputHealth::of(1, 4).applicability(),
            Applicability::Degraded,
            "0.25 is UNFOUNDED_BELOW exactly and must not be Unfounded"
        );
        assert_eq!(
            InputHealth::of(4, 5).applicability(),
            Applicability::Sound,
            "0.80 is DEGRADED_BELOW exactly and must not be Degraded"
        );
        // And a hair under each flips it.
        assert_eq!(
            InputHealth::of(24, 100).applicability(),
            Applicability::Unfounded
        );
        assert_eq!(
            InputHealth::of(79, 100).applicability(),
            Applicability::Degraded
        );
    }

    /// Unmeasured is its own answer, not a flattering or a damning one.
    #[test]
    fn tensors_the_core_did_not_build_are_unmeasured_rather_than_sound() {
        let health = InputHealth::unmeasured();
        assert_eq!(health.applicability(), Applicability::Unmeasured);
        assert_ne!(health.applicability(), Applicability::Sound);
        assert_ne!(health.applicability(), Applicability::Unfounded);
        assert!(health.presentable(), "the replay path is not a broken path");
    }

    #[test]
    fn an_empty_input_is_unfounded_rather_than_dividing_by_zero() {
        assert_eq!(
            InputHealth::of(0, 0).applicability(),
            Applicability::Unfounded
        );
    }
}
