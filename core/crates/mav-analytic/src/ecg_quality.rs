//! Scale-independent signal-quality gate for ECG capture calibration.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EcgQualityReason {
    Contact,
    Motion,
    Saturation,
    Flatline,
    Dropout,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EcgQuality {
    pub good: bool,
    pub reason: Option<EcgQualityReason>,
    pub score_milli: u16,
}

pub fn assess_ecg_quality(samples: &[f64], sample_rate_hz: f64) -> EcgQuality {
    let minimum_samples = (sample_rate_hz * 2.0).ceil().max(1.0) as usize;
    if !sample_rate_hz.is_finite()
        || sample_rate_hz <= 0.0
        || samples.len() < minimum_samples
        || samples.iter().any(|sample| !sample.is_finite())
    {
        return failed(EcgQualityReason::Dropout);
    }

    let longest_zero_run = samples
        .iter()
        .fold((0usize, 0usize), |(current, longest), sample| {
            let next = if *sample == 0.0 { current + 1 } else { 0 };
            (next, longest.max(next))
        })
        .1;
    if longest_zero_run >= (sample_rate_hz / 5.0).ceil() as usize {
        return failed(EcgQualityReason::Dropout);
    }

    let (minimum, maximum) = samples.iter().fold(
        (f64::INFINITY, f64::NEG_INFINITY),
        |(minimum, maximum), sample| (minimum.min(*sample), maximum.max(*sample)),
    );
    let scale = samples
        .iter()
        .map(|sample| sample.abs())
        .fold(0.0_f64, f64::max)
        .max(1.0);
    if maximum - minimum <= f64::EPSILON * scale * 8.0 {
        return failed(EcgQualityReason::Flatline);
    }

    let rail_count = samples
        .iter()
        .filter(|sample| **sample == minimum || **sample == maximum)
        .count();
    if rail_count * 20 >= samples.len() {
        return failed(EcgQualityReason::Saturation);
    }

    let derivatives: Vec<f64> = samples
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .collect();
    let span = maximum - minimum;
    let motion_limit = span * 0.30;
    let motion_samples = derivatives
        .iter()
        .filter(|derivative| **derivative > motion_limit)
        .count();
    // At 100 Hz a legitimate QRS upstroke/downstroke occupies a larger fraction of the record
    // than it does at 256 Hz. The former 2.5% cutoff therefore called a clean, reference-derived
    // WHOOP-rate lead "motion". Five percent still rejects the injected-motion fixture (whose
    // discontinuities contribute both a jump and return) without treating ordinary QRS edges as
    // artefact.
    if motion_samples * 20 >= derivatives.len() {
        return failed(EcgQualityReason::Motion);
    }

    let peaks = crate::ecg::r_peaks(samples, sample_rate_hz);
    if peaks.len() < 2 {
        return failed(EcgQualityReason::Contact);
    }

    let motion_penalty = ((motion_samples * 2_000) / derivatives.len()).min(200) as u16;
    EcgQuality {
        good: true,
        reason: None,
        score_milli: 900_u16.saturating_sub(motion_penalty),
    }
}

fn failed(reason: EcgQualityReason) -> EcgQuality {
    EcgQuality {
        good: false,
        reason: Some(reason),
        score_milli: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_regular_signal_passes_calibration() {
        let samples: Vec<f64> = include_str!("../../../../fixtures/ecg/n_regular_72_v1.csv")
            .lines()
            .skip(1)
            .map(|line| line.split(',').nth(1).unwrap().parse::<f64>().unwrap())
            .collect();
        let quality = assess_ecg_quality(&samples[..768], 256.0);
        assert!(quality.good);
        assert_eq!(quality.reason, None);
        assert!(quality.score_milli >= 700);
    }

    #[test]
    fn every_model_family_can_calibrate_at_the_whoop_rate() {
        const CASES: [(&str, &str); 9] = [
            (
                "n_regular_55",
                include_str!("../../../../fixtures/ecg/corpus/n_regular_55.csv"),
            ),
            (
                "n_regular_72",
                include_str!("../../../../fixtures/ecg/corpus/n_regular_72.csv"),
            ),
            (
                "n_regular_90",
                include_str!("../../../../fixtures/ecg/corpus/n_regular_90.csv"),
            ),
            (
                "a_irregular_70",
                include_str!("../../../../fixtures/ecg/corpus/a_irregular_70.csv"),
            ),
            (
                "a_irregular_90",
                include_str!("../../../../fixtures/ecg/corpus/a_irregular_90.csv"),
            ),
            (
                "a_irregular_110",
                include_str!("../../../../fixtures/ecg/corpus/a_irregular_110.csv"),
            ),
            (
                "o_tachy_120",
                include_str!("../../../../fixtures/ecg/corpus/o_tachy_120.csv"),
            ),
            (
                "o_brady_40",
                include_str!("../../../../fixtures/ecg/corpus/o_brady_40.csv"),
            ),
            (
                "o_bigeminy_80",
                include_str!("../../../../fixtures/ecg/corpus/o_bigeminy_80.csv"),
            ),
        ];
        for (name, csv) in CASES {
            let source = csv
                .lines()
                .skip(1)
                .map(|line| line.split(',').nth(1).unwrap().parse::<f64>().unwrap())
                .collect::<Vec<_>>();
            let at_whoop_rate = (0..500)
                .map(|index| source[index * 256 / 100])
                .collect::<Vec<_>>();
            let quality = assess_ecg_quality(&at_whoop_rate, 100.0);
            assert!(
                quality.good,
                "{name} should calibrate at 100 Hz, got {:?}",
                quality.reason
            );
        }
    }

    #[test]
    fn failures_have_stable_actionable_reasons() {
        let flatline = vec![1.0; 300];
        assert_eq!(
            assess_ecg_quality(&flatline, 100.0).reason,
            Some(EcgQualityReason::Flatline)
        );

        let mut dropout = vec![0.1; 300];
        dropout[100] = f64::NAN;
        assert_eq!(
            assess_ecg_quality(&dropout, 100.0).reason,
            Some(EcgQualityReason::Dropout)
        );

        let saturated: Vec<f64> = (0..300)
            .map(|index| if index % 2 == 0 { -32_768.0 } else { 32_767.0 })
            .collect();
        assert_eq!(
            assess_ecg_quality(&saturated, 100.0).reason,
            Some(EcgQualityReason::Saturation)
        );

        let contact: Vec<f64> = (0..300).map(|index| index as f64 / 300.0).collect();
        assert_eq!(
            assess_ecg_quality(&contact, 100.0).reason,
            Some(EcgQualityReason::Contact)
        );

        let mut motion: Vec<f64> = (0..300).map(|index| ((index as f64) * 0.1).sin()).collect();
        for index in (20..300).step_by(25) {
            motion[index] += if index % 2 == 0 { 50.0 } else { -50.0 };
        }
        assert_eq!(
            assess_ecg_quality(&motion, 100.0).reason,
            Some(EcgQualityReason::Motion)
        );
    }
}
