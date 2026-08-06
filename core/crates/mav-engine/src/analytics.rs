//! The scheduler between "what the device has" and "what the platform should run next".
//!
//! [`crate::model_host`] is a queue: it takes a validated request and hands it out. It has no
//! opinion about which of the forty-one models is worth running, in what order, or whether the
//! answer is already known. Without something holding those opinions, each platform would grow
//! its own copy — and two copies of a dependency order is two chances to run
//! `halite_risk_tree` before the score it consumes exists.
//!
//! So the opinions live here, once, over
//! [`mav_analytic::model_zoo::pipeline`]'s declarations:
//!
//! - **Availability.** A model whose streams are absent is not a failure and not a blank; it is
//!   unavailable with the sensors named, in the same shape [`mav_analytic::capability`] already
//!   reports analytics. A surface renders the reason.
//! - **Order.** Dependencies come from the pipeline table, so the plan is a topological sort and
//!   no platform has to know that the encoder precedes the probes.
//! - **Duplicate work.** Every completed inference is remembered against a fingerprint of its
//!   inputs *and* the artefact hash that produced it. Asking again with the same inputs is
//!   answered from memory; asking after a re-conversion is not, because the hash moved.
//! - **Pressure.** Interactive work wants the first useful result soon and is allowed to be
//!   expensive; deferred work wants to not be noticed. That is one number — how many stages a
//!   pass will start — and it belongs next to the ordering rather than in two schedulers.
//!
//! What is deliberately *not* here: threads, timers, and wall-clock policy. The core does not
//! know whether the app is foregrounded, whether the phone is charging, or whether the OS will
//! grant a background window. Those decide *when* a pass happens, which is the platform's to
//! answer — the same split `model_host` already draws.

use mav_analytic::model_zoo::pipeline::{
    pipeline_of, FrontEnd, ModelPipeline, ProfileField, Signal, PIPELINE,
};
use mav_analytic::model_zoo::{ModelId, NamedTensor};
use mav_model::stream::StreamKind;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// How hard a pass is allowed to push.
///
/// The two modes differ in one thing — how many stages one pass starts — because that is the
/// only lever the core owns. Everything else that distinguishes a burst from a trickle
/// (thread count, delegate, whether to prewarm) is a platform runtime decision, and pretending
/// to make it here would mean making it twice.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunMode {
    /// The wearer is looking at the screen. Start as much as the queue will hold, so the first
    /// useful result arrives sooner and the rest follow without another round trip.
    Interactive,
    /// Nobody is watching. Start a few stages per pass so a background window is not spent in
    /// one burst and a cancellation lands promptly.
    Deferred,
}

impl RunMode {
    /// How many stages one pass may start.
    ///
    /// Interactive is bounded by the host queue rather than by a smaller number of its own:
    /// the point of the mode is to not leave the accelerator idle between round trips.
    pub const fn burst(self) -> usize {
        match self {
            Self::Interactive => crate::model_host::MAX_PENDING_REQUESTS,
            Self::Deferred => 4,
        }
    }
}

/// What the device can currently offer a model.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Evidence {
    /// Stream kinds the day actually holds, as `Store::streams_between` reports them.
    pub streams: HashSet<StreamKind>,
    /// Profile fields the wearer has filled in.
    pub profile: HashSet<ProfileField>,
}

impl Evidence {
    pub fn new(streams: &[StreamKind], profile: &[ProfileField]) -> Self {
        Self {
            streams: streams.iter().copied().collect(),
            profile: profile.iter().copied().collect(),
        }
    }
}

/// Why a model cannot run, in the terms a surface needs to explain it.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Unmet {
    /// The sensors are not there. Naming them lets a surface say "needs a strap that reports
    /// SpO2" rather than showing an empty card.
    MissingStreams { streams: Vec<StreamKind> },
    /// The wearer has not filled these in. Different from a missing sensor because it is
    /// fixable from inside the app, and a surface should offer the fix rather than wait.
    MissingProfile { fields: Vec<ProfileField> },
    /// An upstream model could not run, so neither can this one. Carries the first cause so
    /// the surface reports the sensor that is actually missing rather than an inner model
    /// name the wearer has never heard of.
    UpstreamUnavailable { model: String },
    /// This build cannot assemble the model's input tensors: the feature assembly lived in the
    /// training wrapper and is not ported. Reported ahead of a missing sensor because it does
    /// not change when the wearer changes strap — telling someone to buy an SpO2 strap for a
    /// model that could not run either way would be a lie of omission.
    PreprocessingNotPorted { detail: String },
}

/// Where one model stands in this pass.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum StageState {
    /// Inputs exist and nothing upstream is outstanding. Eligible to start now.
    Ready,
    /// Runnable, but an upstream stage has to complete first.
    Blocked { upstream: Vec<String> },
    /// Already computed for these exact inputs by this exact artefact.
    Cached,
    /// Cannot run on this device as it stands.
    Unavailable { reason: Unmet },
}

/// One model's place in a plan.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct PlannedStage {
    pub model: String,
    /// Topological rank: every stage depends only on strictly lower ranks. Stages sharing a
    /// rank are independent and may run concurrently, which is what makes parallelism safe to
    /// read off the plan rather than guess at.
    pub rank: u32,
    pub signal: String,
    pub state: StageState,
    /// True when this stage's output may be rendered as a value rather than only as a state.
    pub displayable: bool,
}

/// One pass's worth of decisions.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Plan {
    pub mode: RunMode,
    pub stages: Vec<PlannedStage>,
}

impl Plan {
    /// The stages a pass should start now, in rank order, bounded by the mode's burst.
    ///
    /// Blocked stages are deliberately excluded rather than queued behind their upstream: the
    /// platform re-plans after each pass, and by then the upstream has either completed (so the
    /// stage is ready) or failed (so it is not worth starting).
    pub fn startable(&self) -> Vec<&PlannedStage> {
        self.stages
            .iter()
            .filter(|stage| stage.state == StageState::Ready)
            .take(self.mode.burst())
            .collect()
    }

    pub fn stage(&self, slug: &str) -> Option<&PlannedStage> {
        self.stages.iter().find(|stage| stage.model == slug)
    }

    /// How many stages are unavailable, which is what a surface counts to decide between "still
    /// working" and "this device cannot do this".
    pub fn unavailable(&self) -> usize {
        self.stages
            .iter()
            .filter(|stage| matches!(stage.state, StageState::Unavailable { .. }))
            .count()
    }
}

/// A remembered result: which inputs produced it, and which artefact.
///
/// The fingerprint is over the input values, so a re-request with the same window is answered
/// without touching the accelerator. The hash is the artefact that produced it, so a
/// re-conversion invalidates every entry it touched without anyone having to remember to clear
/// a cache — which is the failure mode that makes a stale embedding outlive the weights that
/// justified it.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct CacheEntry {
    pub model: String,
    pub fingerprint: u64,
    pub model_sha256: String,
    /// When the platform last completed this, in milliseconds since the epoch. Carried so a
    /// surface can say how old a reading is, and so a stale-but-present result is
    /// distinguishable from a missing one.
    pub completed_at_ms: i64,
}

/// A fingerprint over one model's inputs.
///
/// FNV-1a over the tensor names and the raw bit patterns of their values. Bit patterns rather
/// than the floats themselves so that two inputs differing only in the sign of a zero, or in
/// which NaN they carry, are treated as different — a cache that collapsed them would answer a
/// question it was not asked.
pub fn fingerprint(inputs: &[NamedTensor]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    let mut eat = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(PRIME);
        }
    };
    // Sorted so that a platform reordering the tensor list cannot change the fingerprint;
    // the contract binds by name, so order carries no meaning to preserve.
    let mut ordered: Vec<&NamedTensor> = inputs.iter().collect();
    ordered.sort_by(|left, right| left.name.cmp(&right.name));
    for tensor in ordered {
        eat(tensor.name.as_bytes());
        eat(&[0xff]);
        for value in &tensor.values {
            eat(&value.to_bits().to_le_bytes());
        }
    }
    hash
}

/// The planner and its memory of what has already been computed.
#[derive(Debug, Default)]
pub struct AnalyticsScheduler {
    completed: HashMap<String, CacheEntry>,
    /// Inferences handed to the platform, against the fingerprint of the inputs they were
    /// issued for. Kept so the completion can be filed against the right inputs without the
    /// platform carrying a fingerprint it has no use for and could get wrong.
    issued: HashMap<u64, (ModelId, u64)>,
}

impl AnalyticsScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Restore remembered results, as the platform persisted them.
    ///
    /// Entries naming a model this build no longer ships are dropped rather than kept: the
    /// alternative is a cache that grows across a conversion and answers for a contract that
    /// no longer exists.
    pub fn restore(&mut self, entries: Vec<CacheEntry>) {
        self.completed = entries
            .into_iter()
            .filter(|entry| ModelId::from_slug(&entry.model).is_some())
            .map(|entry| (entry.model.clone(), entry))
            .collect();
    }

    /// Everything worth persisting, so a relaunch does not recompute a night's work.
    pub fn snapshot(&self) -> Vec<CacheEntry> {
        let mut entries: Vec<CacheEntry> = self.completed.values().cloned().collect();
        entries.sort_by(|left, right| left.model.cmp(&right.model));
        entries
    }

    /// Remember one completed inference.
    pub fn remember(
        &mut self,
        model: ModelId,
        fingerprint: u64,
        model_sha256: String,
        completed_at_ms: i64,
    ) {
        let slug = model.contract().slug.to_owned();
        self.completed.insert(
            slug.clone(),
            CacheEntry {
                model: slug,
                fingerprint,
                model_sha256,
                completed_at_ms,
            },
        );
    }

    /// True when this model has already answered for exactly these inputs, from an artefact
    /// this build still admits.
    pub fn is_fresh(&self, model: ModelId, fingerprint: u64) -> bool {
        let contract = model.contract();
        self.completed.get(contract.slug).is_some_and(|entry| {
            entry.fingerprint == fingerprint
                && (entry.model_sha256 == contract.coreml_sha256
                    || entry.model_sha256 == contract.tflite_sha256)
        })
    }

    /// Forget one model's result. Used when a configuration change invalidates it without any
    /// input having moved — a timezone edit moves day boundaries, so yesterday is a different
    /// day and every daily model has to answer again.
    pub fn forget(&mut self, model: ModelId) -> bool {
        self.completed.remove(model.contract().slug).is_some()
    }

    /// Forget everything.
    pub fn forget_all(&mut self) {
        self.completed.clear();
    }

    /// The remembered entry for one model, if any.
    pub fn entry(&self, model: ModelId) -> Option<&CacheEntry> {
        self.completed.get(model.contract().slug)
    }

    /// Every model with a usable remembered result, which is what [`Self::plan`] treats as
    /// fresh when the caller has not named a specific fingerprint.
    pub fn fresh_models(&self) -> HashSet<ModelId> {
        self.completed
            .values()
            .filter_map(|entry| {
                let model = ModelId::from_slug(&entry.model)?;
                self.is_fresh(model, entry.fingerprint).then_some(model)
            })
            .collect()
    }

    /// Record that an inference was handed out for these inputs.
    pub fn note_issued(&mut self, request_id: u64, model: ModelId, fingerprint: u64) {
        self.issued.insert(request_id, (model, fingerprint));
    }

    /// File a completed inference against the inputs it was issued for.
    ///
    /// Returns false for a request this scheduler never issued — a replay or a test driving the
    /// raw queue — which is not an error, only a result there is nothing to remember about.
    pub fn note_completed(
        &mut self,
        request_id: u64,
        model_sha256: String,
        completed_at_ms: i64,
    ) -> bool {
        let Some((model, fingerprint)) = self.issued.remove(&request_id) else {
            return false;
        };
        self.remember(model, fingerprint, model_sha256, completed_at_ms);
        true
    }

    /// Drop an issued request without filing a result. A cancelled inference must not leave its
    /// fingerprint behind, or a later completion with a recycled id files against the wrong
    /// inputs.
    pub fn note_abandoned(&mut self, request_id: u64) -> bool {
        self.issued.remove(&request_id).is_some()
    }

    /// True when this model already has an inference in flight for exactly these inputs.
    ///
    /// [`Self::is_fresh`] answers about *completed* work, which is not enough on its own: a
    /// second planning pass that runs before the first has been answered would queue the same
    /// tensors again, and the platform would run them twice. That is the duplicate work this
    /// scheduler exists to prevent, and nothing else notices it — both passes look correct in
    /// isolation.
    pub fn is_issued(&self, model: ModelId, fingerprint: u64) -> bool {
        self.issued
            .values()
            .any(|(issued, mark)| *issued == model && *mark == fingerprint)
    }

    /// Decide what this pass should do.
    ///
    /// `fresh` names the models already answered for the inputs this pass would use. It is
    /// passed in rather than computed here because only the caller can build the input tensors,
    /// and building all forty-one to discover that forty are cached would cost more than the
    /// inferences saved.
    pub fn plan(&self, evidence: &Evidence, mode: RunMode, fresh: &HashSet<ModelId>) -> Plan {
        let ranks = topological_ranks();
        let mut states: HashMap<ModelId, StageState> = HashMap::new();

        // Rank order guarantees every upstream is decided before anything that depends on it,
        // so the upstream lookup below never sees an undecided stage.
        let mut ordered: Vec<&'static ModelPipeline> = PIPELINE.iter().collect();
        ordered.sort_by_key(|entry| ranks.get(&entry.model).copied().unwrap_or(0));

        for entry in &ordered {
            let model = &entry.model;
            let state = if let Some(unmet) = unmet_for(entry, evidence) {
                StageState::Unavailable { reason: unmet }
            } else if let Some(blocking) = entry.depends_on.iter().find_map(|upstream| match states
                .get(upstream)
            {
                Some(StageState::Unavailable { .. }) => Some(*upstream),
                _ => None,
            }) {
                StageState::Unavailable {
                    reason: Unmet::UpstreamUnavailable {
                        model: blocking.contract().slug.to_owned(),
                    },
                }
            } else {
                let waiting: Vec<String> = entry
                    .depends_on
                    .iter()
                    .filter(|upstream| {
                        !matches!(states.get(upstream), Some(StageState::Cached))
                            && !fresh.contains(upstream)
                    })
                    .map(|upstream| upstream.contract().slug.to_owned())
                    .collect();
                if fresh.contains(model) {
                    StageState::Cached
                } else if waiting.is_empty() {
                    StageState::Ready
                } else {
                    StageState::Blocked { upstream: waiting }
                }
            };
            states.insert(*model, state);
        }

        let mut stages: Vec<PlannedStage> = ordered
            .iter()
            .filter_map(|entry| {
                Some(PlannedStage {
                    model: entry.model.contract().slug.to_owned(),
                    rank: ranks.get(&entry.model).copied().unwrap_or(0),
                    signal: entry.signal.name().to_owned(),
                    state: states.remove(&entry.model)?,
                    displayable: entry.interpretation.is_displayable(),
                })
            })
            .collect();
        stages.sort_by(|left, right| {
            left.rank
                .cmp(&right.rank)
                .then_with(|| left.model.cmp(&right.model))
        });
        Plan { mode, stages }
    }
}

/// Which sensors and profile fields one model is missing, if any.
fn unmet_for(entry: &ModelPipeline, evidence: &Evidence) -> Option<Unmet> {
    if let FrontEnd::NotPorted(_, detail) = entry.front_end {
        return Some(Unmet::PreprocessingNotPorted {
            detail: detail.to_owned(),
        });
    }
    let mut missing: Vec<StreamKind> = entry
        .requires_all
        .iter()
        .copied()
        .filter(|stream| !evidence.streams.contains(stream))
        .collect();
    missing.extend(
        entry
            .requires_any
            .iter()
            .filter(|group| !group.iter().any(|stream| evidence.streams.contains(stream)))
            // The group's first member is the one the preprocessing would rather have, and is
            // therefore the one worth naming to the wearer.
            .filter_map(|group| group.first().copied()),
    );
    if !missing.is_empty() {
        return Some(Unmet::MissingStreams { streams: missing });
    }
    let fields: Vec<ProfileField> = entry
        .requires_profile
        .iter()
        .copied()
        .filter(|field| !evidence.profile.contains(field))
        .collect();
    if !fields.is_empty() {
        return Some(Unmet::MissingProfile { fields });
    }
    None
}

/// Longest-path rank of every model over the dependency edges.
///
/// Longest rather than shortest so that a stage never shares a rank with something it
/// transitively depends on: `halite_risk_tree` must outrank `pulsenet_foundation` even though
/// it is only one edge from `halite_ppg_score`. Stages that share a rank are genuinely
/// independent, which is what makes "run one rank concurrently" a safe reading.
fn topological_ranks() -> HashMap<ModelId, u32> {
    fn rank_of(model: ModelId, ranks: &mut HashMap<ModelId, u32>) -> u32 {
        if let Some(known) = ranks.get(&model) {
            return *known;
        }
        let depth = pipeline_of(model)
            .map(|entry| entry.depends_on)
            .unwrap_or(&[])
            .iter()
            .map(|upstream| rank_of(*upstream, ranks) + 1)
            .max()
            .unwrap_or(0);
        ranks.insert(model, depth);
        depth
    }
    let mut ranks = HashMap::new();
    for entry in PIPELINE {
        rank_of(entry.model, &mut ranks);
    }
    ranks
}

/// The wearer's own figures, as the profile heads take them.
///
/// Not sensor readings and not derived: the wearer typed them in. Held by the core rather than
/// passed per call so that the chaining below can complete `cva_probes` without the platform
/// having to know that BMI is substituted for weight, or which branch a sex selects.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct WearerProfile {
    /// True selects the male probe branch, false the female one. The head takes its branch by a
    /// string argument between two tensor arguments, which is why it converted as two artefacts
    /// and why the choice is made here.
    pub male: bool,
    pub age_years: f32,
    pub height_m: f32,
    pub weight_kg: f32,
}

impl WearerProfile {
    /// Body-mass index, which is what the probe heads take in the weight slot.
    pub fn bmi(&self) -> f32 {
        if self.height_m <= 0.0 {
            return f32::NAN;
        }
        self.weight_kg / (self.height_m * self.height_m)
    }

    /// Which fields are filled in, for [`Evidence`]. A non-finite or non-positive figure counts
    /// as absent: a zero height would otherwise reach a model as an infinite BMI.
    pub fn filled(&self) -> Vec<ProfileField> {
        let mut fields = vec![ProfileField::Sex];
        if self.age_years.is_finite() && self.age_years > 0.0 {
            fields.push(ProfileField::Age);
        }
        if self.height_m.is_finite() && self.height_m > 0.0 {
            fields.push(ProfileField::Height);
        }
        if self.weight_kg.is_finite() && self.weight_kg > 0.0 {
            fields.push(ProfileField::Weight);
        }
        fields
    }
}

/// What to run next, now that one model has answered.
///
/// The heads whose whole input is an encoder's output are the reason this exists. Without it the
/// platform would have to catch `pulsenet_foundation`'s embedding, remember that
/// `halite_ppg_score` wants it, and hand the same numbers back across the FFI — twice, in two
/// languages, from a dependency table that lives in Rust. So the core does it: an encoder
/// completing is enough for its heads to be queued, and the platform only ever runs tensors.
///
/// `cva_probes` additionally needs the wearer's figures, and picks a branch by sex. Both are
/// applied here, which is where the manifest note says the branch selection belongs.
pub fn chain_after(
    model: ModelId,
    outputs: &[NamedTensor],
    profile: Option<&WearerProfile>,
) -> Vec<(ModelId, Vec<NamedTensor>)> {
    let mut queued = Vec::new();
    for entry in PIPELINE {
        if entry.front_end != FrontEnd::Upstream || entry.depends_on != [model] {
            continue;
        }
        // A branch head runs only for the branch the wearer's sex selects; queueing both would
        // compute a number from weights fitted on the other population.
        if let Some(profile) = profile {
            let male_only = entry.model == ModelId::CvaProbesMale;
            let female_only = entry.model == ModelId::CvaProbesFemale;
            if (male_only && !profile.male) || (female_only && profile.male) {
                continue;
            }
        }
        let mut inputs = Vec::new();
        let mut complete = true;
        for spec in entry.model.contract().inputs {
            if let Some(tensor) = outputs.iter().find(|tensor| {
                tensor.name == spec.name && tensor.values.len() == spec.element_count()
            }) {
                inputs.push(tensor.clone());
                continue;
            }
            match (spec.name, profile) {
                ("age", Some(profile)) => {
                    inputs.push(NamedTensor::new("age", vec![profile.age_years]))
                }
                // `cva_probes` takes weight and BMI as two separate tensors, so weight is
                // weight here. The BMI-for-weight substitution `docs/ml.md` records belongs to
                // `halite_risk_tree`, which packs four profile fields into one vector — a
                // different model with a different contract, and not this branch's business.
                ("weight", Some(profile)) => {
                    inputs.push(NamedTensor::new("weight", vec![profile.weight_kg]))
                }
                ("bmi", Some(profile)) => inputs.push(NamedTensor::new("bmi", vec![profile.bmi()])),
                _ => {
                    complete = false;
                    break;
                }
            }
        }
        if complete {
            queued.push((entry.model, inputs));
        }
    }
    queued
}

/// Every signal, with how many of its models this device can run.
///
/// A surface needs this to choose between "computing", "partly unavailable" and "this strap
/// cannot do this at all", and counting stages itself would put the same loop on two platforms.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct SignalCoverage {
    pub signal: String,
    pub total: u32,
    pub runnable: u32,
    /// True when at least one runnable model in this signal may be rendered as a value.
    pub any_displayable: bool,
}

pub fn coverage(plan: &Plan) -> Vec<SignalCoverage> {
    let mut ordered: Vec<Signal> = Vec::new();
    for entry in PIPELINE {
        if !ordered.contains(&entry.signal) {
            ordered.push(entry.signal);
        }
    }
    ordered
        .into_iter()
        .map(|signal| {
            let name = signal.name();
            let stages: Vec<&PlannedStage> = plan
                .stages
                .iter()
                .filter(|stage| stage.signal == name)
                .collect();
            let runnable = stages
                .iter()
                .filter(|stage| !matches!(stage.state, StageState::Unavailable { .. }))
                .count();
            SignalCoverage {
                signal: name.to_owned(),
                total: stages.len() as u32,
                runnable: runnable as u32,
                any_displayable: stages.iter().any(|stage| {
                    stage.displayable && !matches!(stage.state, StageState::Unavailable { .. })
                }),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mav_analytic::model_zoo::ALL_MODELS;

    /// A strap that produces everything the zoo asks for.
    fn everything() -> Evidence {
        Evidence::new(
            &[
                StreamKind::HeartRate,
                StreamKind::RrInterval,
                StreamKind::PulseInterval,
                StreamKind::Ppg,
                StreamKind::Imu,
                StreamKind::SkinTemp,
                StreamKind::Spo2Percent,
                StreamKind::StepCount,
            ],
            &[
                ProfileField::Sex,
                ProfileField::Age,
                ProfileField::Height,
                ProfileField::Weight,
            ],
        )
    }

    fn model(slug: &str) -> ModelId {
        ModelId::from_slug(slug).expect("a model this build ships")
    }

    /// The honest ceiling of this build, pinned so it cannot drift quietly in either direction.
    ///
    /// Every one of the forty-one models converts, loads and matches its reference on stored
    /// vectors. Twelve can be fed from a wearer's own samples: three ported PPG front-ends,
    /// three heads reading those encoders' outputs, and the six cycle heads that share
    /// `cycle::cycle_input`. The rest need feature assembly that stayed in the training wrapper,
    /// and eleven of those read Oura ring firmware features no supported strap emits at all.
    ///
    /// Porting a front-end should make this list longer. Nothing else should.
    #[test]
    fn a_complete_strap_runs_the_models_this_build_can_feed() {
        let plan =
            AnalyticsScheduler::new().plan(&everything(), RunMode::Interactive, &HashSet::new());
        assert_eq!(plan.stages.len(), ALL_MODELS.len());
        let runnable: Vec<&str> = plan
            .stages
            .iter()
            .filter(|stage| !matches!(stage.state, StageState::Unavailable { .. }))
            .map(|stage| stage.model.as_str())
            .collect();
        assert_eq!(
            runnable,
            vec![
                "cva_encoder",
                "popsicle_ovulation_detection",
                "popsicle_ovulation_detection_v16",
                "popsicle_ovulation_prediction",
                "popsicle_ovulation_prediction_v16",
                "popsicle_period_prediction",
                "popsicle_period_prediction_v16",
                "pulse_ppg",
                "pulsenet_foundation",
                "cva_probes_female",
                "cva_probes_male",
                "halite_ppg_score",
            ],
        );
        assert_eq!(plan.unavailable(), ALL_MODELS.len() - runnable.len());
    }

    /// A model this build cannot feed says so in those words, rather than blaming a sensor the
    /// wearer could go and buy.
    #[test]
    fn an_unported_front_end_is_named_rather_than_blamed_on_a_sensor() {
        let plan =
            AnalyticsScheduler::new().plan(&everything(), RunMode::Interactive, &HashSet::new());
        let sleep = plan.stage("sleepnet_bdi").expect("planned");
        match &sleep.state {
            StageState::Unavailable {
                reason: Unmet::PreprocessingNotPorted { detail },
            } => assert!(
                detail.contains("ibi"),
                "the reason should name the channels that are missing, not shrug: {detail}"
            ),
            other => panic!("sleepnet_bdi reported {other:?}"),
        }
    }

    #[test]
    fn a_stage_never_outranks_something_it_depends_on() {
        let plan =
            AnalyticsScheduler::new().plan(&everything(), RunMode::Interactive, &HashSet::new());
        for entry in PIPELINE {
            let stage = plan
                .stage(entry.model.contract().slug)
                .expect("every model is planned");
            for upstream in entry.depends_on {
                let above = plan
                    .stage(upstream.contract().slug)
                    .expect("every model is planned");
                assert!(
                    above.rank < stage.rank,
                    "{} (rank {}) must come after {} (rank {})",
                    stage.model,
                    stage.rank,
                    above.model,
                    above.rank,
                );
            }
        }
    }

    #[test]
    fn only_the_roots_are_startable_before_anything_has_run() {
        let plan =
            AnalyticsScheduler::new().plan(&everything(), RunMode::Interactive, &HashSet::new());
        for stage in plan.startable() {
            assert!(
                pipeline_of(model(&stage.model)).is_some_and(|e| e.depends_on.is_empty()),
                "{} was startable with upstream work outstanding",
                stage.model
            );
        }
    }

    #[test]
    fn a_head_unblocks_once_its_encoder_is_fresh() {
        let scheduler = AnalyticsScheduler::new();
        let plan = scheduler.plan(&everything(), RunMode::Interactive, &HashSet::new());
        assert!(matches!(
            plan.stage("halite_ppg_score").expect("planned").state,
            StageState::Blocked { .. }
        ));

        let fresh = HashSet::from([model("pulsenet_foundation")]);
        let plan = scheduler.plan(&everything(), RunMode::Interactive, &fresh);
        assert_eq!(
            plan.stage("halite_ppg_score").expect("planned").state,
            StageState::Ready
        );
        assert_eq!(
            plan.stage("pulsenet_foundation").expect("planned").state,
            StageState::Cached
        );
    }

    #[test]
    fn a_missing_sensor_names_itself_rather_than_going_blank() {
        // No optical stream at all: every PPG-fed model must say so.
        let evidence = Evidence::new(&[StreamKind::RrInterval], &[]);
        let plan = AnalyticsScheduler::new().plan(&evidence, RunMode::Deferred, &HashSet::new());
        assert_eq!(
            plan.stage("pulse_ppg").expect("planned").state,
            StageState::Unavailable {
                reason: Unmet::MissingStreams {
                    streams: vec![StreamKind::Ppg]
                }
            }
        );
        // Intervals alone are enough for the interval-only sleep models.
        assert_ne!(
            plan.stage("sleepnet_bdi").expect("planned").state,
            StageState::Unavailable {
                reason: Unmet::MissingStreams {
                    streams: vec![StreamKind::RrInterval]
                }
            }
        );
    }

    #[test]
    fn an_unavailable_encoder_makes_its_head_unavailable_not_blocked() {
        let evidence = Evidence::new(&[StreamKind::RrInterval], &[]);
        let plan = AnalyticsScheduler::new().plan(&evidence, RunMode::Deferred, &HashSet::new());
        // The head asks for no sensor of its own, so without this rule it would sit Ready
        // forever waiting for an encoder that can never run.
        assert_eq!(
            plan.stage("step_head").expect("planned").state,
            StageState::Unavailable {
                reason: Unmet::UpstreamUnavailable {
                    model: "step_eligibility".to_owned()
                }
            }
        );
    }

    #[test]
    fn a_missing_profile_field_is_reported_apart_from_a_missing_sensor() {
        let evidence = Evidence::new(&[StreamKind::Ppg], &[]);
        let plan = AnalyticsScheduler::new().plan(&evidence, RunMode::Interactive, &HashSet::new());
        assert_eq!(
            plan.stage("cva_probes_male").expect("planned").state,
            StageState::Unavailable {
                reason: Unmet::MissingProfile {
                    fields: vec![
                        ProfileField::Sex,
                        ProfileField::Age,
                        ProfileField::Height,
                        ProfileField::Weight
                    ]
                }
            }
        );
        // ... while the encoder it depends on runs perfectly well without a profile.
        assert_eq!(
            plan.stage("cva_encoder").expect("planned").state,
            StageState::Ready
        );
    }

    #[test]
    fn deferred_mode_starts_fewer_stages_than_interactive() {
        let scheduler = AnalyticsScheduler::new();
        let evidence = everything();
        let deferred = scheduler.plan(&evidence, RunMode::Deferred, &HashSet::new());
        let interactive = scheduler.plan(&evidence, RunMode::Interactive, &HashSet::new());
        // Eleven roots are startable before anything has run, which is over the deferred burst
        // of four and under the interactive bound. That is exactly the case the two modes exist
        // to distinguish.
        assert!(RunMode::Interactive.burst() > RunMode::Deferred.burst());
        assert_eq!(deferred.startable().len(), RunMode::Deferred.burst());
        assert!(interactive.startable().len() > deferred.startable().len());
    }

    #[test]
    fn the_same_inputs_fingerprint_the_same_and_different_ones_do_not() {
        let one = vec![NamedTensor::new("ppg", vec![0.25, 0.5])];
        let same = vec![NamedTensor::new("ppg", vec![0.25, 0.5])];
        let other = vec![NamedTensor::new("ppg", vec![0.25, 0.5000001])];
        assert_eq!(fingerprint(&one), fingerprint(&same));
        assert_ne!(fingerprint(&one), fingerprint(&other));
    }

    #[test]
    fn tensor_order_does_not_change_the_fingerprint() {
        let one = vec![
            NamedTensor::new("age", vec![40.0]),
            NamedTensor::new("embeddings", vec![1.0, 2.0]),
        ];
        let reversed = vec![
            NamedTensor::new("embeddings", vec![1.0, 2.0]),
            NamedTensor::new("age", vec![40.0]),
        ];
        assert_eq!(fingerprint(&one), fingerprint(&reversed));
    }

    /// Two tensors whose values are the same numbers under different names must not collide;
    /// otherwise a cache answers `age` with `weight`.
    #[test]
    fn the_name_is_part_of_the_fingerprint() {
        let age = vec![NamedTensor::new("age", vec![40.0])];
        let weight = vec![NamedTensor::new("weight", vec![40.0])];
        assert_ne!(fingerprint(&age), fingerprint(&weight));
    }

    /// In-flight work is not queued a second time.
    #[test]
    fn work_already_in_flight_is_not_issued_again() {
        let mut scheduler = AnalyticsScheduler::new();
        let id = model("pulse_ppg");
        assert!(!scheduler.is_issued(id, 7));
        scheduler.note_issued(1, id, 7);
        assert!(scheduler.is_issued(id, 7), "the same inputs are in flight");
        assert!(!scheduler.is_issued(id, 8), "different inputs are not");
        assert!(
            !scheduler.is_issued(model("cva_encoder"), 7),
            "a different model with the same fingerprint is not in flight"
        );
        // Once abandoned it may be issued again.
        assert!(scheduler.note_abandoned(1));
        assert!(!scheduler.is_issued(id, 7));
    }

    #[test]
    fn a_remembered_result_is_fresh_only_for_the_inputs_that_produced_it() {
        let mut scheduler = AnalyticsScheduler::new();
        let id = model("pulse_ppg");
        scheduler.remember(id, 42, id.contract().tflite_sha256.to_owned(), 1_000);
        assert!(scheduler.is_fresh(id, 42));
        assert!(!scheduler.is_fresh(id, 43));
    }

    #[test]
    fn a_result_from_an_artefact_this_build_does_not_admit_is_never_fresh() {
        let mut scheduler = AnalyticsScheduler::new();
        let id = model("pulse_ppg");
        scheduler.remember(id, 42, "f".repeat(64), 1_000);
        assert!(
            !scheduler.is_fresh(id, 42),
            "a cached result outlived the weights that produced it"
        );
    }

    #[test]
    fn forgetting_one_model_leaves_the_others_alone() {
        let mut scheduler = AnalyticsScheduler::new();
        let kept = model("pulse_ppg");
        let dropped = model("cva_encoder");
        scheduler.remember(kept, 1, kept.contract().tflite_sha256.to_owned(), 0);
        scheduler.remember(dropped, 1, dropped.contract().tflite_sha256.to_owned(), 0);
        assert!(scheduler.forget(dropped));
        assert!(!scheduler.forget(dropped));
        assert!(scheduler.is_fresh(kept, 1));
    }

    #[test]
    fn a_restored_cache_survives_a_relaunch_and_drops_models_this_build_lost() {
        let mut scheduler = AnalyticsScheduler::new();
        let id = model("pulse_ppg");
        scheduler.remember(id, 7, id.contract().coreml_sha256.to_owned(), 99);
        let persisted = scheduler.snapshot();
        assert_eq!(persisted.len(), 1);

        let mut restored = AnalyticsScheduler::new();
        let mut with_ghost = persisted.clone();
        with_ghost.push(CacheEntry {
            model: "a_model_from_a_previous_build".to_owned(),
            fingerprint: 7,
            model_sha256: "0".repeat(64),
            completed_at_ms: 1,
        });
        restored.restore(with_ghost);
        assert!(restored.is_fresh(id, 7));
        assert_eq!(restored.snapshot().len(), 1, "the ghost was kept");
        assert_eq!(restored.entry(id).expect("remembered").completed_at_ms, 99);
    }

    fn profile() -> WearerProfile {
        WearerProfile {
            male: true,
            age_years: 41.0,
            height_m: 1.80,
            weight_kg: 78.0,
        }
    }

    fn embedding(name: &str, width: usize) -> Vec<NamedTensor> {
        vec![NamedTensor::new(name, vec![0.25; width])]
    }

    #[test]
    fn an_encoder_completing_queues_the_head_that_reads_it() {
        let queued = chain_after(
            model("pulsenet_foundation"),
            &embedding("embeddings", 256),
            None,
        );
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].0, model("halite_ppg_score"));
        assert_eq!(queued[0].1[0].values.len(), 256);
    }

    #[test]
    fn the_probe_branch_follows_the_wearers_sex_and_never_runs_both() {
        let male = chain_after(
            model("cva_encoder"),
            &embedding("embeddings", 128),
            Some(&profile()),
        );
        assert_eq!(
            male.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![model("cva_probes_male")]
        );

        let female = chain_after(
            model("cva_encoder"),
            &embedding("embeddings", 128),
            Some(&WearerProfile {
                male: false,
                ..profile()
            }),
        );
        assert_eq!(
            female.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![model("cva_probes_female")]
        );
    }

    #[test]
    fn a_chained_head_gets_every_tensor_its_contract_declares() {
        let queued = chain_after(
            model("cva_encoder"),
            &embedding("embeddings", 128),
            Some(&profile()),
        );
        let (id, inputs) = &queued[0];
        for spec in id.contract().inputs {
            let tensor = inputs
                .iter()
                .find(|tensor| tensor.name == spec.name)
                .unwrap_or_else(|| panic!("chained head is missing {}", spec.name));
            assert_eq!(tensor.values.len(), spec.element_count(), "{}", spec.name);
        }
        // 78 kg at 1.80 m.
        let bmi = inputs
            .iter()
            .find(|tensor| tensor.name == "bmi")
            .expect("bmi");
        assert!(
            (bmi.values[0] - 24.074_074).abs() < 1e-4,
            "{:?}",
            bmi.values
        );
    }

    #[test]
    fn without_a_profile_the_probe_heads_are_not_queued_at_all() {
        // Better nothing than a head fed a guessed age: the numbers would look like readings.
        let queued = chain_after(model("cva_encoder"), &embedding("embeddings", 128), None);
        assert!(queued.is_empty(), "{queued:?}");
    }

    #[test]
    fn an_encoder_with_the_wrong_output_width_chains_nothing() {
        let queued = chain_after(
            model("pulsenet_foundation"),
            &embedding("embeddings", 255),
            None,
        );
        assert!(queued.is_empty(), "a short embedding was accepted");
    }

    #[test]
    fn a_zero_height_is_an_unfilled_profile_rather_than_an_infinite_bmi() {
        let broken = WearerProfile {
            height_m: 0.0,
            ..profile()
        };
        assert!(!broken.filled().contains(&ProfileField::Height));
        assert!(!broken.bmi().is_finite());
    }

    #[test]
    fn coverage_counts_every_signal_and_says_when_nothing_is_displayable() {
        let plan =
            AnalyticsScheduler::new().plan(&everything(), RunMode::Interactive, &HashSet::new());
        let coverage = coverage(&plan);
        let total: u32 = coverage.iter().map(|item| item.total).sum();
        assert_eq!(total as usize, ALL_MODELS.len());

        let sleep = coverage
            .iter()
            .find(|item| item.signal == "sleep")
            .expect("sleep is a signal");
        assert_eq!(sleep.total, 3, "three sleep models ship");
        assert_eq!(
            sleep.runnable, 0,
            "no sleep front-end is ported, so none of the three can be fed"
        );
        assert!(
            !sleep.any_displayable,
            "the staging vocabulary is not admitted either, so nothing in sleep may be rendered"
        );

        // The signal that does work end to end, for contrast.
        let cardio = coverage
            .iter()
            .find(|item| item.signal == "cardiovascular")
            .expect("cardiovascular is a signal");
        assert_eq!(
            cardio.runnable, 3,
            "the encoder and both probe branches run"
        );
        assert!(cardio.any_displayable);
    }

    #[test]
    fn a_strap_with_no_streams_makes_nothing_runnable_and_explains_each_case() {
        let plan = AnalyticsScheduler::new().plan(
            &Evidence::default(),
            RunMode::Deferred,
            &HashSet::new(),
        );
        assert_eq!(plan.startable().len(), 0);
        for stage in &plan.stages {
            assert!(
                matches!(stage.state, StageState::Unavailable { .. }),
                "{} claimed it could run with no evidence at all",
                stage.model
            );
        }
    }
}
