//! The queue between the core and the platform inference runtimes.
//!
//! `ecg_capture` already established the shape of a native inference in Maverick: the core
//! prepares tensors, the platform pulls a request, runs it, and submits the result back. That
//! worked because there was one model and one caller. With a registry of models the same
//! interaction needs an addressable queue, and this is it.
//!
//! The design is deliberately pull-based, matching the connector transport seam:
//!
//! 1. Core code calls [`ModelHost::enqueue`] with a validated request and gets a request id.
//! 2. The platform polls [`ModelHost::next_request`], loads the model named in the request,
//!    runs it, and calls [`ModelHost::submit`] with the outputs and the artefact hash it loaded.
//! 3. The core validates the response against the same contract, and the caller reads it.
//!
//! No callbacks, no threads, no async: the platform decides when inference happens, which is
//! the only party that knows whether the app is foregrounded and whether the accelerator is
//! busy. What the core keeps is the right to refuse a result that does not match the contract.
//!
//! The queue is bounded. An inference that is never collected must not accumulate tensors —
//! `pulse_ppg` alone takes a 48 KiB input and a 2 KiB output, and a leaked queue of them is a
//! memory bug that would present as a crash far from its cause.

use mav_analytic::model_zoo::{
    validate_request, validate_response, ModelId, ModelRequest, ModelResponse, NamedTensor,
};
use mav_model::error::{codes, MavError, Result};
use std::collections::VecDeque;

/// How many inferences may be outstanding before the core refuses to queue more.
///
/// Deep enough for a batch of PPG windows across one night's recompute, shallow enough that a
/// platform that has stopped collecting is noticed on the next enqueue rather than at OOM.
pub const MAX_PENDING_REQUESTS: usize = 32;

/// A request with the identity the platform answers against.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingInference {
    pub request_id: u64,
    pub model: ModelId,
    pub inputs: Vec<NamedTensor>,
}

/// A completed inference, validated and ready to interpret.
#[derive(Clone, Debug, PartialEq)]
pub struct CompletedInference {
    pub request_id: u64,
    pub model: ModelId,
    pub outputs: Vec<NamedTensor>,
    /// The artefact hash the platform reported, carried through to provenance.
    pub model_sha256: String,
}

impl CompletedInference {
    /// One named output tensor.
    pub fn output(&self, name: &str) -> Result<&[f32]> {
        self.outputs
            .iter()
            .find(|tensor| tensor.name == name)
            .map(|tensor| tensor.values.as_slice())
            .ok_or_else(|| {
                MavError::new(
                    codes::ML_MODEL_TENSOR_INVALID,
                    format!(
                        "{} did not return an output named {name}",
                        self.model.contract().slug
                    ),
                )
            })
    }
}

/// The core's side of the native inference boundary.
#[derive(Debug, Default)]
pub struct ModelHost {
    next_id: u64,
    pending: VecDeque<PendingInference>,
    /// Requests handed to the platform and not yet answered. Kept so a submission can be
    /// checked against the model it was actually issued for, rather than trusting the id.
    in_flight: Vec<(u64, ModelId)>,
    completed: Vec<CompletedInference>,
}

impl ModelHost {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            pending: VecDeque::new(),
            in_flight: Vec::new(),
            completed: Vec::new(),
        }
    }

    /// Validate and queue one inference. Returns the id the platform will answer against.
    pub fn enqueue(&mut self, request: ModelRequest) -> Result<u64> {
        validate_request(&request)?;
        if self.pending.len() + self.in_flight.len() >= MAX_PENDING_REQUESTS {
            return Err(MavError::new(
                codes::ML_MODEL_REQUEST_UNKNOWN,
                format!("more than {MAX_PENDING_REQUESTS} inferences are outstanding"),
            ));
        }
        let request_id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.pending.push_back(PendingInference {
            request_id,
            model: request.model,
            inputs: request.inputs,
        });
        Ok(request_id)
    }

    /// Hand the oldest queued inference to the platform. Ordering is FIFO so a recompute that
    /// enqueued a night of windows gets them back in the order it asked.
    pub fn next_request(&mut self) -> Option<PendingInference> {
        let request = self.pending.pop_front()?;
        self.in_flight.push((request.request_id, request.model));
        Some(request)
    }

    /// How many inferences the platform still owes an answer for.
    pub fn outstanding(&self) -> usize {
        self.pending.len() + self.in_flight.len()
    }

    /// Accept a platform result. Rejects an unknown id, a model that does not match the one the
    /// request was issued for, an unadmitted artefact hash, or an output that breaks the contract.
    pub fn submit(
        &mut self,
        request_id: u64,
        outputs: Vec<NamedTensor>,
        model_sha256: String,
    ) -> Result<CompletedInference> {
        let position = self
            .in_flight
            .iter()
            .position(|(id, _)| *id == request_id)
            .ok_or_else(|| {
                MavError::new(
                    codes::ML_MODEL_REQUEST_UNKNOWN,
                    format!("no inference {request_id} is awaiting a result"),
                )
            })?;
        let (_, model) = self.in_flight[position];
        let response = ModelResponse {
            model,
            outputs,
            model_sha256,
        };
        // Only retire the in-flight entry once the response is known good. A platform that
        // returns garbage may retry; dropping the entry first would turn the retry into an
        // "unknown request" and hide the real fault.
        validate_response(&response)?;
        self.in_flight.remove(position);
        let completed = CompletedInference {
            request_id,
            model: response.model,
            outputs: response.outputs,
            model_sha256: response.model_sha256,
        };
        self.completed.push(completed.clone());
        Ok(completed)
    }

    /// Take a completed inference by id, if it has arrived.
    pub fn take(&mut self, request_id: u64) -> Option<CompletedInference> {
        let position = self
            .completed
            .iter()
            .position(|completed| completed.request_id == request_id)?;
        Some(self.completed.remove(position))
    }

    /// Abandon one inference, whether queued, in flight, or already answered.
    ///
    /// A capture the wearer cancelled must not leave its tensors in the queue, and the platform
    /// may still submit for it afterwards; a submission for a cancelled id is then an unknown
    /// request, which is the correct answer.
    pub fn cancel(&mut self, request_id: u64) -> bool {
        let before = self.pending.len() + self.in_flight.len() + self.completed.len();
        self.pending
            .retain(|request| request.request_id != request_id);
        self.in_flight.retain(|(id, _)| *id != request_id);
        self.completed
            .retain(|completed| completed.request_id != request_id);
        before != self.pending.len() + self.in_flight.len() + self.completed.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mav_analytic::model_zoo::TensorDtype;
    use mav_analytic::model_zoo::ALL_MODELS;

    fn model() -> ModelId {
        ALL_MODELS[0]
    }

    /// A request that satisfies every contract, whatever dtypes it declares.
    ///
    /// The fill has to respect dtype: `validate_request` rejects a fractional value in an
    /// integer slot, and it is right to. A helper that ignored that would fail on the first
    /// model with an integer input rather than exercising it.
    fn request(id: ModelId) -> ModelRequest {
        ModelRequest {
            model: id,
            inputs: id
                .contract()
                .inputs
                .iter()
                .map(|spec| {
                    let fill = match spec.dtype {
                        TensorDtype::Int32 => 1.0,
                        TensorDtype::Float32 => 0.25,
                    };
                    NamedTensor::new(spec.name, vec![fill; spec.element_count()])
                })
                .collect(),
        }
    }

    fn outputs(id: ModelId) -> Vec<NamedTensor> {
        id.contract()
            .outputs
            .iter()
            .map(|spec| NamedTensor::new(spec.name, vec![0.5; spec.element_count()]))
            .collect()
    }

    #[test]
    fn a_queued_request_comes_back_in_order_and_completes() {
        let mut host = ModelHost::new();
        let first = host.enqueue(request(model())).expect("enqueue");
        let second = host.enqueue(request(model())).expect("enqueue");
        assert_eq!(host.outstanding(), 2);

        let handed = host.next_request().expect("first request");
        assert_eq!(handed.request_id, first);
        let handed = host.next_request().expect("second request");
        assert_eq!(handed.request_id, second);
        assert!(host.next_request().is_none());

        let completed = host
            .submit(
                first,
                outputs(model()),
                model().contract().coreml_sha256.to_owned(),
            )
            .expect("submit");
        assert_eq!(completed.request_id, first);
        assert_eq!(host.outstanding(), 1);
        let taken = host.take(first).expect("take");
        assert_eq!(taken.model, model());
        assert!(host.take(first).is_none());
    }

    #[test]
    fn a_malformed_request_is_refused_before_it_is_queued() {
        let mut host = ModelHost::new();
        let mut broken = request(model());
        broken.inputs[0].values.clear();
        let error = host.enqueue(broken).expect_err("malformed request");
        assert_eq!(error.code, codes::ML_MODEL_TENSOR_INVALID);
        assert_eq!(host.outstanding(), 0);
    }

    #[test]
    fn an_unknown_submission_is_rejected() {
        let mut host = ModelHost::new();
        let error = host
            .submit(
                99,
                outputs(model()),
                model().contract().tflite_sha256.to_owned(),
            )
            .expect_err("unknown id");
        assert_eq!(error.code, codes::ML_MODEL_REQUEST_UNKNOWN);
    }

    #[test]
    fn an_unadmitted_hash_leaves_the_request_in_flight_for_a_retry() {
        let mut host = ModelHost::new();
        let id = host.enqueue(request(model())).expect("enqueue");
        host.next_request().expect("hand out");
        let error = host
            .submit(id, outputs(model()), "f".repeat(64))
            .expect_err("unadmitted hash");
        assert_eq!(error.code, codes::ML_MODEL_NOT_ADMITTED);
        assert_eq!(host.outstanding(), 1);

        let completed = host
            .submit(
                id,
                outputs(model()),
                model().contract().tflite_sha256.to_owned(),
            )
            .expect("retry after a good load");
        assert_eq!(completed.request_id, id);
    }

    #[test]
    fn a_short_output_is_rejected() {
        let mut host = ModelHost::new();
        let id = host.enqueue(request(model())).expect("enqueue");
        host.next_request().expect("hand out");
        let mut broken = outputs(model());
        broken[0].values.pop();
        let error = host
            .submit(id, broken, model().contract().coreml_sha256.to_owned())
            .expect_err("short output");
        assert_eq!(error.code, codes::ML_MODEL_TENSOR_INVALID);
    }

    #[test]
    fn the_queue_is_bounded() {
        let mut host = ModelHost::new();
        for _ in 0..MAX_PENDING_REQUESTS {
            host.enqueue(request(model())).expect("enqueue");
        }
        let error = host.enqueue(request(model())).expect_err("queue full");
        assert_eq!(error.code, codes::ML_MODEL_REQUEST_UNKNOWN);
        assert_eq!(host.outstanding(), MAX_PENDING_REQUESTS);
    }

    #[test]
    fn cancelling_removes_a_request_from_every_stage() {
        let mut host = ModelHost::new();
        let queued = host.enqueue(request(model())).expect("enqueue");
        assert!(host.cancel(queued));
        assert_eq!(host.outstanding(), 0);
        assert!(!host.cancel(queued));

        let flying = host.enqueue(request(model())).expect("enqueue");
        host.next_request().expect("hand out");
        assert!(host.cancel(flying));
        let error = host
            .submit(
                flying,
                outputs(model()),
                model().contract().coreml_sha256.to_owned(),
            )
            .expect_err("cancelled id");
        assert_eq!(error.code, codes::ML_MODEL_REQUEST_UNKNOWN);
    }

    #[test]
    fn every_registered_model_round_trips_through_the_host() {
        for id in ALL_MODELS {
            let mut host = ModelHost::new();
            let request_id = host.enqueue(request(*id)).expect("enqueue");
            let handed = host.next_request().expect("hand out");
            assert_eq!(handed.model, *id);
            assert_eq!(handed.inputs.len(), id.contract().inputs.len());
            let completed = host
                .submit(
                    request_id,
                    outputs(*id),
                    id.contract().tflite_sha256.to_owned(),
                )
                .expect("submit");
            let first = id.contract().outputs[0].name;
            assert_eq!(
                completed.output(first).expect("named output").len(),
                id.contract().outputs[0].element_count()
            );
        }
    }
}
