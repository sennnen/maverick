//! `cumulative_stress 1.2.2` — a month of nights reduced to one chronic-stress score.
//!
//! Nine summary features are taken across the last thirty-one nights — how often the wearer
//! got up, how low their heart rate went relative to their own resting rate, how fragmented
//! their sleep was, how their HRV and skin temperature sat against their baselines — and
//! projected into a five-factor space by a fitted factor-analysis model. Five cluster centres
//! live in that space, two of which are the stressed ones. The score is how much of the
//! wearer's probability mass falls on those two, as a percentage.
//!
//! That structure is why the output is a *score* and *contributors* together: the score is the
//! cluster membership, and each contributor is one factor's coordinate scaled against the
//! population's first and ninety-ninth percentile, so a wearer can be told which of the five
//! is carrying their number.
//!
//! Three things gate it:
//!
//! * **Fever.** A night above 38 °C, or a temperature deviation past its baseline, blanks that
//!   night out of every series. The baseline itself moves with the menstrual cycle — the luteal
//!   phase runs warmer, so the threshold is raised by 0.2 °C there rather than reading a normal
//!   luteal night as a fever.
//! * **Short sleep.** Under four hours and the night's intermediates are not computed at all,
//!   though the night still occupies its place in the history.
//! * **Coverage.** Every one of the nine features needs twenty-one usable nights out of the
//!   thirty-one. Below that there is no score, only the intermediates.
//!
//! Two robust statistics do the reducing: a Huber M-estimator for the got-up counts, which are
//! spiky, and a median for everything else.

/// Summary features projected by the factor model.
const FEATURES: usize = 9;

/// Factors kept after the first is dropped.
const FACTORS: usize = 5;

/// Cluster centres in that space.
const CLUSTERS: usize = 5;

/// Nights of history the archive expects.
pub const HISTORY_NIGHTS: usize = 31;

/// Usable nights each feature needs before a score can be produced.
const MIN_USABLE_NIGHTS: usize = 21;

/// Above this the night is feverish however the deviation reads.
const FEVER_LIMIT: f32 = 38.0;

/// How much the deviation threshold is raised in the luteal phase.
const LUTEAL_CORRECTION: f32 = 0.2;

/// The shortest night that still gets its intermediates computed, in seconds.
const MIN_SLEEP_SECONDS: f32 = 4.0 * 3600.0;

/// The least HRV coverage a night needs before its interquartile range means anything.
const MIN_HRV_COVERAGE: f32 = 0.2;

/// Beyond this many days either way, a cycle-phase prediction is not trusted.
const MAX_CYCLE_DAYS: f32 = 40.0;

/// The factor dropped before clustering — it carries overall level rather than shape.
const DROPPED_FACTOR: usize = 0;

/// Huber M-estimator settings, as the archive fixes them.
const HUBER_C: f32 = 1.5;
const HUBER_TOLERANCE: f32 = 1e-5;
const HUBER_MAX_ITERATIONS: usize = 50;
const HUBER_EPSILON: f32 = 1e-8;
/// Feature means and deviations the nine inputs are standardised by before projection.
const FA_MEAN: [f32; FEATURES] = [
    0.104575, 0.993044, 0.24977, 1.12265, 0.881184, 1.49967, 0.391883, 0.999726, 0.966935,
];
const FA_STD: [f32; FEATURES] = [
    0.0438704, 0.0264546, 0.0590555, 0.0457132, 0.0503628, 0.150166, 0.11124, 0.0480153, 0.00908734,
];

/// The factor-analysis loading matrix: nine standardised features onto six factors.
const FA_WEIGHTS: [[f32; 6]; FEATURES] = [
    [
        -0.0703622, 0.43585, -0.138835, 0.355951, 0.430466, -0.699815,
    ],
    [
        0.793537, -0.0185962, -0.107996, 0.149569, 0.264756, 0.072838,
    ],
    [
        0.0905627, 0.613483, 0.0873615, -0.112702, 0.0271959, 0.275402,
    ],
    [
        0.200113, -0.154528, 0.602943, -0.211177, 0.140065, -0.372242,
    ],
    [
        -0.00461252,
        -0.0125032,
        0.100932,
        0.687853,
        -0.0187686,
        0.171092,
    ],
    [
        0.0546296,
        -0.0236794,
        -0.00126781,
        -0.0008396,
        -0.364587,
        -0.116628,
    ],
    [
        0.0130428,
        0.0244703,
        0.457804,
        0.0902371,
        -0.00458877,
        0.0809482,
    ],
    [
        -0.0573597,
        -0.0308031,
        0.00673931,
        -0.00353417,
        0.0642564,
        0.00819035,
    ],
    [
        -0.0163072, 0.0318505, 0.0190903, 0.0569365, 0.0753338, 0.221172,
    ],
];

/// The five cluster centres, in the five-factor space the first factor is dropped from.
const CENTROIDS: [[f32; FACTORS]; CLUSTERS] = [
    [-0.1535, -0.8428, -1.205, -0.7879, 0.1214],
    [1.0158, 0.3265, 0.6563, 1.2822, -1.4237],
    [0.788, -0.3499, 0.0395, 0.4098, 0.1014],
    [-0.3693, 1.0162, -0.0351, 0.097, 0.009],
    [-0.6066, -0.265, 0.3389, -0.5015, 0.4711],
];

/// Which clusters count towards the score.
const POSITIVE_CLUSTERS: [usize; 2] = [1, 3];

/// Contributor centring and the percentiles each side is scaled by.
const CONTRIBUTOR_MEANS: [f32; FACTORS] = [0.5884, -0.2734, 0.1361, -0.0636, 0.2081];
const CONTRIBUTOR_LOW: [f32; FACTORS] = [-2.03, -1.81, -3.8, -1.95, -3.54];
const CONTRIBUTOR_HIGH: [f32; FACTORS] = [5.26, 2.78, 1.85, 2.8, 1.72];

/// The piecewise-linear map from a raw contributor to the 0-100 figure shown.
///
/// Column zero is the display level; the other five are that level's raw threshold for
/// each contributor in turn.
const CONTRIBUTOR_LEVELS: [[f32; 6]; 5] = [
    [100.0, -100.0, -100.0, -100.0, -100.0, -100.0],
    [85.0, -17.0, -7.0, -1.0, -10.0, -20.0],
    [70.0, -10.0, 8.0, 8.0, 4.0, -3.0],
    [60.0, 21.0, 35.0, 30.0, 25.0, 24.0],
    [0.0, 100.0, 100.0, 100.0, 100.0, 100.0],
];

/// Why the archive refused the input, with the code it refuses under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CumulativeStressError {
    /// 1 — a series is the wrong length, or its latest value is out of range.
    SeriesOutOfContract,
    /// 15 — the sleep, HRV-quality and HRV-median series are different lengths.
    SleepSeriesLengthMismatch,
    /// 19 — the skin-temperature readings and their timestamps are different lengths.
    TemperatureLengthMismatch,
    /// 14 — too few usable nights, and the latest night is unusable too.
    NotEnoughHistory,
    /// 16 — too few usable nights, and the latest night is feverish.
    NotEnoughHistoryWithFever,
}

impl CumulativeStressError {
    /// The archive's own code for this refusal.
    pub fn code(self) -> u8 {
        match self {
            Self::SeriesOutOfContract => 1,
            Self::SleepSeriesLengthMismatch => 15,
            Self::TemperatureLengthMismatch => 19,
            Self::NotEnoughHistory => 14,
            Self::NotEnoughHistoryWithFever => 16,
        }
    }
}

/// The thirty-one nights of summaries the score is computed over.
#[derive(Debug, Clone)]
pub struct NightlyHistory {
    /// Times the wearer got up, per night.
    pub got_ups: Vec<f32>,
    /// Lowest heart rate reached, per night.
    pub lowest_heart_rate: Vec<f32>,
    /// Average HRV, per night.
    pub average_hrv: Vec<f32>,
    /// Resting heart rate average, per night.
    pub resting_heart_rate: Vec<f32>,
    /// Average MET minutes, for the thirty nights before the latest.
    pub average_met_minutes: Vec<f32>,
    /// Long-sleep HRV, per night.
    pub long_sleep_hrv: Vec<f32>,
    /// Total sleep duration in seconds, per night.
    pub total_sleep_duration: Vec<f32>,
    /// Highest temperature reached, per night.
    pub highest_temperature: Vec<f32>,
    /// Temperature deviation from baseline, per night.
    pub temperature_deviation: Vec<f32>,
    /// The deviation baseline, per night.
    pub temperature_deviation_baseline: Vec<f32>,
}

/// The intermediates already computed for the thirty nights before the latest.
#[derive(Debug, Clone)]
pub struct EarlierIntermediates {
    /// Sleep fragmentation index, as a percentage.
    pub sleep_fragmentation_index: Vec<f32>,
    /// Median sleeping heart rate over the resting average.
    pub normalised_median_heart_rate: Vec<f32>,
    /// Median HRV quality, as a fraction.
    pub median_hrv_quality: Vec<f32>,
    /// Normalised interquartile range of the HRV samples.
    pub normalised_iqr: Vec<f32>,
    /// Waking skin temperature over the night's average.
    pub normalised_wake_temperature: Vec<f32>,
}

/// The latest night's raw signals, from which its own intermediates are computed.
#[derive(Debug, Clone)]
pub struct LatestNight {
    /// Sleep stage per thirty-second epoch: 1–3 asleep, 4 awake.
    pub sleep_phase_30s: Vec<f32>,
    /// Individual HRV samples.
    pub hrv_items: Vec<f32>,
    /// Median heart rate per epoch.
    pub hrv_median_heart_rate: Vec<f32>,
    /// HRV quality per epoch, as a percentage.
    pub hrv_quality: Vec<f32>,
    /// Skin temperature readings.
    pub skin_temperature: Vec<f32>,
    /// When each of those was taken, in milliseconds.
    pub skin_temperature_timestamps_ms: Vec<i64>,
    /// When the wearer went to bed, in milliseconds.
    pub bedtime_start_ms: i64,
    /// The night's average temperature, which the waking temperature is normalised by.
    pub temperature_average: f32,
}

/// Where the wearer is in their menstrual cycle, as far as it is known.
#[derive(Debug, Clone)]
pub struct CycleContext {
    /// Days until ovulation, negative once past it.
    pub days_to_ovulation: Option<f32>,
    /// Days until the next period.
    pub days_to_period: Option<f32>,
    /// The recorded phase per night: one for luteal.
    pub cycle_phase: Vec<f32>,
    /// The interpreted phase for the thirty earlier nights.
    pub interpreted_cycle_phase: Vec<f32>,
}

/// The latest night's own intermediates, absent where the night was too short.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NightIntermediates {
    /// Sleep fragmentation index, as a percentage.
    pub sleep_fragmentation_index: Option<f32>,
    /// Median sleeping heart rate over the resting average.
    pub normalised_median_heart_rate: Option<f32>,
    /// Median HRV quality, as a fraction.
    pub median_hrv_quality: Option<f32>,
    /// Normalised interquartile range of the HRV samples.
    pub normalised_iqr: Option<f32>,
    /// Waking skin temperature over the night's average.
    pub normalised_wake_temperature: Option<f32>,
}

/// Everything one call returns.
#[derive(Debug, Clone, PartialEq)]
pub struct CumulativeStress {
    /// The chronic-stress score, 0–100, absent without enough usable nights.
    pub score: Option<f32>,
    /// The five raw contributors, in the archive's order.
    pub contributors: Option<[f32; FACTORS]>,
    /// The same five mapped onto the 0–100 scale the app shows.
    pub display_contributors: Option<[f32; FACTORS]>,
    /// Cluster membership probabilities.
    pub cluster_probabilities: Option<[f32; CLUSTERS]>,
    /// The latest night's intermediates.
    pub latest: NightIntermediates,
    /// The interpreted cycle phase for the latest night.
    pub interpreted_cycle_phase: f32,
    /// Whether the latest night was feverish.
    pub fever: bool,
}

/// The median as NumPy computes it, over the values that are present.
fn median(values: &[f32]) -> Option<f32> {
    let mut present: Vec<f32> = values.iter().copied().filter(|v| !v.is_nan()).collect();
    if present.is_empty() {
        return None;
    }
    present.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
    let middle = present.len() / 2;
    Some(if present.len() % 2 == 1 {
        present[middle]
    } else {
        (present[middle - 1] + present[middle]) / 2.0
    })
}

/// Linear-interpolated quantile, matching `torch.quantile`'s default.
fn quantile(sorted: &[f32], q: f32) -> f32 {
    if sorted.is_empty() {
        return f32::NAN;
    }
    let position = q * (sorted.len() - 1) as f32;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    if lower == upper {
        sorted[lower]
    } else {
        sorted[lower] + (position - lower as f32) * (sorted[upper] - sorted[lower])
    }
}

/// The Huber M-estimator's robust *scale*, which is what the archive uses here.
///
/// Not the location: the got-up counts are spiky and what the model wants from them is how
/// spread out they are, so this returns the converged scale rather than the centre. Outliers
/// are trimmed once up front — anything past 3.4 robust scales above the median that is also
/// more than seven above the 90th percentile — and then the estimate is iterated to a fixed
/// point, down-weighting each residual past `c` scales.
fn huber_scale(values: &[f32]) -> Option<f32> {
    let present: Vec<f32> = values.iter().copied().filter(|v| !v.is_nan()).collect();
    if present.is_empty() {
        return None;
    }
    let centre = median(&present)?;
    let mean = present.iter().sum::<f32>() / present.len() as f32;
    // The sample standard deviation, as `torch.std` computes it by default.
    let mut scale = if present.len() > 1 {
        (present
            .iter()
            .map(|value| (value - mean).powi(2))
            .sum::<f32>()
            / (present.len() - 1) as f32)
            .sqrt()
    } else {
        0.0
    };
    let mut sorted = present.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
    let p90 = quantile(&sorted, 0.9);
    let spread = p90 - quantile(&sorted, 0.1);
    let outlier_scale = scale.max(spread);
    let kept: Vec<f32> = present
        .iter()
        .copied()
        .filter(|value| *value <= centre + outlier_scale * 3.4 || *value < p90 + 7.0)
        .collect();

    let mut location = centre;
    for _ in 0..HUBER_MAX_ITERATIONS {
        if scale < HUBER_EPSILON {
            break;
        }
        let threshold = scale * HUBER_C;
        let weights: Vec<f32> = kept
            .iter()
            .map(|value| {
                let residual = (value - location).abs();
                if residual <= threshold {
                    1.0
                } else {
                    threshold / (residual + HUBER_EPSILON)
                }
            })
            .collect();
        let weight_sum: f32 = weights.iter().sum();
        location = kept
            .iter()
            .zip(&weights)
            .map(|(value, weight)| value * weight)
            .sum::<f32>()
            / weight_sum;
        let next = (kept
            .iter()
            .zip(&weights)
            .map(|(value, weight)| weight * (value - location).powi(2))
            .sum::<f32>()
            / weight_sum)
            .sqrt();
        let converged = (next - scale).abs() < HUBER_TOLERANCE;
        scale = next;
        if converged {
            break;
        }
    }
    Some(scale)
}

/// The sleep fragmentation index: awakenings plus stage transitions, per two hours asleep.
fn fragmentation_index(hypnogram: &[f32]) -> Option<f32> {
    if hypnogram.iter().all(|value| value.is_nan()) {
        return None;
    }
    let asleep = hypnogram
        .iter()
        .filter(|value| **value == 1.0 || **value == 2.0 || **value == 3.0)
        .count();
    if asleep == 0 {
        return None;
    }
    let awakenings = hypnogram.iter().filter(|value| **value == 4.0).count();
    let transitions = hypnogram
        .windows(2)
        .filter(|pair| pair[0] != pair[1])
        .count()
        .saturating_sub(1);
    // Two hours is 120 epochs of a minute — the archive divides the epoch count by 120.
    let index = (awakenings + transitions) as f32 / (asleep as f32 / 120.0);
    Some(index.min(100.0))
}

/// The interquartile range of the night's HRV samples over their mean, and the coverage.
fn normalised_iqr(hrv_items: &[f32]) -> (Option<f32>, f32) {
    let present: Vec<f32> = hrv_items.iter().copied().filter(|v| !v.is_nan()).collect();
    let coverage = if hrv_items.is_empty() {
        0.0
    } else {
        present.len() as f32 / hrv_items.len() as f32
    };
    if coverage < MIN_HRV_COVERAGE || present.is_empty() {
        return (None, coverage);
    }
    let mut sorted = present.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
    let range = quantile(&sorted, 0.75) - quantile(&sorted, 0.25);
    let mean = present.iter().sum::<f32>() / present.len() as f32;
    (Some(range / mean), coverage)
}

/// Waking skin temperature over the night's average.
///
/// The sleep stages are at thirty seconds and the temperature readings are not, so the epochs
/// are folded to one minute — taking the *worse* of each pair, so a minute containing any
/// waking counts as waking — and each minute is matched to the nearest reading within five
/// seconds. The matched readings are then trimmed to their own 5th–95th percentile before
/// being averaged, because a wrist coming out from under a duvet is not a fever.
fn wake_temperature(night: &LatestNight, hypnogram: &[f32]) -> Option<f32> {
    if night.skin_temperature.iter().all(|value| value.is_nan())
        || night.bedtime_start_ms == -1
        || night.skin_temperature_timestamps_ms.is_empty()
    {
        return None;
    }
    // The archive divides both clocks to whole seconds first, then snaps bedtime to the
    // thirty-second grid the sleep stages sit on. Matching in milliseconds would make the
    // five-unit tolerance below five *milliseconds* and match almost nothing.
    let bedtime = night.bedtime_start_ms.div_euclid(1000).div_euclid(30) * 30;
    let minutes = hypnogram.len().div_ceil(2);
    let mut matched = Vec::new();
    for minute in 0..minutes {
        let first = hypnogram.get(minute * 2).copied().unwrap_or(-1.0);
        let second = hypnogram.get(minute * 2 + 1).copied().unwrap_or(-1.0);
        if first.max(second) != 4.0 {
            continue;
        }
        let at = bedtime as f32 + minute as f32 * 60.0;
        let nearest = night
            .skin_temperature_timestamps_ms
            .iter()
            .enumerate()
            .min_by(|(_, left), (_, right)| {
                let left = (left.div_euclid(1000) as f32 - at).abs();
                let right = (right.div_euclid(1000) as f32 - at).abs();
                left.partial_cmp(&right)
                    .unwrap_or(core::cmp::Ordering::Equal)
            });
        if let Some((index, timestamp)) = nearest {
            // Both sides are cast to f32 before the comparison, as the archive does. Near a
            // 2023 epoch that is a 128-second grid, so the tolerance is coarser in practice
            // than the five seconds it reads as — reproduced rather than tightened.
            if (timestamp.div_euclid(1000) as f32 - at).abs() < 5.0 {
                matched.push(night.skin_temperature[index]);
            }
        }
    }
    if matched.is_empty() {
        return None;
    }
    let below_fever: Vec<f32> = matched
        .into_iter()
        .filter(|value| *value < FEVER_LIMIT)
        .collect();
    if below_fever.is_empty() || below_fever.iter().all(|value| value.is_nan()) {
        return None;
    }
    let mut sorted = below_fever.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
    let low = quantile(&sorted, 0.05);
    let high = quantile(&sorted, 0.95);
    let trimmed: Vec<f32> = below_fever
        .into_iter()
        .filter(|value| *value >= low && *value <= high)
        .collect();
    if trimmed.is_empty() {
        return None;
    }
    let mean = trimmed.iter().sum::<f32>() / trimmed.len() as f32;
    Some(mean / night.temperature_average)
}

/// Project nine features into the five-factor space the clusters live in.
fn project(features: [f32; FEATURES]) -> [f32; FACTORS] {
    let mut scores = [0.0f32; 6];
    for (factor, score) in scores.iter_mut().enumerate() {
        *score = (0..FEATURES)
            .map(|feature| {
                (features[feature] - FA_MEAN[feature]) / FA_STD[feature]
                    * FA_WEIGHTS[feature][factor]
            })
            .sum();
    }
    // The first factor is dropped: it carries overall level rather than the shape the
    // clusters are separated on.
    let mut kept = [0.0f32; FACTORS];
    let mut slot = 0;
    for (factor, score) in scores.iter().enumerate() {
        if factor == DROPPED_FACTOR {
            continue;
        }
        kept[slot] = *score;
        slot += 1;
    }
    kept
}

/// Softmin over the distances to each centre: nearer centres get more of the mass.
fn cluster_probabilities(projection: [f32; FACTORS]) -> [f32; CLUSTERS] {
    let distances: Vec<f32> = CENTROIDS
        .iter()
        .map(|centre| {
            centre
                .iter()
                .zip(&projection)
                .map(|(c, p)| (c - p).powi(2))
                .sum::<f32>()
                .sqrt()
        })
        .collect();
    let smallest = distances.iter().copied().fold(f32::INFINITY, f32::min);
    let exponentials: Vec<f32> = distances
        .iter()
        .map(|distance| (-(distance - smallest)).exp())
        .collect();
    let total: f32 = exponentials.iter().sum();
    let mut out = [0.0f32; CLUSTERS];
    for (slot, value) in out.iter_mut().enumerate() {
        *value = exponentials[slot] / total;
    }
    out
}

/// Scale each factor against the population percentile on its own side of the mean.
fn scale_contributors(projection: [f32; FACTORS]) -> [f32; FACTORS] {
    let mut out = [0.0f32; FACTORS];
    for (slot, value) in out.iter_mut().enumerate() {
        let centred = projection[slot] - CONTRIBUTOR_MEANS[slot];
        let scaled = if centred > 0.0 {
            centred / CONTRIBUTOR_HIGH[slot]
        } else {
            centred / -CONTRIBUTOR_LOW[slot]
        };
        *value = scaled.clamp(-1.0, 1.0) * 100.0;
    }
    out
}

/// Map a raw contributor onto the 0–100 figure the app shows.
///
/// The map is piecewise linear through four segments, and a raw value outside every segment
/// produces no figure at all rather than being clamped into one.
fn display_contributor(raw: f32, index: usize) -> f32 {
    let flipped = -raw;
    let mut total = 0.0;
    for segment in 0..CONTRIBUTOR_LEVELS.len() - 1 {
        let upper_display = CONTRIBUTOR_LEVELS[segment][0];
        let lower_display = CONTRIBUTOR_LEVELS[segment + 1][0];
        let upper_raw = CONTRIBUTOR_LEVELS[segment][index + 1];
        let lower_raw = CONTRIBUTOR_LEVELS[segment + 1][index + 1];
        if flipped >= upper_raw && flipped < lower_raw {
            let slope = (lower_display - upper_display) / (lower_raw - upper_raw);
            total += flipped * slope + (lower_display - slope * lower_raw);
        }
    }
    total
}

/// Which nights are feverish, given the cycle-adjusted deviation threshold.
fn fever_mask(history: &NightlyHistory, phase: &[f32]) -> Vec<bool> {
    history
        .highest_temperature
        .iter()
        .zip(&history.temperature_deviation)
        .zip(&history.temperature_deviation_baseline)
        .zip(phase)
        .map(|(((temperature, deviation), baseline), luteal)| {
            // The luteal phase runs warmer, so its threshold is raised rather than every
            // luteal night being read as a fever.
            *temperature > FEVER_LIMIT || *deviation > baseline + luteal * LUTEAL_CORRECTION
        })
        .collect()
}

/// The interpreted cycle phase per night, and the latest night's own.
fn cycle_phases(cycle: &CycleContext) -> (Vec<f32>, f32) {
    let nights = cycle.cycle_phase.len();
    let mut filled: Vec<f32> = (0..nights.saturating_sub(1))
        .map(|index| {
            let interpreted = cycle.interpreted_cycle_phase.get(index).copied();
            match interpreted {
                Some(value) if !value.is_nan() => value,
                // Fall back to the recorded phase, and to zero where that is missing too.
                _ => cycle
                    .cycle_phase
                    .get(index)
                    .copied()
                    .filter(|v| !v.is_nan())
                    .unwrap_or(0.0),
            }
        })
        .collect();
    let ovulation = cycle.days_to_ovulation.filter(|v| !v.is_nan());
    let period = cycle.days_to_period.filter(|v| !v.is_nan());
    let latest = match (ovulation, period) {
        (Some(ovulation), Some(period))
            if ovulation.abs() <= MAX_CYCLE_DAYS && period.abs() <= MAX_CYCLE_DAYS =>
        {
            // Past ovulation, or with ovulation predicted after the next period, the wearer
            // is in the luteal phase.
            f32::from(u8::from(ovulation < 0.0 || ovulation > period))
        }
        // Without a usable prediction, the recorded phase for the night stands.
        _ => cycle.cycle_phase.last().copied().unwrap_or(0.0),
    };
    filled.push(latest);
    (filled, latest)
}

/// Blank a series wherever the night was feverish.
fn blanked(values: &[f32], mask: &[bool]) -> Vec<f32> {
    values
        .iter()
        .zip(mask)
        .map(|(value, feverish)| if *feverish { f32::NAN } else { *value })
        .collect()
}

/// Compute the chronic-stress score for the latest night.
pub fn cumulative_stress(
    history: &NightlyHistory,
    earlier: &EarlierIntermediates,
    night: &LatestNight,
    cycle: &CycleContext,
) -> Result<CumulativeStress, CumulativeStressError> {
    if history.got_ups.len() != HISTORY_NIGHTS
        || history.average_met_minutes.len() != HISTORY_NIGHTS - 1
        || history.temperature_deviation_baseline.len() != HISTORY_NIGHTS
    {
        return Err(CumulativeStressError::SeriesOutOfContract);
    }
    if night.sleep_phase_30s.len() != night.hrv_quality.len()
        || night.sleep_phase_30s.len() != night.hrv_median_heart_rate.len()
    {
        return Err(CumulativeStressError::SleepSeriesLengthMismatch);
    }
    if night.skin_temperature.len() != night.skin_temperature_timestamps_ms.len() {
        return Err(CumulativeStressError::TemperatureLengthMismatch);
    }

    let (phases, interpreted_cycle_phase) = cycle_phases(cycle);
    let feverish = fever_mask(history, &phases);
    let latest_feverish = *feverish.last().unwrap_or(&false);

    let got_ups = blanked(&history.got_ups, &feverish);
    let lowest_heart_rate = blanked(&history.lowest_heart_rate, &feverish);
    let average_hrv = blanked(&history.average_hrv, &feverish);
    let resting = blanked(&history.resting_heart_rate, &feverish);
    let long_sleep_hrv = blanked(&history.long_sleep_hrv, &feverish);
    let total_sleep = blanked(&history.total_sleep_duration, &feverish);
    // MET minutes cover the thirty nights *before* the latest, so they take the mask without
    // its last entry.
    let met_minutes = blanked(
        &history.average_met_minutes,
        &feverish[..feverish.len() - 1],
    );

    let long_enough = total_sleep
        .last()
        .copied()
        .is_some_and(|seconds| seconds >= MIN_SLEEP_SECONDS);

    // A feverish latest night has its raw signals blanked wholesale, which is what makes its
    // intermediates absent rather than merely unusual.
    let hypnogram: Vec<f32> = if latest_feverish {
        vec![f32::NAN; night.sleep_phase_30s.len()]
    } else {
        night.sleep_phase_30s.clone()
    };
    let quality: Vec<f32> = if latest_feverish {
        vec![f32::NAN; night.hrv_quality.len()]
    } else {
        night.hrv_quality.clone()
    };

    let latest = if long_enough {
        let (iqr, _coverage) = normalised_iqr(&night.hrv_items);
        let median_heart_rate: Vec<f32> = night
            .hrv_median_heart_rate
            .iter()
            .map(|value| if *value < 1.0 { f32::NAN } else { *value })
            .collect();
        let present: Vec<f32> = median_heart_rate
            .iter()
            .copied()
            .filter(|v| !v.is_nan())
            .collect();
        let normalised_median = (!present.is_empty())
            .then(|| {
                present.iter().sum::<f32>()
                    / present.len() as f32
                    / resting.last().copied().unwrap_or(f32::NAN)
            })
            .filter(|value| !value.is_nan());
        let quality_present: Vec<f32> = quality.iter().copied().filter(|v| !v.is_nan()).collect();
        NightIntermediates {
            sleep_fragmentation_index: fragmentation_index(&hypnogram),
            normalised_median_heart_rate: normalised_median,
            median_hrv_quality: (!quality_present.is_empty()).then(|| {
                quality_present.iter().sum::<f32>() / quality_present.len() as f32 / 100.0
            }),
            normalised_iqr: iqr,
            normalised_wake_temperature: wake_temperature(night, &hypnogram),
        }
    } else {
        NightIntermediates {
            sleep_fragmentation_index: None,
            normalised_median_heart_rate: None,
            median_hrv_quality: None,
            normalised_iqr: None,
            normalised_wake_temperature: None,
        }
    };

    let extend = |earlier: &[f32], latest: Option<f32>, scale: f32| -> Vec<f32> {
        let mut out: Vec<f32> = earlier.iter().map(|value| value / scale).collect();
        out.push(latest.unwrap_or(f32::NAN));
        out
    };
    let fragmentation = extend(
        &earlier.sleep_fragmentation_index,
        latest.sleep_fragmentation_index.map(|value| value / 100.0),
        100.0,
    );
    let normalised_median = extend(
        &earlier.normalised_median_heart_rate,
        latest.normalised_median_heart_rate,
        1.0,
    );
    let hrv_quality = extend(&earlier.median_hrv_quality, latest.median_hrv_quality, 1.0);
    let iqr_series = extend(&earlier.normalised_iqr, latest.normalised_iqr, 1.0);
    let wake_temperature_series = extend(
        &earlier.normalised_wake_temperature,
        latest.normalised_wake_temperature,
        1.0,
    );

    let normalised_hr_min: Vec<f32> = lowest_heart_rate
        .iter()
        .zip(&resting)
        .map(|(lowest, resting)| lowest / resting)
        .collect();
    let hrv_ratio: Vec<f32> = average_hrv
        .iter()
        .zip(&long_sleep_hrv)
        .map(|(average, long)| average / long)
        .collect();

    // Every feature needs the same twenty-one usable nights; one short and there is no score.
    let usable = |series: &[f32]| series.iter().filter(|v| !v.is_nan()).count();
    let enough = [
        usable(&fragmentation),
        usable(&normalised_median),
        usable(&hrv_quality),
        usable(&iqr_series),
        usable(&wake_temperature_series),
        usable(&normalised_hr_min),
        usable(&met_minutes),
        usable(&hrv_ratio),
        usable(&got_ups),
    ]
    .iter()
    .all(|count| *count >= MIN_USABLE_NIGHTS);

    if !enough && !long_enough {
        return Err(if latest_feverish {
            CumulativeStressError::NotEnoughHistoryWithFever
        } else {
            CumulativeStressError::NotEnoughHistory
        });
    }

    if !enough {
        return Ok(CumulativeStress {
            score: None,
            contributors: None,
            display_contributors: None,
            cluster_probabilities: None,
            latest,
            interpreted_cycle_phase,
            fever: latest_feverish,
        });
    }

    let sleep_hours = {
        let present: Vec<f32> = total_sleep
            .iter()
            .copied()
            .filter(|v| !v.is_nan())
            .collect();
        present.iter().sum::<f32>() / present.len() as f32 / 3600.0
    };
    let features = [
        huber_scale(&got_ups).unwrap_or(f32::NAN) / sleep_hours,
        median(&normalised_hr_min).unwrap_or(f32::NAN),
        median(&fragmentation).unwrap_or(f32::NAN),
        median(&normalised_median).unwrap_or(f32::NAN),
        median(&hrv_quality).unwrap_or(f32::NAN),
        median(&met_minutes).unwrap_or(f32::NAN),
        median(&iqr_series).unwrap_or(f32::NAN),
        median(&hrv_ratio).unwrap_or(f32::NAN),
        median(&wake_temperature_series).unwrap_or(f32::NAN),
    ];

    let projection = project(features);
    let probabilities = cluster_probabilities(projection);
    let positive: f32 = POSITIVE_CLUSTERS
        .iter()
        .map(|cluster| probabilities[*cluster])
        .sum();
    let scaled = scale_contributors(projection);
    // The first four contributors are reported with their sign flipped, so that for every one
    // of the five a higher number means more stress.
    let contributors = [-scaled[0], -scaled[1], -scaled[2], -scaled[3], scaled[4]];
    let mut display = [0.0f32; FACTORS];
    for (index, value) in display.iter_mut().enumerate() {
        *value = display_contributor(contributors[index], index);
    }

    Ok(CumulativeStress {
        score: Some((positive * 100.0).round()),
        contributors: Some(contributors),
        display_contributors: Some(display),
        cluster_probabilities: Some(probabilities),
        latest,
        interpreted_cycle_phase,
        fever: latest_feverish,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// float32 through decimal, over a thirty-one-night median and a six-factor projection.
    const TOLERANCE: f32 = 5e-3;

    /// Vectors generated by `tools/ml/deterministic_vectors.py cumulative_stress_1_2_2`.
    const VECTORS: &str =
        include_str!("../../../../../../artifacts/models/vectors/cumulative_stress_1_2_2.json");

    /// Every input arrives as a column vector; a missing value is written as null.
    fn column(inputs: &serde_json::Value, name: &str) -> Vec<f32> {
        inputs[name]
            .as_array()
            .unwrap_or_else(|| panic!("{name} should be a column"))
            .iter()
            .map(|row| {
                row.as_array().expect("a row")[0]
                    .as_f64()
                    .map_or(f32::NAN, |value| value as f32)
            })
            .collect()
    }

    fn parse(
        inputs: &serde_json::Value,
    ) -> (
        NightlyHistory,
        EarlierIntermediates,
        LatestNight,
        CycleContext,
    ) {
        let history = NightlyHistory {
            got_ups: column(inputs, "got_ups"),
            lowest_heart_rate: column(inputs, "lowest_heart_rate"),
            average_hrv: column(inputs, "average_hrv"),
            resting_heart_rate: column(inputs, "resting_hr_average"),
            average_met_minutes: column(inputs, "average_met_minutes"),
            long_sleep_hrv: column(inputs, "long_sleep_hrv"),
            total_sleep_duration: column(inputs, "total_sleep_duration"),
            highest_temperature: column(inputs, "highest_temperature"),
            temperature_deviation: column(inputs, "temperature_dev"),
            temperature_deviation_baseline: column(inputs, "temperature_dev_baseline"),
        };
        let earlier = EarlierIntermediates {
            sleep_fragmentation_index: column(inputs, "sleep_fragmentation_index"),
            normalised_median_heart_rate: column(inputs, "norm_hrv_medianHR_5min"),
            median_hrv_quality: column(inputs, "median_hrv_quality_5min"),
            normalised_iqr: column(inputs, "normalised_iqr"),
            normalised_wake_temperature: column(inputs, "norm_temp_wake"),
        };
        let night = LatestNight {
            sleep_phase_30s: column(inputs, "sleep_phase_30_sec"),
            hrv_items: column(inputs, "hrv_items"),
            hrv_median_heart_rate: column(inputs, "hrv_medianHR_5min"),
            hrv_quality: column(inputs, "hrv_quality_5min"),
            skin_temperature: column(inputs, "temp_skin"),
            skin_temperature_timestamps_ms: column(inputs, "temp_skin_timestamps")
                .into_iter()
                .map(|value| value as i64)
                .collect(),
            bedtime_start_ms: column(inputs, "bedtime_start")[0] as i64,
            temperature_average: column(inputs, "temperature_avg")[0],
        };
        let cycle = CycleContext {
            days_to_ovulation: Some(column(inputs, "n_days_to_ovulation")[0]),
            days_to_period: Some(column(inputs, "n_days_to_period")[0]),
            cycle_phase: column(inputs, "cycle_phase"),
            interpreted_cycle_phase: column(inputs, "interpreted_cycle_phase"),
        };
        (history, earlier, night, cycle)
    }

    #[test]
    fn matches_the_archive_on_generated_vectors() {
        let file: serde_json::Value =
            serde_json::from_str(VECTORS).expect("the vector file should parse");
        let mut scored = 0;
        let mut unscored = 0;
        for vector in file["vectors"]
            .as_array()
            .expect("vectors should be a list")
        {
            let (history, earlier, night, cycle) = parse(&vector["inputs"]);
            let got = cumulative_stress(&history, &earlier, &night, &cycle)
                .expect("the archive accepted this input");
            let want = vector["outputs"].as_array().expect("outputs are a list");
            // Each output is nested to its own depth; this reaches the scalar inside.
            fn scalar(value: &serde_json::Value) -> Option<f32> {
                match value {
                    serde_json::Value::Array(items) => items.first().and_then(scalar),
                    serde_json::Value::Number(number) => number.as_f64().map(|v| v as f32),
                    _ => None,
                }
            }
            let close = |name: &str, got: Option<f32>, index: usize| match scalar(&want[index]) {
                None => assert!(got.is_none(), "{name} should be absent, was {got:?}"),
                Some(expected) => {
                    let value = got.unwrap_or_else(|| panic!("{name} should be present"));
                    assert!(
                        (value - expected).abs() <= TOLERANCE * expected.abs().max(1.0),
                        "{name}: {value} vs {expected}"
                    );
                }
            };
            close("score", got.score, 0);
            for slot in 0..FACTORS {
                close(
                    "contributor",
                    got.contributors.map(|values| values[slot]),
                    1 + slot,
                );
                close(
                    "display contributor",
                    got.display_contributors.map(|values| values[slot]),
                    12 + slot,
                );
            }
            close(
                "sleep fragmentation",
                got.latest.sleep_fragmentation_index,
                6,
            );
            close(
                "normalised median heart rate",
                got.latest.normalised_median_heart_rate,
                7,
            );
            close("median hrv quality", got.latest.median_hrv_quality, 8);
            close("normalised iqr", got.latest.normalised_iqr, 9);
            close(
                "wake temperature",
                got.latest.normalised_wake_temperature,
                10,
            );
            close(
                "interpreted cycle phase",
                Some(got.interpreted_cycle_phase),
                11,
            );
            if got.score.is_some() {
                scored += 1;
            } else {
                unscored += 1;
            }
        }
        assert_eq!((scored, unscored), (5, 1), "five scores and one refusal");
    }

    #[test]
    fn the_huber_scale_is_not_moved_by_a_single_outlier() {
        let steady = [2.0f32, 2.0, 3.0, 2.0, 3.0, 2.0, 2.0, 3.0, 2.0, 2.0];
        let mut spiked = steady;
        spiked[4] = 40.0;
        let base = huber_scale(&steady).expect("scaled");
        let spike = huber_scale(&spiked).expect("scaled");
        let mean_shift = {
            let plain = |values: &[f32]| {
                let mean = values.iter().sum::<f32>() / values.len() as f32;
                (values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / values.len() as f32)
                    .sqrt()
            };
            plain(&spiked) / plain(&steady)
        };
        assert!(
            spike / base < mean_shift,
            "the robust scale should move less than the plain one: {spike} vs {base}"
        );
    }

    #[test]
    fn the_luteal_phase_raises_the_fever_threshold_rather_than_flagging_it() {
        let nights = HISTORY_NIGHTS;
        let history = NightlyHistory {
            got_ups: vec![2.0; nights],
            lowest_heart_rate: vec![52.0; nights],
            average_hrv: vec![55.0; nights],
            resting_heart_rate: vec![58.0; nights],
            average_met_minutes: vec![1.4; nights - 1],
            long_sleep_hrv: vec![55.0; nights],
            total_sleep_duration: vec![27_000.0; nights],
            highest_temperature: vec![36.8; nights],
            // Just past the follicular threshold, comfortably inside the luteal one.
            temperature_deviation: vec![0.5; nights],
            temperature_deviation_baseline: vec![0.4; nights],
        };
        let follicular = fever_mask(&history, &vec![0.0; nights]);
        let luteal = fever_mask(&history, &vec![1.0; nights]);
        assert!(follicular.iter().all(|feverish| *feverish));
        assert!(luteal.iter().all(|feverish| !*feverish));
    }

    #[test]
    fn cluster_probabilities_sum_to_one_and_favour_the_nearest_centre() {
        let probabilities = cluster_probabilities(CENTROIDS[2]);
        let total: f32 = probabilities.iter().sum();
        assert!(
            (total - 1.0).abs() < 1e-5,
            "probabilities summed to {total}"
        );
        let best = probabilities
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).expect("finite"))
            .expect("a maximum")
            .0;
        assert_eq!(best, 2, "sitting on a centre should favour that centre");
    }

    #[test]
    fn refuses_series_whose_lengths_do_not_line_up() {
        let file: serde_json::Value =
            serde_json::from_str(VECTORS).expect("the vector file should parse");
        let vector = &file["vectors"].as_array().expect("a list")[0];
        let (history, earlier, mut night, cycle) = parse(&vector["inputs"]);
        night.hrv_quality.pop();
        assert_eq!(
            cumulative_stress(&history, &earlier, &night, &cycle),
            Err(CumulativeStressError::SleepSeriesLengthMismatch)
        );
        let (history, earlier, mut night, cycle) = parse(&vector["inputs"]);
        night.skin_temperature.pop();
        assert_eq!(
            cumulative_stress(&history, &earlier, &night, &cycle),
            Err(CumulativeStressError::TemperatureLengthMismatch)
        );
        let (mut history, earlier, night, cycle) = parse(&vector["inputs"]);
        history.got_ups.pop();
        assert_eq!(
            cumulative_stress(&history, &earlier, &night, &cycle),
            Err(CumulativeStressError::SeriesOutOfContract)
        );
    }
}
