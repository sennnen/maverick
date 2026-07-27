//! What a published model needs to know about the person it is being applied to.
//!
//! These are model parameters, not identity. Several of the imported regressions were fitted on
//! sex-stratified cohorts and publish two coefficient sets; picking one is applying the model, and
//! declining to pick is a statement about confidence rather than a default to the larger group.

use serde::{Deserialize, Serialize};

/// Which coefficient set of a sex-stratified model applies.
///
/// Typed rather than a string because two modules parsed the same field two different ways —
/// `starts_with('f')` in one and an exact `"female"` match in the other — so the same profile
/// scored as a woman in a training-load estimate and as a man in a fitness-age estimate.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BiologicalSex {
    Female,
    Male,
    /// Not stated, or stated as something the published cohorts did not stratify on. Estimators
    /// still produce a number, and say their confidence is lower for it.
    #[default]
    Unstated,
}

impl BiologicalSex {
    /// Read a profile field. Anything that is not recognisably one of the two cohorts is
    /// `Unstated`, which is the honest answer rather than a silent fallback to one of them.
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "f" | "female" | "woman" => Self::Female,
            "m" | "male" | "man" => Self::Male,
            _ => Self::Unstated,
        }
    }

    /// Which of a two-set model's coefficients to use. An unstated profile takes the male set,
    /// because that is what the published models default to, and every estimator that does this
    /// also reports reduced confidence.
    pub fn is_female(self) -> bool {
        self == Self::Female
    }

    /// True when no published cohort matches, so the estimate is being extrapolated.
    pub fn is_extrapolated(self) -> bool {
        self == Self::Unstated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The disagreement this type exists to end: `"F"` has to mean the same thing everywhere.
    #[test]
    fn every_spelling_of_a_cohort_parses_to_the_same_answer() {
        for female in ["f", "F", "female", "Female", " FEMALE ", "woman"] {
            assert_eq!(
                BiologicalSex::parse(female),
                BiologicalSex::Female,
                "{female}"
            );
        }
        for male in ["m", "M", "male", "Male", "man"] {
            assert_eq!(BiologicalSex::parse(male), BiologicalSex::Male, "{male}");
        }
    }

    #[test]
    fn an_unrecognised_profile_is_unstated_and_says_it_is_extrapolating() {
        for other in ["", "x", "nonbinary", "prefer not to say"] {
            let parsed = BiologicalSex::parse(other);
            assert_eq!(parsed, BiologicalSex::Unstated, "{other}");
            assert!(parsed.is_extrapolated());
            assert!(!parsed.is_female());
        }
        assert!(!BiologicalSex::Female.is_extrapolated());
        assert!(!BiologicalSex::Male.is_extrapolated());
    }
}
