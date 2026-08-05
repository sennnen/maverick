//! `training_stress_score 0.2.1` — how hard the last twelve hours were.
//!
//! One score per minute, each over the twelve hours behind it, and the shape of that window is
//! the whole idea: a MET reading's weight **halves every hour into the past**, so the last hour
//! carries as much as the six before it. Effort an hour ago is still felt; effort ten hours ago
//! mostly is not.
//!
//! Two further weightings sit on top:
//!
//! * **Intensity.** A minute at 8 METs is worth more than eight minutes at 1.9. The reading is
//!   mapped through `1 + 7·(clamp(met, 1, 10) − 1)/9`, so the hardest minute counts eight times
//!   the easiest one rather than eight-and-a-bit times as its raw MET would suggest.
//! * **Fitness.** The same effort costs a fitter person less, so the score is scaled by VO₂max
//!   band where one is known, and by where the resting heart rate falls in its age-and-sex
//!   percentile table where it is not.
//!
//! A window needs 360 of its 720 minutes present to produce anything; below that the score is
//! absent rather than estimated from half a window. Readings below 0.9 MET are treated as
//! missing, not as rest — the strap was off, or the wearer was not wearing it properly.

/// Resting-heart-rate percentiles by age band, nine columns spanning the population.
const FEMALE_PERCENTILES: [[f32; 9]; 8] = [
    [
        46.7138, 49.6104, 51.7862, 53.5301, 55.3299, 57.0888, 58.8657, 60.983, 64.2303,
    ],
    [
        47.7381, 50.5618, 52.6541, 54.4143, 56.0933, 57.8085, 59.6168, 61.8335, 65.1604,
    ],
    [
        48.2473, 51.1075, 53.1712, 54.9694, 56.6722, 58.4197, 60.3506, 62.6837, 66.1132,
    ],
    [
        48.6121, 51.6139, 53.7495, 55.5819, 57.367, 59.1793, 61.1849, 63.6211, 67.2313,
    ],
    [
        48.4951, 51.3885, 53.5352, 55.4186, 57.174, 59.0302, 61.0343, 63.4526, 67.1304,
    ],
    [
        48.6772, 51.4444, 53.4191, 55.2059, 56.9584, 58.7348, 60.6756, 63.0252, 66.5816,
    ],
    [
        49.4089, 52.0557, 53.891, 55.726, 57.1849, 58.8782, 60.6082, 62.6687, 65.7634,
    ],
    [
        48.9608, 51.6009, 53.5882, 55.3759, 57.0037, 58.7479, 60.5916, 62.7483, 66.0808,
    ],
];

/// The male table; see [`FEMALE_PERCENTILES`].
const MALE_PERCENTILES: [[f32; 9]; 8] = [
    [
        43.1856, 45.4768, 47.109, 48.7468, 50.1919, 51.6476, 53.2414, 55.3949, 58.3715,
    ],
    [
        42.9343, 45.3842, 47.2283, 48.774, 50.2902, 51.8282, 53.5643, 55.6631, 58.6931,
    ],
    [
        44.1796, 46.794, 48.7847, 50.5199, 52.1184, 53.771, 55.6184, 57.8759, 61.272,
    ],
    [
        45.2586, 47.9686, 50.0405, 51.8437, 53.6122, 55.3975, 57.3881, 59.8113, 63.4918,
    ],
    [
        46.2718, 49.0411, 51.0897, 52.925, 54.7857, 56.6209, 58.6587, 61.2236, 65.0758,
    ],
    [
        46.0455, 48.8954, 50.9619, 52.7391, 54.5565, 56.4306, 58.482, 60.9294, 64.7308,
    ],
    [
        45.821, 48.4321, 50.5503, 52.3744, 54.3355, 56.3204, 58.4249, 60.6934, 64.4364,
    ],
    [
        45.4018, 48.1113, 50.1872, 51.8867, 53.6766, 55.5676, 57.7816, 60.3522, 64.2489,
    ],
];

/// The unspecified-sex table, which covers six age bands rather than eight.
const OTHER_PERCENTILES: [[f32; 9]; 6] = [
    [
        45.455, 49.2027, 51.8005, 53.7047, 55.6611, 57.4783, 59.4602, 60.8848, 63.6719,
    ],
    [
        45.9016, 48.6362, 50.9393, 53.0264, 55.0199, 57.0245, 58.9543, 61.235, 65.2591,
    ],
    [
        46.3749, 49.6817, 52.0171, 54.1698, 55.8509, 57.8355, 59.428, 62.4627, 66.9093,
    ],
    [
        46.4334, 49.4139, 51.3332, 52.8699, 54.4958, 56.3572, 58.2695, 60.7675, 64.3841,
    ],
    [
        49.2544, 51.5784, 53.2624, 54.8408, 56.7219, 58.2236, 59.944, 62.1965, 66.0582,
    ],
    [
        46.0658, 48.0895, 51.1609, 53.4154, 55.147, 58.0184, 59.6856, 61.5542, 65.64,
    ],
];

/// VO2max band edges as `[sex, age_min, age_max, low_fair, fair_high, high_peak]`.
const VO2MAX_THRESHOLDS: [[f32; 6]; 24] = [
    [0.0, 0.0, 24.0, 32.0, 39.0, 46.0],
    [0.0, 25.0, 29.0, 31.0, 38.0, 44.0],
    [0.0, 30.0, 34.0, 30.0, 35.0, 42.0],
    [0.0, 35.0, 39.0, 28.0, 33.0, 40.0],
    [0.0, 40.0, 44.0, 26.0, 31.0, 37.0],
    [0.0, 45.0, 49.0, 24.0, 29.0, 35.0],
    [0.0, 50.0, 54.0, 23.0, 27.0, 32.0],
    [0.0, 55.0, 59.0, 21.0, 25.0, 30.0],
    [0.0, 60.0, 64.0, 19.0, 22.0, 27.0],
    [0.0, 65.0, 69.0, 18.0, 20.0, 25.0],
    [0.0, 70.0, 74.0, 16.0, 18.0, 22.0],
    [0.0, 75.0, 125.0, 14.0, 16.0, 20.0],
    [1.0, 0.0, 24.0, 38.0, 46.0, 56.0],
    [1.0, 25.0, 29.0, 36.0, 45.0, 53.0],
    [1.0, 30.0, 34.0, 35.0, 42.0, 51.0],
    [1.0, 35.0, 39.0, 33.0, 40.0, 48.0],
    [1.0, 40.0, 44.0, 32.0, 38.0, 46.0],
    [1.0, 45.0, 49.0, 30.0, 36.0, 43.0],
    [1.0, 50.0, 54.0, 28.0, 34.0, 41.0],
    [1.0, 55.0, 59.0, 27.0, 32.0, 39.0],
    [1.0, 60.0, 64.0, 25.0, 30.0, 36.0],
    [1.0, 65.0, 69.0, 23.0, 28.0, 34.0],
    [1.0, 70.0, 74.0, 21.0, 26.0, 31.0],
    [1.0, 75.0, 125.0, 20.0, 24.0, 29.0],
];

/// Resting-heart-rate percentile weights: a wearer at the top percentile carries 1.2× the
/// score of one at the bottom, because the same effort costs them more.
const RHR_WEIGHTS: [f32; 10] = [
    1.0, 1.0222, 1.0444, 1.0667, 1.0889, 1.1111, 1.1333, 1.1556, 1.1778, 1.2,
];
/// VO2max band weights, fittest first — the mirror image of the resting-heart-rate ones.
const VO2MAX_WEIGHTS: [f32; 4] = [1.2, 1.1333, 1.0667, 1.0];

/// Minutes in the window each score covers.
const WINDOW_MINUTES: usize = 720;

/// How many of those minutes must carry a reading for the score to exist.
const MIN_PRESENT_MINUTES: usize = 360;

/// The weight of a reading halves this often, in minutes.
const WEIGHT_HALF_LIFE_MINUTES: f32 = 60.0;

/// Below this metabolic equivalent the reading is treated as missing rather than as rest.
const MIN_MET: f32 = 0.9;

/// The intensity mapping's endpoints: METs are clamped here before being scaled.
const INTENSITY_MET_FLOOR: f32 = 1.0;
const INTENSITY_MET_CEILING: f32 = 10.0;

/// The hardest minute is worth this many of the easiest.
const INTENSITY_RANGE: f32 = 8.0;

/// The score floor, applied to every score that exists.
const MIN_SCORE: f32 = 0.9;

/// Above this the day is flagged as a high-load one.
const HIGH_SCORE_THRESHOLD: f32 = 4.0;

/// Below this readiness the high-load threshold drops by a tenth.
const LOW_READINESS: f32 = 60.0;
const LOW_READINESS_FACTOR: f32 = 0.9;

/// Age bands for the female and male percentile tables.
const AGE_GROUPS: [f32; 8] = [10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0];

/// Age bands for the unspecified-sex table, which is narrower at both ends.
const OTHER_AGE_GROUPS: [f32; 6] = [20.0, 30.0, 40.0, 50.0, 60.0, 70.0];

/// Which percentile table and VO2max band a wearer is read against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sex {
    /// The female tables.
    Female,
    /// The male tables.
    Male,
    /// Unspecified, which has its own narrower tables.
    Unspecified,
}

/// Why the archive refused the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrainingStressError {
    /// 10 — a timezone change inside the window makes its timeline meaningless.
    TimezoneChanged,
    /// Fewer readings than one window needs.
    NotEnoughReadings,
}

impl TrainingStressError {
    /// The archive's own code, where it has one.
    pub fn code(self) -> u8 {
        match self {
            Self::TimezoneChanged => 10,
            Self::NotEnoughReadings => 1,
        }
    }
}

/// One minute's score.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StressScore {
    /// When the window this score covers ends, in milliseconds.
    pub timestamp_ms: i64,
    /// The score, absent when the window was too sparse to produce one.
    pub score: Option<f32>,
    /// Whether the score is above the high-load threshold, absent for the same reason.
    pub high: Option<bool>,
}

/// Everything the wearer's profile contributes to the scaling.
#[derive(Debug, Clone, Copy)]
pub struct Profile {
    /// Years.
    pub age: f32,
    /// Which tables to read.
    pub sex: Sex,
    /// Resting heart rate, in beats per minute.
    pub resting_heart_rate: f32,
    /// VO2max where it is known; the resting-heart-rate path is used where it is not.
    pub vo2max: Option<f32>,
    /// Today's readiness, which lowers the high-load threshold when it is poor.
    pub readiness: f32,
}

/// The weight of a reading `minutes_ago` before the end of its window.
fn recency_weight(minutes_ago: usize) -> f32 {
    0.5f32.powf(minutes_ago as f32 / WEIGHT_HALF_LIFE_MINUTES)
}

/// How much a reading counts for its intensity, from one at rest to eight at the ceiling.
fn intensity_weight(met: f32) -> f32 {
    let clamped = met.clamp(INTENSITY_MET_FLOOR, INTENSITY_MET_CEILING);
    let fraction = (clamped - INTENSITY_MET_FLOOR) / (INTENSITY_MET_CEILING - INTENSITY_MET_FLOOR);
    fraction * (INTENSITY_RANGE - 1.0) + 1.0
}

/// Which row of the percentile tables this wearer falls in.
fn age_group(age: f32, sex: Sex) -> usize {
    let (bands, clamped) = match sex {
        // The unspecified table stops at both ends rather than only the top.
        Sex::Unspecified => (&OTHER_AGE_GROUPS[..], age.clamp(20.0, 60.0)),
        _ => (&AGE_GROUPS[..], age.min(80.0)),
    };
    let decade = (clamped as i32 / 10 * 10) as f32;
    bands
        .iter()
        .position(|band| *band == decade)
        .unwrap_or(bands.len() - 1)
}

/// Which percentile of its table the resting heart rate is nearest to.
fn resting_heart_rate_band(profile: &Profile) -> usize {
    let row: &[f32; 9] = match profile.sex {
        Sex::Female => &FEMALE_PERCENTILES[age_group(profile.age, profile.sex)],
        Sex::Male => &MALE_PERCENTILES[age_group(profile.age, profile.sex)],
        Sex::Unspecified => &OTHER_PERCENTILES[age_group(profile.age, profile.sex)],
    };
    let mut best = 0;
    let mut smallest = f32::INFINITY;
    for (index, value) in row.iter().enumerate() {
        let distance = (value - profile.resting_heart_rate).abs();
        if distance < smallest {
            smallest = distance;
            best = index;
        }
    }
    best
}

/// Which VO2max band the wearer is in, if their age and sex appear in the table at all.
fn vo2max_band(profile: &Profile) -> Option<usize> {
    let vo2max = profile.vo2max?;
    if vo2max.is_nan() {
        return None;
    }
    // The table's own sex column is binary and it is the *female* rows that are zero; every
    // other value, unspecified included, reads against the male rows.
    let wanted = if profile.sex == Sex::Female { 0.0 } else { 1.0 };
    let years = profile.age as i32;
    let row = VO2MAX_THRESHOLDS
        .iter()
        .find(|row| row[0] == wanted && row[1] as i32 <= years && years <= row[2] as i32)?;
    Some(match vo2max {
        value if value < row[3] => 0,
        value if value < row[4] => 1,
        value if value < row[5] => 2,
        _ => 3,
    })
}

/// Score every minute that has a full window behind it.
pub fn training_stress(
    start_timestamp_ms: i64,
    mets: &[f32],
    profile: &Profile,
    timezone_changed: bool,
) -> Result<Vec<StressScore>, TrainingStressError> {
    if timezone_changed {
        return Err(TrainingStressError::TimezoneChanged);
    }
    if mets.len() < WINDOW_MINUTES {
        return Err(TrainingStressError::NotEnoughReadings);
    }

    // A reading below the floor is the strap not being worn, which is missing rather than rest.
    let cleaned: Vec<f32> = mets
        .iter()
        .map(|met| if *met < MIN_MET { f32::NAN } else { *met })
        .collect();

    let weights: Vec<f32> = (0..WINDOW_MINUTES)
        .map(|index| recency_weight(WINDOW_MINUTES - 1 - index))
        .collect();
    let total_weight: f32 = weights.iter().sum();

    // The fitness scaling is per wearer, not per window, so it is resolved once.
    let scale = match vo2max_band(profile) {
        Some(band) => VO2MAX_WEIGHTS[band],
        None => RHR_WEIGHTS[resting_heart_rate_band(profile)],
    };
    let threshold = if profile.readiness < LOW_READINESS {
        HIGH_SCORE_THRESHOLD * LOW_READINESS_FACTOR
    } else {
        HIGH_SCORE_THRESHOLD
    };

    let start_seconds = start_timestamp_ms.div_euclid(1000);
    let mut scores = Vec::with_capacity(cleaned.len() - WINDOW_MINUTES + 1);
    for first in 0..=cleaned.len() - WINDOW_MINUTES {
        let window = &cleaned[first..first + WINDOW_MINUTES];
        let mut present = 0;
        let mut weighted = 0.0f32;
        for (index, met) in window.iter().enumerate() {
            if met.is_nan() {
                continue;
            }
            present += 1;
            weighted += met * intensity_weight(*met) * weights[index];
        }
        let last_minute = first + WINDOW_MINUTES - 1;
        // The floor comes after the fitness scaling, not before: it bounds the number that
        // is shown, and scaling a floored value would let a fit wearer fall back under it.
        let score = if present >= MIN_PRESENT_MINUTES {
            Some((weighted / total_weight * scale).max(MIN_SCORE))
        } else {
            None
        };
        scores.push(StressScore {
            timestamp_ms: (start_seconds + last_minute as i64 * 60) * 1000,
            score,
            high: score.map(|value| value > threshold),
        });
    }
    Ok(scores)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The archive stores its weight table at float32; the port computes it, so the two differ
    /// in the last few bits of each of 720 weights.
    const TOLERANCE: f32 = 2e-3;

    fn profile() -> Profile {
        Profile {
            age: 35.0,
            sex: Sex::Male,
            resting_heart_rate: 55.0,
            vo2max: Some(45.0),
            readiness: 80.0,
        }
    }

    #[test]
    fn a_readings_weight_halves_every_hour_into_the_past() {
        assert!((recency_weight(0) - 1.0).abs() < 1e-6);
        assert!((recency_weight(60) - 0.5).abs() < 1e-6);
        assert!((recency_weight(120) - 0.25).abs() < 1e-6);
        // Twelve hours back, a reading is worth about a four-thousandth of the newest one.
        assert!(recency_weight(719) < 1.0 / 4000.0);
    }

    #[test]
    fn the_hardest_minute_is_worth_eight_of_the_easiest() {
        assert!((intensity_weight(1.0) - 1.0).abs() < 1e-6);
        assert!((intensity_weight(10.0) - 8.0).abs() < 1e-6);
        // Clamped at both ends rather than extrapolated.
        assert_eq!(intensity_weight(0.2), intensity_weight(1.0));
        assert_eq!(intensity_weight(40.0), intensity_weight(10.0));
    }

    #[test]
    fn a_window_missing_more_than_half_its_minutes_produces_nothing() {
        let mut mets = vec![3.0f32; WINDOW_MINUTES];
        for slot in mets
            .iter_mut()
            .take(WINDOW_MINUTES - MIN_PRESENT_MINUTES + 1)
        {
            *slot = 0.0; // below the floor, so missing
        }
        let got = training_stress(0, &mets, &profile(), false).expect("valid");
        assert_eq!(got.len(), 1);
        assert!(got[0].score.is_none());
        assert!(got[0].high.is_none());
    }

    #[test]
    fn recent_effort_counts_for_far_more_than_old_effort() {
        let mut recent = vec![1.0f32; WINDOW_MINUTES];
        let mut old = vec![1.0f32; WINDOW_MINUTES];
        for slot in recent.iter_mut().skip(WINDOW_MINUTES - 60) {
            *slot = 8.0;
        }
        for slot in old.iter_mut().take(60) {
            *slot = 8.0;
        }
        let recent = training_stress(0, &recent, &profile(), false).expect("valid")[0]
            .score
            .expect("scored");
        let old = training_stress(0, &old, &profile(), false).expect("valid")[0]
            .score
            .expect("scored");
        assert!(recent > old * 10.0, "{recent} vs {old}");
    }

    #[test]
    fn poor_readiness_lowers_the_bar_for_a_high_load_day() {
        // A steady 2.0 METs scores about 3.79 for this wearer, which sits between the tired
        // threshold of 3.6 and the rested one of 4.0 — so it is a high-load day only if they
        // came into it tired.
        let mets = vec![2.0f32; WINDOW_MINUTES];
        let rested = training_stress(0, &mets, &profile(), false).expect("valid")[0];
        let tired = training_stress(
            0,
            &mets,
            &Profile {
                readiness: 40.0,
                ..profile()
            },
            false,
        )
        .expect("valid")[0];
        assert_eq!(
            rested.score, tired.score,
            "the score itself does not change"
        );
        assert_eq!(rested.high, Some(false));
        assert_eq!(tired.high, Some(true));
    }

    #[test]
    fn a_wearer_without_a_vo2max_is_scaled_by_their_resting_heart_rate_instead() {
        let mets = vec![3.0f32; WINDOW_MINUTES];
        let with_vo2max = training_stress(0, &mets, &profile(), false).expect("valid")[0]
            .score
            .expect("scored");
        let without = training_stress(
            0,
            &mets,
            &Profile {
                vo2max: None,
                ..profile()
            },
            false,
        )
        .expect("valid")[0]
            .score
            .expect("scored");
        assert!(
            (with_vo2max - without).abs() > 1e-3,
            "the two paths should not coincide by accident"
        );
    }

    #[test]
    fn refuses_a_timezone_change_and_too_short_a_series() {
        assert_eq!(
            training_stress(0, &[3.0; WINDOW_MINUTES], &profile(), true),
            Err(TrainingStressError::TimezoneChanged)
        );
        assert_eq!(
            training_stress(0, &[3.0; 10], &profile(), false),
            Err(TrainingStressError::NotEnoughReadings)
        );
    }

    /// Vectors generated by `tools/ml/deterministic_vectors.py training_stress_score_0_2_1`.
    #[test]
    fn matches_the_archive_on_generated_vectors() {
        let raw = include_str!(
            "../../../../../../artifacts/models/vectors/training_stress_score_0_2_1.json"
        );
        let file: serde_json::Value =
            serde_json::from_str(raw).expect("the vector file should parse");
        let mut checked = 0;
        for vector in file["vectors"]
            .as_array()
            .expect("vectors should be a list")
        {
            let inputs = &vector["inputs"];
            let scalar = |name: &str| -> f64 {
                inputs[name].as_array().expect("a list")[0]
                    .as_f64()
                    .unwrap_or(f64::NAN)
            };
            let mets: Vec<f32> = inputs["mets"]
                .as_array()
                .expect("mets are a list")
                .iter()
                .map(|value| value.as_f64().expect("a met") as f32)
                .collect();
            let vo2max = scalar("vo2max");
            let profile = Profile {
                age: scalar("age") as f32,
                sex: match scalar("biological_sex") as i32 {
                    1 => Sex::Male,
                    -1 => Sex::Female,
                    _ => Sex::Unspecified,
                },
                resting_heart_rate: scalar("rhr") as f32,
                vo2max: if vo2max.is_nan() {
                    None
                } else {
                    Some(vo2max as f32)
                },
                readiness: scalar("readiness") as f32,
            };
            let start = inputs["start_timestamp"].as_array().expect("a list")[0]
                .as_i64()
                .expect("a timestamp");
            let got = training_stress(start, &mets, &profile, scalar("tz_change") == 1.0);
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
                    let got = got.expect("the archive produced scores");
                    let want = vector["outputs"].as_array().expect("outputs are a list");
                    let scores = want[0].as_array().expect("scores are a list");
                    let stamps = want[1].as_array().expect("timestamps are a list");
                    let highs = want[2].as_array().expect("flags are a list");
                    assert_eq!(got.len(), scores.len(), "score count");
                    for (index, entry) in got.iter().enumerate() {
                        assert_eq!(
                            entry.timestamp_ms,
                            stamps[index].as_i64().expect("a timestamp"),
                            "score {index} timestamp"
                        );
                        match scores[index].as_f64() {
                            None => {
                                assert!(entry.score.is_none(), "score {index} should be absent")
                            }
                            Some(expected) => {
                                let value = entry.score.expect("score should be present");
                                assert!(
                                    (value - expected as f32).abs()
                                        <= TOLERANCE * (expected as f32).abs().max(1.0),
                                    "score {index}: {value} vs {expected}"
                                );
                            }
                        }
                        assert_eq!(
                            entry.high,
                            highs[index].as_f64().map(|flag| flag == 1.0),
                            "score {index} high flag"
                        );
                    }
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 6, "every produced vector should be checked");
    }
}
