//! The one beat-series core. Everything that measures variability builds on this and nothing
//! re-derives it, because three implementations of RMSSD were three artefact policies.
//!
//! Two rules shape it. A successive difference must be between two beats that genuinely followed
//! one another, so differences never cross a gap in the recording and never cross a beat the
//! filter rejected — deleting a beat and differencing its neighbours invents a change that no
//! heart made. And a rejected beat is marked, never replaced: the pipeline does not interpolate
//! (docs/pipeline.md), so a filtered series is shorter, not smoothed.

use mav_model::raw::RawValue;
use mav_model::stream::Sample;

/// The longest gap between two interval samples that can still be the same run of beats. Straps
/// deliver intervals in short bursts minutes apart; inside a burst they arrive a second or so
/// apart, and differencing across a burst boundary differences two beats that never met.
pub const RUN_GAP_MS: i64 = 3_000;

/// Karlsson's tolerance: an interval further than this fraction from the local median is not a
/// normal-to-normal interval.
pub const KARLSSON_TOLERANCE: f64 = 0.20;
/// Beats either side of the one under test that form its local median. Five gives a nine-beat
/// window, the width Karlsson et al. use.
const KARLSSON_HALF_WINDOW: usize = 4;

/// One run of beats that followed one another, already filtered. `accepted` holds the intervals
/// in milliseconds and `adjacent` is true where the interval at that index and the one before it
/// were both accepted and adjacent in the original recording.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct BeatSeries {
    accepted: Vec<f64>,
    adjacent: Vec<bool>,
    excluded: u32,
}

impl BeatSeries {
    /// Build from interval samples in milliseconds, splitting runs on recording gaps and applying
    /// the Karlsson filter within each run. Samples must already be range- and quality-gated.
    pub fn from_ordered(intervals: &[(i64, f64)]) -> Self {
        let mut series = Self::default();
        let mut run_start = 0usize;
        for index in 0..intervals.len() {
            let breaks = index + 1 == intervals.len()
                || intervals[index + 1].0.saturating_sub(intervals[index].0) > RUN_GAP_MS;
            if breaks {
                series.push_run(&intervals[run_start..=index]);
                run_start = index + 1;
            }
        }
        series
    }

    fn push_run(&mut self, run: &[(i64, f64)]) {
        let values: Vec<f64> = run.iter().map(|(_, ms)| *ms).collect();
        let mut previous_accepted = false;
        for (index, keep) in karlsson_mask(&values).into_iter().enumerate() {
            if !keep {
                self.excluded += 1;
                previous_accepted = false;
                continue;
            }
            self.accepted.push(values[index]);
            self.adjacent.push(previous_accepted);
            previous_accepted = true;
        }
    }

    /// Intervals that survived filtering, pooled across every run.
    pub fn accepted(&self) -> &[f64] {
        &self.accepted
    }

    /// How many intervals the filter rejected.
    pub fn excluded(&self) -> u32 {
        self.excluded
    }

    pub fn len(&self) -> usize {
        self.accepted.len()
    }

    pub fn is_empty(&self) -> bool {
        self.accepted.is_empty()
    }

    /// Differences between beats that genuinely followed one another. The count is what pNN50 and
    /// RMSSD divide by, and it is smaller than `len() - 1` whenever the recording had gaps.
    pub fn successive_differences(&self) -> impl Iterator<Item = f64> + '_ {
        self.accepted
            .windows(2)
            .zip(self.adjacent.iter().skip(1))
            .filter(|(_, adjacent)| **adjacent)
            .map(|(pair, _)| pair[1] - pair[0])
    }

    /// Root mean square of successive differences (ESC/NASPE 1996).
    pub fn rmssd_ms(&self) -> Option<f64> {
        let (sum, count) = self
            .successive_differences()
            .fold((0.0, 0usize), |(sum, count), delta| {
                (sum + delta * delta, count + 1)
            });
        (count > 0).then(|| (sum / count as f64).sqrt())
    }

    /// Standard deviation of successive differences (ESC/NASPE 1996). Distinct from RMSSD whenever
    /// the mean difference is not zero, which is what makes the Poincaré identities exact.
    pub fn sdsd_ms(&self) -> Option<f64> {
        sample_sd(&self.successive_differences().collect::<Vec<_>>())
    }

    /// Standard deviation of the accepted intervals (ESC/NASPE 1996). A distribution statistic
    /// rather than an adjacency one, so it pools across runs without bridging anything.
    pub fn sdnn_ms(&self) -> Option<f64> {
        sample_sd(&self.accepted)
    }

    pub fn mean_interval_ms(&self) -> Option<f64> {
        (!self.accepted.is_empty())
            .then(|| self.accepted.iter().sum::<f64>() / self.accepted.len() as f64)
    }

    /// Successive differences over 50 ms, and that count as a percentage of the differences that
    /// exist. Reported together because the percentage is meaningless without its denominator.
    pub fn nn50(&self) -> Option<(u32, f64)> {
        let (over, total) = self
            .successive_differences()
            .fold((0u32, 0usize), |(over, total), delta| {
                (over + u32::from(delta.abs() > 50.0), total + 1)
            });
        (total > 0).then(|| (over, f64::from(over) * 100.0 / total as f64))
    }

    /// The contiguous runs of accepted beats, each one an uninterrupted recording.
    pub fn runs(&self) -> impl Iterator<Item = &[f64]> {
        let mut boundaries: Vec<usize> = (0..self.accepted.len())
            .filter(|index| !self.adjacent[*index])
            .collect();
        boundaries.push(self.accepted.len());
        boundaries
            .windows(2)
            .map(|pair| &self.accepted[pair[0]..pair[1]])
            .collect::<Vec<_>>()
            .into_iter()
    }

    /// Short-term detrended fluctuation scaling exponent, DFA α1.
    ///
    /// Peng et al. (1995) integrate the mean-removed series, then measure how the root-mean-square
    /// residual of a piecewise linear fit grows with box size; the slope of that growth in log-log
    /// is the exponent. It is the one HRV measure that describes the *correlation structure* of the
    /// beats rather than their spread, which is why a series can look identical in RMSSD and quite
    /// different here. Around 1.0 is healthy fractal behaviour; toward 0.5 is uncorrelated noise
    /// and toward 1.5 is a random walk.
    ///
    /// Computed per uninterrupted run and pooled, because a gap is not a scaling property. `None`
    /// when no run is long enough for the largest box to appear at least twice.
    pub fn alpha1(&self) -> Option<f64> {
        let exponents: Vec<f64> = self.runs().filter_map(dfa_alpha1).collect();
        (!exponents.is_empty()).then(|| exponents.iter().sum::<f64>() / exponents.len() as f64)
    }

    /// Poincaré descriptors, in milliseconds. Brennan, Palaniswami and Kamen (2001) showed these
    /// are exact functions of the time-domain measures rather than a separate estimate: SD1 is the
    /// short-term scatter across the identity line, SD2 the long-term scatter along it.
    pub fn poincare_ms(&self) -> Option<(f64, f64)> {
        let sdsd = self.sdsd_ms()?;
        let sdnn = self.sdnn_ms()?;
        let sd1 = (sdsd * sdsd / 2.0).sqrt();
        let sd2 = (2.0 * sdnn * sdnn - sd1 * sd1).max(0.0).sqrt();
        Some((sd1, sd2))
    }
}

/// Which intervals in one contiguous run are normal-to-normal beats.
///
/// Karlsson et al. (2012) reject an interval that differs from the median of its local
/// neighbourhood by more than 20%. It is a local test on purpose: a resting series drifting slowly
/// is not artefact, while a doubled or dropped beat lands far from its neighbours however slowly
/// the series is drifting. A run too short for a neighbourhood is accepted whole — there is no
/// local median to test against, and inventing one would reject real beats.
pub fn karlsson_mask(run: &[f64]) -> Vec<bool> {
    if run.len() <= KARLSSON_HALF_WINDOW {
        return vec![true; run.len()];
    }
    let mut window = Vec::with_capacity(2 * KARLSSON_HALF_WINDOW + 1);
    (0..run.len())
        .map(|index| {
            let low = index.saturating_sub(KARLSSON_HALF_WINDOW);
            let high = (index + KARLSSON_HALF_WINDOW + 1).min(run.len());
            window.clear();
            window.extend_from_slice(&run[low..high]);
            window.sort_by(f64::total_cmp);
            let local_median = median_of_sorted(&window);
            (run[index] - local_median).abs() <= KARLSSON_TOLERANCE * local_median
        })
        .collect()
}

/// Box sizes, in beats, over which short-term scaling is measured. Peng's α1 range.
const DFA_BOXES: std::ops::RangeInclusive<usize> = 4..=16;

/// DFA α1 over one uninterrupted run.
fn dfa_alpha1(run: &[f64]) -> Option<f64> {
    let largest = *DFA_BOXES.end();
    if run.len() < largest * 2 {
        return None;
    }
    let average = run.iter().sum::<f64>() / run.len() as f64;
    let mut walk = Vec::with_capacity(run.len());
    let mut running = 0.0;
    for interval in run {
        running += interval - average;
        walk.push(running);
    }

    let points: Vec<(f64, f64)> = DFA_BOXES
        .filter_map(|box_size| {
            fluctuation(&walk, box_size).map(|f| ((box_size as f64).ln(), f.ln()))
        })
        .filter(|(_, log_f)| log_f.is_finite())
        .collect();
    if points.len() < 3 {
        return None;
    }
    let mean_x = points.iter().map(|(x, _)| x).sum::<f64>() / points.len() as f64;
    let mean_y = points.iter().map(|(_, y)| y).sum::<f64>() / points.len() as f64;
    let covariance: f64 = points
        .iter()
        .map(|(x, y)| (x - mean_x) * (y - mean_y))
        .sum();
    let spread: f64 = points
        .iter()
        .map(|(x, _)| (x - mean_x) * (x - mean_x))
        .sum();
    (spread > 0.0).then(|| covariance / spread)
}

/// Root-mean-square residual of a per-box straight-line fit to the integrated series.
fn fluctuation(walk: &[f64], box_size: usize) -> Option<f64> {
    let boxes = walk.len() / box_size;
    if boxes < 2 {
        return None;
    }
    let mut squared = 0.0;
    for index in 0..boxes {
        let segment = &walk[index * box_size..(index + 1) * box_size];
        let mean_x = (box_size - 1) as f64 / 2.0;
        let mean_y = segment.iter().sum::<f64>() / box_size as f64;
        let covariance: f64 = segment
            .iter()
            .enumerate()
            .map(|(at, value)| (at as f64 - mean_x) * (value - mean_y))
            .sum();
        let spread: f64 = (0..box_size)
            .map(|at| (at as f64 - mean_x) * (at as f64 - mean_x))
            .sum();
        let slope = if spread > 0.0 {
            covariance / spread
        } else {
            0.0
        };
        squared += segment
            .iter()
            .enumerate()
            .map(|(at, value)| {
                let residual = value - (mean_y + slope * (at as f64 - mean_x));
                residual * residual
            })
            .sum::<f64>();
    }
    Some((squared / (boxes * box_size) as f64).sqrt())
}

fn median_of_sorted(sorted: &[f64]) -> f64 {
    let middle = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    } else {
        sorted[middle]
    }
}

fn sample_sd(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let sum_squares: f64 = values.iter().map(|v| (v - mean) * (v - mean)).sum();
    Some((sum_squares / (values.len() - 1) as f64).sqrt())
}

/// Interval samples as `(device time in milliseconds, interval in milliseconds)`, ordered and
/// gated. The gate is the caller's: this only sorts and projects.
pub fn ordered_intervals(
    samples: &[Sample<RawValue>],
    accept: impl Fn(&Sample<RawValue>) -> bool,
) -> (Vec<(i64, f64)>, u32) {
    let mut kept: Vec<(i64, u16, f64)> = Vec::with_capacity(samples.len());
    let mut rejected = 0u32;
    for sample in samples {
        if accept(sample) {
            kept.push((
                sample.device_time.as_nanos().div_euclid(1_000_000),
                sample.seq,
                sample.value.as_f64(),
            ));
        } else {
            rejected += 1;
        }
    }
    kept.sort_unstable_by_key(|(at, seq, _)| (*at, *seq));
    (
        kept.into_iter().map(|(at, _, ms)| (at, ms)).collect(),
        rejected,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dense(values: &[f64]) -> Vec<(i64, f64)> {
        values
            .iter()
            .enumerate()
            .map(|(index, ms)| (index as i64 * 1_000, *ms))
            .collect()
    }

    fn close(left: f64, right: f64) {
        assert!((left - right).abs() < 1e-9, "{left} != {right}");
    }

    #[test]
    fn published_formulas_match_a_hand_calculated_vector() {
        // 800, 850, 790, 900 ms. Differences +50, -60, +110; squared sum 18,200.
        // Squared deviations around mean 835 sum to 7,700.
        let series = BeatSeries::from_ordered(&dense(&[800.0, 850.0, 790.0, 900.0]));
        close(series.mean_interval_ms().unwrap(), 835.0);
        close(series.rmssd_ms().unwrap(), (18_200.0f64 / 3.0).sqrt());
        close(series.sdnn_ms().unwrap(), (7_700.0f64 / 3.0).sqrt());
        assert_eq!(series.nn50().unwrap().0, 2);
        close(series.nn50().unwrap().1, 200.0 / 3.0);
    }

    /// The Poincaré descriptors are algebraic identities, not a second estimate, so they must
    /// agree with the time-domain numbers to the last bit.
    #[test]
    fn poincare_descriptors_follow_the_time_domain_ones_exactly() {
        let series = BeatSeries::from_ordered(&dense(&[800.0, 850.0, 790.0, 900.0, 870.0]));
        let (sd1, sd2) = series.poincare_ms().unwrap();
        let sdsd = series.sdsd_ms().unwrap();
        let sdnn = series.sdnn_ms().unwrap();
        close(sd1, (sdsd * sdsd / 2.0).sqrt());
        close(sd1 * sd1 + sd2 * sd2, 2.0 * sdnn * sdnn);
    }

    /// The finding this module exists for. Rejecting a beat and then differencing its neighbours
    /// manufactures a change spanning two real beats. The difference has to disappear with the
    /// beat.
    #[test]
    fn a_rejected_beat_removes_its_differences_rather_than_bridging_them() {
        let steady: Vec<f64> = (0..20)
            .map(|i| if i % 2 == 0 { 900.0 } else { 950.0 })
            .collect();
        let clean = BeatSeries::from_ordered(&dense(&steady));
        close(clean.rmssd_ms().unwrap(), 50.0);
        assert_eq!(clean.excluded(), 0);

        let mut artefact = steady.clone();
        artefact[10] = 1_800.0;
        let filtered = BeatSeries::from_ordered(&dense(&artefact));
        assert_eq!(filtered.excluded(), 1);
        close(filtered.rmssd_ms().unwrap(), 50.0);
        assert_eq!(
            filtered.successive_differences().count(),
            17,
            "the two differences touching the rejected beat are gone, not bridged"
        );
    }

    /// A recording gap is not a beat-to-beat change either. Two bursts minutes apart contribute
    /// their own differences and nothing spanning the silence.
    #[test]
    fn differences_never_cross_a_recording_gap() {
        let mut samples = dense(&[800.0, 810.0, 820.0]);
        samples.extend([(600_000, 500.0), (601_000, 510.0)]);
        let series = BeatSeries::from_ordered(&samples);
        assert_eq!(series.len(), 5);
        assert_eq!(
            series.successive_differences().count(),
            3,
            "two differences in the first burst, one in the second"
        );
        close(series.rmssd_ms().unwrap(), (300.0f64 / 3.0).sqrt());
    }

    #[test]
    fn a_run_too_short_for_a_local_median_is_kept_whole() {
        let series = BeatSeries::from_ordered(&dense(&[800.0, 1_600.0, 810.0]));
        assert_eq!(series.len(), 3);
        assert_eq!(series.excluded(), 0);
    }

    /// A slow resting drift is physiology, not artefact: the local median tracks it, so nothing is
    /// rejected however far the series has travelled end to end.
    #[test]
    fn a_slow_drift_survives_the_local_median_filter() {
        let drifting: Vec<f64> = (0..60).map(|i| 800.0 + f64::from(i) * 5.0).collect();
        assert_eq!(BeatSeries::from_ordered(&dense(&drifting)).excluded(), 0);
    }

    #[test]
    fn an_empty_series_answers_nothing_rather_than_zero() {
        let empty = BeatSeries::from_ordered(&[]);
        assert!(empty.is_empty());
        assert_eq!(empty.rmssd_ms(), None);
        assert_eq!(empty.sdnn_ms(), None);
        assert_eq!(empty.poincare_ms(), None);
        assert_eq!(empty.nn50(), None);
    }
}
