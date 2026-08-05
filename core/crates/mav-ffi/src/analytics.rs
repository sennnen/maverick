//! The production analytics path across the uniffi boundary.
//!
//! `models.rs` carries the catalogue and the raw queue. Both are deliberately low-level: the
//! queue takes a model slug and tensors and asks no questions, which is right for replay and
//! for a test driving one model from stored vectors, and wrong for the app. An app driving the
//! raw queue would have to decide for itself which of the forty-one models are worth running on
//! this device, in what order, and whether the answer is already known — on two platforms, in
//! two languages, from the same table of tensor shapes.
//!
//! So the app drives this instead. Three calls:
//!
//! 1. [`MavRuntime::analytics_plan`] — what this device can run right now, in dependency order,
//!    with a named reason for everything it cannot.
//! 2. [`MavRuntime::admit_analytics_stage`] — queue one stage's tensors, or learn that the same
//!    tensors have already been answered and no accelerator needs to wake up.
//! 3. The existing drain (`next_model_inference` / `submit_model_inference`), unchanged, because
//!    the platform half of that seam was already right.
//!
//! The cache is filed automatically: [`super::MavRuntime::submit_model_inference`] tells the
//! scheduler which request completed, and the scheduler already knows which inputs that request
//! was issued for. A platform never carries a fingerprint, so a platform can never file one
//! against the wrong tensors.

use crate::{FfiError, MavRuntime};
use mav_analytic::model_zoo::pipeline::{ProfileField, COMPOSITES, PIPELINE};
use mav_analytic::model_zoo::{ppg, ModelId, NamedTensor};
use mav_engine::analytics::{
    coverage, fingerprint, AnalyticsScheduler, CacheEntry, Evidence, RunMode, StageState, Unmet,
};
use mav_model::error::{codes, MavError};
use mav_model::ids::DeviceId;
use mav_model::stream::StreamKind;
use mav_model::time::WallTime;

/// One model's place in a plan, flattened for uniffi.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ModelStageReport {
    pub model_slug: String,
    /// Topological rank. Stages sharing a rank are independent, so a platform may run a whole
    /// rank concurrently without consulting the dependency table itself.
    pub rank: u32,
    pub signal: String,
    /// `ready`, `blocked`, `cached` or `unavailable`.
    pub state: String,
    /// For `blocked`, the upstream models still outstanding.
    pub blocked_on: Vec<String>,
    /// For `unavailable`, which of `missing_streams`, `missing_profile` or
    /// `upstream_unavailable` applies.
    pub unavailable_reason: Option<String>,
    /// The streams this device does not have. Named so a surface can say what is needed.
    pub missing_streams: Vec<String>,
    /// The profile fields the wearer has not filled in. Distinct from a missing stream because
    /// the wearer can fix it from inside the app.
    pub missing_profile: Vec<String>,
    /// For `upstream_unavailable`, the model that could not run.
    pub blocking_model: Option<String>,
    /// For `preprocessing_not_ported`, what this build would have to be able to build. Present
    /// so a surface can say which piece of work is missing rather than "unavailable".
    pub missing_preprocessing: Option<String>,
    /// False when this model's output may be computed and stored but not rendered as a value.
    /// A surface that ignores this would present the sleep staging vocabulary as sleep stages.
    pub displayable: bool,
}

/// Why one model needs what it needs, for diagnostics and the report bundle.
///
/// A call of its own rather than a field on [`ModelStageReport`]. The notes are constants — they
/// come from the pipeline table and cannot change between passes — and carrying them on the plan
/// meant rebuilding three and a half kilobytes of prose, and doing two linear slug lookups per
/// model to find it, on every foreground resume and every background window. A surface that wants
/// them asks once.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ModelNote {
    pub model_slug: String,
    pub note: String,
}

/// How much of one product signal this device can produce.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SignalCoverageReport {
    pub signal: String,
    pub total: u32,
    pub runnable: u32,
    /// True when at least one runnable model in this signal may be rendered as a value. False
    /// means the signal computes and has nothing a surface may show, which is a state to
    /// render honestly rather than an empty card.
    pub any_displayable: bool,
}

/// One pass's decisions.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct AnalyticsPlanReport {
    /// `interactive` or `deferred`, echoed so a log line can tell the two apart.
    pub mode: String,
    pub stages: Vec<ModelStageReport>,
    pub coverage: Vec<SignalCoverageReport>,
    /// The stages this pass should start now, in rank order, already bounded by the mode. A
    /// platform runs exactly this list; it does not re-filter `stages` itself.
    pub startable: Vec<String>,
}

/// What happened when a stage's tensors were offered.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct StageAdmission {
    pub model_slug: String,
    /// The id to answer against, absent when the answer was already known.
    pub request_id: Option<u64>,
    /// True when these exact tensors had already been answered by an artefact this build still
    /// admits, so nothing was queued.
    pub already_known: bool,
}

/// A remembered result, as the platform persists it.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct AnalyticsCacheEntry {
    pub model_slug: String,
    /// The fingerprint travels as a string because uniffi has no unsigned-64 in every binding
    /// target, and a fingerprint that silently wrapped through a signed type would collide.
    pub fingerprint: String,
    pub model_sha256: String,
    pub completed_at_ms: i64,
}

/// A wrapper archive whose neural core ships as several models.
///
/// Exposed because `manifest.json`'s `not_shipped` list reads like six missing capabilities and
/// is not: in every case the wrapper failed to convert and its parameters ship as cores. A
/// diagnostics surface that shows one without the other tells the wrong story.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CompositeModelReport {
    pub archive: String,
    pub cores: Vec<String>,
    pub rust_glue: String,
}

#[uniffi::export]
impl MavRuntime {
    /// What this device can run for the day containing `at_ms`.
    ///
    /// `profile_fields` are the wearer-profile fields that are filled in — `sex`, `age`,
    /// `height`, `weight`. They come from the platform because that is where the profile lives;
    /// an unknown name is ignored rather than refused, so adding a field to the profile does not
    /// require a core release to go with it.
    pub fn analytics_plan(
        &self,
        device_id: u64,
        at_ms: i64,
        mode: String,
        profile_fields: Vec<String>,
    ) -> Result<AnalyticsPlanReport, FfiError> {
        let mode = run_mode(&mode)?;
        let device = DeviceId::new(device_id);
        let at = WallTime::from_unix_millis(at_ms);

        let (from, until) = {
            let spine = self.spine_lock()?;
            let day = spine.day_of(at);
            let zone = spine.timezone();
            (day.start(zone), day.offset(1).start(zone))
        };
        let streams = {
            let reader = self.reader_lock()?;
            reader.streams_between(device, from, until)?
        };
        let profile: Vec<ProfileField> = profile_fields
            .iter()
            .filter_map(|name| profile_field(name))
            .collect();
        let evidence = Evidence::new(&streams, &profile);

        let scheduler = self.scheduler_lock()?;
        let plan = scheduler.plan(&evidence, mode, &scheduler.fresh_models());
        Ok(AnalyticsPlanReport {
            mode: match mode {
                RunMode::Interactive => "interactive".to_owned(),
                RunMode::Deferred => "deferred".to_owned(),
            },
            startable: plan
                .startable()
                .iter()
                .map(|stage| stage.model.clone())
                .collect(),
            coverage: coverage(&plan)
                .into_iter()
                .map(|item| SignalCoverageReport {
                    signal: item.signal,
                    total: item.total,
                    runnable: item.runnable,
                    any_displayable: item.any_displayable,
                })
                .collect(),
            stages: plan.stages.into_iter().map(stage_report).collect(),
        })
    }

    /// Queue one stage's tensors, unless the same tensors have already been answered.
    ///
    /// This is the production enqueue. It differs from `enqueue_model_inference` in exactly one
    /// way — it remembers — and that difference is why the app uses it: a wearer who opens the
    /// app twice in a minute should not pay for the night twice.
    pub fn admit_analytics_stage(
        &self,
        slug: String,
        inputs: Vec<crate::models::ModelTensor>,
    ) -> Result<StageAdmission, FfiError> {
        let model = ModelId::from_slug(&slug).ok_or_else(|| {
            MavError::new(
                codes::ML_MODEL_NOT_ADMITTED,
                format!("this build ships no model named {slug}"),
            )
        })?;
        let tensors: Vec<NamedTensor> = inputs
            .into_iter()
            .map(|tensor| NamedTensor {
                name: tensor.name,
                values: tensor.values,
            })
            .collect();
        let mark = fingerprint(&tensors);

        let mut scheduler = self.scheduler_lock()?;
        if scheduler.is_fresh(model, mark) {
            return Ok(StageAdmission {
                model_slug: slug,
                request_id: None,
                already_known: true,
            });
        }
        // Validate and queue before recording the fingerprint: a request the host refuses must
        // not leave an issued entry behind that a later id could collide with.
        let request_id = {
            let mut host = self.model_host_lock()?;
            host.enqueue(mav_analytic::model_zoo::ModelRequest {
                model,
                inputs: tensors,
            })?
        };
        scheduler.note_issued(request_id, model, mark);
        Ok(StageAdmission {
            model_slug: slug,
            request_id: Some(request_id),
            already_known: false,
        })
    }

    /// Everything worth persisting across a relaunch.
    pub fn analytics_cache(&self) -> Result<Vec<AnalyticsCacheEntry>, FfiError> {
        Ok(self
            .scheduler_lock()?
            .snapshot()
            .into_iter()
            .map(|entry| AnalyticsCacheEntry {
                model_slug: entry.model,
                fingerprint: entry.fingerprint.to_string(),
                model_sha256: entry.model_sha256,
                completed_at_ms: entry.completed_at_ms,
            })
            .collect())
    }

    /// Restore a persisted cache. Entries naming a model this build no longer ships, or whose
    /// fingerprint does not parse, are dropped rather than trusted.
    pub fn restore_analytics_cache(
        &self,
        entries: Vec<AnalyticsCacheEntry>,
    ) -> Result<u32, FfiError> {
        let restored: Vec<CacheEntry> = entries
            .into_iter()
            .filter_map(|entry| {
                Some(CacheEntry {
                    model: entry.model_slug,
                    fingerprint: entry.fingerprint.parse().ok()?,
                    model_sha256: entry.model_sha256,
                    completed_at_ms: entry.completed_at_ms,
                })
            })
            .collect();
        let mut scheduler = self.scheduler_lock()?;
        scheduler.restore(restored);
        Ok(scheduler.snapshot().len() as u32)
    }

    /// Forget every remembered result. The platform calls this when something outside the
    /// tensors changes what they mean — a timezone edit moves day boundaries, so every daily
    /// model has to answer again even though no input value moved.
    pub fn invalidate_analytics_cache(&self) -> Result<(), FfiError> {
        self.scheduler_lock()?.forget_all();
        Ok(())
    }

    /// Read the day's stored optical signal and queue every encoder this build can feed from it.
    ///
    /// This is the only production path that starts from a raw signal, and it deliberately does
    /// not hand that signal to the platform. `docs/ml.md`'s split is that Rust owns everything
    /// up to the tensor; a platform that could assemble a model input would be a second place
    /// for the resample-off-by-one class of bug to live.
    ///
    /// Returns one admission per encoder queued. An encoder whose window this day cannot fill
    /// is simply absent from the result — a short night is not an error.
    pub fn admit_ppg_stages(
        &self,
        device_id: u64,
        at_ms: i64,
    ) -> Result<Vec<StageAdmission>, FfiError> {
        let device = DeviceId::new(device_id);
        let at = WallTime::from_unix_millis(at_ms);
        let (from, until) = {
            let spine = self.spine_lock()?;
            let day = spine.day_of(at);
            let zone = spine.timezone();
            (day.start(zone), day.offset(1).start(zone))
        };

        // Whichever optical channel this strap publishes. The front-ends take one channel of
        // reflectance and do not care which LED lit it; `pipeline::ANY_PPG` is the same list.
        let mut samples = Vec::new();
        for kind in [StreamKind::Ppg, StreamKind::RedPpg, StreamKind::InfraredPpg] {
            let read = {
                let reader = self.reader_lock()?;
                reader.samples_between(device, kind, from, until)?
            };
            if !read.is_empty() {
                samples = read;
                break;
            }
        }
        if samples.is_empty() {
            return Ok(Vec::new());
        }

        let rate = sample_rate_hz(&samples).ok_or_else(|| {
            MavError::new(
                codes::ML_MODEL_TENSOR_INVALID,
                "the stored optical samples are not evenly enough spaced to name a rate",
            )
        })?;
        let signal: Vec<f32> = samples.iter().map(|s| s.value.as_f64() as f32).collect();

        let mut admitted = Vec::new();
        // Pulse-PPG: the four-minute window, resampled and z-scored by its own front-end.
        if let Ok(prepared) = ppg::pulse_ppg_input(&signal, rate) {
            admitted.push(
                self.admit_prepared(ModelId::PulsePpg, vec![NamedTensor::new("ppg", prepared)])?,
            );
        }
        // The two thirty-second front-ends share a resample and a fit, so do it once.
        let at_fifty = if rate == ppg::PPG_SAMPLE_RATE_HZ {
            signal.clone()
        } else {
            ppg::linear_resample(&signal, rate, ppg::PPG_SAMPLE_RATE_HZ)
        };
        let segment = ppg::fit_or_pad(&at_fifty, ppg::PPG_SEGMENT_SAMPLES);
        if let Ok(prepared) = ppg::pulsenet_input(&segment) {
            admitted.push(self.admit_prepared(
                ModelId::PulsenetFoundation,
                vec![NamedTensor::new("ppg", prepared)],
            )?);
        }
        if let Ok(pulse) = ppg::cva_pulse(&segment) {
            // The encoder's own block_size is 1,024, which is why the 1,499-sample train is
            // truncated rather than resampled: the weights were fitted on a block, not a window.
            let block = ModelId::CvaEncoder
                .contract()
                .input("pulses")
                .map(|spec| spec.element_count())
                .unwrap_or(0);
            if pulse.pulses.len() >= block {
                admitted.push(self.admit_prepared(
                    ModelId::CvaEncoder,
                    vec![NamedTensor::new("pulses", pulse.pulses[..block].to_vec())],
                )?);
            }
        }
        Ok(admitted)
    }

    /// Record the wearer's own figures.
    ///
    /// Sex selects which of the two `cva_probes` branches runs; the rest fill the scalar inputs
    /// beside an encoder's embedding. A field left non-finite or non-positive counts as
    /// unfilled, and the models that need it report `missing_profile` rather than receiving a
    /// guess — a probe head fed a made-up age returns a number that looks exactly like a
    /// reading.
    pub fn set_wearer_profile(
        &self,
        male: bool,
        age_years: f32,
        height_m: f32,
        weight_kg: f32,
    ) -> Result<(), FfiError> {
        let profile = mav_engine::WearerProfile {
            male,
            age_years,
            height_m,
            weight_kg,
        };
        *self
            .profile
            .lock()
            .map_err(|_| crate::poisoned("wearer profile"))? = Some(profile);
        Ok(())
    }

    /// The profile fields currently filled in, as `analytics_plan` takes them.
    pub fn wearer_profile_fields(&self) -> Result<Vec<String>, FfiError> {
        let profile = self
            .profile
            .lock()
            .map_err(|_| crate::poisoned("wearer profile"))?;
        Ok(profile
            .map(|profile| {
                profile
                    .filled()
                    .into_iter()
                    .map(|field| field.name().to_owned())
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Why each model needs what it needs. Constant for a build, so a surface reads it once
    /// rather than receiving it again with every plan.
    pub fn model_notes(&self) -> Vec<ModelNote> {
        PIPELINE
            .iter()
            .map(|entry| ModelNote {
                model_slug: entry.model.contract().slug.to_owned(),
                note: entry.note.to_owned(),
            })
            .collect()
    }

    /// The six wrapper archives and the cores that carry their parameters.
    pub fn composite_models(&self) -> Vec<CompositeModelReport> {
        COMPOSITES
            .iter()
            .map(|composite| CompositeModelReport {
                archive: composite.archive.to_owned(),
                cores: composite
                    .cores
                    .iter()
                    .map(|core| core.contract().slug.to_owned())
                    .collect(),
                rust_glue: composite.rust_glue.to_owned(),
            })
            .collect()
    }
}

fn stage_report(stage: mav_engine::analytics::PlannedStage) -> ModelStageReport {
    let mut report = ModelStageReport {
        model_slug: stage.model,
        rank: stage.rank,
        signal: stage.signal,
        state: String::new(),
        blocked_on: Vec::new(),
        unavailable_reason: None,
        missing_streams: Vec::new(),
        missing_profile: Vec::new(),
        blocking_model: None,
        missing_preprocessing: None,
        displayable: stage.displayable,
    };
    match stage.state {
        StageState::Ready => report.state = "ready".to_owned(),
        StageState::Cached => report.state = "cached".to_owned(),
        StageState::Blocked { upstream } => {
            report.state = "blocked".to_owned();
            report.blocked_on = upstream;
        }
        StageState::Unavailable { reason } => {
            report.state = "unavailable".to_owned();
            match reason {
                Unmet::MissingStreams { streams } => {
                    report.unavailable_reason = Some("missing_streams".to_owned());
                    report.missing_streams =
                        streams.iter().map(|kind| kind.name().to_owned()).collect();
                }
                Unmet::MissingProfile { fields } => {
                    report.unavailable_reason = Some("missing_profile".to_owned());
                    report.missing_profile =
                        fields.iter().map(|field| field.name().to_owned()).collect();
                }
                Unmet::UpstreamUnavailable { model } => {
                    report.unavailable_reason = Some("upstream_unavailable".to_owned());
                    report.blocking_model = Some(model);
                }
                Unmet::PreprocessingNotPorted { detail } => {
                    report.unavailable_reason = Some("preprocessing_not_ported".to_owned());
                    report.missing_preprocessing = Some(detail);
                }
            }
        }
    }
    report
}

fn run_mode(name: &str) -> Result<RunMode, FfiError> {
    match name {
        "interactive" => Ok(RunMode::Interactive),
        "deferred" => Ok(RunMode::Deferred),
        other => Err(FfiError::from(MavError::new(
            codes::FFI_RUNTIME_STATE,
            format!("run mode must be interactive or deferred, not {other}"),
        ))),
    }
}

fn profile_field(name: &str) -> Option<ProfileField> {
    match name {
        "sex" => Some(ProfileField::Sex),
        "age" => Some(ProfileField::Age),
        "height" => Some(ProfileField::Height),
        "weight" => Some(ProfileField::Weight),
        _ => None,
    }
}

/// The sampling rate implied by a run of stored samples, from the median gap between them.
///
/// The connector declares a rate on the wire, and the store does not keep it. Deriving it from
/// the timestamps is the honest alternative to assuming one: a window resampled from the wrong
/// rate is exactly the bug the front-ends exist to prevent. A run whose median gap is outside
/// 1 Hz to 1 kHz names no rate, and the caller reports that rather than guessing.
fn sample_rate_hz(samples: &[mav_model::stream::Sample<mav_model::raw::RawValue>]) -> Option<u32> {
    let mut gaps: Vec<i64> = samples
        .windows(2)
        .filter_map(|pair| {
            let first = pair[0].wall_time()?.as_nanos();
            let second = pair[1].wall_time()?.as_nanos();
            (second > first).then_some(second - first)
        })
        .collect();
    if gaps.is_empty() {
        return None;
    }
    gaps.sort_unstable();
    let median = gaps[gaps.len() / 2];
    let rate = (1_000_000_000_f64 / median as f64).round();
    (1.0..=1000.0).contains(&rate).then_some(rate as u32)
}

impl MavRuntime {
    /// Queue one prepared stage, deduplicating against what has already been answered.
    fn admit_prepared(
        &self,
        model: ModelId,
        inputs: Vec<NamedTensor>,
    ) -> Result<StageAdmission, FfiError> {
        let slug = model.contract().slug.to_owned();
        let mark = fingerprint(&inputs);
        let mut scheduler = self.scheduler_lock()?;
        if scheduler.is_fresh(model, mark) {
            return Ok(StageAdmission {
                model_slug: slug,
                request_id: None,
                already_known: true,
            });
        }
        let request_id = {
            let mut host = self.model_host_lock()?;
            host.enqueue(mav_analytic::model_zoo::ModelRequest { model, inputs })?
        };
        scheduler.note_issued(request_id, model, mark);
        Ok(StageAdmission {
            model_slug: slug,
            request_id: Some(request_id),
            already_known: false,
        })
    }

    /// Queue whatever the completion of one inference makes runnable.
    ///
    /// Failures here are deliberately not propagated to the submitting caller: the result that
    /// just arrived is valid and has been accepted, and a full queue or an unfillable head is a
    /// reason to run the chained stage on the next pass rather than to reject work already done.
    pub(crate) fn chain_from(
        &self,
        completed: &mav_engine::CompletedInference,
    ) -> Result<(), FfiError> {
        let profile = *self
            .profile
            .lock()
            .map_err(|_| crate::poisoned("wearer profile"))?;
        let queued = mav_engine::chain_after(completed.model, &completed.outputs, profile.as_ref());
        for (model, inputs) in queued {
            let mark = fingerprint(&inputs);
            {
                let scheduler = self.scheduler_lock()?;
                if scheduler.is_fresh(model, mark) {
                    continue;
                }
            }
            let request = mav_analytic::model_zoo::ModelRequest { model, inputs };
            let issued = {
                let mut host = self.model_host_lock()?;
                host.enqueue(request)
            };
            match issued {
                Ok(request_id) => self.scheduler_lock()?.note_issued(request_id, model, mark),
                // A bounded queue refusing more work is back-pressure doing its job. The next
                // plan will show the head as ready and it will be queued then.
                Err(_) => break,
            }
        }
        Ok(())
    }

    pub(crate) fn scheduler_lock(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, AnalyticsScheduler>, FfiError> {
        self.scheduler
            .lock()
            .map_err(|_| crate::poisoned("analytics scheduler"))
    }
}
