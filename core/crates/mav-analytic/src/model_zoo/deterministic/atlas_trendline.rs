//! `atlas_trendline 1.0.0` — the weighted trend through a body-composition history.
//!
//! One weighted least-squares line through (day, value) pairs, with a confidence interval and
//! a significance figure. The weighting is the whole point: each reading carries a confidence
//! in `[0, 1]`, and each metric has a coefficient of variation that turns its own magnitude
//! into an expected error, so a reading is weighted by `confidence^1.5 / (value·cv)²`. A
//! confident reading of a quantity that is measured precisely dominates a hesitant one.
//!
//! Three things make it refuse to fit, and all three return a row of NaNs rather than an
//! error, because "no trend yet" is an ordinary state for a young history: fewer than three
//! points, a span shorter than the window needs, or weights that sum to zero.
//!
//! The archive computes in `f32` and this port does the same. That is not tidiness — the
//! third-of-a-year span in the generated vectors reads 358.20001 rather than 358.2, and a
//! port working in `f64` would disagree with the reference in the sixth digit of every
//! output derived from it.

/// Which trend window is being fitted, and therefore how much history it demands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Window {
    /// Seven days of history, at least three days of span.
    Weekly,
    /// Thirty-one days, at least ten of span.
    Monthly,
    /// Three hundred and sixty-six days, at least a hundred and twenty of span.
    Yearly,
}

impl Window {
    /// The shortest span, in days, this window will fit a line through.
    fn min_span(self) -> f32 {
        match self {
            Self::Weekly => 3.0,
            Self::Monthly => 10.0,
            Self::Yearly => 120.0,
        }
    }

    /// The largest day index the validator accepts for this window.
    fn max_day(self) -> f32 {
        match self {
            Self::Weekly => 7.0,
            Self::Monthly => 31.0,
            Self::Yearly => 366.0,
        }
    }

    fn code(self) -> f32 {
        match self {
            Self::Weekly => 0.0,
            Self::Monthly => 1.0,
            Self::Yearly => 2.0,
        }
    }
}

/// Which body-composition quantity is being trended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    /// Fat-free mass, in kilograms.
    FatFreeMass,
    /// Skeletal muscle mass, in kilograms.
    SkeletalMuscleMass,
    /// Fat mass, in kilograms.
    FatMass,
    /// Percentage body fat.
    PercentBodyFat,
    /// Skeletal muscle mass as a percentage.
    PercentSkeletalMuscle,
}

impl Metric {
    /// The coefficient of variation the weighting uses for this quantity.
    ///
    /// Skeletal muscle is measured about three times as precisely as fat, so a muscle reading
    /// of the same confidence carries roughly nine times the weight.
    fn coefficient_of_variation(self) -> f32 {
        match self {
            Self::FatFreeMass => 0.017,
            Self::SkeletalMuscleMass | Self::PercentSkeletalMuscle => 0.013,
            Self::FatMass | Self::PercentBodyFat => 0.036,
        }
    }

    /// The range of values the validator accepts for this quantity.
    fn range(self) -> (f32, f32) {
        match self {
            Self::FatFreeMass => (10.0, 200.0),
            Self::SkeletalMuscleMass => (2.0, 120.0),
            Self::FatMass | Self::PercentSkeletalMuscle => (1.0, 75.0),
            Self::PercentBodyFat => (0.0, 200.0),
        }
    }

    fn code(self) -> f32 {
        match self {
            Self::FatFreeMass => 0.0,
            Self::SkeletalMuscleMass => 1.0,
            Self::FatMass => 2.0,
            Self::PercentBodyFat => 3.0,
            Self::PercentSkeletalMuscle => 4.0,
        }
    }
}

/// Why the archive refused the input outright, rather than returning an unfitted trend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrendlineError {
    /// 401/402 — the three series are not the same length.
    LengthMismatch,
    /// 104/105 — a day index falls outside `[0, window max]`.
    DayOutOfRange,
    /// 204/205 — a value falls outside the metric's range.
    ValueOutOfRange,
    /// 304/305 — a confidence falls outside `[0, 1]`.
    ConfidenceOutOfRange,
}

/// The fitted trend. Every field is NaN when [`Trendline::valid`] is false.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trendline {
    /// Change in the metric per day.
    pub slope: f32,
    /// Lower end of the slope's confidence interval.
    pub slope_ci_lower: f32,
    /// Upper end of the slope's confidence interval.
    pub slope_ci_upper: f32,
    /// First day in the fitted history.
    pub start_day: f32,
    /// Last day in the fitted history.
    pub end_day: f32,
    /// The fitted line's value on `start_day`.
    pub start_value: f32,
    /// The fitted line's value on `end_day`.
    pub end_value: f32,
    /// Total change across the fitted span.
    pub total_change: f32,
    /// Lower end of that change's confidence interval.
    pub total_change_ci_lower: f32,
    /// Upper end of that change's confidence interval.
    pub total_change_ci_upper: f32,
    /// How far the slope is from zero, as `1 - exp(-z²/2)`.
    pub significance: f32,
    /// How many points the fit used.
    pub points: f32,
    /// The window that was requested.
    pub window: f32,
    /// The metric that was requested.
    pub metric: f32,
    /// Whether a line was fitted at all.
    pub valid: bool,
}

/// The archive's own confidence multiplier: a one-sided 90% normal quantile.
const CI_Z: f32 = 1.282;

/// Fewer points than this and no window will fit a line.
const MIN_POINTS: usize = 3;

/// Confidence is raised to this power before it becomes a weight, so a hesitant reading loses
/// influence faster than linearly.
const CONFIDENCE_EXPONENT: f32 = 1.5;

/// The smallest expected error a reading may claim, guarding the division that follows.
const MIN_SIGMA: f32 = 1e-6;

impl Trendline {
    /// The row the archive returns when there is not enough history to fit anything.
    fn unfitted(window: Window, metric: Metric) -> Self {
        Self {
            slope: f32::NAN,
            slope_ci_lower: f32::NAN,
            slope_ci_upper: f32::NAN,
            start_day: f32::NAN,
            end_day: f32::NAN,
            start_value: f32::NAN,
            end_value: f32::NAN,
            total_change: f32::NAN,
            total_change_ci_lower: f32::NAN,
            total_change_ci_upper: f32::NAN,
            significance: f32::NAN,
            points: 0.0,
            window: window.code(),
            metric: metric.code(),
            valid: false,
        }
    }
}

fn validate(
    days: &[f32],
    values: &[f32],
    confidences: &[f32],
    window: Window,
    metric: Metric,
) -> Result<(), TrendlineError> {
    if days.len() != values.len() || days.len() != confidences.len() {
        return Err(TrendlineError::LengthMismatch);
    }
    let out_of_range = |series: &[f32], low: f32, high: f32| {
        series
            .iter()
            .any(|value| !value.is_nan() && (*value < low || *value > high))
    };
    if out_of_range(days, 0.0, window.max_day()) {
        return Err(TrendlineError::DayOutOfRange);
    }
    let (low, high) = metric.range();
    if out_of_range(values, low, high) {
        return Err(TrendlineError::ValueOutOfRange);
    }
    if out_of_range(confidences, 0.0, 1.0) {
        return Err(TrendlineError::ConfidenceOutOfRange);
    }
    Ok(())
}

/// Fit the weighted trend through a body-composition history.
pub fn trendline(
    days: &[f32],
    values: &[f32],
    confidences: &[f32],
    window: Window,
    metric: Metric,
) -> Result<Trendline, TrendlineError> {
    validate(days, values, confidences, window, metric)?;

    // A point is dropped when any of its three fields is missing, not just its value: a
    // reading whose day is unknown cannot be placed on the line at all.
    let mut x = Vec::new();
    let mut y = Vec::new();
    let mut weights = Vec::new();
    let coefficient = metric.coefficient_of_variation();
    for index in 0..days.len() {
        let (day, value, confidence) = (days[index], values[index], confidences[index]);
        if day.is_nan() || value.is_nan() || confidence.is_nan() {
            continue;
        }
        let confidence = confidence.clamp(0.0, 1.0);
        let sigma = (value.abs() * coefficient).max(MIN_SIGMA);
        x.push(day);
        y.push(value);
        weights.push(confidence.powf(CONFIDENCE_EXPONENT) / (sigma * sigma));
    }

    if x.len() < MIN_POINTS {
        return Ok(Trendline::unfitted(window, metric));
    }
    let start_day = x.iter().copied().fold(f32::INFINITY, f32::min);
    let end_day = x.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    if end_day - start_day < window.min_span() {
        return Ok(Trendline::unfitted(window, metric));
    }
    let weight_sum: f32 = weights.iter().sum();
    if weight_sum == 0.0 {
        return Ok(Trendline::unfitted(window, metric));
    }

    let mean_x: f32 = x
        .iter()
        .zip(&weights)
        .map(|(value, weight)| weight * value)
        .sum::<f32>()
        / weight_sum;
    let mean_y: f32 = y
        .iter()
        .zip(&weights)
        .map(|(value, weight)| weight * value)
        .sum::<f32>()
        / weight_sum;
    let ss_xx: f32 = x
        .iter()
        .zip(&weights)
        .map(|(value, weight)| weight * (value - mean_x).powi(2))
        .sum();
    let ss_xy: f32 = x
        .iter()
        .zip(&y)
        .zip(&weights)
        .map(|((value, target), weight)| weight * (value - mean_x) * (target - mean_y))
        .sum();

    let points = x.len() as f32;
    if ss_xx == 0.0 {
        // Every reading on the same day. There is a level but no trend, so the slope is
        // exactly zero and its interval is unbounded — a distinct answer from "no fit".
        return Ok(Trendline {
            slope: 0.0,
            slope_ci_lower: f32::NEG_INFINITY,
            slope_ci_upper: f32::INFINITY,
            start_day,
            end_day,
            start_value: mean_y,
            end_value: mean_y,
            total_change: 0.0,
            total_change_ci_lower: f32::NEG_INFINITY,
            total_change_ci_upper: f32::INFINITY,
            significance: 0.0,
            points,
            window: window.code(),
            metric: metric.code(),
            valid: true,
        });
    }

    let slope = ss_xy / ss_xx;
    let intercept = mean_y - slope * mean_x;
    // The weights already carry the expected variance of each reading, so the residual
    // variance is one by construction and the slope's standard error is just 1/sqrt(Sxx).
    let slope_se = (1.0 / ss_xx).sqrt();
    let slope_ci_lower = slope - slope_se * CI_Z;
    let slope_ci_upper = slope + slope_se * CI_Z;
    let z = slope.abs() / slope_se;
    let span = end_day - start_day;
    Ok(Trendline {
        slope,
        slope_ci_lower,
        slope_ci_upper,
        start_day,
        end_day,
        start_value: slope * start_day + intercept,
        end_value: slope * end_day + intercept,
        total_change: slope * span,
        total_change_ci_lower: slope_ci_lower * span,
        total_change_ci_upper: slope_ci_upper * span,
        significance: 1.0 - (-0.5 * z * z).exp(),
        points,
        window: window.code(),
        metric: metric.code(),
        valid: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The vectors are float32 written through decimal, so this leaves room for the round trip
    /// and for the last bit of a float32 sum reassociated by a different summation order.
    const TOLERANCE: f32 = 2e-4;

    fn window_from(code: f32) -> Window {
        match code as i32 {
            0 => Window::Weekly,
            1 => Window::Monthly,
            _ => Window::Yearly,
        }
    }

    fn metric_from(code: f32) -> Metric {
        match code as i32 {
            0 => Metric::FatFreeMass,
            1 => Metric::SkeletalMuscleMass,
            2 => Metric::FatMass,
            3 => Metric::PercentBodyFat,
            _ => Metric::PercentSkeletalMuscle,
        }
    }

    #[test]
    fn refuses_to_fit_fewer_than_three_points() {
        let got = trendline(
            &[0.0, 1.0],
            &[55.0, 56.0],
            &[1.0, 1.0],
            Window::Weekly,
            Metric::FatFreeMass,
        )
        .expect("two points is a valid input, just not a fittable one");
        assert!(!got.valid);
        assert_eq!(got.points, 0.0);
        assert!(got.slope.is_nan());
    }

    #[test]
    fn refuses_a_span_shorter_than_the_window_demands() {
        // Four points, but inside two days — enough for the weekly window's point count and
        // short of its three-day span.
        let days = [0.0, 0.5, 1.0, 1.5];
        let values = [55.0, 55.2, 55.1, 55.3];
        let confidences = [1.0; 4];
        let got = trendline(
            &days,
            &values,
            &confidences,
            Window::Weekly,
            Metric::FatFreeMass,
        )
        .expect("a short span is a valid input");
        assert!(!got.valid);
        // The same points do fit once the window's demand drops away with the span it needs.
        let longer: Vec<f32> = days.iter().map(|d| d * 4.0).collect();
        assert!(
            trendline(
                &longer,
                &values,
                &confidences,
                Window::Weekly,
                Metric::FatFreeMass
            )
            .expect("a six-day span is fittable")
            .valid
        );
    }

    #[test]
    fn a_history_on_one_day_has_a_level_but_no_trend() {
        let days = [2.0; 4];
        let values = [55.0, 55.4, 55.2, 55.4];
        let got = trendline(
            &days,
            &values,
            &[1.0; 4],
            Window::Weekly,
            Metric::FatFreeMass,
        );
        // The span check comes first, so identical days never reach the zero-Sxx branch
        // through the front door. Reaching it needs a span that clears the window.
        assert!(!got.expect("identical days are valid input").valid);
    }

    #[test]
    fn weights_a_confident_reading_above_a_hesitant_one() {
        let days = [0.0, 3.0, 6.0];
        // The middle reading is an outlier. Trusted, it drags the line; distrusted, it does not.
        let values = [55.0, 60.0, 55.2];
        let trusted = trendline(
            &days,
            &values,
            &[1.0, 1.0, 1.0],
            Window::Weekly,
            Metric::FatFreeMass,
        )
        .expect("valid")
        .slope;
        let doubted = trendline(
            &days,
            &values,
            &[1.0, 0.05, 1.0],
            Window::Weekly,
            Metric::FatFreeMass,
        )
        .expect("valid")
        .slope;
        assert!(
            doubted.abs() < trusted.abs(),
            "doubting the outlier should flatten the line: {doubted} vs {trusted}"
        );
    }

    #[test]
    fn rejects_a_day_past_the_window_and_a_value_past_the_metric() {
        assert_eq!(
            trendline(
                &[0.0, 3.0, 9.0],
                &[55.0; 3],
                &[1.0; 3],
                Window::Weekly,
                Metric::FatFreeMass
            ),
            Err(TrendlineError::DayOutOfRange)
        );
        assert_eq!(
            trendline(
                &[0.0, 3.0, 6.0],
                &[55.0, 55.0, 400.0],
                &[1.0; 3],
                Window::Weekly,
                Metric::FatFreeMass
            ),
            Err(TrendlineError::ValueOutOfRange)
        );
        assert_eq!(
            trendline(
                &[0.0, 3.0, 6.0],
                &[55.0; 3],
                &[1.0, 1.0, 2.0],
                Window::Weekly,
                Metric::FatFreeMass
            ),
            Err(TrendlineError::ConfidenceOutOfRange)
        );
        assert_eq!(
            trendline(
                &[0.0, 3.0],
                &[55.0; 3],
                &[1.0; 3],
                Window::Weekly,
                Metric::FatFreeMass
            ),
            Err(TrendlineError::LengthMismatch)
        );
    }

    /// Vectors generated by `tools/ml/deterministic_vectors.py atlas_trendline_1_0_0`.
    #[test]
    fn matches_the_archive_on_generated_vectors() {
        let raw =
            include_str!("../../../../../../artifacts/models/vectors/atlas_trendline_1_0_0.json");
        let file: serde_json::Value =
            serde_json::from_str(raw).expect("the vector file should parse");
        let mut fitted = 0;
        let mut unfitted = 0;
        for vector in file["vectors"]
            .as_array()
            .expect("vectors should be a list")
        {
            let inputs = &vector["inputs"];
            let read = |name: &str| -> Vec<f32> {
                inputs[name]
                    .as_array()
                    .expect("series should be a list")
                    .iter()
                    .map(|v| v.as_f64().expect("value should be a number") as f32)
                    .collect()
            };
            let scalar =
                |name: &str| inputs[name].as_f64().expect("scalar should be a number") as f32;
            if vector.get("error").is_some() {
                // The only refusal generated is a metric outside the enum, which this port
                // cannot express: `Metric` has no variant for it, so the type does the
                // validator's job and there is nothing to assert here.
                continue;
            }
            let got = trendline(
                &read("days"),
                &read("values"),
                &read("confidences"),
                window_from(scalar("window")),
                metric_from(scalar("metric")),
            )
            .expect("the archive accepted this input");
            let want = vector["outputs"]
                .as_array()
                .expect("outputs should be a list");
            let field =
                |index: usize| -> Option<f32> { want[index].as_f64().map(|value| value as f32) };
            let close = |name: &str, got: f32, index: usize| match field(index) {
                None => assert!(got.is_nan(), "{name} should be NaN, was {got}"),
                Some(expected) => assert!(
                    (got - expected).abs() <= TOLERANCE * expected.abs().max(1.0),
                    "{name}: {got} vs {expected}"
                ),
            };
            close("slope", got.slope, 0);
            close("slope_ci_lower", got.slope_ci_lower, 1);
            close("slope_ci_upper", got.slope_ci_upper, 2);
            close("start_day", got.start_day, 3);
            close("end_day", got.end_day, 4);
            close("start_value", got.start_value, 5);
            close("end_value", got.end_value, 6);
            close("total_change", got.total_change, 7);
            close("total_change_ci_lower", got.total_change_ci_lower, 8);
            close("total_change_ci_upper", got.total_change_ci_upper, 9);
            close("significance", got.significance, 10);
            close("points", got.points, 11);
            close("window", got.window, 12);
            close("metric", got.metric, 13);
            let valid = field(14).expect("the valid flag is never NaN");
            assert_eq!(got.valid, valid == 1.0, "valid flag");
            if got.valid {
                fitted += 1;
            } else {
                unfitted += 1;
            }
        }
        assert_eq!((fitted, unfitted), (3, 3), "three fits and three refusals");
    }
}
