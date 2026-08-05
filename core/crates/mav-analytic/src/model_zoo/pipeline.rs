//! What each model needs before it can run, and what its output is for.
//!
//! [`super::registry`] says what a model *is* — tensor names, shapes, hashes. It says nothing
//! about where those tensors come from, and that gap is why forty-one converted models sat in
//! the bundle with nothing calling them: a platform holding `features (1, 13)` has no way to
//! know that thirteen values are four profile fields, eight baselines and one aggregated PPG
//! score, and no business guessing.
//!
//! This module closes the gap in the core, once, for both platforms. For every model it
//! records:
//!
//! - the streams its preprocessing reads, in the same `requires_all` / `requires_any` shape
//!   [`crate::capability`] already uses for analytics, so an absent sensor produces the same
//!   kind of named reason rather than a silent nothing;
//! - the wearer-profile fields its input vector carries;
//! - the models whose outputs are its inputs, so the runner can order the graph rather than
//!   asking a platform to know that `halite_risk_tree` runs after `halite_ppg_score`;
//! - the product signal it contributes to, and how far that contribution may be *read*.
//!
//! ## Every dependency here is evidenced, not inferred
//!
//! Six edges are provable from the contracts alone: the upstream output has the same tensor
//! name and the same shape as the downstream input (`cva_encoder` → the probe heads,
//! `pulsenet_foundation` → `halite_ppg_score`, `step_eligibility` → `step_head` →
//! `step_multiplier`, `whr_unet_encoder` → `whr_unet_head`). `tests::declared_dependencies_are_
//! shape_compatible` re-derives them from the registry, so a re-conversion that changes a shape
//! fails here rather than at run time.
//!
//! The rest come from the registry's own role text and the conversion notes in
//! `artifacts/models/manifest.json`, quoted at each edge below. Nothing is inferred from a bare
//! shape coincidence: `illness_detection.scalars (1, 4)` happens to match
//! `step_eligibility.eligibility_features (1, 4)` and the two have nothing to do with each other.
//!
//! ## Reading an output is a separate permission from running the model
//!
//! A model that runs is not a number a surface may show. `docs/ml.md` withholds the sleep
//! staging vocabulary — four logits per epoch whose class order has never been mapped onto
//! Maverick's — and withholds the hypertension risk level, because `halite_risk_tree`'s `label`
//! is the ensemble's own argmax and the calibration onto a risk level is post-processing that
//! is not in the graph. Both models ship, both run, and neither may be rendered as the thing
//! its name suggests. [`Interpretation`] carries that distinction so a surface cannot lose it.

use super::registry::ModelId;
use mav_model::stream::StreamKind;
use serde::{Deserialize, Serialize};

/// Beat-to-beat intervals from either physiological source, matching
/// [`crate::capability`]'s rule: an analytic that only needs the timing of beats is served by
/// an optical pulse as well as an electrical R peak.
const ANY_INTERVAL: &[StreamKind] = &[StreamKind::RrInterval, StreamKind::PulseInterval];

/// A single-channel optical signal, however the strap labels it. The PPG front-ends in
/// [`super::ppg`] take one channel of reflectance; `RedPpg` and `InfraredPpg` are that same
/// signal from a named LED, so a strap that publishes only those still feeds the encoders.
const ANY_PPG: &[StreamKind] = &[StreamKind::Ppg, StreamKind::RedPpg, StreamKind::InfraredPpg];

/// A wearer-profile field a model's input vector carries directly.
///
/// These are not sensor readings and cannot be waited for: absent a profile the model does not
/// become available later in the night, it stays unavailable until the wearer fills the field
/// in. Surfaces distinguish the two, which is why this is not folded into a stream.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileField {
    Sex,
    Age,
    Height,
    Weight,
}

impl ProfileField {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Sex => "sex",
            Self::Age => "age",
            Self::Height => "height",
            Self::Weight => "weight",
        }
    }
}

/// The product signal a model contributes to.
///
/// One signal is one thing the wearer is shown, which is deliberately coarser than one model:
/// five activity cores and a Rust ensemble produce one activity readout, and splitting that
/// across five surfaces would describe the conversion rather than the product.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Signal {
    /// Activity recognition over the day's candidate segments.
    Activity,
    /// Active energy expenditure for a window.
    EnergyExpenditure,
    /// Step eligibility from the step-motion vector.
    StepEligibility,
    /// Awake heart rate imputed across a gap in the optical signal.
    AwakeHeartRate,
    /// Daytime variability imputed across the same kind of gap.
    DaytimeHrv,
    /// Per-column heart rate through a workout, from the PPG spectrogram.
    WorkoutHeartRate,
    /// Cardiovascular-age family: the encoder and the probe heads over it.
    Cardiovascular,
    /// The hypertension-risk path: PulseNet embedding, per-segment score, tree.
    HypertensionRisk,
    /// Sleep staging and apnea over a night.
    Sleep,
    /// Illness likelihood from thirty days of daily deviations.
    IllnessRisk,
    /// Menstrual-cycle awareness. Awareness only — never contraception or fertility.
    CycleAwareness,
    /// A general-purpose PPG embedding with no head fitted against it in this build.
    PpgFoundation,
}

impl Signal {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Activity => "activity",
            Self::EnergyExpenditure => "energy_expenditure",
            Self::StepEligibility => "step_eligibility",
            Self::AwakeHeartRate => "awake_heart_rate",
            Self::DaytimeHrv => "daytime_hrv",
            Self::WorkoutHeartRate => "workout_heart_rate",
            Self::Cardiovascular => "cardiovascular",
            Self::HypertensionRisk => "hypertension_risk",
            Self::Sleep => "sleep",
            Self::IllnessRisk => "illness_risk",
            Self::CycleAwareness => "cycle_awareness",
            Self::PpgFoundation => "ppg_foundation",
        }
    }
}

/// How far a model's output may be read once it exists.
///
/// Every model in this build is provisional in the sense `docs/ml.md` means — none has been
/// checked against labelled ground truth — so this axis is not about confidence. It is about
/// whether the numbers coming out of the graph have a *meaning* this build has admitted.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpretation {
    /// The output is a quantity in known units, and a surface may render it as one.
    Quantity,
    /// The output is a probability the graph already normalised. Rendering it as a percentage
    /// is fair; rendering it as a diagnosis is not.
    Probability,
    /// The output is an embedding. It is real, it is reproducible, and it means nothing on its
    /// own — a surface may say it was computed and may not turn it into a reading.
    Embedding,
    /// The output is numerically valid and its vocabulary has not been admitted. It may be
    /// stored and counted; it may not be rendered as the thing its tensor name suggests.
    /// Carries the reason so a surface can say *why* rather than showing an empty box.
    VocabularyNotAdmitted(&'static str),
}

impl Interpretation {
    /// True when a surface may render this output as a value rather than as a state.
    pub const fn is_displayable(self) -> bool {
        matches!(self, Self::Quantity | Self::Probability)
    }
}

/// Why a front-end is not ported — and whether that is work or a wall.
///
/// The distinction is the whole value of this type. "Not ported" covers two situations that look
/// identical in a status table and are nothing alike: one is a piece of deterministic Rust nobody
/// has written yet, the other is a feature stream that no device Maverick supports produces at
/// all. Reporting them the same way is how the second gets promised.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Blocker {
    /// The wrapper's logic is readable in the archive and every input exists on a supported
    /// strap. This is work, and the note names what has to be built.
    Recoverable,
    /// The model reads features an Oura ring's own firmware computes — `stride_frequency`,
    /// `gait_amplitude_frac`, `acm_average_*`, `ring_met`, `motion_seconds` and the rest of the
    /// `stepmotion`/`motion` blocks. The archives consume those features; they do not contain the
    /// code that produces them, and no strap Maverick supports emits them. Porting the wrapper
    /// would produce a correctly shaped tensor with nothing real in it.
    RingFirmwareFeatures,
}

/// Whether this build can actually assemble this model's input tensors.
///
/// Conversion succeeded for all forty-one models; that is a statement about the *graph*. Filling
/// its input tensor is a separate problem, and for most of the zoo the code that does it lived in
/// the training wrapper — the part `docs/ml.md` deliberately does not convert, because
/// data-dependent windowing and feature assembly belong in shared, fixture-tested Rust.
///
/// Three of those front-ends are ported ([`super::ppg`]). The rest are not, and the archives they
/// would be ported from are not in this repository. A model in that state loads, runs, and
/// matches its reference on stored vectors — and there is nothing in this build that can honestly
/// fill its input from a wearer's samples. Saying so is the point of this type. The alternative
/// is a plausible-looking feature vector assembled by guesswork, which would produce a number
/// that means nothing and looks exactly like one that does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrontEnd {
    /// A named, golden-vector-tested Rust front-end builds this model's inputs.
    Ported(&'static str),
    /// Every input tensor is an upstream model's output, assembled by named Rust glue. Runnable
    /// exactly when its upstream is.
    Upstream,
    /// The training wrapper's feature assembly is not in this build. Names what is missing, so
    /// the gap is a specific piece of work rather than a shrug.
    NotPorted(Blocker, &'static str),
}

impl FrontEnd {
    /// True when something in this build can produce this model's inputs.
    pub const fn is_available(self) -> bool {
        matches!(self, Self::Ported(_) | Self::Upstream)
    }
}

/// Everything the runtime needs to know about running one model in production.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModelPipeline {
    pub model: ModelId,
    /// Streams whose absence makes this model unrunnable. All must be present.
    pub requires_all: &'static [StreamKind],
    /// Groups within which any one member will do. A group with nothing present is reported by
    /// its first member — the stream the preprocessing would rather have.
    pub requires_any: &'static [&'static [StreamKind]],
    /// Profile fields the input vector carries directly.
    pub requires_profile: &'static [ProfileField],
    /// Models whose outputs are this model's inputs. Ordering comes from this, not from a
    /// hand-written list on either platform.
    pub depends_on: &'static [ModelId],
    pub signal: Signal,
    pub interpretation: Interpretation,
    /// Whether this build can assemble the input tensors at all.
    pub front_end: FrontEnd,
    /// Why this model needs what it needs — the evidence for the row, in one line.
    pub note: &'static str,
}

// One row is nine facts about one model, and every one of them is load-bearing. Bundling them
// into sub-structs to satisfy the argument count would put a name between the reader and the
// data without adding meaning.
#[allow(clippy::too_many_arguments)]
const fn entry(
    model: ModelId,
    requires_all: &'static [StreamKind],
    requires_any: &'static [&'static [StreamKind]],
    requires_profile: &'static [ProfileField],
    depends_on: &'static [ModelId],
    signal: Signal,
    interpretation: Interpretation,
    front_end: FrontEnd,
    note: &'static str,
) -> ModelPipeline {
    ModelPipeline {
        model,
        requires_all,
        requires_any,
        requires_profile,
        depends_on,
        signal,
        interpretation,
        front_end,
        note,
    }
}

/// The staging vocabulary is not Maverick's. `docs/ml.md`: the class order is the training
/// vocabulary and has not been mapped, and "until it is made the staging output is not
/// something a surface may display".
const SLEEP_STAGING_NOT_ADMITTED: &str =
    "the four staging logits are in the training vocabulary, which has not been mapped onto \
     Maverick's sleep stages";

/// `label` is the ensemble's own argmax. `docs/ml.md`: the mapping onto a risk level uses age-
/// and sex-specific thresholds and "is post-processing and is not in the converted graph".
const RISK_LEVEL_NOT_ADMITTED: &str =
    "the tree returns its own argmax, not a risk level; the calibration onto one is not in this \
     build";

/// One row per admitted model, in the same order as [`ALL_MODELS`].
///
/// `tests::every_model_has_exactly_one_row` holds the two lists together, so a conversion that
/// adds a model fails here until someone says where its inputs come from. That is the point:
/// the failure should be a question about the product, not a model that quietly ships unused.
pub const PIPELINE: &[ModelPipeline; 41] = &[
    // ---- Activity ------------------------------------------------------------------
    // The five 3.1.11 cores plus the two standalone heads. Features come from MET, motion,
    // step-motion, heart rate and temperature (docs/ml.md, `activity_detection`).
    entry(
        ModelId::ActivityContextEmbedding,
        &[StreamKind::Imu],
        &[&[StreamKind::HeartRate], &[StreamKind::StepCount]],
        &[],
        &[],
        Signal::Activity,
        Interpretation::Embedding,
        FrontEnd::NotPorted(Blocker::RingFirmwareFeatures, "the 85 context features per segment"),
        "85 context features per segment; one of the three tables the history transformer's \
         573-column input is assembled from",
    ),
    entry(
        ModelId::ActivityDetection,
        &[StreamKind::Imu],
        &[&[StreamKind::HeartRate], &[StreamKind::StepCount]],
        &[],
        &[],
        Signal::Activity,
        Interpretation::Quantity,
        FrontEnd::NotPorted(Blocker::RingFirmwareFeatures, "the 77 per-segment features over MET, motion, step-motion, heart rate and temperature"),
        "64 candidate segments x 77 features from MET, motion, step-motion, heart rate and \
         temperature; the 262 columns are four probabilities, a 256-value embedding and the \
         segment bounds",
    ),
    entry(
        ModelId::ActivityEnsemble,
        &[],
        &[],
        &[],
        // Registry role: "Combines the primary and secondary segment heads into the final
        // per-segment output". Both inputs are (1, 16, 260); both heads output (1, 16, 260).
        &[
            ModelId::ActivityPrimarySegments,
            ModelId::ActivitySecondarySegments,
        ],
        Signal::Activity,
        Interpretation::Quantity,
        FrontEnd::Upstream,
        "combines the primary and secondary segment heads; needs no sensor of its own",
    ),
    entry(
        ModelId::ActivityHistoryTransformer,
        &[],
        &[],
        &[],
        // 312 (context) + 256 (behaviour) + 8 (source) = 576, and the source wrapper "writes
        // three of the eight values back over the is_labeling, is_workout and is_sleep feature
        // slots" (manifest note), which is why the input is 573 rather than 576.
        &[
            ModelId::ActivityContextEmbedding,
            ModelId::BehaviorEmbedding,
            ModelId::SourceEmbedding,
        ],
        Signal::Activity,
        Interpretation::Embedding,
        FrontEnd::Upstream,
        "self-attention over sixteen history segments; its 573 columns are the context, \
         behaviour and source tables with three source values written back over feature slots",
    ),
    entry(
        ModelId::ActivityPrimarySegments,
        &[StreamKind::Imu],
        &[&[StreamKind::HeartRate], &[StreamKind::StepCount]],
        &[],
        &[],
        Signal::Activity,
        Interpretation::Quantity,
        FrontEnd::NotPorted(Blocker::RingFirmwareFeatures, "the 110 per-segment features"),
        "primary per-segment head over 110 features",
    ),
    entry(
        ModelId::ActivitySecondarySegments,
        &[],
        &[],
        &[],
        // Registry role: "Secondary per-segment activity head, over the history encoder's
        // output". Input (1, 16, 336) is the transformer's `encoded` (1, 16, 336).
        &[ModelId::ActivityHistoryTransformer],
        Signal::Activity,
        Interpretation::Quantity,
        FrontEnd::Upstream,
        "secondary per-segment head, over the history encoder's output",
    ),
    entry(
        ModelId::ActivityTransition,
        &[StreamKind::Imu],
        &[&[StreamKind::HeartRate]],
        &[],
        &[],
        Signal::Activity,
        Interpretation::Quantity,
        FrontEnd::NotPorted(Blocker::RingFirmwareFeatures, "the 29 per-minute features"),
        "one transition score per minute over a 64-minute window; answers when, not what",
    ),
    entry(
        ModelId::BehaviorEmbedding,
        &[],
        &[],
        &[],
        &[],
        Signal::Activity,
        Interpretation::Embedding,
        FrontEnd::NotPorted(Blocker::RingFirmwareFeatures, "the behaviour id vocabulary, which is the activity model's and is not in this build"),
        "sixteen behaviour ids into the 256-d space; ids are Maverick's own, not a sensor",
    ),
    entry(
        ModelId::SourceEmbedding,
        &[],
        &[],
        &[],
        &[],
        Signal::Activity,
        Interpretation::Embedding,
        FrontEnd::NotPorted(Blocker::RingFirmwareFeatures, "the provenance id vocabulary, and the three feature slots its wrapper writes back over"),
        "an eleven-row provenance table over how each segment was recorded; not a sensor",
    ),
    // ---- Awake heart rate ----------------------------------------------------------
    entry(
        ModelId::AwhrImputation,
        &[StreamKind::Imu],
        &[&[StreamKind::HeartRate]],
        &[],
        &[],
        Signal::AwakeHeartRate,
        Interpretation::Quantity,
        FrontEnd::NotPorted(Blocker::RingFirmwareFeatures, "the 13 context features either side of a gap"),
        "fills awake heart rate across a gap from step-motion and activity context either \
         side; bidirectional, so it needs both sides of the gap",
    ),
    entry(
        ModelId::AwhrProfileCore,
        &[StreamKind::Imu],
        &[&[StreamKind::HeartRate]],
        &[],
        &[],
        Signal::AwakeHeartRate,
        Interpretation::Embedding,
        FrontEnd::NotPorted(Blocker::RingFirmwareFeatures, "the 19 per-step features and the mask over them"),
        "per-step activity features behind the profile choice; the masking that LiteRT rejects \
         is Rust's",
    ),
    entry(
        ModelId::AwhrProfileRecurrent,
        &[],
        &[],
        &[],
        // Registry role: "Bidirectional recurrent layer between the profile features and the
        // profile head". Core emits (60, 6); this takes (1, 60, 6).
        &[ModelId::AwhrProfileCore],
        Signal::AwakeHeartRate,
        Interpretation::Embedding,
        FrontEnd::Upstream,
        "the recurrent layer between the profile features and the profile head",
    ),
    entry(
        ModelId::AwhrProfileHead,
        &[],
        &[],
        &[],
        // Registry role: "Three-way profile head over the recurrent layer's output". Recurrent
        // emits (1, 60, 32); this takes (60, 32).
        &[ModelId::AwhrProfileRecurrent],
        Signal::AwakeHeartRate,
        Interpretation::Quantity,
        FrontEnd::Upstream,
        "three-way profile head over the recurrent layer's output",
    ),
    // ---- Cardiovascular ------------------------------------------------------------
    entry(
        ModelId::CvaEncoder,
        &[],
        &[ANY_PPG],
        &[],
        &[],
        Signal::Cardiovascular,
        Interpretation::Embedding,
        FrontEnd::Ported("ppg::cva_pulse, truncated to the encoder's 1,024-sample block"),
        "1,024 normalised pulse samples from ppg::cva_pulse, truncated to the model's own \
         block_size, into a 128-d embedding",
    ),
    entry(
        ModelId::CvaProbesMale,
        &[],
        &[ANY_PPG],
        &[
            ProfileField::Sex,
            ProfileField::Age,
            ProfileField::Height,
            ProfileField::Weight,
        ],
        // Contract-provable: input `embeddings` (1, 128) is the encoder's output `embeddings`
        // (1, 128). The head selects its branch by a string gender argument, so Rust picks.
        &[ModelId::CvaEncoder],
        Signal::Cardiovascular,
        Interpretation::Quantity,
        FrontEnd::Upstream,
        "the male branch of the probe head; Rust picks the branch because the selector is a \
         string argument between tensor arguments",
    ),
    entry(
        ModelId::CvaProbesFemale,
        &[],
        &[ANY_PPG],
        &[
            ProfileField::Sex,
            ProfileField::Age,
            ProfileField::Height,
            ProfileField::Weight,
        ],
        &[ModelId::CvaEncoder],
        Signal::Cardiovascular,
        Interpretation::Quantity,
        FrontEnd::Upstream,
        "the female branch of the same head",
    ),
    entry(
        ModelId::CvaPredictorV1Base,
        &[],
        &[ANY_PPG],
        &[
            ProfileField::Age,
            ProfileField::Height,
            ProfileField::Weight,
        ],
        &[],
        Signal::Cardiovascular,
        Interpretation::Quantity,
        FrontEnd::NotPorted(Blocker::Recoverable, "the pulse/VPG/APG triple and the eight exogenous values"),
        "the previous-generation CNN: one 256-sample block across the pulse/VPG/APG triple plus \
         eight exogenous values; the windowing is Rust's",
    ),
    // ---- Hypertension risk ---------------------------------------------------------
    entry(
        ModelId::PulsenetFoundation,
        &[],
        &[ANY_PPG],
        &[],
        &[],
        Signal::HypertensionRisk,
        Interpretation::Embedding,
        FrontEnd::Ported("ppg::pulsenet_input"),
        "30 s at 50 Hz through ppg::pulsenet_input into a 256-d embedding; the space the \
         hypertension heads were fitted against",
    ),
    entry(
        ModelId::HalitePpgScore,
        &[],
        &[ANY_PPG],
        &[],
        // Contract-provable: input `embeddings` (1, 256) is PulseNet's output.
        &[ModelId::PulsenetFoundation],
        Signal::HypertensionRisk,
        Interpretation::Quantity,
        FrontEnd::Upstream,
        "a linear head over one PulseNet embedding; one segment, one score",
    ),
    entry(
        ModelId::HaliteRiskTree,
        &[],
        &[ANY_PPG],
        &[
            ProfileField::Sex,
            ProfileField::Age,
            ProfileField::Height,
            ProfileField::Weight,
        ],
        // Manifest note: "Feature vector is user_info(4) || baselines(8) || aggregated ppg
        // score(1)" — thirteen values, and the aggregation across segments is Rust's.
        &[ModelId::HalitePpgScore],
        Signal::HypertensionRisk,
        Interpretation::VocabularyNotAdmitted(RISK_LEVEL_NOT_ADMITTED),
        FrontEnd::NotPorted(Blocker::Recoverable, "the eight baseline columns; the profile four and the aggregated score are available"),
        "user_info(4) with BMI substituted for weight, eight baselines, and the weighted-mean \
         PPG score across the history window",
    ),
    // ---- PPG foundation ------------------------------------------------------------
    entry(
        ModelId::PulsePpg,
        &[],
        &[ANY_PPG],
        &[],
        &[],
        Signal::PpgFoundation,
        Interpretation::Embedding,
        FrontEnd::Ported("ppg::pulse_ppg_input"),
        "240 s at 50 Hz into a 512-d embedding; open weights, and no head in this build is \
         fitted against this space",
    ),
    // ---- Sleep ---------------------------------------------------------------------
    entry(
        ModelId::SleepnetMoonstone,
        &[StreamKind::Imu],
        &[
            ANY_INTERVAL,
            &[StreamKind::Spo2Percent, StreamKind::Spo2Raw],
        ],
        &[],
        &[],
        Signal::Sleep,
        Interpretation::VocabularyNotAdmitted(SLEEP_STAGING_NOT_ADMITTED),
        FrontEnd::NotPorted(Blocker::Recoverable, "the per-epoch ibi, amplitude and spo2 channels at 64 samples an epoch, and the motion low-res channel"),
        "1,800 thirty-second epochs; high_res channels are ibi, amplitude and spo2, low_res is \
         motion seconds per epoch",
    ),
    entry(
        ModelId::SleepnetBdi,
        &[],
        &[ANY_INTERVAL],
        &[],
        &[],
        Signal::Sleep,
        Interpretation::VocabularyNotAdmitted(SLEEP_STAGING_NOT_ADMITTED),
        FrontEnd::NotPorted(Blocker::Recoverable, "the per-epoch ibi and amplitude channels at 64 samples an epoch"),
        "the same night from intervals alone: high_res channels are ibi and amplitude, no \
         low_res, so it runs on any strap that produces intervals",
    ),
    entry(
        ModelId::SleepnetBdiV3,
        &[],
        &[ANY_INTERVAL],
        &[],
        &[],
        Signal::Sleep,
        Interpretation::VocabularyNotAdmitted(SLEEP_STAGING_NOT_ADMITTED),
        FrontEnd::NotPorted(Blocker::Recoverable, "the per-epoch ibi and amplitude channels at 64 samples an epoch"),
        "the previous generation of the same contract, a third of the size",
    ),
    // ---- Steps ---------------------------------------------------------------------
    entry(
        ModelId::StepEligibility,
        &[StreamKind::Imu],
        &[],
        &[],
        &[],
        Signal::StepEligibility,
        Interpretation::Embedding,
        FrontEnd::NotPorted(Blocker::RingFirmwareFeatures, "the 19-value step-motion vector; steps_motion_decoder emits 11 columns, not these"),
        "one 19-value step-motion vector into four eligibility features; the boolean mask that \
         LiteRT rejects is Rust's",
    ),
    entry(
        ModelId::StepHead,
        &[],
        &[],
        &[],
        // Contract-provable: `eligibility_features` (1, 4) both sides.
        &[ModelId::StepEligibility],
        Signal::StepEligibility,
        Interpretation::Quantity,
        FrontEnd::Upstream,
        "turns the eligibility features into one score",
    ),
    entry(
        ModelId::StepMultiplier,
        &[],
        &[],
        &[],
        // Contract-provable: `eligibility` (1, 1) both sides.
        &[ModelId::StepHead],
        Signal::StepEligibility,
        Interpretation::Quantity,
        FrontEnd::Upstream,
        "a two-parameter sigmoid scaling the raw step count by eligibility",
    ),
    // ---- Energy --------------------------------------------------------------------
    entry(
        ModelId::EnergyExpenditureHr,
        &[StreamKind::Imu],
        &[&[StreamKind::HeartRate]],
        &[],
        &[],
        Signal::EnergyExpenditure,
        Interpretation::Quantity,
        FrontEnd::NotPorted(Blocker::RingFirmwareFeatures, "the 50-value window feature vector"),
        "the heart-rate-available branch; Rust picks between the two branches",
    ),
    entry(
        ModelId::EnergyExpenditureNoHr,
        &[StreamKind::Imu],
        &[],
        &[],
        &[],
        Signal::EnergyExpenditure,
        Interpretation::Quantity,
        FrontEnd::NotPorted(Blocker::RingFirmwareFeatures, "the 42-value window feature vector"),
        "the sibling branch for a window with no usable heart rate",
    ),
    // ---- Imputation ----------------------------------------------------------------
    entry(
        ModelId::DhrvImputation,
        &[StreamKind::SkinTemp],
        &[ANY_INTERVAL, &[StreamKind::HeartRate]],
        &[],
        &[],
        Signal::DaytimeHrv,
        Interpretation::Quantity,
        FrontEnd::NotPorted(Blocker::Recoverable, "the ten-value vector of temperature, MET, heart rate and the wearer's baselines"),
        "skin temperature, ring and total MET, heart rate, and the wearer's own baselines for \
         variability, heart rate and temperature, reduced to ten values in Rust",
    ),
    // ---- Workout heart rate --------------------------------------------------------
    entry(
        ModelId::WhrUnetEncoder,
        &[],
        &[ANY_PPG],
        &[],
        &[],
        Signal::WorkoutHeartRate,
        Interpretation::Embedding,
        FrontEnd::NotPorted(Blocker::Recoverable, "the 4-channel 128x128 workout PPG spectrogram"),
        "a U-Net over the workout PPG spectrogram window",
    ),
    entry(
        ModelId::WhrUnetHead,
        &[],
        &[ANY_PPG],
        &[],
        // Contract-provable: `features` (1, 2, 128, 128) both sides. The aten.equal.default
        // guard that blocked the combined core is the glue between them, which is Rust's.
        &[ModelId::WhrUnetEncoder],
        Signal::WorkoutHeartRate,
        Interpretation::Quantity,
        FrontEnd::Upstream,
        "the recurrent head turning U-Net features into a per-column heart rate",
    ),
    // ---- Daily health --------------------------------------------------------------
    entry(
        ModelId::IllnessDetection,
        &[StreamKind::HeartRate, StreamKind::SkinTemp],
        &[ANY_INTERVAL],
        &[],
        &[],
        Signal::IllnessRisk,
        Interpretation::Probability,
        FrontEnd::NotPorted(Blocker::Recoverable, "the eight daily biometric deviation series and the four scalars"),
        "eight daily biometrics over thirty days plus four scalars; the output is already a \
         probability and applying another sigmoid would be wrong",
    ),
    // ---- Cycle awareness -----------------------------------------------------------
    // All eight take (1, 40, 3) daily series and (1, 40, 4) scalars over up to forty cycle
    // days. The series is nightly skin temperature and the beats that time it, which is the
    // same evidence `capability::AnalyticId::CyclePhase` already negotiates for.
    entry(
        ModelId::PopsicleOvulationDetection,
        &[StreamKind::SkinTemp],
        &[ANY_INTERVAL],
        &[ProfileField::Sex],
        &[],
        Signal::CycleAwareness,
        Interpretation::Probability,
        FrontEnd::Ported("cycle::cycle_input"),
        "ovulation detection over up to forty cycle days of temperature and beats",
    ),
    entry(
        ModelId::PopsicleOvulationDetectionV16,
        &[StreamKind::SkinTemp],
        &[ANY_INTERVAL],
        &[ProfileField::Sex],
        &[],
        Signal::CycleAwareness,
        Interpretation::Probability,
        FrontEnd::Ported("cycle::cycle_input"),
        "the previous generation of the ovulation detector",
    ),
    entry(
        ModelId::PopsicleOvulationPrediction,
        &[StreamKind::SkinTemp],
        &[ANY_INTERVAL],
        &[ProfileField::Sex],
        &[],
        Signal::CycleAwareness,
        Interpretation::Quantity,
        FrontEnd::Ported("cycle::cycle_input"),
        "days until the next ovulation, per cycle day",
    ),
    entry(
        ModelId::PopsicleOvulationPredictionV16,
        &[StreamKind::SkinTemp],
        &[ANY_INTERVAL],
        &[ProfileField::Sex],
        &[],
        Signal::CycleAwareness,
        Interpretation::Quantity,
        FrontEnd::Ported("cycle::cycle_input"),
        "the previous generation of the ovulation predictor",
    ),
    entry(
        ModelId::PopsiclePeriodPrediction,
        &[StreamKind::SkinTemp],
        &[ANY_INTERVAL],
        &[ProfileField::Sex],
        &[],
        Signal::CycleAwareness,
        Interpretation::Quantity,
        FrontEnd::Ported("cycle::cycle_input"),
        "days until the next period, per cycle day",
    ),
    entry(
        ModelId::PopsiclePeriodPredictionV16,
        &[StreamKind::SkinTemp],
        &[ANY_INTERVAL],
        &[ProfileField::Sex],
        &[],
        Signal::CycleAwareness,
        Interpretation::Quantity,
        FrontEnd::Ported("cycle::cycle_input"),
        "the previous generation of the period predictor",
    ),
    entry(
        ModelId::PopsicleMinFollicular,
        &[StreamKind::SkinTemp],
        &[ANY_INTERVAL],
        &[ProfileField::Sex],
        &[],
        Signal::CycleAwareness,
        Interpretation::Quantity,
        FrontEnd::NotPorted(Blocker::Recoverable, "the nine-value feature vector; cycle::cycle_input builds the day-sequence pair the other six cycle heads take, and this head takes neither of those tensors"),
        "the shortest plausible follicular phase for this wearer, from nine features",
    ),
    entry(
        ModelId::PopsicleMinFollicularV16,
        &[StreamKind::SkinTemp],
        &[ANY_INTERVAL],
        &[ProfileField::Sex],
        &[],
        Signal::CycleAwareness,
        Interpretation::Quantity,
        FrontEnd::NotPorted(Blocker::Recoverable, "the nine-value feature vector, as for the current generation"),
        "the previous generation of the follicular-length head",
    ),
];

/// One model's row.
///
/// `None` cannot happen for a model this build ships — `tests::every_model_has_exactly_one_row`
/// is what makes that true — and it is still returned rather than unwrapped, because a registry
/// regenerated without its pipeline row should degrade to "no opinion about this model" instead
/// of taking the app down.
pub fn pipeline_of(model: ModelId) -> Option<&'static ModelPipeline> {
    PIPELINE.iter().find(|entry| entry.model == model)
}

/// Every model that contributes to one signal, in [`ALL_MODELS`] order.
pub fn models_for(signal: Signal) -> Vec<ModelId> {
    PIPELINE
        .iter()
        .filter(|entry| entry.signal == signal)
        .map(|entry| entry.model)
        .collect()
}

/// Every signal this build can produce, in first-appearance order.
pub fn all_signals() -> Vec<Signal> {
    let mut signals: Vec<Signal> = Vec::new();
    for entry in PIPELINE {
        if !signals.contains(&entry.signal) {
            signals.push(entry.signal);
        }
    }
    signals
}

/// A training archive whose neural core ships as several models with the rest as Rust.
///
/// Six archives failed conversion as a whole and are recorded in `manifest.json`'s
/// `not_shipped`. That entry is easy to misread as "this capability is missing". It is not:
/// in every case the *wrapper* is what failed — a boolean mask, a data-dependent window, a
/// string branch selector — and the wrapper is precisely the part that belongs in Rust anyway.
/// The parameters ship. This table is what makes that checkable rather than asserted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Composite {
    /// The archive as `not_shipped` names it.
    pub archive: &'static str,
    /// The shipped cores that carry its parameters.
    pub cores: &'static [ModelId],
    /// What the wrapper did that is now Rust.
    pub rust_glue: &'static str,
}

pub const COMPOSITES: &[Composite; 6] = &[
    Composite {
        archive: "activity_segments",
        cores: &[
            ModelId::ActivityContextEmbedding,
            ModelId::BehaviorEmbedding,
            ModelId::SourceEmbedding,
            ModelId::ActivityHistoryTransformer,
            ModelId::ActivityPrimarySegments,
            ModelId::ActivitySecondarySegments,
            ModelId::ActivityEnsemble,
        ],
        rust_glue: "assembling the transformer's 573 columns from the three tables, including \
                    writing three source values back over the is_labeling, is_workout and \
                    is_sleep slots",
    },
    Composite {
        archive: "awhr_profile_selector",
        cores: &[
            ModelId::AwhrProfileCore,
            ModelId::AwhrProfileRecurrent,
            ModelId::AwhrProfileHead,
        ],
        rust_glue: "the boolean mask over the per-step features, which LiteRT will not trace",
    },
    Composite {
        archive: "cva_predictor",
        cores: &[
            ModelId::CvaEncoder,
            ModelId::CvaProbesMale,
            ModelId::CvaProbesFemale,
        ],
        rust_glue: "ppg::cva_pulse, truncation to the encoder's 1,024-sample block, and picking \
                    the probe branch the string gender argument selects",
    },
    Composite {
        archive: "cva_predictor_v1",
        cores: &[ModelId::CvaPredictorV1Base],
        rust_glue: "windowing the longer pulse train into 272-column blocks",
    },
    Composite {
        archive: "step_counter",
        cores: &[
            ModelId::StepEligibility,
            ModelId::StepHead,
            ModelId::StepMultiplier,
        ],
        rust_glue: "the boolean-mask gating in the parent module",
    },
    Composite {
        archive: "whr_unet",
        cores: &[ModelId::WhrUnetEncoder, ModelId::WhrUnetHead],
        rust_glue: "the aten.equal.default guard in the glue between encoder and head",
    },
];

#[cfg(test)]
mod tests {
    use super::super::registry::ALL_MODELS;
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_model_has_exactly_one_row() {
        assert_eq!(PIPELINE.len(), ALL_MODELS.len());
        let declared: HashSet<_> = PIPELINE.iter().map(|entry| entry.model).collect();
        assert_eq!(declared.len(), PIPELINE.len(), "a model appears twice");
        for model in ALL_MODELS {
            assert!(
                declared.contains(model),
                "{} has no pipeline row: say where its inputs come from",
                model.contract().slug
            );
        }
    }

    /// Every declared edge is either a *fill* — the upstream output has exactly the element
    /// count of a downstream input, which is the provable case — or a *component*, where the
    /// upstream contributes part of a wider vector Rust assembles. An upstream that produces
    /// more values than the widest input it feeds is neither, and is a shape drift.
    ///
    /// This is what turns a re-conversion that changes a shape into a failure here rather than
    /// at run time. It deliberately does not claim to verify the component edges' arithmetic;
    /// the two compositions this build has are asserted by name below.
    #[test]
    fn every_dependency_either_fills_an_input_or_is_a_component_of_one() {
        for entry in PIPELINE {
            let contract = entry.model.contract();
            let consumed: Vec<usize> = contract
                .inputs
                .iter()
                .map(|spec| spec.element_count())
                .collect();
            let widest = consumed.iter().copied().max().unwrap_or(0);
            for upstream in entry.depends_on {
                let produced: Vec<usize> = upstream
                    .contract()
                    .outputs
                    .iter()
                    .map(|spec| spec.element_count())
                    .collect();
                assert!(
                    produced.iter().any(|out| *out <= widest),
                    "{} depends on {}, which produces {:?}; its widest input takes only {}",
                    contract.slug,
                    upstream.contract().slug,
                    produced,
                    widest,
                );
            }
        }
    }

    /// The six edges where the upstream output *is* the downstream input, name and shape. These
    /// need no interpretation, so they are pinned exactly: a re-conversion that changes either
    /// side breaks this test rather than producing a runtime tensor-shape error on a phone.
    #[test]
    fn the_provable_edges_match_on_name_and_shape() {
        const FILLS: &[(ModelId, ModelId, &str)] = &[
            (ModelId::CvaEncoder, ModelId::CvaProbesMale, "embeddings"),
            (ModelId::CvaEncoder, ModelId::CvaProbesFemale, "embeddings"),
            (
                ModelId::PulsenetFoundation,
                ModelId::HalitePpgScore,
                "embeddings",
            ),
            (
                ModelId::StepEligibility,
                ModelId::StepHead,
                "eligibility_features",
            ),
            (ModelId::StepHead, ModelId::StepMultiplier, "eligibility"),
            (ModelId::WhrUnetEncoder, ModelId::WhrUnetHead, "features"),
        ];
        for (upstream, downstream, tensor) in FILLS {
            let produced = upstream
                .contract()
                .output(tensor)
                .unwrap_or_else(|| panic!("{} has no output {tensor}", upstream.contract().slug));
            let consumed = downstream
                .contract()
                .input(tensor)
                .unwrap_or_else(|| panic!("{} has no input {tensor}", downstream.contract().slug));
            assert_eq!(
                produced.shape,
                consumed.shape,
                "{} -> {} on {tensor}",
                upstream.contract().slug,
                downstream.contract().slug
            );
            assert!(
                pipeline_of(*downstream).is_some_and(|entry| entry.depends_on.contains(upstream)),
                "{} takes {}'s {tensor} and does not declare the edge",
                downstream.contract().slug,
                upstream.contract().slug
            );
        }
    }

    /// The history transformer's 573 columns, from the manifest note that explains them: three
    /// tables totalling 576, less the three source values its wrapper "writes back over the
    /// is_labeling, is_workout and is_sleep feature slots" rather than appending.
    ///
    /// Written out because 573 is otherwise an unexplained number, and an unexplained number in
    /// a tensor contract is how a preprocessing bug survives review.
    #[test]
    fn the_transformer_input_is_three_tables_less_three_overwritten_slots() {
        fn width(model: ModelId, tensor: &str) -> usize {
            *model
                .contract()
                .output(tensor)
                .expect("declared output")
                .shape
                .last()
                .expect("a shape has a last axis")
        }
        let context = width(ModelId::ActivityContextEmbedding, "embeddings");
        let behaviour = width(ModelId::BehaviorEmbedding, "embeddings");
        let source = width(ModelId::SourceEmbedding, "source_features");
        let overwritten = 3;
        let segments = ModelId::ActivityHistoryTransformer
            .contract()
            .input("segments")
            .expect("declared input");
        assert_eq!(
            context + behaviour + source - overwritten,
            *segments.shape.last().expect("a shape has a last axis"),
            "{context} + {behaviour} + {source} - {overwritten} should be the segment width",
        );
    }

    /// `halite_risk_tree` takes thirteen values, of which the score head supplies one. The
    /// manifest note spells the split out; this holds the arithmetic to it.
    #[test]
    fn the_risk_tree_takes_four_profile_fields_eight_baselines_and_one_score() {
        let features = ModelId::HaliteRiskTree
            .contract()
            .input("features")
            .expect("declared input");
        let score = ModelId::HalitePpgScore
            .contract()
            .output("ppg_score")
            .expect("declared output");
        assert_eq!(score.element_count(), 1, "one score per aggregation");
        assert_eq!(features.element_count(), 4 + 8 + score.element_count());
        assert_eq!(
            pipeline_of(ModelId::HaliteRiskTree)
                .expect("declared row")
                .requires_profile
                .len(),
            4,
            "user_info is four fields",
        );
    }

    #[test]
    fn the_dependency_graph_is_acyclic() {
        fn visit(model: ModelId, seen: &mut Vec<ModelId>) {
            assert!(
                !seen.contains(&model),
                "{} is part of a dependency cycle",
                model.contract().slug
            );
            seen.push(model);
            for upstream in pipeline_of(model).expect("declared row").depends_on {
                visit(*upstream, seen);
            }
            seen.pop();
        }
        for model in ALL_MODELS {
            visit(*model, &mut Vec::new());
        }
    }

    /// A model whose inputs are entirely another model's outputs must not also claim a sensor:
    /// that would make it unavailable on a strap where its upstream ran perfectly well, which
    /// is the exact bug a hand-written per-platform list produces.
    #[test]
    fn a_pure_head_asks_for_no_sensor_of_its_own() {
        for entry in PIPELINE {
            let inputs = entry.model.contract().inputs.len();
            if entry.depends_on.len() >= inputs && !entry.depends_on.is_empty() {
                assert!(
                    entry.requires_all.is_empty(),
                    "{} is fed entirely by other models yet demands {:?}",
                    entry.model.contract().slug,
                    entry.requires_all,
                );
            }
        }
    }

    #[test]
    fn every_composite_names_shipped_cores_and_the_glue_that_replaced_its_wrapper() {
        for composite in COMPOSITES {
            assert!(
                !composite.cores.is_empty(),
                "{} claims no cores",
                composite.archive
            );
            assert!(
                !composite.rust_glue.is_empty(),
                "{} does not say what its wrapper became",
                composite.archive
            );
            for core in composite.cores {
                assert!(
                    ALL_MODELS.contains(core),
                    "{} names {} which this build does not ship",
                    composite.archive,
                    core.contract().slug
                );
            }
        }
    }

    /// The two withheld vocabularies are the ones `docs/ml.md` withholds, and no others. A
    /// model quietly marked undisplayable would look like a bug in the UI; a model quietly
    /// marked displayable would be a claim this build has not earned.
    #[test]
    fn only_sleep_staging_and_the_risk_level_are_withheld() {
        let withheld: Vec<&str> = PIPELINE
            .iter()
            .filter(|entry| {
                matches!(
                    entry.interpretation,
                    Interpretation::VocabularyNotAdmitted(_)
                )
            })
            .map(|entry| entry.model.contract().slug)
            .collect();
        assert_eq!(
            withheld,
            vec![
                "halite_risk_tree",
                "sleepnet_moonstone",
                "sleepnet_bdi",
                "sleepnet_bdi_v3",
            ]
        );
    }

    #[test]
    fn every_signal_has_at_least_one_model() {
        for signal in all_signals() {
            assert!(
                !models_for(signal).is_empty(),
                "{} has no models",
                signal.name()
            );
        }
    }

    /// The ported front-ends, pinned by name. This is the number that decides how much of the
    /// zoo is reachable from a wearer's samples, so it should move only when someone ports a
    /// front-end and says so in the same commit.
    #[test]
    fn the_ported_front_ends_are_pinned_and_the_rest_say_what_is_missing() {
        let ported: Vec<&str> = PIPELINE
            .iter()
            .filter(|entry| matches!(entry.front_end, FrontEnd::Ported(_)))
            .map(|entry| entry.model.contract().slug)
            .collect();
        // Declaration order, which groups by signal rather than by slug. Three PPG front-ends
        // from `ppg`, and the eight cycle heads that share `cycle::cycle_input`.
        assert_eq!(
            ported,
            vec![
                "cva_encoder",
                "pulsenet_foundation",
                "pulse_ppg",
                "popsicle_ovulation_detection",
                "popsicle_ovulation_detection_v16",
                "popsicle_ovulation_prediction",
                "popsicle_ovulation_prediction_v16",
                "popsicle_period_prediction",
                "popsicle_period_prediction_v16",
            ],
        );
        for entry in PIPELINE {
            if let FrontEnd::NotPorted(_, detail) = entry.front_end {
                assert!(
                    detail.len() > 15,
                    "{} does not say what its front-end would have to build",
                    entry.model.contract().slug
                );
            }
        }
    }

    /// A model marked `Upstream` must actually have upstream models, or it is a root claiming
    /// someone else will feed it and nothing ever will.
    /// Eleven models are blocked on Oura ring firmware features, and the list is pinned. A model
    /// moving out of this set means someone found a way to produce those features; a model moving
    /// into it means a capability was quietly given up. Both deserve to be a failing test.
    #[test]
    fn the_ring_firmware_wall_is_exactly_these_eleven() {
        let blocked: Vec<&str> = PIPELINE
            .iter()
            .filter(|entry| {
                matches!(
                    entry.front_end,
                    FrontEnd::NotPorted(Blocker::RingFirmwareFeatures, _)
                )
            })
            .map(|entry| entry.model.contract().slug)
            .collect();
        assert_eq!(blocked.len(), 11, "{blocked:?}");
        assert!(blocked.contains(&"activity_detection"));
        assert!(blocked.contains(&"step_eligibility"));
        assert!(blocked.contains(&"energy_expenditure_hr"));
    }

    /// The other eight are work rather than a wall, and every one names what has to be built.
    #[test]
    fn every_recoverable_front_end_says_what_remains() {
        for entry in PIPELINE {
            if let FrontEnd::NotPorted(Blocker::Recoverable, detail) = entry.front_end {
                assert!(
                    detail.len() > 15,
                    "{} is recoverable and does not say what from",
                    entry.model.contract().slug
                );
            }
        }
    }

    #[test]
    fn an_upstream_fed_model_has_upstream_models() {
        for entry in PIPELINE {
            if entry.front_end == FrontEnd::Upstream {
                assert!(
                    !entry.depends_on.is_empty(),
                    "{} says it is fed by upstream models and names none",
                    entry.model.contract().slug
                );
            }
        }
    }

    #[test]
    fn every_row_carries_its_evidence() {
        for entry in PIPELINE {
            assert!(
                entry.note.len() > 20,
                "{} has no note saying why it needs what it needs",
                entry.model.contract().slug
            );
        }
    }
}
