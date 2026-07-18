//! Sleep staging (WHOOP-P6/P8, `[WRS]`) — a per-30 s-epoch hypnogram over a detected in-bed span.
//!
//! Two recipes, ported brand-neutral from `tanarchytan/whoop-rs`:
//!   - V2 (cardiorespiratory) — the gen5/MG default; z-scored HR / HR-variability / motion emissions, a
//!     deep gate on HR-flatness, a soft sleep-cycle prior, a self-calibrating jerk wake gate, an R-R RSA
//!     respiration term, and Viterbi transition smoothing.
//!   - V1 (Cole-Kripke) — the gen4 path and the session-detection source of truth.
//!
//! Inputs are protocol-free (see [`input`]): plain per-sample values plus the in-bed `[start, end]`.
//! [`stage_v2`] is the default; [`stage_v1`] is the gen4 recipe. Both are pure and deterministic. Like
//! the rest of the ported library these are wellness estimates, never medical advice, and are not
//! wired into the snapshot or the capability graph — promoting one to an emitted analytic stays a
//! separate admission packet (docs/analytics.md).

mod common;
mod input;
mod v1;
mod v2;

pub use input::{AccelSample, HrSample, RespSample, RrRun, SleepInput};
pub use v1::stage as stage_v1;
pub use v2::{stage as stage_v2, DEEP_GATE_THRESH};

/// A sleep stage. String forms are `"wake" | "light" | "deep" | "rem"` for cross-platform parity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SleepStage {
    Wake,
    Light,
    Deep,
    Rem,
}

impl SleepStage {
    /// The JSON label, identical across the platform twins.
    pub fn as_str(self) -> &'static str {
        match self {
            SleepStage::Wake => "wake",
            SleepStage::Light => "light",
            SleepStage::Deep => "deep",
            SleepStage::Rem => "rem",
        }
    }
}

/// A contiguous run of one stage. Times are wall-clock unix seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StageSegment {
    pub start: i64,
    pub end: i64,
    pub stage: SleepStage,
}

#[cfg(test)]
mod golden_tests;
