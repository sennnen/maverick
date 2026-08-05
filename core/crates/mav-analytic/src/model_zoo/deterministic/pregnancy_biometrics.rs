//! `pregnancy_biometrics 0.4.0` — a pregnancy's four biometrics against their expected bands.
//!
//! Heart rate, HRV, breath rate and temperature deviation, one reading per gestational day for
//! 350 days, each smoothed and compared with the band that day is expected to fall in.
//!
//! What makes it more than a rolling mean is where the baseline comes from. A wearer's own
//! baseline is taken from the first fifteen-day window holding at least eight usable readings —
//! and "usable" excludes every day their temperature deviation reached a full degree, because a
//! fever moves all four biometrics at once and a baseline measured through one is not theirs. If
//! the window is found, the population's own median change over those same days is subtracted,
//! so a baseline established at week 30 is not read as if it were week 5. Temperature is the
//! exception: it takes a population baseline by age group and never a personal one.
//!
//! Where no window qualifies, the biometric has no personal baseline at all. Its readings still
//! come back, but the band comparison does not: the archive returns an empty tensor rather than
//! comparing against a baseline it does not have, and this port returns `None` for the same
//! reason.
//!
//! Two behaviours here are the archive's rather than the obvious thing, and both are reproduced
//! deliberately — see [`smooth`] and [`band_for`].

use super::pregnancy_tables::{
    BIOMETRICS, GESTATIONAL_DAYS, POPULATION_BASELINE, POPULATION_MEDIAN, QUANTILE_RANGE,
};

/// Days in the baseline search window.
const BASELINE_WINDOW: usize = 15;

/// Usable readings that window must hold.
const BASELINE_MIN_VALID: usize = 8;

/// Temperature deviation at or above this masks the day out of every biometric.
const FEVER_DEVIATION: f32 = 1.0;

/// Days in the smoothing window.
const FILTER_WINDOW: usize = 7;

/// The opening days the "enough readings" rule is lifted for; see [`smooth`].
const RESTORED_LEAD_DAYS: usize = 3;

/// Age bands the population baseline is indexed by.
const AGE_BAND_YOUNG: f32 = 30.0;
const AGE_BAND_MIDDLE: f32 = 35.0;

/// Which biometric, in the archive's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Biometric {
    /// Average heart rate.
    HeartRate,
    /// Average heart-rate variability.
    HeartRateVariability,
    /// Average breath rate.
    BreathRate,
    /// Temperature deviation from the wearer's own normal.
    TemperatureDeviation,
}

impl Biometric {
    const ALL: [Self; BIOMETRICS] = [
        Self::HeartRate,
        Self::HeartRateVariability,
        Self::BreathRate,
        Self::TemperatureDeviation,
    ];

    fn index(self) -> usize {
        match self {
            Self::HeartRate => 0,
            Self::HeartRateVariability => 1,
            Self::BreathRate => 2,
            Self::TemperatureDeviation => 3,
        }
    }
}

/// Where a day's reading sits relative to its expected band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandPosition {
    /// Below the band.
    Below,
    /// Inside it.
    Inside,
    /// Above it.
    Above,
}

/// Why the archive refused the input, with the code it refuses under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PregnancyError {
    /// 102 — a series is not 350 days long.
    WrongSeriesLength,
    /// 201 — the age is outside the table's range.
    AgeOutOfRange,
    /// 202 — the gestational day is outside the table's range.
    GestationalDayOutOfRange,
    /// 203 — the backfill flag is neither zero nor one.
    BackfillNotBoolean,
}

impl PregnancyError {
    /// The archive's own code for this refusal.
    pub fn code(self) -> u16 {
        match self {
            Self::WrongSeriesLength => 102,
            Self::AgeOutOfRange => 201,
            Self::GestationalDayOutOfRange => 202,
            Self::BackfillNotBoolean => 203,
        }
    }
}

/// The archive's validation bounds, in its own order: the four biometrics, then age and day.
const VALIDATION_MIN: [f32; 6] = [0.0, 0.0, -7.0, 0.0, 0.0, 0.0];
const VALIDATION_MAX: [f32; 6] = [254.0, 254.0, 7.0, 100.0, 150.0, 349.0];

/// One biometric's answer for one day.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DayReading {
    /// The smoothed reading, absent where the window held too little to smooth.
    pub value: Option<f32>,
    /// Where it sits in its band, absent where there is no personal baseline.
    pub position: Option<BandPosition>,
    /// How far outside the band it is, zero when inside, absent for the same reason.
    pub deviation: Option<f32>,
}

/// One biometric's answer across the requested days.
#[derive(Debug, Clone, PartialEq)]
pub struct BiometricSeries {
    /// Which biometric this is.
    pub biometric: Biometric,
    /// The baseline in force, personal where one could be established.
    pub baseline: f32,
    /// Whether that baseline is the wearer's own rather than the population's.
    pub personal_baseline: bool,
    /// One entry per requested gestational day.
    pub days: Vec<DayReading>,
    /// The expected band, offset by the baseline — absent without a personal baseline.
    pub band: Option<Vec<(f32, f32)>>,
}

/// Why a day's reading could not be used, where it could not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayCode {
    /// The day is usable.
    Usable,
    /// 301 — the temperature reading for that day is missing.
    TemperatureMissing,
    /// 302 — too few readings around it to smooth.
    TooSparse,
}

/// Everything the archive returns for one call.
#[derive(Debug, Clone, PartialEq)]
pub struct PregnancyBiometrics {
    /// One series per biometric, in the archive's order.
    pub series: [BiometricSeries; BIOMETRICS],
    /// One code per requested day.
    pub codes: Vec<DayCode>,
}

/// The mean of the values that are present, or `None` when none is.
fn nanmean(values: &[f32]) -> Option<f32> {
    let mut sum = 0.0f32;
    let mut count = 0;
    for value in values {
        if !value.is_nan() {
            sum += value;
            count += 1;
        }
    }
    (count > 0).then(|| sum / count as f32)
}

/// Which age band the wearer falls in.
fn age_band(age: f32) -> usize {
    if age < AGE_BAND_YOUNG {
        0
    } else if age < AGE_BAND_MIDDLE {
        1
    } else {
        2
    }
}

/// The first fifteen-day window holding enough usable readings, if any does.
fn baseline_window(masked: &[f32]) -> Option<usize> {
    if masked.len() < BASELINE_WINDOW {
        return None;
    }
    let first = (0..=masked.len() - BASELINE_WINDOW).find(|start| {
        masked[*start..*start + BASELINE_WINDOW]
            .iter()
            .filter(|value| !value.is_nan())
            .count()
            >= BASELINE_MIN_VALID
    })?;
    // The archive re-checks the found index against the same bound the search already
    // respected. Kept because it is the archive's own guard, not because it can fire.
    (first <= masked.len() - BASELINE_WINDOW).then_some(first)
}

/// Smooth a debaselined series with the archive's trailing seven-day mean.
///
/// Two things here are not what a rolling mean would do, and both are the archive's:
///
/// * The window is *trailing* — six NaNs are padded on the front — so each day's value is the
///   mean of that day and the six before it, never a day after it.
/// * The "at least four of seven present" rule is applied, and then **lifted for the first
///   three days**, which the archive restores from the unmasked window mean afterwards. Those
///   days cannot have four readings behind them however complete the series is — day zero has
///   one — so applying the rule there would drop the start of every pregnancy. Days three
///   onwards keep it.
fn smooth(debaselined: &[f32]) -> Vec<f32> {
    let min_present = FILTER_WINDOW / 2 + 1;
    let mut out = Vec::with_capacity(debaselined.len());
    for day in 0..debaselined.len() {
        // The padded window: positions before day zero are absent.
        let start = (day + 1).saturating_sub(FILTER_WINDOW);
        let window = &debaselined[start..=day];
        let present = window.iter().filter(|value| !value.is_nan()).count();
        let mean = nanmean(window);
        let enough = present >= min_present || day < RESTORED_LEAD_DAYS;
        out.push(if enough {
            mean.unwrap_or(f32::NAN)
        } else {
            f32::NAN
        });
    }
    out
}

/// Where an absolute reading sits against a day's band, and how far outside it is.
///
/// The comparison is the archive's and it is not the one the names suggest: the band is a
/// *deviation* range but the value compared against it is the reading with its baseline added
/// back, so a heart rate of 66 is read as "above" a band of −3.9 to 24. The band that comes
/// back out has the baseline added to it, which is the form a caller can plot against the
/// reading. Reproduced exactly, because the position and the distance are what downstream
/// thresholds were fitted to.
fn band_for(value: f32, day: usize, biometric: Biometric) -> (BandPosition, f32) {
    let low = QUANTILE_RANGE[biometric.index()][0][day];
    let high = QUANTILE_RANGE[biometric.index()][1][day];
    if value < low {
        (BandPosition::Below, low - value)
    } else if value > high {
        (BandPosition::Above, value - high)
    } else {
        (BandPosition::Inside, 0.0)
    }
}

fn validate(
    series: &[&[f32]; BIOMETRICS],
    age: f32,
    gestational_day: f32,
    backfill: f32,
) -> Result<(), PregnancyError> {
    if series.iter().any(|s| s.len() != GESTATIONAL_DAYS) {
        return Err(PregnancyError::WrongSeriesLength);
    }
    if !age.is_nan() && !(VALIDATION_MIN[4]..=VALIDATION_MAX[4]).contains(&age) {
        return Err(PregnancyError::AgeOutOfRange);
    }
    if !gestational_day.is_nan()
        && !(VALIDATION_MIN[5]..=VALIDATION_MAX[5]).contains(&gestational_day)
    {
        return Err(PregnancyError::GestationalDayOutOfRange);
    }
    if backfill != 0.0 && backfill != 1.0 {
        return Err(PregnancyError::BackfillNotBoolean);
    }
    Ok(())
}

/// Establish one biometric's baseline and smooth its series.
fn process(
    samples: &[f32],
    temperature: &[f32],
    biometric: Biometric,
    age: f32,
) -> (Vec<f32>, f32, bool) {
    // A fever moves every biometric at once, so the days it covers are masked out of the
    // baseline search for all four — not only for temperature.
    let masked: Vec<f32> = samples
        .iter()
        .zip(temperature)
        .map(|(value, degrees)| {
            if *degrees >= FEVER_DEVIATION {
                f32::NAN
            } else {
                *value
            }
        })
        .collect();

    let population = POPULATION_BASELINE[age_band(age)][biometric.index()];
    let (baseline, personal) = match baseline_window(&masked) {
        Some(start) => {
            let window = &masked[start..start + BASELINE_WINDOW];
            let own = nanmean(window).unwrap_or(f32::NAN);
            if biometric == Biometric::TemperatureDeviation {
                // Temperature never takes a personal baseline, even when a window qualifies:
                // its deviation is already relative to the wearer.
                (population, true)
            } else {
                // Subtract the population's own median change over the same days, so where in
                // the pregnancy the window fell does not bias the baseline.
                let change: Vec<f32> = POPULATION_MEDIAN[biometric.index()]
                    [start..start + BASELINE_WINDOW]
                    .iter()
                    .zip(window)
                    .map(|(median, sample)| if sample.is_nan() { f32::NAN } else { *median })
                    .collect();
                (own - nanmean(&change).unwrap_or(0.0), true)
            }
        }
        // No window qualified. Temperature still has somewhere to fall back to; the others
        // are left at zero and marked as having no personal baseline.
        None if biometric == Biometric::TemperatureDeviation => (population, false),
        None => (0.0, false),
    };

    // Smoothing runs on the *unmasked* series: the fever days are excluded from establishing
    // the baseline, not from being reported.
    let debaselined: Vec<f32> = samples.iter().map(|value| value - baseline).collect();
    (smooth(&debaselined), baseline, personal)
}

/// Run the archive's biometric comparison for one wearer.
///
/// `backfill` false returns only the requested day; true returns every day up to it.
pub fn pregnancy_biometrics(
    heart_rate: &[f32],
    heart_rate_variability: &[f32],
    breath_rate: &[f32],
    temperature_deviation: &[f32],
    age: f32,
    gestational_day: f32,
    backfill: bool,
) -> Result<PregnancyBiometrics, PregnancyError> {
    let inputs = [
        heart_rate,
        heart_rate_variability,
        breath_rate,
        temperature_deviation,
    ];
    validate(&inputs, age, gestational_day, f32::from(u8::from(backfill)))?;

    let day = gestational_day as usize;
    let start = if backfill { 0 } else { day };

    let mut series = Vec::with_capacity(BIOMETRICS);
    for (biometric, samples) in Biometric::ALL.iter().zip(inputs) {
        let (filtered, baseline, personal) =
            process(samples, temperature_deviation, *biometric, age);
        let days: Vec<DayReading> = (start..=day)
            .map(|index| {
                let smoothed = filtered[index];
                // The band is judged on the reading with its baseline added back, and a day
                // whose smoothed value is absent has no position either.
                let absolute = smoothed + baseline;
                let judged =
                    (!smoothed.is_nan() && personal).then(|| band_for(absolute, index, *biometric));
                DayReading {
                    value: (!smoothed.is_nan()).then_some(absolute),
                    position: judged.map(|(position, _)| position),
                    deviation: judged.map(|(_, distance)| distance),
                }
            })
            .collect();
        let band = personal.then(|| {
            (0..GESTATIONAL_DAYS)
                .map(|index| {
                    let low = QUANTILE_RANGE[biometric.index()][0][index] + baseline;
                    let high = QUANTILE_RANGE[biometric.index()][1][index] + baseline;
                    // HRV's lower band is clamped at zero: a negative variability is not a
                    // reading anyone can be below.
                    let low = if *biometric == Biometric::HeartRateVariability {
                        low.max(0.0)
                    } else {
                        low
                    };
                    (low, high)
                })
                .collect()
        });
        series.push(BiometricSeries {
            biometric: *biometric,
            baseline,
            personal_baseline: personal,
            days,
            band,
        });
    }

    // The day codes describe the *temperature* series, because that is what gates every
    // other biometric's usability.
    let min_present = FILTER_WINDOW / 2 + 1;
    let codes = (start..=day)
        .map(|index| {
            let window_start = (index + 1).saturating_sub(FILTER_WINDOW);
            let present = temperature_deviation[window_start..=index]
                .iter()
                .filter(|value| !value.is_nan())
                .count();
            // Order matters: a missing temperature is reported as missing even when the
            // window around it is also too sparse, because the archive writes 301 last.
            if temperature_deviation[index].is_nan() {
                DayCode::TemperatureMissing
            } else if present < min_present {
                DayCode::TooSparse
            } else {
                DayCode::Usable
            }
        })
        .collect();

    let series: [BiometricSeries; BIOMETRICS] = series
        .try_into()
        .unwrap_or_else(|_| unreachable!("one series per biometric was pushed"));
    Ok(PregnancyBiometrics { series, codes })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// float32 through decimal, over a fifteen-day mean of a 350-day series.
    const TOLERANCE: f32 = 2e-3;

    fn flat(value: f32) -> Vec<f32> {
        vec![value; GESTATIONAL_DAYS]
    }

    #[test]
    fn a_fever_is_excluded_from_the_baseline_but_not_from_the_readings() {
        let mut temperature = flat(0.0);
        // The first forty days are feverish, so the baseline window has to start after them.
        for slot in temperature.iter_mut().take(40) {
            *slot = 1.4;
        }
        let mut heart_rate = flat(60.0);
        for slot in heart_rate.iter_mut().take(40) {
            *slot = 95.0;
        }
        let got = pregnancy_biometrics(
            &heart_rate,
            &flat(48.0),
            &flat(15.5),
            &temperature,
            30.0,
            100.0,
            false,
        )
        .expect("valid");
        let hr = &got.series[0];
        assert!(hr.personal_baseline);
        // The 95 bpm fever days must not have pulled the baseline up towards them.
        assert!(hr.baseline < 70.0, "baseline was {}", hr.baseline);
        // But day 10 is still reported when asked for.
        let feverish = pregnancy_biometrics(
            &heart_rate,
            &flat(48.0),
            &flat(15.5),
            &temperature,
            30.0,
            10.0,
            false,
        )
        .expect("valid");
        assert!(feverish.series[0].days[0].value.is_some());
    }

    #[test]
    fn without_a_qualifying_window_there_is_no_band_at_all() {
        // Every reading missing: no window can hold eight of them.
        let empty = vec![f32::NAN; GESTATIONAL_DAYS];
        let got = pregnancy_biometrics(&empty, &empty, &empty, &empty, 30.0, 100.0, false)
            .expect("valid");
        for series in &got.series {
            assert!(!series.personal_baseline, "{:?}", series.biometric);
            assert!(series.band.is_none(), "{:?}", series.biometric);
            assert!(series.days[0].position.is_none());
        }
    }

    #[test]
    fn temperature_takes_a_population_baseline_by_age_band() {
        let young = pregnancy_biometrics(
            &flat(60.0),
            &flat(48.0),
            &flat(15.5),
            &flat(0.0),
            25.0,
            100.0,
            false,
        )
        .expect("valid");
        let older = pregnancy_biometrics(
            &flat(60.0),
            &flat(48.0),
            &flat(15.5),
            &flat(0.0),
            40.0,
            100.0,
            false,
        )
        .expect("valid");
        assert_ne!(
            young.series[3].baseline, older.series[3].baseline,
            "the age bands should not share a temperature baseline"
        );
        assert_eq!(young.series[3].baseline, POPULATION_BASELINE[0][3]);
        assert_eq!(older.series[3].baseline, POPULATION_BASELINE[2][3]);
    }

    #[test]
    fn backfill_returns_every_day_up_to_the_requested_one() {
        let got = pregnancy_biometrics(
            &flat(60.0),
            &flat(48.0),
            &flat(15.5),
            &flat(0.0),
            30.0,
            120.0,
            true,
        )
        .expect("valid");
        assert_eq!(got.series[0].days.len(), 121);
        assert_eq!(got.codes.len(), 121);
        let single = pregnancy_biometrics(
            &flat(60.0),
            &flat(48.0),
            &flat(15.5),
            &flat(0.0),
            30.0,
            120.0,
            false,
        )
        .expect("valid");
        assert_eq!(single.series[0].days.len(), 1);
    }

    #[test]
    fn the_sparse_rule_is_lifted_for_the_opening_days_only() {
        // The first three days cannot have four readings behind them, so they keep whatever
        // they have. A day further in with the same single reading is dropped.
        let mut series = vec![f32::NAN; GESTATIONAL_DAYS];
        series[0] = 5.0;
        series[30] = 5.0;
        let smoothed = smooth(&series);
        assert!(smoothed[0].is_finite(), "day zero keeps its one reading");
        assert!(smoothed[1].is_finite(), "so does day one");
        assert!(smoothed[2].is_finite(), "and day two");
        assert!(smoothed[3].is_nan(), "day three does not");
        assert!(smoothed[30].is_nan(), "nor does a lone reading later on");
    }

    #[test]
    fn hrv_lower_band_is_clamped_at_zero() {
        let got = pregnancy_biometrics(
            &flat(60.0),
            &flat(48.0),
            &flat(15.5),
            &flat(0.0),
            30.0,
            200.0,
            false,
        )
        .expect("valid");
        let band = got.series[1].band.as_ref().expect("hrv has a band");
        assert!(band.iter().all(|(low, _)| *low >= 0.0));
        // At least one day would have gone negative without the clamp.
        assert!(band.iter().any(|(low, _)| *low == 0.0));
    }

    #[test]
    fn refuses_the_inputs_the_archive_refuses() {
        assert_eq!(
            pregnancy_biometrics(
                &flat(60.0),
                &flat(48.0),
                &flat(15.5),
                &flat(0.0),
                30.0,
                400.0,
                false
            ),
            Err(PregnancyError::GestationalDayOutOfRange)
        );
        assert_eq!(
            pregnancy_biometrics(
                &flat(60.0),
                &flat(48.0),
                &flat(15.5),
                &flat(0.0),
                200.0,
                100.0,
                false
            ),
            Err(PregnancyError::AgeOutOfRange)
        );
        assert_eq!(
            pregnancy_biometrics(
                &[60.0],
                &flat(48.0),
                &flat(15.5),
                &flat(0.0),
                30.0,
                10.0,
                false
            ),
            Err(PregnancyError::WrongSeriesLength)
        );
    }

    /// Vectors generated by `tools/ml/deterministic_vectors.py pregnancy_biometrics_0_4_0`.
    #[test]
    fn matches_the_archive_on_generated_vectors() {
        let raw = include_str!(
            "../../../../../../artifacts/models/vectors/pregnancy_biometrics_0_4_0.json"
        );
        let file: serde_json::Value =
            serde_json::from_str(raw).expect("the vector file should parse");
        let mut checked = 0;
        for vector in file["vectors"]
            .as_array()
            .expect("vectors should be a list")
        {
            let inputs = &vector["inputs"];
            // Each series is a column vector; a missing reading is written as null.
            let column = |name: &str| -> Vec<f32> {
                inputs[name]
                    .as_array()
                    .expect("a column")
                    .iter()
                    .map(|row| {
                        row.as_array().expect("a row")[0]
                            .as_f64()
                            .map_or(f32::NAN, |value| value as f32)
                    })
                    .collect()
            };
            let scalar = |name: &str| -> f32 {
                inputs[name].as_array().expect("a list")[0]
                    .as_f64()
                    .expect("a number") as f32
            };
            let got = pregnancy_biometrics(
                &column("average_heart_rate"),
                &column("average_hrv"),
                &column("average_breath"),
                &column("temperature_deviation"),
                scalar("age"),
                scalar("gestational_day"),
                scalar("is_backfill") == 1.0,
            );
            match vector.get("error").and_then(|e| e.as_str()) {
                Some(message) => {
                    let error = got.expect_err("the archive refused this input");
                    assert!(
                        message.contains(&format!("{}", error.code()))
                            || message.contains("outside allowed range"),
                        "expected code {} for {message}",
                        error.code()
                    );
                }
                None => {
                    let got = got.expect("the archive produced a result");
                    let want = vector["outputs"].as_array().expect("outputs are a list");
                    // Outputs 0..3 are the smoothed readings, 4..7 the band positions,
                    // 8..11 the distances, 12..19 the bands, 20 the debug block.
                    let rows = |index: usize| -> Vec<Option<f32>> {
                        want[index]
                            .as_array()
                            .expect("a list")
                            .iter()
                            .map(|row| {
                                row.as_array().expect("a row")[0]
                                    .as_f64()
                                    .map(|value| value as f32)
                            })
                            .collect()
                    };
                    for (slot, series) in got.series.iter().enumerate() {
                        let values = rows(slot);
                        assert_eq!(series.days.len(), values.len(), "series {slot} length");
                        for (index, day) in series.days.iter().enumerate() {
                            match values[index] {
                                None => assert!(
                                    day.value.is_none(),
                                    "series {slot} day {index} should be absent"
                                ),
                                Some(expected) => {
                                    let value = day.value.unwrap_or_else(|| {
                                        panic!("series {slot} day {index} absent, archive had {expected}")
                                    });
                                    assert!(
                                        (value - expected).abs()
                                            <= TOLERANCE * expected.abs().max(1.0),
                                        "series {slot} day {index}: {value} vs {expected}"
                                    );
                                }
                            }
                        }
                        let positions = rows(4 + slot);
                        let distances = rows(8 + slot);
                        if positions.is_empty() {
                            assert!(
                                !series.personal_baseline,
                                "series {slot} reported a band the archive did not"
                            );
                            continue;
                        }
                        for (index, day) in series.days.iter().enumerate() {
                            let expected = positions[index];
                            let position = day.position.map(|position| match position {
                                BandPosition::Below => 0.0f32,
                                BandPosition::Inside => 1.0,
                                BandPosition::Above => 2.0,
                            });
                            assert_eq!(position, expected, "series {slot} day {index} position");
                            if let (Some(distance), Some(target)) =
                                (day.deviation, distances[index])
                            {
                                assert!(
                                    (distance - target).abs() <= TOLERANCE * target.abs().max(1.0),
                                    "series {slot} day {index} distance: {distance} vs {target}"
                                );
                            }
                        }
                        let low = rows(12 + slot * 2);
                        let high = rows(13 + slot * 2);
                        let band = series.band.as_ref().expect("a band");
                        assert_eq!(band.len(), low.len(), "series {slot} band length");
                        for (index, (got_low, got_high)) in band.iter().enumerate() {
                            let (want_low, want_high) = (
                                low[index].expect("a low bound"),
                                high[index].expect("a high bound"),
                            );
                            assert!(
                                (got_low - want_low).abs() <= TOLERANCE * want_low.abs().max(1.0),
                                "series {slot} band {index} low: {got_low} vs {want_low}"
                            );
                            assert!(
                                (got_high - want_high).abs()
                                    <= TOLERANCE * want_high.abs().max(1.0),
                                "series {slot} band {index} high: {got_high} vs {want_high}"
                            );
                        }
                    }
                    // The debug block's last column is the day code.
                    let debug = want[20].as_array().expect("a debug block");
                    assert_eq!(got.codes.len(), debug.len(), "code count");
                    for (index, code) in got.codes.iter().enumerate() {
                        let row = debug[index].as_array().expect("a debug row");
                        let expected = row[4].as_f64().expect("a code") as u16;
                        let mine = match code {
                            DayCode::Usable => 0,
                            DayCode::TemperatureMissing => 301,
                            DayCode::TooSparse => 302,
                        };
                        assert_eq!(mine, expected, "day {index} code");
                    }
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 6, "every produced vector should be checked");
    }
}
