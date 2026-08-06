//! The model-zoo half of the uniffi facade.
//!
//! Two things cross here, and it matters that they are separate.
//!
//! The **catalogue** is description: what models this build carries, what tensors each takes,
//! which artefact hash each platform must have loaded, and how far a reading may be believed.
//! A platform reads it to know what to bundle-check and what to label; it never invents an entry.
//!
//! The **inference bridge** is the pull-based queue from `mav_engine::model_host`, flattened into
//! records uniffi can carry: the platform asks for work, runs one tensor call, and hands the
//! result back with the hash of the artefact it actually loaded. Preprocessing is not on this
//! surface at all — the tensors are already prepared by the time a request appears, which is the
//! whole point of the split in `docs/ml.md`.
//!
//! There are also a small number of **prepare** calls. Those are the only places the platform
//! passes a raw signal instead of a tensor, and each one runs a named, fixture-tested Rust
//! front-end from `mav_analytic::model_zoo::ppg` before queueing. A platform cannot assemble a
//! model input itself; if it could, the preprocessing would have escaped the core.

use crate::{FfiError, MavRuntime};
use mav_analytic::model_zoo::{ppg, ModelId, ModelRequest, NamedTensor};
use mav_engine::model_host::ModelHost;
use mav_model::error::{codes, MavError};

/// One tensor as the platform sees it: a name, a flat row-major buffer, and the shape it
/// unflattens to. Shapes are static, so the platform binding can assert them at load rather
/// than discovering a mismatch mid-inference.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ModelTensor {
    pub name: String,
    pub values: Vec<f32>,
}

/// A tensor's declared shape and element type, straight from the registry.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ModelTensorSpec {
    pub name: String,
    pub shape: Vec<u32>,
    /// `float32` or `int32`. An `int32` tensor still travels as whole-numbered floats; the
    /// platform binding casts it when it binds the runtime input.
    pub dtype: String,
}

/// Everything a platform needs to know about one admitted model.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ModelDescriptor {
    /// Stable slug, and the artefact base name on both platforms.
    pub slug: String,
    pub algorithm_id: String,
    pub algorithm_version: String,
    pub inputs: Vec<ModelTensorSpec>,
    pub outputs: Vec<ModelTensorSpec>,
    /// SHA-256 of the `model.mlmodel` inside the shipped `.mlpackage`.
    pub coreml_sha256: String,
    /// SHA-256 of the shipped `.tflite` flatbuffer.
    pub tflite_sha256: String,
    /// `first_party` or `open_licensed`.
    pub standing: String,
    /// True when an upstream attribution notice must travel with the artefact.
    pub requires_attribution: bool,
    pub licence: String,
    pub role: String,
}

/// One inference the platform should run now.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ModelInferenceRequest {
    pub request_id: u64,
    pub model_slug: String,
    pub inputs: Vec<ModelTensor>,
}

/// A completed inference, after the core has accepted it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ModelInferenceResult {
    pub request_id: u64,
    pub model_slug: String,
    pub outputs: Vec<ModelTensor>,
    pub model_sha256: String,
}

#[uniffi::export]
impl MavRuntime {
    /// Every model this build ships. The platform checks its bundle against this and labels
    /// readings from it; the list is generated from the conversion manifest, not hand-written.
    pub fn model_catalog(&self) -> Vec<ModelDescriptor> {
        mav_analytic::model_zoo::ALL_MODELS
            .iter()
            .map(|model| descriptor(*model))
            .collect()
    }

    /// One model's description, or an error naming the slug that is not in this build.
    pub fn model_descriptor(&self, slug: String) -> Result<ModelDescriptor, FfiError> {
        Ok(descriptor(resolve(&slug)?))
    }

    /// Queue an inference from tensors the caller already holds.
    ///
    /// This exists for replay and for tests that drive a model from stored tensors. Live paths
    /// use the `prepare_*` calls, so that the preprocessing behind a reading is always a named
    /// version in the core rather than whatever the app assembled.
    pub fn enqueue_model_inference(
        &self,
        slug: String,
        inputs: Vec<ModelTensor>,
    ) -> Result<u64, FfiError> {
        let model = resolve(&slug)?;
        let request = ModelRequest {
            model,
            inputs: inputs.into_iter().map(tensor_into_core).collect(),
        };
        let mut host = self.model_host_lock()?;
        host.enqueue(request).map_err(Into::into)
    }

    /// Prepare and queue a Pulse-PPG embedding from a raw PPG window.
    ///
    /// The core resamples to 50 Hz, fits the four-minute pre-training window and z-scores it.
    /// `source_rate_hz` is the connector's declared rate, not a guess.
    pub fn prepare_pulse_ppg_embedding(
        &self,
        samples: Vec<f32>,
        source_rate_hz: u32,
    ) -> Result<u64, FfiError> {
        let prepared = ppg::pulse_ppg_input(&samples, source_rate_hz)?;
        let request = ModelRequest {
            model: ModelId::PulsePpg,
            inputs: vec![NamedTensor::new("ppg", prepared)],
        };
        let mut host = self.model_host_lock()?;
        host.enqueue(request).map_err(Into::into)
    }

    /// Prepare and queue a PulseNet embedding from one thirty-second PPG segment at 50 Hz.
    ///
    /// The core applies PulseNet's own detrend chain. Feeding an unfiltered segment would run,
    /// and would be wrong, so the raw segment is what this takes and the filter is not optional.
    pub fn prepare_pulsenet_embedding(
        &self,
        segment: Vec<f32>,
        source_rate_hz: u32,
    ) -> Result<u64, FfiError> {
        let fitted = ppg::resample_window(
            &segment,
            source_rate_hz,
            ppg::PPG_SAMPLE_RATE_HZ,
            ppg::PPG_SEGMENT_SAMPLES,
        );
        let prepared = ppg::pulsenet_input(&fitted)?;
        let request = ModelRequest {
            model: ModelId::PulsenetFoundation,
            inputs: vec![NamedTensor::new("ppg", prepared)],
        };
        let mut host = self.model_host_lock()?;
        host.enqueue(request).map_err(Into::into)
    }

    /// Take the next inference the platform should run, if any.
    pub fn next_model_inference(&self) -> Result<Option<ModelInferenceRequest>, FfiError> {
        let mut host = self.model_host_lock()?;
        Ok(host.next_request().map(|request| ModelInferenceRequest {
            request_id: request.request_id,
            model_slug: request.model.contract().slug.to_owned(),
            inputs: request.inputs.into_iter().map(tensor_from_core).collect(),
        }))
    }

    /// Hand a platform result back to the core. `model_sha256` is the hash of the artefact the
    /// platform loaded; a hash this build does not admit fails here rather than becoming a
    /// number with no provenance.
    ///
    /// `completed_at_ms` is the platform's clock. The core reads no clock of its own — the same
    /// rule that keeps day boundaries reproducible in `recompute` — so the one timestamp worth
    /// remembering about an inference has to arrive with it.
    pub fn submit_model_inference(
        &self,
        request_id: u64,
        outputs: Vec<ModelTensor>,
        model_sha256: String,
        completed_at_ms: i64,
    ) -> Result<ModelInferenceResult, FfiError> {
        let completed = {
            let mut host = self.model_host_lock()?;
            host.submit(
                request_id,
                outputs.into_iter().map(tensor_into_core).collect(),
                model_sha256,
            )?
        };
        // File the result against the inputs it was issued for, so the next pass knows not to
        // ask again. Only requests that came through `admit_analytics_stage` are known here; a
        // replay driving the raw queue files nothing, which is correct — it never asked to be
        // remembered. The host lock is released first: the facade's order is scheduler before
        // models (see `MavRuntime`), and taking them the other way round here would invert it.
        self.scheduler_lock()?.note_completed(
            completed.request_id,
            completed.model_sha256.clone(),
            completed_at_ms,
        );
        // An encoder answering is enough to queue the heads that read it. Doing this here rather
        // than on the platform is what keeps `pulsenet_foundation -> halite_ppg_score` a fact
        // about the dependency table instead of a convention two apps have to keep in step.
        self.chain_from(&completed)?;
        Ok(ModelInferenceResult {
            request_id: completed.request_id,
            model_slug: completed.model.contract().slug.to_owned(),
            outputs: completed
                .outputs
                .into_iter()
                .map(tensor_from_core)
                .collect(),
            model_sha256: completed.model_sha256,
        })
    }

    /// Collect a completed inference by id.
    pub fn take_model_inference(
        &self,
        request_id: u64,
    ) -> Result<Option<ModelInferenceResult>, FfiError> {
        let mut host = self.model_host_lock()?;
        Ok(host.take(request_id).map(|completed| ModelInferenceResult {
            request_id: completed.request_id,
            model_slug: completed.model.contract().slug.to_owned(),
            outputs: completed
                .outputs
                .into_iter()
                .map(tensor_from_core)
                .collect(),
            model_sha256: completed.model_sha256,
        }))
    }

    /// Abandon one inference. True when there was something to abandon.
    pub fn cancel_model_inference(&self, request_id: u64) -> Result<bool, FfiError> {
        let cancelled = {
            let mut host = self.model_host_lock()?;
            host.cancel(request_id)
        };
        // Drop the issued fingerprint too. Leaving it would let a later request that reuses the
        // id file its result against tensors it never saw.
        self.scheduler_lock()?.note_abandoned(request_id);
        Ok(cancelled)
    }

    /// How many inferences the platform still owes an answer for.
    pub fn outstanding_model_inferences(&self) -> Result<u32, FfiError> {
        let host = self.model_host_lock()?;
        Ok(host.outstanding() as u32)
    }
}

fn descriptor(model: ModelId) -> ModelDescriptor {
    let contract = model.contract();
    ModelDescriptor {
        slug: contract.slug.to_owned(),
        algorithm_id: contract.algorithm_id.to_owned(),
        algorithm_version: contract.algorithm_version.to_string(),
        inputs: contract.inputs.iter().map(spec).collect(),
        outputs: contract.outputs.iter().map(spec).collect(),
        coreml_sha256: contract.coreml_sha256.to_owned(),
        tflite_sha256: contract.tflite_sha256.to_owned(),
        standing: contract.standing.name().to_owned(),
        requires_attribution: contract.standing.requires_attribution(),
        licence: contract.licence.to_owned(),
        role: contract.role.to_owned(),
    }
}

fn spec(spec: &mav_analytic::model_zoo::TensorSpec) -> ModelTensorSpec {
    ModelTensorSpec {
        name: spec.name.to_owned(),
        shape: spec.shape.iter().map(|side| *side as u32).collect(),
        dtype: spec.dtype.name().to_owned(),
    }
}

fn tensor_into_core(tensor: ModelTensor) -> NamedTensor {
    NamedTensor {
        name: tensor.name,
        values: tensor.values,
    }
}

fn tensor_from_core(tensor: NamedTensor) -> ModelTensor {
    ModelTensor {
        name: tensor.name,
        values: tensor.values,
    }
}

fn resolve(slug: &str) -> Result<ModelId, FfiError> {
    ModelId::from_slug(slug).ok_or_else(|| {
        FfiError::from(MavError::new(
            codes::ML_MODEL_NOT_ADMITTED,
            format!("this build ships no model named {slug}"),
        ))
    })
}

impl MavRuntime {
    pub(crate) fn model_host_lock(&self) -> Result<std::sync::MutexGuard<'_, ModelHost>, FfiError> {
        self.models
            .lock()
            .map_err(|_| crate::poisoned("model inference host"))
    }
}
