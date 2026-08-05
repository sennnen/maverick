//! The model zoo: every admitted learned model, its tensor contract, and the deterministic
//! Rust that stands on either side of the native inference call.
//!
//! `ecg_model` was Maverick's first model and it is still the shape of the rule: Rust owns
//! everything up to the tensor, the platform runtime owns the tensor call, and the prediction
//! comes back as a feature with provenance. This module generalises that from one model to a
//! registry, because a second model would otherwise have meant a second copy of the same
//! plumbing on both platforms.
//!
//! ## What lives where
//!
//! - [`registry`] is generated from the conversion contracts by `tools/ml/generate_registry.py`.
//!   It is the single source of truth for shapes, dtypes, algorithm ids, artefact names and the
//!   SHA-256 of each shipped artefact. Nothing here is hand-typed, so a re-conversion cannot
//!   leave the Rust admission gate describing a model that is no longer in the bundle.
//! - [`deterministic`] holds the twelve archives that carry no weights at all. They are
//!   arithmetic, so they are ported to Rust rather than converted; the module says why.
//! - [`ppg`] holds the PPG front-end preprocessing: the detrend chains and pulse normalisation
//!   each training wrapper performed in TorchScript before its neural core ran, ported to
//!   deterministic Rust and golden-vector tested.
//! - This file holds the vocabulary: what a tensor is, what a request is, and what makes a
//!   returned prediction admissible.
//!
//! ## Why only the neural core is converted
//!
//! Each model ships from training as a TorchScript wrapper: validation, resampling, filtering,
//! the network, then post-processing, all in one archive. Converting the whole wrapper would
//! bake data-dependent behaviour — peak counts, window counts, median filter lengths — into a
//! fixed graph, and would put the part of the path most likely to hold a subtle bug inside an
//! opaque binary. Maverick converts the tensor-in / tensor-out core only, and ports the rest.
//! That is the same split `docs/ml.md` already required of the ECG classifier.
//!
//! ## Admission
//!
//! A model is admitted when the registry knows its artefact hash and the platform reports that
//! same hash back with its outputs. An unknown hash is an error, not a warning: it means the app
//! is running weights that were never validated against the contract in this build.

pub mod cycle;
pub mod deterministic;
pub mod pipeline;
pub mod ppg;
pub mod registry;

use mav_model::error::{codes, MavError, Result};
use mav_model::version::Version;

pub use registry::{ModelId, ALL_MODELS};

/// The element type a tensor carries across the FFI boundary.
///
/// Both platform runtimes are fed and read as `f32`; `Int32` exists because two cores take an
/// integer sequence-length input, and silently rounding a length through a float is the kind of
/// quiet lie this codebase does not allow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TensorDtype {
    Float32,
    Int32,
}

impl TensorDtype {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Float32 => "float32",
            Self::Int32 => "int32",
        }
    }
}

/// One named tensor in a model's contract: the name the platform runtime binds to, the exact
/// shape, and the element type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TensorSpec {
    pub name: &'static str,
    pub shape: &'static [usize],
    pub dtype: TensorDtype,
}

impl TensorSpec {
    /// Total element count. Shapes are fully static, so this never depends on input data.
    pub fn element_count(&self) -> usize {
        self.shape.iter().product()
    }
}

/// Where a model's weights came from, which decides how they may be shipped.
///
/// This is a provenance flag, not a quality one. How far an output may be *believed* is a
/// separate axis, and today the answer is the same for every model in the zoo: provisional,
/// because none has been checked against labelled ground truth. See `docs/ml.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelStanding {
    /// Maverick's own weights, trained in-house. Redistributable with the app.
    FirstParty,
    /// Third-party weights published under a permissive licence. Redistributable on that
    /// licence's terms, which means the attribution in `licence` travels with the artefact.
    OpenLicensed,
}

impl ModelStanding {
    pub const fn name(self) -> &'static str {
        match self {
            Self::FirstParty => "first_party",
            Self::OpenLicensed => "open_licensed",
        }
    }

    /// True when shipping the artefact requires carrying an upstream attribution notice.
    pub const fn requires_attribution(self) -> bool {
        matches!(self, Self::OpenLicensed)
    }
}

/// Everything the core knows about one model, generated from its conversion contract.
#[derive(Clone, Copy, Debug)]
pub struct ModelContract {
    pub id: ModelId,
    /// Stable slug; also the artefact base name on both platforms.
    pub slug: &'static str,
    pub algorithm_id: &'static str,
    pub algorithm_version: Version,
    pub inputs: &'static [TensorSpec],
    pub outputs: &'static [TensorSpec],
    /// SHA-256 of the `model.mlmodel` inside the shipped `.mlpackage`.
    pub coreml_sha256: &'static str,
    /// SHA-256 of the shipped `.tflite` flatbuffer.
    pub tflite_sha256: &'static str,
    pub standing: ModelStanding,
    pub licence: &'static str,
    /// One line on what the model is for. Surfaces in the report bundle.
    pub role: &'static str,
}

impl ModelContract {
    pub fn input(&self, name: &str) -> Option<&'static TensorSpec> {
        self.inputs.iter().find(|spec| spec.name == name)
    }

    pub fn output(&self, name: &str) -> Option<&'static TensorSpec> {
        self.outputs.iter().find(|spec| spec.name == name)
    }

    /// True when `hash` is one of the two artefacts this build admits for the model.
    pub fn admits(&self, hash: &str) -> bool {
        hash.eq_ignore_ascii_case(self.coreml_sha256)
            || hash.eq_ignore_ascii_case(self.tflite_sha256)
    }
}

/// A tensor on its way to, or back from, the platform runtime.
///
/// Values are always `f32` on the wire. An `Int32` input is carried as whole-numbered floats and
/// converted by the platform binding, which is the only representation uniffi gives both
/// languages without a second record type; the contract's dtype is what makes the intent
/// explicit, and [`validate_request`] rejects a non-integral value in an `Int32` slot.
#[derive(Clone, Debug, PartialEq)]
pub struct NamedTensor {
    pub name: String,
    pub values: Vec<f32>,
}

impl NamedTensor {
    pub fn new(name: impl Into<String>, values: Vec<f32>) -> Self {
        Self {
            name: name.into(),
            values,
        }
    }
}

/// One inference the core wants performed, addressed to exactly one model.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelRequest {
    pub model: ModelId,
    pub inputs: Vec<NamedTensor>,
}

/// What the platform returned, before the core has agreed to believe it.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelResponse {
    pub model: ModelId,
    pub outputs: Vec<NamedTensor>,
    /// The hash the platform read off the artefact it actually loaded.
    pub model_sha256: String,
}

/// Reject a request the platform could not satisfy, before it crosses the boundary.
///
/// Every check here is a shape or a dtype the contract already states. Doing it in Rust means a
/// mismatch surfaces as one typed error with a code, rather than as an exception inside two
/// different native runtimes with two different messages.
pub fn validate_request(request: &ModelRequest) -> Result<()> {
    let contract = request.model.contract();
    if request.inputs.len() != contract.inputs.len() {
        return Err(shape_error(format!(
            "{} expects {} input tensors, received {}",
            contract.slug,
            contract.inputs.len(),
            request.inputs.len()
        )));
    }
    for spec in contract.inputs {
        let tensor = request
            .inputs
            .iter()
            .find(|tensor| tensor.name == spec.name)
            .ok_or_else(|| {
                shape_error(format!(
                    "{} is missing input tensor {}",
                    contract.slug, spec.name
                ))
            })?;
        if tensor.values.len() != spec.element_count() {
            return Err(shape_error(format!(
                "{} input {} needs {} values, received {}",
                contract.slug,
                spec.name,
                spec.element_count(),
                tensor.values.len()
            )));
        }
        if tensor.values.iter().any(|value| !value.is_finite()) {
            return Err(shape_error(format!(
                "{} input {} contains a non-finite value",
                contract.slug, spec.name
            )));
        }
        if spec.dtype == TensorDtype::Int32
            && tensor.values.iter().any(|value| value.fract() != 0.0)
        {
            return Err(shape_error(format!(
                "{} input {} is an integer tensor but carries a fractional value",
                contract.slug, spec.name
            )));
        }
    }
    Ok(())
}

/// Accept a response only if it matches the contract and came from an admitted artefact.
pub fn validate_response(response: &ModelResponse) -> Result<()> {
    let contract = response.model.contract();
    if !contract.admits(&response.model_sha256) {
        return Err(admission_error(format!(
            "{} inference came from an unadmitted artefact hash",
            contract.slug
        )));
    }
    if response.outputs.len() != contract.outputs.len() {
        return Err(shape_error(format!(
            "{} returns {} output tensors, received {}",
            contract.slug,
            contract.outputs.len(),
            response.outputs.len()
        )));
    }
    for spec in contract.outputs {
        let tensor = response
            .outputs
            .iter()
            .find(|tensor| tensor.name == spec.name)
            .ok_or_else(|| {
                shape_error(format!(
                    "{} is missing output tensor {}",
                    contract.slug, spec.name
                ))
            })?;
        if tensor.values.len() != spec.element_count() {
            return Err(shape_error(format!(
                "{} output {} needs {} values, received {}",
                contract.slug,
                spec.name,
                spec.element_count(),
                tensor.values.len()
            )));
        }
        if tensor.values.iter().any(|value| !value.is_finite()) {
            return Err(inference_error(format!(
                "{} output {} contains a non-finite value",
                contract.slug, spec.name
            )));
        }
    }
    Ok(())
}

/// Read one named output tensor out of a validated response.
pub fn output_values<'a>(response: &'a ModelResponse, name: &str) -> Result<&'a [f32]> {
    response
        .outputs
        .iter()
        .find(|tensor| tensor.name == name)
        .map(|tensor| tensor.values.as_slice())
        .ok_or_else(|| {
            shape_error(format!(
                "{} did not return an output named {name}",
                response.model.contract().slug
            ))
        })
}

/// Turn logits into probabilities without letting a large logit overflow.
pub fn softmax(values: &[f32]) -> Vec<f32> {
    let maximum = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    if !maximum.is_finite() {
        return vec![0.0; values.len()];
    }
    let exponentials: Vec<f64> = values
        .iter()
        .map(|value| (f64::from(*value) - f64::from(maximum)).exp())
        .collect();
    let total: f64 = exponentials.iter().sum();
    if total <= 0.0 {
        return vec![0.0; values.len()];
    }
    exponentials
        .iter()
        .map(|value| (value / total) as f32)
        .collect()
}

/// The index of the largest value, or `None` for an empty slice.
pub fn argmax(values: &[f32]) -> Option<usize> {
    values
        .iter()
        .enumerate()
        .max_by(|left, right| left.1.total_cmp(right.1))
        .map(|(index, _)| index)
}

fn shape_error(message: impl Into<String>) -> MavError {
    MavError::new(codes::ML_MODEL_TENSOR_INVALID, message)
}

fn inference_error(message: impl Into<String>) -> MavError {
    MavError::new(codes::ML_MODEL_INFERENCE_INVALID, message)
}

fn admission_error(message: impl Into<String>) -> MavError {
    MavError::new(codes::ML_MODEL_NOT_ADMITTED, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_request(model: ModelId) -> ModelRequest {
        let contract = model.contract();
        ModelRequest {
            model,
            inputs: contract
                .inputs
                .iter()
                .map(|spec| NamedTensor::new(spec.name, vec![0.0; spec.element_count()]))
                .collect(),
        }
    }

    #[test]
    fn every_contract_states_complete_shapes() {
        for model in ALL_MODELS {
            let contract = model.contract();
            assert!(
                !contract.inputs.is_empty(),
                "{} has no inputs",
                contract.slug
            );
            assert!(
                !contract.outputs.is_empty(),
                "{} has no outputs",
                contract.slug
            );
            for spec in contract.inputs.iter().chain(contract.outputs) {
                assert!(
                    spec.element_count() > 0,
                    "{} tensor {} has an empty shape",
                    contract.slug,
                    spec.name
                );
            }
            assert_eq!(contract.coreml_sha256.len(), 64, "{}", contract.slug);
            assert_eq!(contract.tflite_sha256.len(), 64, "{}", contract.slug);
        }
    }

    #[test]
    fn slugs_and_hashes_are_unique() {
        let mut slugs: Vec<&str> = ALL_MODELS.iter().map(|m| m.contract().slug).collect();
        slugs.sort_unstable();
        let count = slugs.len();
        slugs.dedup();
        assert_eq!(slugs.len(), count, "two models share a slug");

        // Two models may legitimately share an artefact hash: the follicular head is unchanged
        // between popsicle 1.6.0 and 1.8.1, so both archives converted to byte-identical
        // flatbuffers. That is safe because admission asks "does *this* model's contract admit
        // this hash", never "which model owns this hash". What must not happen is two models
        // sharing a hash while claiming to be different algorithms.
        let mut by_hash: Vec<(&str, &str)> = ALL_MODELS
            .iter()
            .flat_map(|m| {
                [
                    (m.contract().coreml_sha256, m.contract().algorithm_id),
                    (m.contract().tflite_sha256, m.contract().algorithm_id),
                ]
            })
            .collect();
        by_hash.sort_unstable();
        for pair in by_hash.windows(2) {
            if pair[0].0 == pair[1].0 {
                assert_eq!(
                    pair[0].1, pair[1].1,
                    "one artefact is claimed by two different algorithms"
                );
            }
        }
    }

    #[test]
    fn a_correct_request_validates() {
        for model in ALL_MODELS {
            validate_request(&full_request(*model)).expect("contract-shaped request");
        }
    }

    #[test]
    fn a_short_tensor_is_rejected_with_its_name() {
        let model = ALL_MODELS[0];
        let mut request = full_request(model);
        request.inputs[0].values.pop();
        let error = validate_request(&request).expect_err("short tensor");
        assert_eq!(error.code, codes::ML_MODEL_TENSOR_INVALID);
        assert!(error.message.contains(request.inputs[0].name.as_str()));
    }

    #[test]
    fn a_non_finite_tensor_is_rejected() {
        let model = ALL_MODELS[0];
        let mut request = full_request(model);
        request.inputs[0].values[0] = f32::NAN;
        let error = validate_request(&request).expect_err("non-finite tensor");
        assert_eq!(error.code, codes::ML_MODEL_TENSOR_INVALID);
    }

    #[test]
    fn an_unadmitted_hash_is_rejected() {
        let model = ALL_MODELS[0];
        let contract = model.contract();
        let response = ModelResponse {
            model,
            outputs: contract
                .outputs
                .iter()
                .map(|spec| NamedTensor::new(spec.name, vec![0.0; spec.element_count()]))
                .collect(),
            model_sha256: "0".repeat(64),
        };
        let error = validate_response(&response).expect_err("unadmitted hash");
        assert_eq!(error.code, codes::ML_MODEL_NOT_ADMITTED);
    }

    #[test]
    fn an_admitted_response_validates_and_reads_back() {
        let model = ALL_MODELS[0];
        let contract = model.contract();
        let response = ModelResponse {
            model,
            outputs: contract
                .outputs
                .iter()
                .map(|spec| NamedTensor::new(spec.name, vec![0.5; spec.element_count()]))
                .collect(),
            model_sha256: contract.coreml_sha256.to_owned(),
        };
        validate_response(&response).expect("admitted response");
        let first = contract.outputs[0].name;
        assert_eq!(
            output_values(&response, first).expect("named output").len(),
            contract.outputs[0].element_count()
        );
    }

    #[test]
    fn softmax_normalises_and_argmax_finds_the_peak() {
        let probabilities = softmax(&[1.0, 3.0, 2.0]);
        let total: f32 = probabilities.iter().sum();
        assert!((total - 1.0).abs() < 1e-6, "softmax total was {total}");
        assert_eq!(argmax(&probabilities), Some(1));
        // 3.0 beats 2.0 by one logit: e / (1 + e + e^-0) worked out exactly.
        assert!((probabilities[1] - 0.665_240_9).abs() < 1e-5);
    }

    #[test]
    fn softmax_survives_a_large_logit() {
        let probabilities = softmax(&[0.0, 800.0]);
        assert_eq!(probabilities[0], 0.0);
        assert!((probabilities[1] - 1.0).abs() < 1e-6);
    }
}
