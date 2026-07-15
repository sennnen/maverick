//! The pipeline boundaries a Tap can observe. One variant per stage in docs/pipeline.md, in
//! pipeline order; adding a stage here means the pipeline itself grew, which is an ADR.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Acquisition,
    Frames,
    Decode,
    Sqi,
    Timeline,
    Store,
    Features,
    Predictions,
    Metrics,
    Snapshots,
}

impl Stage {
    pub const ALL: [Stage; 10] = [
        Stage::Acquisition,
        Stage::Frames,
        Stage::Decode,
        Stage::Sqi,
        Stage::Timeline,
        Stage::Store,
        Stage::Features,
        Stage::Predictions,
        Stage::Metrics,
        Stage::Snapshots,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Stage::Acquisition => "acquisition",
            Stage::Frames => "frames",
            Stage::Decode => "decode",
            Stage::Sqi => "sqi",
            Stage::Timeline => "timeline",
            Stage::Store => "store",
            Stage::Features => "features",
            Stage::Predictions => "predictions",
            Stage::Metrics => "metrics",
            Stage::Snapshots => "snapshots",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_lists_every_stage_in_pipeline_order() {
        assert_eq!(Stage::ALL.len(), 10);
        assert_eq!(Stage::ALL[0], Stage::Acquisition);
        assert_eq!(Stage::ALL[9], Stage::Snapshots);
        let mut sorted = Stage::ALL;
        sorted.sort();
        assert_eq!(
            sorted,
            Stage::ALL,
            "ALL must be in pipeline (declaration) order"
        );
    }

    #[test]
    fn names_are_unique() {
        let mut names: Vec<_> = Stage::ALL.iter().map(|s| s.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 10);
    }
}
