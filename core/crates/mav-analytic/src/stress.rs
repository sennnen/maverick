//! Baevsky Stress Index (WHOOP-P6, `[WRS]`): a histogram-based autonomic-balance proxy over an
//! interval series. `SI = AMo / (2 · Mo · MxDMn)`: Mo is the modal interval (s), AMo the modal
//! bin's share (%), MxDMn the range (s). A tall, narrow, low-range histogram (rigid, sympathetic)
//! reads high; a broad, flat one reads low.
//!
//! The bins sit on Baevsky's absolute 50 ms cardiointervalography grid — bin `k` spans
//! `[0.05k, 0.05(k+1))` seconds — not on a grid anchored to whatever the series minimum happened
//! to be. An anchored grid makes the modal bin, and therefore the index, depend on the single
//! shortest beat in the window. Cleaning is the shared [`crate::intervals`] filter. Wellness only,
//! never medical.

use crate::intervals::karlsson_mask;

/// Histogram bin width in seconds (Baevsky's 50 ms cardiointervalography grid).
const BIN_WIDTH_SEC: f64 = 0.05;
/// Minimum clean intervals before an SI is computed.
pub const MIN_BEATS: usize = 20;

/// Interval keep-band (ms); values outside are dropouts or artefacts.
const MIN_INTERVAL_MS: f64 = 300.0;
const MAX_INTERVAL_MS: f64 = 2000.0;

/// Intermediate histogram terms behind an SI, exposed so a caller can show the "why".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StressComponents {
    pub mo_sec: f64,
    pub amo_percent: f64,
    pub mxdmn_sec: f64,
    pub si: f64,
}

/// Baevsky Stress Index from a raw interval series (ms). `None` when too few clean beats survive
/// or the range is degenerate (all-equal beats → MxDMn 0 → an honest `None`, not infinity).
pub fn stress_index_raw(interval_ms: &[f64]) -> Option<f64> {
    components_raw(interval_ms).map(|c| c.si)
}

/// Full SI components from a raw interval series (ms). Pure and deterministic.
pub fn components_raw(interval_ms: &[f64]) -> Option<StressComponents> {
    let clean = clean(interval_ms);
    if clean.len() < MIN_BEATS {
        return None;
    }
    let seconds: Vec<f64> = clean.iter().map(|ms| ms / 1000.0).collect();
    let shortest = seconds.iter().copied().fold(f64::INFINITY, f64::min);
    let longest = seconds.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let mxdmn = longest - shortest;
    if mxdmn <= 0.0 {
        return None;
    }

    let bin_of = |value: f64| (value / BIN_WIDTH_SEC).floor() as i64;
    let (first_bin, last_bin) = (bin_of(shortest), bin_of(longest));
    let mut counts = vec![0usize; (last_bin - first_bin + 1) as usize];
    for value in &seconds {
        counts[(bin_of(*value) - first_bin) as usize] += 1;
    }
    // Ties resolve to the lowest bin, so the answer is identical on every platform.
    let (mode_offset, mode_count) = counts
        .iter()
        .enumerate()
        .max_by_key(|(offset, count)| (**count, std::cmp::Reverse(*offset)))
        .map(|(offset, count)| (offset as i64, *count))?;

    let mo = (first_bin + mode_offset) as f64 * BIN_WIDTH_SEC + BIN_WIDTH_SEC / 2.0;
    let amo = mode_count as f64 / seconds.len() as f64 * 100.0;
    (mo > 0.0).then(|| StressComponents {
        mo_sec: mo,
        amo_percent: amo,
        mxdmn_sec: mxdmn,
        si: amo / (2.0 * mo * mxdmn),
    })
}

/// Range band, then the shared local-median filter, order preserved.
fn clean(intervals: &[f64]) -> Vec<f64> {
    let ranged: Vec<f64> = intervals
        .iter()
        .copied()
        .filter(|ms| (MIN_INTERVAL_MS..=MAX_INTERVAL_MS).contains(ms))
        .collect();
    karlsson_mask(&ranged)
        .into_iter()
        .zip(ranged)
        .filter_map(|(keep, value)| keep.then_some(value))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOLDEN: [f64; 22] = [
        700.0, 720.0, 740.0, 760.0, 780.0, 800.0, 820.0, 840.0, 860.0, 800.0, 800.0, 800.0, 800.0,
        820.0, 780.0, 800.0, 810.0, 790.0, 800.0, 800.0, 805.0, 795.0,
    ];

    /// Hand-computed on the absolute grid: Mo lands on the centre of bin 16, `[0.80, 0.85)`.
    #[test]
    fn golden_stress_index_hand_computed() {
        let components = components_raw(&GOLDEN).expect("scorable");
        assert!((components.mxdmn_sec - 0.16).abs() < 1e-9);
        assert!((components.mo_sec - 0.825).abs() < 1e-9);
        assert!((components.amo_percent - 59.09090909090909).abs() < 1e-9);
        assert!((components.si - 223.82920110192833).abs() < 1e-9);
        assert!((stress_index_raw(&GOLDEN).unwrap() - components.si).abs() < 1e-12);
    }

    /// The bug this file was rewritten for. Two windows with the same rhythm and one differing
    /// short beat must report the same modal interval; under a grid anchored to the series minimum
    /// that single beat moved every bin edge, so the index depended on the shortest beat in the
    /// window rather than on the rhythm.
    #[test]
    fn one_short_beat_does_not_move_the_whole_histogram() {
        let rhythm: Vec<f64> = (0..20).map(|i| 800.0 + f64::from(i % 5) * 10.0).collect();
        let with = |outlier: f64| {
            let mut beats = rhythm.clone();
            beats.push(outlier);
            components_raw(&beats).expect("scorable")
        };
        let (near, far) = (with(770.0), with(760.0));
        assert!((near.mo_sec - far.mo_sec).abs() < 1e-12);
        assert!((near.amo_percent - far.amo_percent).abs() < 1e-12);
        assert!((near.mo_sec - 0.825).abs() < 1e-9);
    }

    #[test]
    fn tighter_histogram_raises_si() {
        let broad: Vec<f64> = (0..30).map(|it| 700.0 + (it % 11) as f64 * 18.0).collect();
        let rigid: Vec<f64> = (0..30)
            .map(|it| if it % 6 == 0 { 810.0 } else { 800.0 })
            .collect();
        assert!(
            stress_index_raw(&rigid).expect("rigid") > stress_index_raw(&broad).expect("broad"),
            "a rigid, tightly-clustered rhythm has a higher SI"
        );
    }

    #[test]
    fn too_few_beats_returns_none() {
        assert!(stress_index_raw(&[800.0; MIN_BEATS - 1]).is_none());
    }

    #[test]
    fn degenerate_range_returns_none() {
        assert!(stress_index_raw(&[800.0; 30]).is_none());
    }
}
