//! `stress_resilience 2.2.1` — how well a fortnight of recovery is keeping up with its stress.
//!
//! Resilience is not "low stress" and not "good sleep". It is the *relationship* between the
//! two: a fortnight carrying heavy load with recovery to match scores higher than a quiet one
//! that recovers badly. The archive expresses that as a curve fitted through stress, and asks
//! where this wearer's recovery sits relative to it — four bands above and below the curve,
//! each one PCA minor axis wide, giving levels one through five.
//!
//! Getting there takes three stages:
//!
//! 1. **The day's stress.** Readings taken while asleep are dropped, the rest are averaged into
//!    ten-minute buckets and laid on an even grid, and each bucket is sorted into one of seven
//!    bands from high stress through neutral to high recovery. Those counts, weighted 4/3/2/1
//!    by intensity, become a stress percentage and a restorative-time percentage.
//! 2. **The night's recovery.** Sleep score, HRV balance, recovery index and resting heart rate
//!    are averaged by weight — HRV balance dropping out of both the sum and the divisor when it
//!    is absent — and put through a cubic that maps the average onto a 0–100 scale.
//! 3. **The fortnight.** Today joins the previous thirteen days, weighted linearly from one for
//!    the oldest to two for today, and the weighted means are compared with the curve.
//!
//! A day with under four hours of waking stress readings scores no daily values at all, but
//! still contributes its absence to the fortnight. A fortnight with fewer than five scored days
//! produces no resilience level, because the curve cannot be read from four points.

/// Minutes per resampling bucket.
const RESOLUTION_MINUTES: i64 = 10;

/// Waking hours of stress readings a day needs before it is scored.
const MIN_DAYTIME_HOURS: f32 = 4.0;

/// Where the moderate band ends and the high one begins, as a fraction of the way from the
/// limit to saturation.
const MODERATE_TO_HIGH: f32 = 0.7;

/// …and where low becomes moderate.
const LOW_TO_MODERATE: f32 = 0.3;

/// Band weights, from high through neutral.
const HIGH_WEIGHT: f32 = 4.0;
const MODERATE_WEIGHT: f32 = 3.0;
const LOW_WEIGHT: f32 = 2.0;
const NEUTRAL_WEIGHT: f32 = 1.0;

/// Contributor weights for the night's recovery.
const SLEEP_SCORE_WEIGHT: f32 = 0.4;
const HRV_BALANCE_WEIGHT: f32 = 0.2;
const RECOVERY_INDEX_WEIGHT: f32 = 0.2;
const RESTING_HEART_RATE_WEIGHT: f32 = 0.2;

/// The cubic mapping the weighted contributor average onto a 0–100 recovery scale.
const SLEEP_RECOVERY_CURVE: [f32; 4] = [8.03e-5, 0.005_347_2, -0.183_514_7, -0.332_090_1];

/// How the fortnight's recovery is split between daytime and sleep.
const DAYTIME_RECOVERY_WEIGHT: f32 = 0.3;
const SLEEP_RECOVERY_WEIGHT: f32 = 0.7;

/// Days in the resilience window, and the weight at each end of it.
const WINDOW_DAYS: usize = 14;
const OLDEST_DAY_WEIGHT: f32 = 1.0;
const TODAY_WEIGHT: f32 = 2.0;

/// Scored days the window needs before a level can be read.
const MIN_SCORED_DAYS: usize = 5;

/// The curve resilience is measured against: `c0·x² + c1·x + c2` in stress.
const PLANE_FIT: [f32; 3] = [-0.001_79, -0.191_29, 65.391];

/// Half-width of one resilience band, in recovery units.
const PCA_MINOR_AXIS: f32 = 16.0;

/// Band edges as multiples of [`PCA_MINOR_AXIS`], below and above the curve.
const LEVEL_MULTIPLIERS: [f32; 4] = [-0.9, -0.3, 0.3, 0.9];

/// The largest fractional part the outermost levels may report.
const MAX_EDGE_FRACTION: f32 = 0.99;

/// Granular resilience is reported to this many decimals, inside these bounds.
const GRANULAR_MIN: f32 = 1.01;
const GRANULAR_MAX: f32 = 5.99;

/// The nine bands each daily index is quantised into. Anything outside reports zero.
const STRESS_BANDS: [f32; 9] = [0.0, 39.0, 46.0, 51.0, 60.0, 68.0, 76.0, 82.0, 90.0];
const RESTORATIVE_BANDS: [f32; 9] = [0.0, 7.0, 13.0, 17.0, 24.0, 31.0, 40.0, 45.0, 53.0];
const SLEEP_RECOVERY_BANDS: [f32; 9] = [0.0, 34.0, 39.0, 43.0, 48.0, 53.0, 58.0, 62.0, 67.0];

/// Why the archive refused the input, with the code it refuses under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResilienceError {
    /// 10 — the stress values and their timestamps are different lengths.
    StressLengthMismatch,
    /// 11 — a sleep timestamp is missing.
    SleepTimestampMissing,
    /// 12 — the sleep starts and ends are different lengths.
    SleepLengthMismatch,
    /// 13 — a sleep period ends before it starts.
    SleepPeriodReversed,
    /// 15 — a history list is not thirteen days long.
    HistoryWrongLength,
}

impl ResilienceError {
    /// The archive's own code for this refusal.
    pub fn code(self) -> u8 {
        match self {
            Self::StressLengthMismatch => 10,
            Self::SleepTimestampMissing => 11,
            Self::SleepLengthMismatch => 12,
            Self::SleepPeriodReversed => 13,
            Self::HistoryWrongLength => 15,
        }
    }
}

/// The band-crossing points a day's stress readings are sorted into.
#[derive(Debug, Clone, Copy)]
pub struct StressLimits {
    /// Below this the wearer is stressed rather than neutral.
    pub stress_limit: f32,
    /// The most negative deviation recorded; readings are clamped up to it.
    pub saturation_stress: f32,
    /// Above this the wearer is recovering rather than neutral.
    pub recovery_limit: f32,
    /// The most positive deviation recorded; readings are clamped down to it.
    pub saturation_recovery: f32,
}

/// The night's four recovery contributors.
#[derive(Debug, Clone, Copy)]
pub struct SleepContributors {
    /// Sleep score, 0–100.
    pub sleep_score: f32,
    /// HRV balance, or `None` where it is absent — it drops out of the average entirely.
    pub hrv_balance: Option<f32>,
    /// Recovery index, 0–100.
    pub recovery_index: f32,
    /// Resting heart rate contributor, 0–100.
    pub resting_heart_rate: f32,
}

/// A day's own indices, absent where too little of the day was measured.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DailyIndices {
    /// Percentage of weighted daytime spent stressed.
    pub stress: Option<f32>,
    /// Percentage spent restoring.
    pub restorative_time: Option<f32>,
    /// The night's recovery on a 0–100 scale.
    pub sleep_recovery: Option<f32>,
    /// The above, quantised into nine bands; zero where the value is absent or out of range.
    pub quantised_stress: u8,
    /// Quantised restorative time.
    pub quantised_restorative_time: u8,
    /// Quantised sleep recovery.
    pub quantised_sleep_recovery: u8,
}

/// The fortnight's answer, absent where too few days were scored.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Resilience {
    /// Weighted mean restorative time across the window.
    pub long_term_restorative_time: Option<f32>,
    /// Weighted mean sleep recovery.
    pub long_term_sleep_recovery: Option<f32>,
    /// The two combined, 30% daytime and 70% sleep.
    pub long_term_recovery: Option<f32>,
    /// Weighted mean stress.
    pub long_term_stress: Option<f32>,
    /// Resilience level, one through five.
    pub level: Option<u8>,
    /// The level with a fraction saying where inside its band the wearer sits.
    pub granular_level: Option<f32>,
    /// How much of the window was scored, as a fraction.
    pub confidence: Option<f32>,
}

/// Everything one call returns.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResilienceOutcome {
    /// Today's indices.
    pub daily: DailyIndices,
    /// The fortnight's.
    pub resilience: Resilience,
}

/// Horner evaluation, coefficients highest power first — the archive's `polyval`.
fn polyval(coefficients: &[f32], x: f32) -> f32 {
    coefficients.iter().fold(0.0, |accumulated, coefficient| {
        accumulated * x + coefficient
    })
}

/// Which of the nine bands a value falls in, or zero when it is outside them.
fn quantise(value: f32, bands: &[f32; 9]) -> u8 {
    if value.is_nan() || value <= bands[0] || value > 100.0 {
        return 0;
    }
    let mut band = 9;
    for (index, edge) in bands.iter().enumerate().skip(1) {
        if value < *edge {
            band = index as u8;
            break;
        }
    }
    band
}

/// Average the readings that fall in each ten-minute bucket, on an even grid.
///
/// Buckets with no reading come back absent rather than interpolated: a gap in the day is a
/// gap, and the coverage check below counts on being able to see it.
fn resample(values: &[f32], timestamps_ms: &[i64], limits: &StressLimits) -> Vec<f32> {
    let interval = RESOLUTION_MINUTES * 60;
    let mut buckets: Vec<(i64, Vec<f32>)> = Vec::new();
    for (value, timestamp) in values.iter().zip(timestamps_ms) {
        if value.is_nan() {
            continue;
        }
        let key = (timestamp / 1000).div_euclid(interval) * interval;
        // Saturate first: a reading past either extreme is recorded at the extreme, so one
        // outlier cannot drag a bucket's mean beyond what the scale can express.
        let clamped = value.clamp(limits.saturation_stress, limits.saturation_recovery);
        match buckets.iter_mut().find(|(bucket, _)| *bucket == key) {
            Some((_, held)) => held.push(clamped),
            None => buckets.push((key, vec![clamped])),
        }
    }
    if buckets.is_empty() {
        return Vec::new();
    }
    buckets.sort_by_key(|(key, _)| *key);
    let first = buckets[0].0;
    let last = buckets[buckets.len() - 1].0;
    let mut out = Vec::new();
    let mut at = first;
    while at <= last {
        let mean = buckets
            .iter()
            .find(|(key, _)| *key == at)
            .map(|(_, held)| held.iter().sum::<f32>() / held.len() as f32);
        out.push(mean.unwrap_or(f32::NAN));
        at += interval;
    }
    out
}

/// The seven band counts, high stress through high recovery.
fn band_counts(resampled: &[f32], limits: &StressLimits) -> [f32; 7] {
    let stress_span = limits.saturation_stress - limits.stress_limit;
    let recovery_span = limits.saturation_recovery - limits.recovery_limit;
    let high_stress = limits.stress_limit + stress_span * MODERATE_TO_HIGH;
    let low_stress = limits.stress_limit + stress_span * LOW_TO_MODERATE;
    let low_recovery = limits.recovery_limit + recovery_span * LOW_TO_MODERATE;
    let high_recovery = limits.recovery_limit + recovery_span * MODERATE_TO_HIGH;

    let mut counts = [0.0f32; 7];
    for value in resampled.iter().filter(|value| !value.is_nan()) {
        // The stress side runs downwards: `stress_span` is negative, so "further than the
        // high threshold" is `<`, not `>`.
        let slot = if *value < high_stress {
            0
        } else if *value < low_stress {
            1
        } else if *value < limits.stress_limit {
            2
        } else if *value <= limits.recovery_limit {
            3
        } else if *value <= low_recovery {
            4
        } else if *value <= high_recovery {
            5
        } else {
            6
        };
        counts[slot] += 1.0;
    }
    counts
}

/// The night's recovery on a 0–100 scale.
fn sleep_recovery(contributors: &SleepContributors) -> f32 {
    let mut sum = contributors.sleep_score * SLEEP_SCORE_WEIGHT
        + contributors.recovery_index * RECOVERY_INDEX_WEIGHT
        + contributors.resting_heart_rate * RESTING_HEART_RATE_WEIGHT;
    let mut divisor = SLEEP_SCORE_WEIGHT + RECOVERY_INDEX_WEIGHT + RESTING_HEART_RATE_WEIGHT;
    if let Some(balance) = contributors.hrv_balance {
        // Absent HRV balance leaves both the sum and the divisor, so the remaining three
        // contributors keep their relative weights instead of being diluted.
        sum += balance * HRV_BALANCE_WEIGHT;
        divisor += HRV_BALANCE_WEIGHT;
    }
    polyval(&SLEEP_RECOVERY_CURVE, sum / divisor).clamp(0.0, 100.0)
}

/// Which resilience band the fortnight falls in.
fn level_for(recovery: f32, stress: f32) -> u8 {
    let curve = polyval(&PLANE_FIT, stress);
    for (index, multiplier) in LEVEL_MULTIPLIERS.iter().enumerate() {
        if recovery < curve + PCA_MINOR_AXIS * multiplier {
            return index as u8 + 1;
        }
    }
    5
}

/// Where inside its band the fortnight sits, as `level + fraction`.
fn granular_level(recovery: f32, stress: f32, level: u8) -> f32 {
    let curve = polyval(&PLANE_FIT, stress);
    let edge = |index: usize| curve + PCA_MINOR_AXIS * LEVEL_MULTIPLIERS[index];
    let value = match level {
        2..=4 => {
            let below = edge(usize::from(level) - 2);
            let above = edge(usize::from(level) - 1);
            f32::from(level) + (recovery - below) / (above - below)
        }
        // The outermost levels are open-ended, so their fraction is measured against one
        // band's width and capped just short of rolling over into the next level.
        1 => {
            let span = edge(2) - edge(1);
            let distance = edge(0) - recovery;
            let fraction = if distance > span {
                MAX_EDGE_FRACTION
            } else {
                distance / span
            };
            f32::from(level) + 1.0 - fraction
        }
        _ => {
            let span = edge(2) - edge(1);
            let distance = recovery - edge(3);
            let fraction = if distance > span {
                MAX_EDGE_FRACTION
            } else {
                distance / span
            };
            f32::from(level) + fraction
        }
    };
    ((value * 100.0).round() / 100.0).clamp(GRANULAR_MIN, GRANULAR_MAX)
}

fn validate(
    sleep_starts: &[i64],
    sleep_ends: &[i64],
    stress: &[f32],
    stress_timestamps_ms: &[i64],
    history: &[DailyHistory],
) -> Result<(), ResilienceError> {
    if sleep_starts.len() != sleep_ends.len() {
        return Err(ResilienceError::SleepLengthMismatch);
    }
    if sleep_starts
        .iter()
        .chain(sleep_ends)
        .any(|value| *value == 0)
    {
        return Err(ResilienceError::SleepTimestampMissing);
    }
    if sleep_starts
        .iter()
        .zip(sleep_ends)
        .any(|(start, end)| start > end)
    {
        return Err(ResilienceError::SleepPeriodReversed);
    }
    if stress.len() != stress_timestamps_ms.len() {
        return Err(ResilienceError::StressLengthMismatch);
    }
    if history.len() != WINDOW_DAYS - 1 {
        return Err(ResilienceError::HistoryWrongLength);
    }
    Ok(())
}

/// One earlier day's contribution to the window.
#[derive(Debug, Clone, Copy)]
pub struct DailyHistory {
    /// That day's stress percentage, absent where it was not scored.
    pub stress: Option<f32>,
    /// Its restorative-time percentage.
    pub restorative_time: Option<f32>,
    /// Its sleep recovery.
    pub sleep_recovery: Option<f32>,
}

/// Score today and read the fortnight's resilience.
pub fn resilience(
    sleep_starts_ms: &[i64],
    sleep_ends_ms: &[i64],
    stress: &[f32],
    stress_timestamps_ms: &[i64],
    limits: &StressLimits,
    contributors: &SleepContributors,
    history: &[DailyHistory],
) -> Result<ResilienceOutcome, ResilienceError> {
    validate(
        sleep_starts_ms,
        sleep_ends_ms,
        stress,
        stress_timestamps_ms,
        history,
    )?;

    // Readings taken while asleep describe the night, not the day, and the night is measured
    // separately by the sleep contributors.
    let mut waking = Vec::new();
    let mut waking_timestamps = Vec::new();
    for (value, timestamp) in stress.iter().zip(stress_timestamps_ms) {
        let asleep = sleep_starts_ms
            .iter()
            .zip(sleep_ends_ms)
            .any(|(start, end)| timestamp >= start && timestamp <= end);
        if !asleep {
            waking.push(*value);
            waking_timestamps.push(*timestamp);
        }
    }

    let resampled = resample(&waking, &waking_timestamps, limits);
    let present = resampled.iter().filter(|value| !value.is_nan()).count();
    let covered_minutes = RESOLUTION_MINUTES as f32 * present as f32;
    let enough = !waking.is_empty() && covered_minutes >= MIN_DAYTIME_HOURS * 60.0;

    let daily = if enough {
        let counts = band_counts(&resampled, limits);
        let stressed =
            counts[0] * HIGH_WEIGHT + counts[1] * MODERATE_WEIGHT + counts[2] * LOW_WEIGHT;
        let restoring =
            counts[6] * HIGH_WEIGHT + counts[5] * MODERATE_WEIGHT + counts[4] * LOW_WEIGHT;
        let total = stressed + restoring + counts[3] * NEUTRAL_WEIGHT;
        let stress_percent = 100.0 * stressed / total;
        let restorative_percent = 100.0 * restoring / total;
        let recovery = sleep_recovery(contributors);
        DailyIndices {
            stress: Some(stress_percent),
            restorative_time: Some(restorative_percent),
            sleep_recovery: Some(recovery),
            quantised_stress: quantise(stress_percent, &STRESS_BANDS),
            quantised_restorative_time: quantise(restorative_percent, &RESTORATIVE_BANDS),
            quantised_sleep_recovery: quantise(recovery, &SLEEP_RECOVERY_BANDS),
        }
    } else {
        // A day that could not be scored still takes its place in the window, as an absence.
        DailyIndices {
            stress: None,
            restorative_time: None,
            sleep_recovery: None,
            quantised_stress: 0,
            quantised_restorative_time: 0,
            quantised_sleep_recovery: 0,
        }
    };

    // Today goes on the end of the window, and the weights run from oldest to newest.
    let mut stresses: Vec<Option<f32>> = history.iter().map(|day| day.stress).collect();
    let mut restoratives: Vec<Option<f32>> =
        history.iter().map(|day| day.restorative_time).collect();
    let mut recoveries: Vec<Option<f32>> = history.iter().map(|day| day.sleep_recovery).collect();
    stresses.push(daily.stress);
    restoratives.push(daily.restorative_time);
    recoveries.push(daily.sleep_recovery);

    let scored = stresses.iter().filter(|value| value.is_some()).count();
    if scored < MIN_SCORED_DAYS {
        return Ok(ResilienceOutcome {
            daily,
            resilience: Resilience {
                long_term_restorative_time: None,
                long_term_sleep_recovery: None,
                long_term_recovery: None,
                long_term_stress: None,
                level: None,
                granular_level: None,
                confidence: None,
            },
        });
    }

    let weights: Vec<f32> = (0..WINDOW_DAYS)
        .map(|index| {
            OLDEST_DAY_WEIGHT
                + (TODAY_WEIGHT - OLDEST_DAY_WEIGHT) * index as f32 / (WINDOW_DAYS - 1) as f32
        })
        .collect();
    // The divisor is the weight of the days that *stress* was scored on, and the same divisor
    // is used for all three means — so a day missing only its sleep recovery still counts in
    // the denominator. That is the archive's, and it biases those means towards zero.
    let used: f32 = stresses
        .iter()
        .zip(&weights)
        .filter_map(|(value, weight)| value.map(|_| *weight))
        .sum();
    let weighted = |series: &[Option<f32>]| -> f32 {
        series
            .iter()
            .zip(&weights)
            .filter_map(|(value, weight)| value.map(|value| value * weight))
            .sum::<f32>()
            / used
    };

    let long_term_stress = weighted(&stresses);
    let long_term_restorative_time = weighted(&restoratives);
    let long_term_sleep_recovery = weighted(&recoveries);
    let long_term_recovery = DAYTIME_RECOVERY_WEIGHT * long_term_restorative_time
        + SLEEP_RECOVERY_WEIGHT * long_term_sleep_recovery;
    let level = level_for(long_term_recovery, long_term_stress);

    Ok(ResilienceOutcome {
        daily,
        resilience: Resilience {
            long_term_restorative_time: Some(long_term_restorative_time),
            long_term_sleep_recovery: Some(long_term_sleep_recovery),
            long_term_recovery: Some(long_term_recovery),
            long_term_stress: Some(long_term_stress),
            level: Some(level),
            granular_level: Some(granular_level(long_term_recovery, long_term_stress, level)),
            confidence: Some(scored as f32 / WINDOW_DAYS as f32),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// float32 through decimal, over a fourteen-day weighted mean.
    const TOLERANCE: f32 = 2e-3;

    const BASE: i64 = 1_700_000_000;

    fn limits() -> StressLimits {
        StressLimits {
            stress_limit: -0.5,
            saturation_stress: -2.0,
            recovery_limit: 0.5,
            saturation_recovery: 2.0,
        }
    }

    fn contributors() -> SleepContributors {
        SleepContributors {
            sleep_score: 78.0,
            hrv_balance: Some(60.0),
            recovery_index: 72.0,
            resting_heart_rate: 80.0,
        }
    }

    /// A day of readings every ten minutes, all at `level`, starting after the night.
    fn day(level: f32, hours: f32) -> (Vec<f32>, Vec<i64>) {
        let count = (hours * 6.0) as usize;
        let values = vec![level; count];
        let stamps = (0..count)
            .map(|index| (BASE + 9 * 3600 + index as i64 * 600) * 1000)
            .collect();
        (values, stamps)
    }

    fn history(stress: f32, restorative: f32, recovery: f32) -> Vec<DailyHistory> {
        vec![
            DailyHistory {
                stress: (!stress.is_nan()).then_some(stress),
                restorative_time: (!restorative.is_nan()).then_some(restorative),
                sleep_recovery: (!recovery.is_nan()).then_some(recovery),
            };
            WINDOW_DAYS - 1
        ]
    }

    #[test]
    fn a_day_under_four_waking_hours_scores_nothing_but_still_joins_the_window() {
        let (values, stamps) = day(0.0, 2.0);
        let got = resilience(
            &[(BASE + 3600) * 1000],
            &[(BASE + 8 * 3600) * 1000],
            &values,
            &stamps,
            &limits(),
            &contributors(),
            &history(45.0, 35.0, 60.0),
        )
        .expect("valid");
        assert!(got.daily.stress.is_none());
        assert_eq!(got.daily.quantised_stress, 0);
        // Thirteen scored days is still enough for the fortnight.
        assert!(got.resilience.level.is_some());
        assert!((got.resilience.confidence.expect("scored") - 13.0 / 14.0).abs() < TOLERANCE);
    }

    #[test]
    fn a_window_of_fewer_than_five_scored_days_has_no_level() {
        let (values, stamps) = day(0.0, 10.0);
        let got = resilience(
            &[(BASE + 3600) * 1000],
            &[(BASE + 8 * 3600) * 1000],
            &values,
            &stamps,
            &limits(),
            &contributors(),
            &history(f32::NAN, f32::NAN, f32::NAN),
        )
        .expect("valid");
        assert!(got.daily.stress.is_some(), "today itself was scored");
        assert!(got.resilience.level.is_none());
        assert!(got.resilience.confidence.is_none());
    }

    #[test]
    fn readings_taken_while_asleep_are_not_part_of_the_day() {
        // Every reading falls inside the sleep period, so nothing is left to score.
        let (values, stamps) = day(-1.0, 10.0);
        let got = resilience(
            &[stamps[0] - 1000],
            &[stamps[stamps.len() - 1] + 1000],
            &values,
            &stamps,
            &limits(),
            &contributors(),
            &history(45.0, 35.0, 60.0),
        )
        .expect("valid");
        assert!(got.daily.stress.is_none());
    }

    #[test]
    fn a_calm_fortnight_scores_higher_than_a_hard_one() {
        let (calm, calm_stamps) = day(0.9, 10.0);
        let calm = resilience(
            &[(BASE + 3600) * 1000],
            &[(BASE + 8 * 3600) * 1000],
            &calm,
            &calm_stamps,
            &limits(),
            &contributors(),
            &history(20.0, 60.0, 85.0),
        )
        .expect("valid");
        let (hard, hard_stamps) = day(-1.2, 10.0);
        let hard = resilience(
            &[(BASE + 3600) * 1000],
            &[(BASE + 8 * 3600) * 1000],
            &hard,
            &hard_stamps,
            &limits(),
            &contributors(),
            &history(85.0, 8.0, 25.0),
        )
        .expect("valid");
        assert!(calm.resilience.level > hard.resilience.level);
        assert_eq!(
            calm.resilience.level,
            Some(5),
            "a calm fortnight tops the scale"
        );
        assert!(
            hard.resilience.level <= Some(2),
            "a hard one sits at the bottom, was {:?}",
            hard.resilience.level
        );
        // The granular figure orders the same way and stays reportable.
        assert!(calm.resilience.granular_level > hard.resilience.granular_level);
    }

    #[test]
    fn absent_hrv_balance_leaves_the_divisor_rather_than_counting_as_zero() {
        let with = sleep_recovery(&contributors());
        let without = sleep_recovery(&SleepContributors {
            hrv_balance: None,
            ..contributors()
        });
        let as_zero = sleep_recovery(&SleepContributors {
            hrv_balance: Some(0.0),
            ..contributors()
        });
        assert!(
            without > as_zero,
            "dropping it must not read as a zero score"
        );
        assert_ne!(without, with);
    }

    #[test]
    fn the_bands_run_downwards_on_the_stress_side() {
        let limits = limits();
        // -2.0 is fully saturated stress, -0.6 is just past the limit, 0 is neutral.
        let counts = band_counts(&[-2.0, -1.0, -0.6, 0.0, 0.6, 1.0, 2.0], &limits);
        assert_eq!(counts[0], 1.0, "high stress");
        assert_eq!(counts[1], 1.0, "moderate stress");
        assert_eq!(counts[2], 1.0, "low stress");
        assert_eq!(counts[3], 1.0, "neutral");
        assert_eq!(counts[4], 1.0, "low recovery");
        assert_eq!(counts[5], 1.0, "moderate recovery");
        assert_eq!(counts[6], 1.0, "high recovery");
    }

    #[test]
    fn granular_resilience_stays_inside_its_level() {
        for stress in [10.0f32, 40.0, 70.0, 95.0] {
            for recovery in [5.0f32, 30.0, 55.0, 80.0, 95.0] {
                let level = level_for(recovery, stress);
                let granular = granular_level(recovery, stress, level);
                assert!(
                    (GRANULAR_MIN..=GRANULAR_MAX).contains(&granular),
                    "{granular} outside the reportable range"
                );
                assert!(
                    (granular - f32::from(level)).abs() <= 1.0,
                    "level {level} reported as {granular}"
                );
            }
        }
    }

    #[test]
    fn refuses_the_inputs_the_archive_refuses() {
        let (values, stamps) = day(0.0, 10.0);
        assert_eq!(
            resilience(
                &[(BASE + 3600) * 1000],
                &[(BASE + 8 * 3600) * 1000],
                &values,
                &stamps,
                &limits(),
                &contributors(),
                &history(40.0, 40.0, 60.0)[..10],
            ),
            Err(ResilienceError::HistoryWrongLength)
        );
        assert_eq!(
            resilience(
                &[(BASE + 3600) * 1000],
                &[],
                &values,
                &stamps,
                &limits(),
                &contributors(),
                &history(40.0, 40.0, 60.0),
            ),
            Err(ResilienceError::SleepLengthMismatch)
        );
        assert_eq!(
            resilience(
                &[(BASE + 8 * 3600) * 1000],
                &[(BASE + 3600) * 1000],
                &values,
                &stamps,
                &limits(),
                &contributors(),
                &history(40.0, 40.0, 60.0),
            ),
            Err(ResilienceError::SleepPeriodReversed)
        );
        assert_eq!(
            resilience(
                &[(BASE + 3600) * 1000],
                &[(BASE + 8 * 3600) * 1000],
                &values[..5],
                &stamps,
                &limits(),
                &contributors(),
                &history(40.0, 40.0, 60.0),
            ),
            Err(ResilienceError::StressLengthMismatch)
        );
    }

    /// Vectors generated by `tools/ml/deterministic_vectors.py stress_resilience_2_2_1`.
    #[test]
    fn matches_the_archive_on_generated_vectors() {
        let raw =
            include_str!("../../../../../../artifacts/models/vectors/stress_resilience_2_2_1.json");
        let file: serde_json::Value =
            serde_json::from_str(raw).expect("the vector file should parse");
        let mut checked = 0;
        for vector in file["vectors"]
            .as_array()
            .expect("vectors should be a list")
        {
            let inputs = &vector["inputs"];
            let flat = |name: &str| -> Vec<Option<f32>> {
                fn walk(value: &serde_json::Value, out: &mut Vec<Option<f32>>) {
                    match value {
                        serde_json::Value::Array(items) => items.iter().for_each(|i| walk(i, out)),
                        serde_json::Value::Number(number) => {
                            out.push(number.as_f64().map(|v| v as f32));
                        }
                        serde_json::Value::Null => out.push(None),
                        _ => {}
                    }
                }
                let mut out = Vec::new();
                walk(&inputs[name], &mut out);
                out
            };
            let present = |name: &str| -> Vec<f32> {
                flat(name)
                    .into_iter()
                    .map(|value| value.unwrap_or(f32::NAN))
                    .collect()
            };
            let millis = |name: &str| -> Vec<i64> {
                present(name)
                    .into_iter()
                    .map(|value| value as i64)
                    .collect()
            };
            let one = |name: &str| present(name)[0];
            let hrv = flat("hrv_balance")[0];
            let history: Vec<DailyHistory> = {
                let stress = flat("daily_stress_list");
                let restorative = flat("daily_restorative_time_list");
                let recovery = flat("daily_sleep_recovery_list");
                (0..stress.len())
                    .map(|index| DailyHistory {
                        stress: stress[index],
                        restorative_time: restorative[index],
                        sleep_recovery: recovery[index],
                    })
                    .collect()
            };
            let got = resilience(
                &millis("sleep_start_timestamps"),
                &millis("sleep_end_timestamps"),
                &present("stress"),
                &millis("stress_timestamps"),
                &StressLimits {
                    stress_limit: one("stress_lim"),
                    saturation_stress: one("saturation_stress_deviation"),
                    recovery_limit: one("recovery_lim"),
                    saturation_recovery: one("saturation_recovery_deviation"),
                },
                &SleepContributors {
                    sleep_score: one("sleep_score"),
                    hrv_balance: hrv,
                    recovery_index: one("recovery_index"),
                    resting_heart_rate: one("resting_heart_rate"),
                },
                &history,
            );
            match vector.get("error").and_then(|e| e.as_str()) {
                Some(message) => {
                    let error = got.expect_err("the archive refused this input");
                    assert!(
                        message.starts_with(&format!("builtins.Exception: {}", error.code())),
                        "expected code {} for {message}",
                        error.code()
                    );
                }
                None => {
                    let got = got.expect("the archive produced an outcome");
                    let want = vector["outputs"].as_array().expect("outputs are a list");
                    let scalar = |index: usize| -> Option<f32> {
                        want[index].as_array().expect("a matrix")[0]
                            .as_array()
                            .expect("a row")[0]
                            .as_f64()
                            .map(|value| value as f32)
                    };
                    let close = |name: &str, got: Option<f32>, index: usize| match scalar(index) {
                        None => assert!(got.is_none(), "{name} should be absent, was {got:?}"),
                        Some(expected) => {
                            let value = got.unwrap_or_else(|| panic!("{name} should be present"));
                            assert!(
                                (value - expected).abs() <= TOLERANCE * expected.abs().max(1.0),
                                "{name}: {value} vs {expected}"
                            );
                        }
                    };
                    close("daily stress", got.daily.stress, 0);
                    close("daily restorative", got.daily.restorative_time, 1);
                    close("daily sleep recovery", got.daily.sleep_recovery, 2);
                    assert_eq!(
                        f32::from(got.daily.quantised_stress),
                        scalar(3).expect("a band"),
                        "quantised stress"
                    );
                    assert_eq!(
                        f32::from(got.daily.quantised_restorative_time),
                        scalar(4).expect("a band"),
                        "quantised restorative"
                    );
                    assert_eq!(
                        f32::from(got.daily.quantised_sleep_recovery),
                        scalar(5).expect("a band"),
                        "quantised sleep recovery"
                    );
                    close(
                        "long term restorative",
                        got.resilience.long_term_restorative_time,
                        6,
                    );
                    close(
                        "long term sleep recovery",
                        got.resilience.long_term_sleep_recovery,
                        7,
                    );
                    close("long term recovery", got.resilience.long_term_recovery, 8);
                    close("long term stress", got.resilience.long_term_stress, 9);
                    close("level", got.resilience.level.map(f32::from), 10);
                    close("granular level", got.resilience.granular_level, 11);
                    close("confidence", got.resilience.confidence, 12);
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 7, "every produced vector should be checked");
    }
}
