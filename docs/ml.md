# The ML boundary

This document describes where machine-learning inference sits in Maverick's architecture. Maverick
has one admitted learned model: the provisional three-class ECG classifier documented below.
Every other current analytic remains deterministic DSP or statistics. The ECG path deliberately
keeps those deterministic stages in shared Rust and places only the final tensor inference in the
native platform runtime.

## The boundary

Inference runs natively: Core ML on iOS, TensorFlow Lite on Android. This is a hard
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

### `nao_full_v2` ECG classifier — provisional contract

This is Maverick's first admitted model implementation. It was recovered from a third-party Android
package and converted for native Apple inference. Its software behaviour is covered by deterministic
fixtures, but it has not been validated against an independent clinically labelled cohort. Every
surface therefore labels it **research-only** and **provisional**. Redistribution outside a local
development build additionally requires the model owner's permission.

| Field | Contract |
|---|---|
| Version | `2.0.0` |
| Input | `FLOAT32 [1, 7680, 1]` |
| Source record | exactly 30 seconds |
| Target rate | 256 Hz |
| Fit | centre crop or zero pad to 7,680 samples |
| Normalisation | z-score independently per record |
| Output | three values in class order `N`, `A`, `O` |
| Meaning | sinus rhythm, atrial fibrillation, other abnormal rhythm |
| Confidence | winning-minus-runner-up margin mapped linearly over `0...0.2`; model certainty only |
| XAI | bounded ordered occlusion runs interpreted by the shared core |
| Algorithm id | `nao_full_v2_ecg_classifier` |
| Algorithm version | `2.0.0` |

Preprocessing is byte-frozen in this order:

1. convert the declared source unit to millivolts;
2. linearly resample to 256 Hz using source positions `i / 256 * source_rate`;
3. apply the recovered unpadded forward/reverse SOS chain (band-pass, 50 Hz notch, 60 Hz notch);
4. apply the recovered 0.5 Hz one-pole high-pass fallback only when the filtered mean magnitude is
   above `0.02` or standard deviation is below `0.0001`;
5. centre crop or zero pad to 7,680 samples;
6. z-score using population variance with a `1e-9` floor.

The selected platform artefacts are:

| Platform | Artefact | Runtime | Minimum | SHA-256 |
|---|---|---|---|---|
| iOS | `nao_full_ecg_model_fp16.mlpackage` | Core ML ML Program, FP16 | iOS 15; Maverick device floor A13 | package contents recorded below |
| Android | `nao_full_ecg_model_fp16.tflite` | TensorFlow Lite, FP16 weights with FLOAT32 I/O | Android 10 / API 29 | `0be97329077d5d5d2791b8b7850baf8acaf8f12f96fdfad7bcdb4af37156ea21` |

Core ML package member hashes:

- `Manifest.json`: `2760ca6f4696a0519091fa43ee9ddbfae1bbda4e61fb85a5438d2cb3317ab288`
- `Data/com.apple.CoreML/model.mlmodel`:
  `24230f1c635ab45fba02bb80c86f3b9a9dc2499436bda16a1d97b34ffb63f6d3`
- `Data/com.apple.CoreML/weights/weight.bin`:
  `24111a56f73dc262cf600a73f18a647bf8ad623ecaa7336da5463e87325de0d9`

The FP16 TFLite graph takes and returns FLOAT32 tensors. The recovered wrapper preserves an
already-normalized output, otherwise applies softmax, then applies the recovered `1e-7` probability
floor and renormalizes. Core ML already returns normalized probabilities and must not receive an
additional softmax.

Precision is selected at build time, not by benchmarking several bundled models on first launch.
FP32, FP16 and INT8 are numeric representations, not ordered model versions, and runtime
"support" is not a stable device tier: both native runtimes may silently fall back to CPU. Bundling
all variants would add their weights to every install and make a one-time choice stale after OS or
delegate changes. Maverick therefore ships one FP16 artifact per platform—Core ML on iOS and
TensorFlow Lite on Android—with no bundled alternate and no first-launch selector. Android's
original recovered FP16 graph is 4,067,100 bytes.

The model does not separate class `O`. Its output tensor has exactly three values, so tachycardia,
bradycardia, bigeminy and other non-`N`/non-`A` morphologies cannot be relabelled as model
diagnoses. Fixtures with those shapes exercise the broad bucket only. A future subtype must be
admitted as a separately validated model or deterministic analytic with its own ground truth;
Maverick does not infer a diagnostic subtype from the fixture filename or heart rate alone.

The current validation ceiling is conversion consistency:

- nine deterministic software fixtures, three each for expected `N`, `A`, and `O`;
- all selected runtimes agree on the winning class for those fixtures;
- all nine fixture families pass the host calibration quality gate after resampling to the WHOOP
  connector's declared 100 Hz source rate;
- Core ML FP16 agrees with its TFLite FP16 reference within the recorded parity tolerance;
- generated explanations cover the complete input, and generated reports reopen and render.

These fixtures are not clinical ECG simulations and do not establish sensitivity, specificity,
positive predictive value, or diagnostic fitness.

## Runtime dependency rule

The iOS app uses the Core ML framework already shipped by iOS. The Android app admits exactly one
TensorFlow Lite runtime dependency because it ships exactly one contracted `.tflite` artefact.
Additional runtimes, delegates, or model variants require their own measured need, contract, bundle
audit, and admission review; they are not added speculatively.

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
