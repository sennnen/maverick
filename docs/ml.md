# The ML boundary

This document describes where machine-learning inference sits in Maverick's architecture, and it
starts with an honest statement of how much ML there currently is: none.

Neither of the codebases Maverick learned from ships a neural model. No `.mlmodel`, `.mlpackage`,
`.tflite`, or `.onnx` exists anywhere in either repo. Every analytic in both is classical DSP and
statistics: RMSSD, Welch PSD, Cole-Kripke actigraphy, rule-based sleep staging, EWMA baselines,
z-score recovery, Banister and Edwards strain. The one thing in either repo that could be called AI
is an opt-in, bring-your-own-key cloud LLM text coach, which is not an analytic at all. Even the
"golden fixtures for models" one repo advertises turn out to be decoder oracles, not model weights.
Maverick's analytics today are the same kind of thing: deterministic computations that can be
fixture-tested, not learned models.

So this document describes architecture held in reserve. The boundary is designed now, because
boundaries are cheap to draw early and expensive to retrofit; the machinery behind it is not built
until something real needs it.

## The boundary

When a model exists, inference runs natively: CoreML on iOS, TFLite on Android. This is a hard
line, for the same reasons the BLE radio stays native (see [architecture.md](architecture.md)):
CoreML and TFLite are the accelerated runtimes each platform actually ships, models compiled for
one do not run on the other, and a Rust inference stack would forfeit the hardware acceleration
while adding a dependency nothing needs yet. ML inference in Rust is on the explicitly-not-building
list.

Rust owns everything up to the tensor. Preprocessing — resampling, filtering, FFT (rustfft),
spectrogram construction, feature extraction — is pure, deterministic Rust in the core, and it is
golden-vector tested: a stored input signal must produce a byte-identical preprocessed tensor. The
prior codebases already worked this way for their DSP, with golden `.npy` and `.f64` vectors for
their CPPPG, PulsePPG, and SleepStage pipelines; Maverick ports the concept and regenerates its own
vectors per model. The split means the part of the ML path most likely to harbour a subtle bug (a
resample off by one, a window function misapplied) is in shared, fixture-tested code, and the
native side is reduced to a tensor-in, tensor-out call.

A prediction that comes back across the boundary enters the pipeline as a first-class feature with
provenance, exactly like every computed feature: a value, a confidence, and a `MetadataId` linking
to the algorithm id and version that produced it. Nothing about being a model output exempts a
prediction from the walk-back requirement.

## The per-model contract

Every model that ever ships gets a documented contract in this file before it lands:

- Input tensor shape, dtype, and the exact normalisation applied.
- How the output is to be interpreted.
- What the confidence value means, and what it does not mean.
- The hash of the model file, so the artefact in the app is verifiably the artefact that was
  validated.
- The model version, tied into the same versioning conventions as every algorithm.

A model without a written contract does not ship. The contract is what makes a model's behaviour a
specification rather than a vibe, and the file hash is what makes "which model produced this" a
question with an answer.

## No dependency before a model

No CoreML or TFLite dependency is added to either app until a real model, with a golden vector,
actually exists. A dependency added "for when we need it" is a cost with no benefit: it enlarges
the build, adds an update treadmill, and tempts speculative scaffolding around it. The day a real
model arrives, adding the runtime dependency is the easy part.

## How analytics are admitted

Whether an analytic is a learned model or a closed-form formula, it enters Maverick under the same
admission rule, stated in [testing.md](testing.md): a golden fixture derived from a real capture or
a published reference implementation, or property tests that can genuinely fail. And the same
validation distinction applies. A future sleep-staging model that agrees with itself on both
platforms is consistent; only a model checked against ground truth (polysomnography-labelled data,
or a published reference with known outputs) is validated, and anything less is labelled
provisional. The prior codebases accumulated some forty speculative analytics engines that no
fixture could fail; Maverick's rule exists so that the model era, when it comes, does not repeat
that with weights instead of formulas.
