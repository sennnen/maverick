# The ML boundary

Maverick runs the model zoo on-device. This document says where inference sits in the
architecture, what every model does, and how far each one may be believed.

Two things are true at once and both matter. Every model here is Maverick's own work, trained
in-house, and ships with the app under no external restriction — the one exception is Pulse-PPG,
which is third-party and MIT-licensed. And not one of them is *validated*: each has a written
tensor contract, fixture-covered software behaviour, and a recorded conversion parity, and none
has been checked against labelled ground truth. Provenance and trustworthiness are separate axes,
and the second is why no zoo model reaches a snapshot or a screen yet.

**Where the numbers in this document came from.** Everything about Android — latency, memory,
delegate choice, thermal state, and every parity figure with "device" in its name — was measured
on a Pixel 7 (Tensor G2, API 37, arm64-v8a) and is reproducible from
`artifacts/models/device/`. Everything about Core ML — processor assignment, per-policy parity,
the compute-unit sweep — was measured on a development Mac; **no iOS device has been run, so
there is no iOS latency or power figure here at all.** Android power draw is likewise absent
rather than estimated: the handset was on AC at 100% throughout, so battery current reported the
charger and not the workload. Where a number could not be measured this document says so instead
of supplying one.

## The boundary

Inference runs natively: Core ML on iOS, TensorFlow Lite on Android. This is a hard line, for the
same reason the BLE radio stays native (see [architecture.md](architecture.md)): these are the
accelerated runtimes each platform actually ships, a model compiled for one does not run on the
other, and a Rust inference stack would forfeit the hardware acceleration while adding a
dependency nothing needs. ML inference in Rust is on the explicitly-not-building list.

Rust owns everything up to the tensor. Resampling, filtering, windowing, feature assembly and
normalisation are deterministic Rust in the core, golden-vector tested: a stored input signal must
produce a byte-identical preprocessed tensor. The native side is reduced to a tensor-in,
tensor-out call.

A prediction that comes back across the boundary enters the pipeline as a first-class feature with
provenance — a value, a confidence, and a `MetadataId` naming the algorithm and version that
produced it. Being a model output exempts nothing from the walk-back requirement.

### Why only the core of each model is converted

Each model leaves training as a TorchScript wrapper: validation, resampling, filtering, windowing,
the network, then post-processing, all in one archive. Only the tensor-in / tensor-out neural core
is converted. The wrapper is ported to Rust.

Converting the wrapper would have been less work and would have been wrong. The wrappers branch on
their data — how many pulse feet were found, how many windows a signal produced, whether a
validator raised — and a trace freezes whichever branch the tracing input happened to take,
producing a graph that is correct for one signal and quietly wrong for the next. It would also put
the resample-and-filter stages inside a binary that two platforms compile differently, which is the
thing this boundary exists to prevent.

## How a model runs

```text
mav-analytic::model_zoo::ppg     prepares the tensor          Rust, deterministic, tested
mav-engine::model_host           queues it, hands out an id   Rust, bounded at 32
mav-ffi                          carries named f32 tensors    uniffi
MavModelRunner (Swift / Kotlin)  one tensor call              Core ML / TensorFlow Lite
mav-engine::model_host           validates and admits         shape, finiteness, artefact hash
```

The queue is pull-based: the platform asks for work rather than the core pushing it, because only
the app knows whether it is foregrounded and whether the accelerator is free. It is bounded,
because an uncollected queue of Pulse-PPG inputs is a memory bug that would surface far from its
cause.

Admission is by artefact hash. The platform reports the hash of the model it actually loaded with
every result, and the core refuses one the registry does not know.

## The models

Every shape below is static, and every tensor crosses the FFI as `f32`. Where a contract declares
an integer tensor, values travel as whole-numbered floats and the platform binding casts them;
`validate_request` rejects a fractional value in an integer slot.

### PPG front-ends

Both encoders turn a raw PPG window into an embedding. Nothing about an embedding is displayable —
it exists to be consumed by a head.

#### `pulse_ppg` — open-weight PPG foundation encoder

| | |
|---|---|
| Input | `ppg` `(1, 1, 12000)` — 240 s at 50 Hz, z-scored |
| Output | `embeddings` `(1, 512)` |
| Architecture | ResNet1D, 12 residual blocks, 128 base filters, kernel 11, stride 2, instance-normalised input, max-pool over time |
| Parameters | 28,497,920 |
| Standing | `open_licensed` — MIT, Pulse-PPG (Xu et al., UbiComp 2025) |

Third-party, and the only model here that is. Pre-trained on roughly 200 million seconds of
uncurated wrist PPG from a 100-day field study, which is the regime Maverick operates in: a strap
on a moving wrist, not a finger clip in a lab. It is the default front-end because it generalises
across the widest range of wear conditions.

Its four-minute window is the window it was pre-trained on. The encoder is fully convolutional, so
a shorter one would run, but a shorter one is not what the weights were fitted against.

#### `pulsenet_foundation` — in-house PPG encoder

| | |
|---|---|
| Input | `ppg` `(1, 1, 1500)` — 30 s at 50 Hz, moving-average detrended |
| Output | `embeddings` `(1, 256)` |
| Architecture | EfficientNet1D (PulseNet-Foundation v0.4.0) |
| Parameters | 890,608 |

Maverick's own encoder, and the one the hypertension heads were fitted against. It stays alongside
Pulse-PPG rather than being replaced by it: `halite_ppg_score` reads *this* embedding space, and
swapping an encoder under a head is not a substitution a contract can make silently.

Shorter window, smaller output, and roughly a thirtieth of the parameters — the cheap option when
a thirty-second segment is all that is available.

### Cardiovascular

#### `halite_ppg_score` — per-segment hypertension score

| | |
|---|---|
| Input | `embeddings` `(1, 256)` — one `pulsenet_foundation` output |
| Output | `ppg_score` `(1, 1)` |
| Parameters | 257 |

A linear head over one PPG embedding. One segment gives one score; the full path aggregates scores
across a history window by weighted mean before the tree sees them, and that aggregation is Rust,
not model.

#### `halite_risk_tree` — hypertension-risk head

| | |
|---|---|
| Input | `features` `(1, 13)` — `user_info` (4) ‖ `baselines` (8) ‖ aggregated PPG score (1) |
| Outputs | `label` `(1,)` — class index; `probabilities` `(1, 2)` |
| Parameters | 19,100 |

`user_info` is `(sex, age, height, weight)`, with the fourth column replaced by BMI
(`weight / height²`) before the tree sees it — that substitution is part of the contract and is
done in Rust.

`label` is the ensemble's own argmax, **not** a risk level. The full path maps the probability onto
a risk level using age- and sex-specific thresholds; that calibration is post-processing and is not
in the converted graph.

### Sleep

All three sleep models score a fifteen-hour night as 1,800 thirty-second epochs. `high_res`
carries 64 samples per epoch (1,800 × 64 = 115,200); `low_res` carries one value per epoch.

#### `sleepnet_moonstone` — staging, apnea and SpO2 events

| | |
|---|---|
| Inputs | `high_res` `(1, 115200, 3)` — channels `ibi`, `amplitude`, `spo2`; `low_res` `(1, 1800, 1)` — motion seconds per epoch |
| Outputs | `staging_logits` `(1, 4, 1800)`; `apnea_logits` `(1, 1, 1800)` |
| Parameters | 1,075,955 |

The widest of the three: it takes pulse oximetry as well as interbeat intervals, which is what lets
it speak to apnea rather than only to stage.

#### `sleepnet_bdi` — staging and apnea from interbeat intervals alone

| | |
|---|---|
| Input | `high_res` `(1, 115200, 2)` — channels `ibi`, `amplitude` |
| Outputs | `staging_logits` `(1, 4, 1800)`; `apnea_logits` `(1, 1, 1800)` |
| Parameters | 1,008,640 |

No SpO2 and no low-resolution channel, so it runs on any strap that produces intervals.

#### `sleepnet_bdi_v3` — the previous generation of the same network

Identical contract, 290,960 parameters, version 0.3.0. Kept for comparison against 0.4.0 on the
same night; it is a third of the size and correspondingly cheaper.

**Reading the staging output.** Four logits per epoch, unnormalised — `model_zoo::softmax` and
`argmax` are the admitted way to read them, and neither is applied inside the graph. The class
order is the training vocabulary and has **not** been mapped onto Maverick's sleep-stage
vocabulary. That mapping is an admission decision, not a rename, and until it is made the staging
output is not something a surface may display.

### Heart rate

#### `awhr_imputation` — awake heart rate over gaps

| | |
|---|---|
| Input | `window` `(1, 60, 13)` — 60 steps × 13 context features |
| Output | `imputed_hr` `(1, 60, 1)` — one imputed value per step |
| Architecture | Bidirectional 2-layer LSTM, hidden 72, four fully-connected layers |
| Parameters | 209,665 |

Fills awake heart rate across a stretch where the optical signal was unusable, from step-motion and
activity context either side of the gap. Bidirectional on purpose: what happened *after* a gap
constrains it as much as what happened before.

#### `dhrv_imputation` — daytime HRV over gaps

| | |
|---|---|
| Input | `features` `(1, 10)` |
| Output | `imputed_dhrv` `(1, 1)` |
| Architecture | Four-layer MLP, 10 → 32 → 64 → 32 → 1 |
| Parameters | 4,577 |

The same idea for daytime HRV, and much simpler: skin temperature, ring and total MET, heart rate,
and the wearer's own baselines for HRV, heart rate and temperature, reduced to a ten-value vector
in Rust.

### Activity and energy

#### `activity_detection` — what the wearer was doing

| | |
|---|---|
| Input | `features` `(1, 64, 77)` — 64 candidate segments × 77 features |
| Output | `activity_output` `(1, 64, 262)` |
| Parameters | 1,338,910 |

The 262 columns are four sigmoid probabilities, then a 256-value activity embedding, then the
segment's start and end in day-minutes. Features come from MET, motion, step-motion, heart rate and
temperature.

#### `activity_transition` — where one activity ends and the next begins

| | |
|---|---|
| Input | `features` `(1, 64, 29)` — a 64-minute window × 29 features |
| Output | `transition_logits` `(1, 64)` — one score per minute |
| Parameters | 86,971 |

Segmentation rather than classification: it answers *when* something changed, and
`activity_detection` answers *what* it was.

#### `energy_expenditure_hr` — active energy for a window with heart rate

| | |
|---|---|
| Input | `features` `(1, 50)` |
| Output | `energy` `(1, 1)` |
| Parameters | 199,653 |

The heart-rate-available branch of energy expenditure. A no-heart-rate sibling exists in training
and is not converted, because the branch selection belongs in Rust and only one branch has a
contract so far.

### Daily health

#### `illness_detection` — illness likelihood from daily deviations

| | |
|---|---|
| Inputs | `scalars` `(1, 4)`; `time_series` `(1, 8, 30)` — 8 daily biometrics × 30 days |
| Output | `illness_probability` `(1, 1)` |
| Parameters | 155,911 |

Two convolutional layers over the thirty-day history, concatenated with the scalars and with the
series' own most-recent value, mean and standard deviation, then three dense layers and a sigmoid.
The output is already a probability; applying another sigmoid would be wrong.

### The provisional ECG classifier

`nao_full_v2` predates the zoo and keeps its own path, its own contract, and its own admission
gate. It is unchanged by this work.

| Field | Contract |
|---|---|
| Version | `2.0.0` |
| Input | `FLOAT32 [1, 7680, 1]` — exactly 30 s, resampled to 256 Hz |
| Fit | centre crop or zero pad to 7,680 samples |
| Normalisation | z-score per record, population variance, `1e-9` floor |
| Output | three values in class order `N`, `A`, `O` |
| Meaning | sinus rhythm, atrial fibrillation, other abnormal rhythm |
| Confidence | the model's own posterior for the winning class; model certainty only |
| Algorithm | `nao_full_v2_ecg_classifier` 2.0.0 |

Preprocessing is byte-frozen: convert to millivolts; resample to 256 Hz at source positions
`i / 256 * source_rate`; apply the unpadded forward/reverse SOS chain (band-pass, 50 Hz notch,
60 Hz notch); apply the 0.5 Hz one-pole high-pass fallback only when the filtered mean magnitude
exceeds `0.02` or the standard deviation is below `0.0001`; fit to 7,680 samples; z-score.

The model does not separate class `O`. Its output tensor has three values, so tachycardia,
bradycardia, bigeminy and other non-`N`/non-`A` morphologies cannot be relabelled as diagnoses;
fixtures with those shapes exercise the broad bucket only. Its validation ceiling is conversion
consistency across nine deterministic fixtures, three per expected class — not sensitivity,
specificity, predictive value, or diagnostic fitness.

## From a converted model to a product

Conversion answers "does this graph run and match its reference". It does not answer "can this
app feed it", and for most of the zoo the answer to the second question is no. The two were
conflated for as long as nothing called the models: forty-one artefacts sat in both bundles with
`MavModelBridge` referenced only by its own tests, and no code anywhere deciding what to run.

`mav_analytic::model_zoo::pipeline` is where the second question is now answered, once, for both
platforms. Each of the forty-one models declares the streams its preprocessing reads, the wearer
profile fields its input vector carries, the models whose outputs are its inputs, the product
signal it feeds, and — the field that matters most — whether this build can assemble its input
tensors at all.

### Three front-ends are ported, and that is what decides the count

Every model's training archive carried its own feature assembly: windowing, filtering, the
construction of a 77-column segment vector out of MET, motion, step-motion, heart rate and
temperature. That code is deliberately not converted, for the reason this document already gives
— data-dependent behaviour belongs in shared, fixture-tested Rust, not in an opaque graph. Three
of those front-ends are ported (`pulse_ppg_input`, `pulsenet_input`, `cva_pulse`). The rest are
not, and the archives they would be ported from are not in this repository.

So a model can convert perfectly, load, run, and match its reference on stored vectors, and still
have nothing in this build able to fill its input from a wearer's samples. `FrontEnd::NotPorted`
records exactly that, with the missing piece named, and the planner reports it as its own
unavailable reason ahead of any missing sensor — because it does not change when the wearer buys
a different strap, and telling someone to go and buy an SpO2 strap for a model that could not run
either way would be a lie of omission.

What that leaves, on a strap that reports an optical signal and a filled-in profile:

| | models | why |
| --- | --- | --- |
| Runnable end to end | **6** | three ported PPG front-ends, and three heads whose whole input is one of those encoders' outputs |
| Fed by an upstream model | 11 | runnable exactly when their upstream is; eight of them wait on a root whose front-end is not ported |
| Front-end not ported | 27 | the feature assembly lived in the training wrapper |

The six are `pulse_ppg`, `pulsenet_foundation`, `cva_encoder`, `halite_ppg_score`,
`cva_probes_male` and `cva_probes_female`. Porting a front-end should make that list longer;
nothing else should, and `tools/ml/check_claims.py` fails if these counts drift from
`pipeline.rs`.

### Running a model and reading it are different permissions

`Interpretation` carries the second one. Two vocabularies in this build are withheld, and both
would otherwise render as the thing their tensor name suggests: sleep staging, whose four logits
per epoch have never been mapped onto Maverick's stages, and the hypertension risk level, which
is post-processing the converted tree does not contain. Both models run. Neither may be drawn as
a reading, and the surfaces say "computed — no reading to show yet" rather than showing a number
or an empty card.

### Who decides what runs

`mav_engine::analytics` holds the scheduler, because two copies of a dependency order is two
chances to run `halite_risk_tree` before the score it consumes exists. It topologically ranks the
graph, reports availability with the sensor or profile field named, remembers every completed
inference against a fingerprint of its inputs *and* the artefact hash that produced it — so a
re-conversion invalidates the cache without anyone remembering to clear it — and chains an
encoder's heads itself when it completes, including picking the `cva_probes` branch the wearer's
sex selects.

What stays on the platforms is only what they alone know: whether the app is foregrounded,
whether the OS will grant a background window, and how many threads the accelerator should get.

## Preprocessing

`mav_analytic::model_zoo::ppg` holds the ported front-ends, each with its own constants:

| Front-end | Steps |
|---|---|
| `pulsenet_input` | jump-limit the first difference at 5,000, integrate, subtract a 100-then-150 moving-mean trend, median-3, mean-3. Returns 1,500 samples. |
| `cva_pulse` | the same chain at jump limit 2,000, then SNR, pulse-foot detection over a 60-sample dominance window, min-max normalisation into a 128-value vocabulary. Returns 1,499 samples plus `(mean_dc, max_min, accepted, snr, heart_rate)`. |
| `pulse_ppg_input` | resample to 50 Hz, fit the 12,000-sample window, z-score. |

The two jump limits are not interchangeable; using one front-end's constant in the other changes
which samples survive.

Two deliberate deviations, recorded because they differ from the training pipeline:

- Pulse-PPG's published pipeline z-scores each user against their own multi-day distribution and
  clips at a per-user border. Maverick has no such distribution at inference time, so the window is
  z-scored against itself and not clipped. The encoder's first layer is an `InstanceNorm1d`, which
  renormalises per window regardless.
- `cva_pulse` is implemented and tested but has no model to feed, because `cva_predictor` does not
  convert. It is kept because the preprocessing is the harder half.

## Input health: what a model answered *about*

These weights cannot be retrained. There is no labelled corpus to fit a replacement against, so
every model ships fitted on a cohort and a wear site that may not be the wearer's. That is a
decision, not an oversight, and `mav_analytic::model_zoo::health` is what makes it survivable.

The split that makes it tractable: **accuracy needs labels and there are none; input health needs
nothing.** Whether the numbers entering a graph are readings or substitutions is fully knowable at
inference time and costs almost nothing to record. It separates two failures that look identical
from outside — a model that ran on real data and was somewhat wrong, which is the price of using
it, and a model that ran on zeros and returned a confident number anyway, which is not a reading.

The second is not hypothetical. `cycle_input` rejects a temperature outside `[35.5, 37.5]` °C to
`NaN` and then fills `NaN` with zero, because that is what the archive does. A wearer whose skin
temperature sits outside that band gets a forty-day series of zeros and an ovulation probability
computed from none of their own data.

`InputHealth` carries three things: the fraction of contract positions holding a real reading, the
archive's own validity gate where it defines one, and why anything was substituted. From those it
reports one of four verdicts:

| verdict | meaning |
| --- | --- |
| `sound` | substantially real input, and any gate the archive defines passed |
| `degraded` | some input substituted, or a gate failed. The reading stands, qualified |
| `unfounded` | below a quarter of the input was real. Storable; not a reading |
| `unmeasured` | the core did not build these tensors — the replay and test path |

Three details are deliberate. `out_of_range` and `missing` are reported separately even though both
become zero, because "your readings fall outside the band this model accepts" and "no readings" send
a wearer to different places. A failed gate is not outvoted by a complete input, because the gate
tests a shape the weights were not fitted against and completeness does not answer that. And
`gate_passed` is an `Option`: no gate is not a passing gate, and defaulting it to `true` would
manufacture assurance.

### The gate the archives already gave us

`cva_pulse` sets `accepted` from the min-max normalised pulse — mean within `[52.35, 79.81]` and
standard deviation at least `20.36`. Because min-max normalisation has already removed absolute
amplitude, this is a **shape** gate rather than an amplitude one: a weaker signal does not fail it,
a differently shaped pulse does. That makes it an out-of-distribution detector calibrated by the
people who fitted the encoder, on exactly the axis that matters when a model fitted at one wear site
is fed another. It was being computed and discarded; it now travels with the prepared pulse.

### What the platforms do with it

`StageAdmission` carries the verdict across the FFI, and both apps reduce it to a per-signal state.
`unfounded` is a **separate UI state**, not a flag on `ready`: a surface that matches on `ready` to
draw a number cannot reach it by accident, which is the mistake worth making impossible rather than
merely documenting. A signal fed by several models takes the worst verdict among them, and an
unrecognised verdict name parses as `unmeasured` rather than `sound`, so a newer core cannot make an
older app more trusting than it should be.

This is not a confidence score and must not be rendered as one. It says what went in, not how right
the answer is.

## Artefacts, precision and size

### Where the hashes live

Not in this document. Core ML packaging is not byte-reproducible — two conversions of the same
weights produce different member hashes — so a table in prose would be stale the first time anyone
re-converts, and nothing mechanical could catch it. `artifacts/models/manifest.json` carries them,
and three things enforce it:

- `tools/ml/generate_bindings.py --check` fails if the Rust, Swift or Kotlin registry no longer
  matches the manifest.
- `tools/check_model_assets.py` fails if either bundle holds a model the manifest does not admit,
  is missing one it does, or ships one whose bytes hash differently.
- At run time the platform reports the hash it loaded and `validate_response` refuses an unknown
  one.

The platforms differ in how much the running app can verify. On Android the shipped bytes are
exactly the bytes the manifest recorded, so the runner hashes the mapped asset at load and a
swapped file fails at open. On iOS the package is compiled to `.mlmodelc` at build time and cannot
be re-hashed at run time, so the hash is a generated constant and the repository gate is what
proves the package behind it.

### Precision: half width everywhere it can be measured to hold

Every model, both platforms, is **stored at half width**. Most are also **computed** at half width,
and which ones are is decided by measurement rather than by policy, because the two questions have
different answers and only one of them is free.

**Storage is free and universal.** Every constant above 1,024 elements ships as float16 on both
platforms. Core ML writes them itself under a half-precision policy, and through MIL's
`constexpr_cast` under the full-width one, so the bytes on disk do not change when a model's
arithmetic does. TensorFlow Lite gets there by rewriting the finished flatbuffer into the form its
own float16 quantisation produces — FLOAT16 constants with a `DEQUANTIZE` before each kernel —
because LiteRT's torch converter has no float16 recipe, a whole-graph half cast fails to legalise
`tfl.pad`, and halving inside the exported graph gets folded away.

The 1,024-element threshold is the same on both platforms, and small constants — biases, batch-norm
scales — stay full width on both. They cost almost nothing to store and their rounding reaches every
output: raising the threshold from 16 to 1,024 on the PulseNet encoder costs 19 kB and improves
parity from 6.6e-3 to 1.9e-3.

**Arithmetic is not free, and full-width arithmetic is not neutral.** The Neural Engine is
half-precision hardware. A program that asks for float32 arithmetic is not slightly slower on it —
it does not run on it at all. `tools/ml/compute_plan.py` reads Core ML's own `MLComputePlan` and
reports the device it assigns each operation to, and under the full-width policy the answer was the
same for every model in the zoo: **zero operations on the Neural Engine**, everything on the CPU or
the GPU. "Float32 for safety" was really "no accelerator at all", and nothing in the parity numbers
said so, because parity does not measure where the work ran.

So `coreml_precision.py` runs a ladder, most accelerator first, and each model takes the first rung
that clears its parity bar *and* actually leaves work on the accelerator:

| policy | arithmetic | what it is for |
| --- | --- | --- |
| `half` | every operation at half width | the default; what most models get |
| `half_pooled` | pooling reductions at full width | the narrowest rung: a global mean over thousands of samples |
| `half_reduced` | every accumulation at full width | reductions and the normalisations over them |
| `half_stable` | the above plus transcendentals | `rsqrt` and `exp` near zero |
| `full` | no half-precision arithmetic | last resort, and no Neural Engine |

`coreml_precision.choose` then picks from what was measured, and the rule has two branches because
the trade is not the same in both:

* **Where a policy clears the bar and reaches the accelerator**, the earliest such rung wins — the
  one leaving most of the graph at half width. That is a model genuinely running float16 at
  runtime, which is the whole point.
* **Where no policy reaches the accelerator at any precision**, half width buys nothing on iOS that
  full width does not, and it costs twice: accuracy against the reference, and agreement with
  Android, whose catalogue flag follows this same decision and would send the model down its own
  float32 path either way. The most accurate passing policy ships instead.

Both branches are recorded per model with every rung's measurement, because "this model is float16"
and "this model is float16 *at runtime*" are different claims and only the second is worth anything.

Exempting operations sometimes partitions cleanly and sometimes does not, and nothing in the op list
predicts which — so the plan decides. On the PulseNet encoder the exemption is decisive and
instructive: `half` sits **4.2e-1** from its reference with 99.1% of its operations on the Neural
Engine, and the cause is one operation. Exempting only the pooling reductions — `half_pooled`, the
narrowest rung — brings it to 4.2e-2, exactly what exempting every accumulation achieves, which
identifies the squeeze-and-excite global mean as the sole culprit: a mean over 1,500 samples per
channel is 1,500 terms summed into a format with three decimal digits. But *any* exemption drops the
model to zero Neural Engine operations, because Core ML will not partition a graph with mixed
precision. So the exception cannot be contained, the encoder ships at full width, and its 3.8e-3 is
the price. That is the documented exception, with the operation named and the reason measured.

The same clause explains most of the models that end up at full width, and the compute plan is what
makes the difference visible. For nearly all of them the Neural Engine was never on offer at any
precision, and the backend does not change with the arithmetic either — `cva_encoder` and
`activity_detection` run on the GPU under every policy, `step_eligibility` on the CPU under every
policy. Full width costs those models nothing and buys back an order of magnitude of accuracy.

Four models genuinely trade the accelerator away, and each is recorded rather than decided
globally. In every one of them the units the model actually speaks are what settles it:

- `whr_unet_head` — 99.9% accelerated at `half`, and **3.86 bpm** from its reference there against
  0.20 bpm at full width. It shipped accelerated until the probe count went from five to eight; see
  [what the phone changed about the precision ladder](#and-what-the-phone-changed-about-the-precision-ladder).
- `pulsenet_foundation` — 99.1% accelerated at `half`, and 4.2e-1 from its reference there against
  3.8e-3 at full width. The encoder's depth does not survive half-width accumulation.
- `awhr_imputation` — every operation accelerated at `half`, and it misses the bar at 1.01e-2
  against 1.00e-2. That is 2.2 bpm on an imputed heart rate against 0.02 bpm at full width, which
  is worth more than the acceleration.

  This is the one model where no narrower rung exists to try, and the graph says so outright. It is
  a two-layer bidirectional LSTM unrolled over sixty timesteps — 720 `sigmoid`, 480 `tanh`, 240
  `split`, two `reverse` — and not one of the nineteen operation types the mixed policies exempt
  appears anywhere in it:

  ```
  half_pooled    exempts  2 op types; present in this graph: NONE
  half_reduced   exempts 11 op types; present in this graph: NONE
  half_stable    exempts 19 op types; present in this graph: NONE
  ```

  All three therefore compile to a program byte-identical to `half`, which is why all three measure
  identically to it at 1.01e-2 and 100% accelerated. There is no reduction, normalisation or
  transcendental to blame: the error is the recurrent state path itself, sixty sequential
  multiply-accumulate updates with nothing renormalising between them. Exempting *that* is
  exempting the network, which is the policy already named `full`.

- `activity_transition` — 51% accelerated at `half` and 5.10e-3 there, which cleared the bar and
  did not clear it by enough. Independent probes put the same artefact at 1.10e-2. At full width it
  is 6.52e-4, and the two platforms agree to 5.96e-7 rather than 3.33e-3.

Half-precision arithmetic is genuinely not free where it does apply. The Neural Engine accumulates
at half width where Core ML's CPU path accumulates at full width, so a deep encoder reads further
from its float32 reference on the accelerated path than on the CPU — the PulseNet encoder sits
4.2e-2 away on the CPU and 4.2e-1 on the Neural Engine at plain `half`, which is why it does not
get `half`. Parity is measured through `ComputeUnit.ALL` for exactly this reason: the accelerated
path is the one the app runs.

**Android used to follow the same decision, and that was wrong.** There is no runtime equivalent of
`MLComputePlan` there, so the catalogue carried the Core ML answer across: any model admitted at a
half-precision policy was offered the GPU delegate, on the reasoning that half width on one
platform should be half width on the other.

A sweep of all forty-one on a Pixel 7 showed it is not the same half width — Apple's Neural Engine
accumulates a half-precision matmul into a wider register and the delegate accumulates in half
width, which cost Pulse-PPG 2.7e-2 against Core ML's 3.9e-3 from identical weights — that the
delegate is wrong outright on four graphs at *either* width, and that it is slower than the CPU on
all but one of them. The path is now measured per model instead of inferred: the CPU is the
default, one model keeps the GPU at full width, and NNAPI is gone. [What the phone changed about
all of this](#what-the-phone-changed-about-all-of-this) has the numbers.

**Compute units stay `ALL`, and `ALL` is a permission rather than a placement.** It tells Core ML
which processors it *may* use; Core ML's compiler then assigns each operation independently, and
what it assigns depends on the program's precision and op set rather than on the request. The two
claims come apart sharply here: all 41 models are admitted on `ALL`, and **30 of them get zero
Neural Engine operations**, because full-width arithmetic cannot run on half-precision hardware.
Across the zoo only **547 operations of 31,090 — 1.8% — land on the ANE**, and `awhr_imputation`
under `ALL` reports GPU 4,099 and unassigned 11 with no ANE at all. The per-model column in the
table above is read from `MLComputePlan`, Core ML's own answer, not inferred from the setting.
(`unassigned` means Core ML deferred the choice to run time rather than committing ahead of it.)

Narrowing them is a real cost — a model barred from the GPU cannot use
it even when it is the only accelerator free — so it takes a failed measurement, not a suspicion.
The three sleep models were pinned to `cpuAndNeuralEngine` because Core ML's GPU backend computed
them wrong by more than one whole relative unit; `tools/ml/gpu_bisect.py` cut the program at each
operation in turn and found the cause at the first `upsample_nearest_neighbor`, a nearest-neighbour
resize with a scale of ½ on one axis. The repair for it already existed for TensorFlow Lite and was
being silently discarded on the Core ML path, because `ExportedProgram.module()` returns a *new*
graph on every call and the rewrite was applied to one call's module and re-exported from another's.
With that fixed, all three are exact on every backend and admitted on `ALL`.

### What each model actually runs at

Storage precision, arithmetic precision, processor assignment and Android path are four
different claims, and a model can satisfy the first without any of the others. This table is
generated by `tools/ml/build_precision_ledger.py` from the manifest, Core ML's own compute
plan, and a measurement of both artefacts, so none of them can be asserted from the others.

The two error columns are the split that matters. **Graph** is Core ML on the CPU against
TensorFlow Lite on the CPU — both at full-width arithmetic, so anything here is the two
graphs or the two sets of weights disagreeing, and all of it is a defect. **Arith** is the
shipped Core ML artefact against its own CPU answer: the cost of half-precision arithmetic,
which is the policy working rather than failing.

The last two columns did not come off this host at all. **Device vs ref** is a Pixel 7
running the shipped flatbuffer against eager PyTorch, and **between platforms** is that same
handset against Core ML on identical inputs — the cross-platform claim measured rather than
composed from two host numbers. `tools/ml/device_compare.py` produces them from tensors the
instrumented test writes out; see [what the phone changed](#what-the-phone-changed-about-all-of-this).

<!-- PRECISION-TABLE -->

| Model | Weights | Arithmetic | Neural Engine | Android path | Graph err | Accel err | Device vs ref | Between platforms |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `activity_context_embedding` | float16 | float16 | 35/36 | xnnpack (float32) | n/a | 6.7e-03 | 3.7e-04 | 2.5e-03 |
| `activity_detection` | float16 | float32 | 0/213 | xnnpack (float32) | 1.2e-04 | 1.9e-07 | 1.4e-03 | 1.6e-04 |
| `activity_ensemble` | float16 | float16 | 24/26 | xnnpack (float32) | n/a | 5.7e-03 | 2.3e-04 | 4.8e-04 |
| `activity_history_transformer` | float16 | float16 | 49/50 | xnnpack (float32) | n/a | 5.5e-03 | 4.6e-04 | 2.5e-03 |
| `activity_primary_segments` | float16 | float32 | 0/636 | xnnpack (float32) | 1.1e-06 | 0.0e+00 | 1.5e-03 | 1.3e-06 |
| `activity_secondary_segments` | float16 | float16 | 26/27 | xnnpack (float32) | n/a | 6.8e-03 | 3.7e-04 | 2.5e-03 |
| `activity_transition` | float16 | float32 | 0/41 | xnnpack (float32) | 5.7e-07 | 0.0e+00 | 5.3e-04 | 7.8e-07 |
| `awhr_imputation` | float16 | float32 | 0/4110 | xnnpack (float32) | 2.9e-07 | 2.9e-07 | 1.3e-04 | 3.0e-07 |
| `awhr_profile_core` | float16 | float32 | 0/322 | xnnpack (float32) | 1.9e-04 | 0.0e+00 | 3.9e-04 | 1.9e-04 |
| `awhr_profile_head` | float16 | float32 | 0/1 | xnnpack (float32) | 1.5e-07 | 0.0e+00 | 2.2e-07 | 2.2e-07 |
| `awhr_profile_recurrent` | float16 | float32 | 0/2045 | xnnpack (float32) | 8.3e-07 | 0.0e+00 | 3.0e-03 | 1.6e-06 |
| `behavior_embedding` | float16 | float16 | 0/7 | xnnpack (float32) | n/a | 0.0e+00 | 3.2e-04 | 0.0e+00 |
| `cva_encoder` | float16 | float32 | 0/464 | xnnpack (float32) | 5.9e-04 | 1.0e-06 | 1.5e-03 | 6.0e-04 |
| `cva_predictor_v1_base` | float16 | float32 | 0/41 | xnnpack (float32) | 1.2e-04 | 0.0e+00 | 5.3e-04 | 8.2e-04 |
| `cva_probes_female` | float16 | float32 | 0/47 | xnnpack (float32) | 3.0e-07 | 0.0e+00 | 1.4e-06 | 1.3e-06 |
| `cva_probes_male` | float16 | float32 | 0/48 | xnnpack (float32) | 3.8e-07 | 0.0e+00 | 1.2e-04 | 4.6e-07 |
| `dhrv_imputation` | float16 | float32 | 0/9 | xnnpack (float32) | 7.5e-08 | 0.0e+00 | 6.4e-05 | 0.0e+00 |
| `energy_expenditure_hr` | float16 | float32 | 0/26 | xnnpack (float32) | 4.3e-04 | 0.0e+00 | 9.2e-04 | 4.7e-04 |
| `energy_expenditure_no_hr` | float16 | float32 | 0/26 | xnnpack (float32) | 3.7e-04 | 0.0e+00 | 6.0e-04 | 1.4e-04 |
| `halite_ppg_score` | float16 | float32 | 0/2 | xnnpack (float32) | 0.0e+00 | 0.0e+00 | 1.3e-06 | 2.2e-06 |
| `halite_risk_tree` | float16 | float32 | 0/78 | xnnpack (float32) | 8.2e-09 | 0.0e+00 | 3.4e-05 | 1.8e-08 |
| `illness_detection` | float16 | float32 | 0/35 | xnnpack (float32) | 3.6e-03 | 0.0e+00 | 5.4e-03 | 2.6e-03 |
| `popsicle_min_follicular` | float16 | float32 | 0/5 | xnnpack (float32) | 1.1e-07 | 0.0e+00 | 1.1e-07 | 1.1e-07 |
| `popsicle_min_follicular_v16` | float16 | float32 | 0/5 | xnnpack (float32) | 1.1e-07 | 0.0e+00 | 1.1e-07 | 1.1e-07 |
| `popsicle_ovulation_detection` | float16 | float32 | 0/2056 | xnnpack (float32) | 6.6e-07 | 0.0e+00 | 6.6e-04 | 5.9e-07 |
| `popsicle_ovulation_detection_v16` | float16 | float32 | 0/2056 | xnnpack (float32) | 1.8e-07 | 0.0e+00 | 5.8e-05 | 3.9e-07 |
| `popsicle_ovulation_prediction` | float16 | float32 | 0/2055 | xnnpack (float32) | 2.8e-07 | 0.0e+00 | 5.0e-05 | 2.8e-07 |
| `popsicle_ovulation_prediction_v16` | float16 | float32 | 0/2055 | xnnpack (float32) | 3.3e-07 | 0.0e+00 | 2.1e-04 | 4.8e-07 |
| `popsicle_period_prediction` | float16 | float32 | 0/2055 | xnnpack (float32) | 3.0e-07 | 0.0e+00 | 7.5e-05 | 3.5e-07 |
| `popsicle_period_prediction_v16` | float16 | float32 | 0/2055 | xnnpack (float32) | 5.1e-07 | 0.0e+00 | 1.8e-04 | 5.9e-07 |
| `pulse_ppg` | float16 | float16 | 131/132 | xnnpack (float32) | n/a | 6.3e-03 | 1.5e-03 | 2.4e-03 |
| `pulsenet_foundation` | float16 | float32 | 0/145 | xnnpack (float32) | 4.0e-04 | 8.8e-07 | 3.1e-03 | 3.9e-04 |
| `sleepnet_bdi` | float16 | float16 | 99/182 | xnnpack (float32) | n/a | 7.7e-03 | 8.4e-04 | 6.2e-03 |
| `sleepnet_bdi_v3` | float16 | float16 | 44/182 | xnnpack (float32) | n/a | 6.7e-03 | 6.5e-04 | 2.4e-03 |
| `sleepnet_moonstone` | float16 | float16 | 108/185 | xnnpack (float32) | n/a | 8.3e-03 | 5.8e-04 | 4.0e-03 |
| `source_embedding` | float16 | float16 | 0/7 | xnnpack (float32) | n/a | 0.0e+00 | 0.0e+00 | 0.0e+00 |
| `step_eligibility` | float16 | float32 | 0/344 | xnnpack (float32) | 1.4e-06 | 0.0e+00 | 2.3e-04 | 4.1e-04 |
| `step_head` | float16 | float32 | 0/2 | xnnpack (float32) | 1.9e-06 | 0.0e+00 | 1.9e-06 | 6.8e-08 |
| `step_multiplier` | float16 | float32 | 0/6 | xnnpack (float32) | 0.0e+00 | 0.0e+00 | 0.0e+00 | 0.0e+00 |
| `whr_unet_encoder` | float16 | float16 | 31/32 | gpu delegate (float32) | n/a | 3.9e-03 | 3.4e-04 | 7.1e-04 |
| `whr_unet_head` | float16 | float32 | 0/9241 | xnnpack (float32) | 1.8e-07 | 1.2e-07 | 1.8e-04 | 3.1e-07 |

11 of 41 models compute at half width; 9 place work on the Neural Engine, 547 operations of 31,090. Graph-only disagreement is separable for the 30 models that kept full-width arithmetic and is worst at 3.65e-03 there; worst as-shipped across all 41 is 4.14e-03.

The last two columns came off a Pixel 7 (Tensor G2, API 37, arm64-v8a) rather than this host: 41 models run on the handset against probes neither converter chose, worst 5.38e-03 from the reference and worst 6.23e-03 between the two platforms.

<!-- /PRECISION-TABLE -->

### Parity

Parity is the worst deviation from the float32 PyTorch reference across every output and **every
probe**. Each model is measured on eight: a synthesised pulse waveform for inputs that carry a
waveform, seeded Gaussian noise for the rest, small whole numbers for index inputs. The last column
compares the two shipped artefacts to each other on identical tensors.

Eight probes rather than one, because one measures a model at a single point of its input space and
that proved too few — three artefacts passed a single-probe gate and then disagreed across platforms
by up to 6e-3 on inputs the pipeline had never tried. Three was not enough either:
`activity_detection`, whose outputs mix probabilities, a 256-value embedding and day-minute
positions on very different scales, passed at three probes and missed at five. Re-measured, the
policy promoted each of them to full precision.

Five was not enough either, and this time the evidence came off a phone. Run against three probes
the pipeline had never tried, `activity_transition` sat 1.10e-2 from its reference — past the 1e-2
bar its half-width policy had been admitted under — with four more models between 6e-3 and 1e-2.

Worse, for a whole class of model those five probes had always been one. `pulse_probe` takes a
heart rate and ignores the seed, so every model whose input is a waveform was measured five times
at 68 bpm and reported the worst of five identical answers. Each probe index now draws its
waveform at a different rate, which is what makes the count mean anything for the PPG front ends.

Probe sensitivity does not disappear with more probes; it only gets cheaper to detect. That is why
`verify_shipped.py` uses its own seeds and its own code path, and why `device_vectors.py` uses
seeds and rates that appear nowhere in this list — a check that shares nothing with the gate is the
one that can still catch it.

Relative error also needs reading with care where an output sits near zero. `step_head` differs
between platforms by 2.3e-8 in absolute terms, which is 2.8e-2 relative — a division by a tiny
scale, not a defect. The independent verification treats a model as aligned when *either* measure
is small; the pipeline's own gate is relative-only, which is the stricter of the two.

The last column here is the two shipped artefacts as the apps will run them, which for a
half-precision model compares Core ML's float16 answer against a full-width interpreter and so
carries the arithmetic width inside it. [The precision ledger](#what-each-model-actually-runs-at)
splits that number into the part the graphs disagree by — which is a defect, and is what the
normalisation fold was for — and the part the arithmetic accounts for, which is not.

<!-- PARITY-TABLE -->

| Model | iOS bytes | Core ML rel. err | Android bytes | TFLite rel. err | Platforms agree to |
|---|---|---|---|---|---|
| `pulse_ppg` | 57,174,572 | 4.11e-03 | 57,155,796 | 1.50e-03 | 1.19e-02 |
| `pulsenet_foundation` | 1,914,925 | 3.76e-03 | 1,843,400 | 3.47e-03 | 3.96e-04 |
| `halite_ppg_score` | 3,510 | 7.18e-06 | 2,128 | 9.90e-07 | 8.25e-08 |
| `halite_risk_tree` | 88,035 | 1.36e-04 | 107,076 | 1.35e-04 | 1.72e-08 |
| `sleepnet_moonstone` | 3,663,701 | 4.70e-03 | 2,295,128 | 6.81e-04 | 3.76e-03 |
| `sleepnet_bdi` | 3,526,442 | 4.86e-03 | 2,142,936 | 8.40e-04 | 4.91e-03 |
| `sleepnet_bdi_v3` | 2,093,281 | 2.65e-03 | 705,736 | 6.25e-04 | 1.62e-03 |
| `awhr_imputation` | 2,283,024 | 9.17e-05 | 972,736 | 9.19e-05 | 1.70e-07 |
| `dhrv_imputation` | 16,918 | 1.10e-03 | 12,400 | 1.10e-03 | 2.86e-07 |
| `activity_detection` | 2,832,526 | 8.43e-04 | 2,720,808 | 8.72e-04 | 1.27e-04 |
| `activity_transition` | 223,838 | 6.52e-04 | 196,272 | 6.51e-04 | 5.96e-07 |
| `energy_expenditure_hr` | 433,985 | 1.43e-03 | 403,616 | 2.57e-03 | 1.88e-04 |
| `illness_detection` | 357,134 | 1.66e-02 | 320,728 | 1.83e-02 | 3.66e-03 |
| `activity_context_embedding` | 1,643,711 | 3.11e-03 | 1,627,680 | 4.62e-04 | 2.12e-03 |
| `activity_ensemble` | 1,094,679 | 7.22e-04 | 1,081,584 | 3.58e-04 | 5.05e-04 |
| `activity_history_transformer` | 1,920,449 | 2.25e-03 | 1,898,440 | 5.19e-04 | 1.86e-03 |
| `activity_primary_segments` | 5,514,206 | 1.76e-03 | 1,356,696 | 1.76e-03 | 1.37e-06 |
| `activity_secondary_segments` | 1,106,889 | 2.63e-03 | 1,091,972 | 3.90e-04 | 2.47e-03 |
| `awhr_profile_core` | 295,365 | 3.91e-04 | 52,064 | 4.97e-04 | 1.72e-04 |
| `awhr_profile_head` | 2,702 | 0.00e+00 | 1,364 | 2.46e-07 | 1.52e-07 |
| `awhr_profile_recurrent` | 924,534 | 5.74e-03 | 282,960 | 5.74e-03 | 1.61e-06 |
| `behavior_embedding` | 51,718 | 3.38e-04 | 47,132 | 3.38e-04 | 0.00e+00 |
| `cva_encoder` | 3,719,258 | 4.66e-03 | 3,502,984 | 2.81e-03 | 5.43e-04 |
| `cva_predictor_v1_base` | 690,450 | 8.38e-04 | 644,124 | 5.88e-04 | 1.13e-04 |
| `cva_probes_female` | 37,748 | 1.81e-07 | 16,780 | 1.37e-06 | 9.86e-08 |
| `cva_probes_male` | 36,259 | 1.25e-04 | 14,984 | 1.25e-04 | 7.02e-08 |
| `energy_expenditure_no_hr` | 271,937 | 5.28e-04 | 241,784 | 4.97e-04 | 3.20e-04 |
| `popsicle_min_follicular` | 8,257 | 1.28e-07 | 5,116 | 1.92e-07 | 1.38e-07 |
| `popsicle_min_follicular_v16` | 8,257 | 1.28e-07 | 5,116 | 1.92e-07 | 1.38e-07 |
| `popsicle_ovulation_detection` | 1,102,638 | 6.76e-04 | 450,252 | 6.76e-04 | 4.29e-07 |
| `popsicle_ovulation_detection_v16` | 1,102,638 | 1.65e-04 | 450,252 | 1.65e-04 | 2.98e-07 |
| `popsicle_ovulation_prediction` | 1,102,319 | 5.61e-05 | 449,868 | 5.62e-05 | 2.27e-07 |
| `popsicle_ovulation_prediction_v16` | 1,102,319 | 4.10e-04 | 449,868 | 4.10e-04 | 3.52e-07 |
| `popsicle_period_prediction` | 1,102,319 | 9.24e-05 | 449,868 | 9.22e-05 | 2.86e-07 |
| `popsicle_period_prediction_v16` | 1,102,319 | 4.88e-04 | 449,868 | 4.87e-04 | 3.27e-07 |
| `source_embedding` | 6,336 | 0.00e+00 | 1,792 | 0.00e+00 | 0.00e+00 |
| `step_eligibility` | 274,948 | 4.18e-04 | 56,016 | 1.47e-03 | 4.06e-06 |
| `step_head` | 2,380 | 5.58e-07 | 1,120 | 4.34e-07 | 0.00e+00 |
| `step_multiplier` | 4,238 | 0.00e+00 | 1,816 | 1.67e-07 | 0.00e+00 |
| `whr_unet_encoder` | 1,030,015 | 9.31e-04 | 978,248 | 4.75e-04 | 7.02e-04 |
| `whr_unet_head` | 5,461,907 | 1.64e-03 | 2,355,020 | 1.64e-03 | 1.14e-07 |

<!-- PARITY-TABLE -->

Two thresholds gate admission, in `tools/ml/build_manifest.py`, and the second is the one that
matters. Deviation from the float32 PyTorch reference is bounded at 3e-2 and *recorded*: half-width
storage is the point, not a defect. What is gated tightly is whether the two platforms agree with
each other — within 5e-3 for a full-width model, and 3e-2 where Core ML computes at half width and
this table's interpreter does not, so the number carries the arithmetic difference inside it. A
shared Rust core reading a platform-dependent number is the failure this boundary exists to
prevent; matching an fp32 reference exactly is not.

The column is worst at 1.19e-2, on `pulse_ppg`, and every other model sits under 5e-3. That figure
is Core ML's half-precision encoder against a full-width interpreter across eight pulse rates, and
it is the largest arithmetic-width gap in the zoo rather than a graph disagreement — measured
against the *real* Android runtime on identical inputs, the same pair agree to
[2.35e-3](#what-the-phone-changed-about-all-of-this).

<!-- BUNDLE-LINE -->

Bundle cost: **105.3 MB on iOS, 86.8 MB on Android**, across 41 models.

<!-- BUNDLE-LINE -->

### What the phone changed about all of this

Every number above this line was, for a long time, measured on a Mac. The conversion pipeline runs
LiteRT's own interpreter over the shipped flatbuffer and reports how far it lands from PyTorch,
which is a true statement about the *file* and no statement at all about the *handset*: Android
picks its delegate on the device, from whatever driver that device ships, and the delegate decides
the arithmetic width. None of that exists here.

Running all forty-one on a Pixel 7 (Tensor G2, API 37, arm64-v8a) found four things, and the first
one meant the models could not run at all.

**An int64 output could not be read.** `halite_risk_tree` returns its chosen class as an INT64
`label` beside its float probabilities. `MavModelRunner` sized every output buffer at four bytes an
element, so the interpreter refused the copy — `Cannot copy from a TensorFlowLite tensor with 8
bytes to a Java Buffer with 4 bytes`. The input side had always asked the tensor for its type; the
output side assumed. iOS was never affected, because Core ML hands back an `NSNumber`. No host test
could have caught it: it needs the interpreter to actually run.

**The GPU delegate was making every model it touched worse.** The delegate was offered to any model
Core ML had admitted at half precision, on the reasoning that half width on one platform should be
half width on the other. The handset does support it and does default to it —

```
DEVICE model=Pixel 7 soc=GS201 manufacturer=Google api=37 abis=arm64-v8a
DEVICE gpu_delegate_supported=true
DEVICE gpu_precision_loss_allowed_by_default=true quantized_allowed=true inference_preference=0
```

— but it is not the same half width. Apple's Neural Engine accumulates a
half-precision matmul into a wider register and this delegate accumulates in half width, so from
identical weights Pulse-PPG lands 3.9e-3 from its reference under Core ML and **2.7e-2** under the
delegate. `activity_context_embedding` went from 3.7e-4 on the CPU to 1.2e-2 on the GPU.

**On four graphs it was not precision at all.** `activity_detection` comes back 7.2e-1 away and
`cva_encoder` 1.4e+1 away at *either* width; `step_head` returns a whole relative unit at half
width and 2.0e-6 at full. Those are wrong answers, not imprecise ones. Three of the four were
already on the CPU for unrelated reasons, which is the only thing that kept them correct.

**And it was slower.** Dispatching to the GPU costs a few hundred microseconds, and all but one of
these graphs are too small to earn it back. `awhr_imputation` takes 3.7 ms on the CPU and 40 ms on
the GPU; the six `popsicle` models take 1.4 ms and 30 ms.

So the delegate is no longer inferred from the other platform. `DelegateSweepInstrumentedTest` runs
every model on the CPU, on the GPU at half width and on the GPU with precision loss refused, and
`tools/ml/android_delegate.py` writes the result to `artifacts/models/android_delegate.json`, which
the binding generator reads. The rule is accuracy first: a model leaves the CPU only where the
accelerator was measured **both** at least twice as fast and no less accurate. Exactly one does —
`whr_unet_encoder`, 61.78 ms against 28.85 ms — and it takes the GPU at full width, because it was
the arithmetic width and not the hardware that had been costing it.

Twice as fast, rather than merely faster, because a single timing run is not trustworthy to a hair.
`activity_context_embedding` measured 10.34 ms and 11.83 ms on the CPU in two sweeps taken straight
after a long parity pass, and 2.13 ms in one taken on a cool device — a five-fold spread in the
*baseline*, while its GPU time held at 5.1–5.5 ms throughout. Under a 20% margin that model flipped
in and out of the accelerator between runs on nothing but thermal state. Under a 2x margin it is
refused every time, which is the right answer: on an unloaded phone its CPU path is the faster one.

NNAPI was removed rather than left unused. It was deprecated in Android 15, no device here could
measure it, and an accelerator with no measurement behind it is the exact thing this sweep exists
to stop.

What it bought, on identical probes neither converter chose:

| | before | after |
|---|---|---|
| worst Android vs PyTorch | 2.70e-2 (`pulse_ppg`) | 5.38e-3 (`illness_detection`, 3.4e-4 absolute) |
| worst between the platforms | 2.65e-2 | 6.23e-3 (`sleepnet_bdi`) |
| `pulse_ppg` on Android | 2.70e-2 | 1.46e-3 |
| device vs this host, same file | up to 1.2e-2 | **5.4e-6** |

That last row is the one that says the delegate is now faithful: whatever the conversion cost, the
phone reproduces the host's answer for the same bytes to within 5.4e-6 on every model, including
both that keep the accelerator. `ModelZooParityInstrumentedTest` asserts it at 1e-4.

What is left between the platforms is Core ML's own half-precision arithmetic, not Android's.

### And what the phone changed about the precision ladder

The device probes did one more thing, and it is the most valuable result here. They exposed that
the ladder's own gate could not see how far a half-width policy really was from its reference, and
fixing the probes caught a model that should never have shipped accelerated.

`whr_unet_head` outputs a heart rate. Measured on five probes it read 9.26e-3 at `half` — inside
the 1e-2 bar — so it shipped there, with 9,229 of its 9,236 operations on the Neural Engine.
Measured on eight, each waveform at a different rate, the same artefact reads:

```
half          rel 3.059e-02 abs 3.856e+00 ANE 0.999
half_pooled   rel 3.059e-02 abs 3.856e+00 ANE 0.999
half_reduced  rel 3.059e-02 abs 3.856e+00 ANE 0.999
half_stable   rel 3.059e-02 abs 3.856e+00 ANE 0.999
full          rel 1.638e-03 abs 1.978e-01 ANE 0.000
```

Three times the bar, and in the units the model actually speaks, **3.9 bpm**. It now ships at full
width at 0.2 bpm. That single correction is why the accelerated operation count in the table above
fell from 9,842 to 547: whr_unet_head was 94% of it, and it had been bought with four beats per
minute.

The gate was also calibrated on its own probes, which is its own problem. Against three probes
whose seeds and pulse rates the ladder never uses, models near the bar measured worse than the
pipeline said — `activity_transition` by 2.16x, `step_head` by 3.30x, `cva_predictor_v1_base` by
1.61x — while the median across the zoo sat at 0.95x. So `coreml_precision.ACCELERATED_BAR` now
holds a policy to half the allowance before it may be *preferred for the accelerator*; the full
bar still governs the fallback, which has to accept something. Two models moved:
`activity_transition` from `half` to `full` (5.10e-3 → 6.52e-4, and 3.33e-3 → 5.96e-7 between
platforms) and `sleepnet_moonstone` from `half` to `half_pooled` (7.11e-3 → 4.70e-3, keeping 108
of 185 operations accelerated).

### What the zoo costs to run, and what that cost was hiding

Everything above is about whether the numbers are right. This is about what producing them costs,
measured on the same Pixel 7 by `ModelBenchmarkInstrumentedTest` — which loads no parity vectors,
settles between models and scales its sample count to each model's own speed, because a timing
taken in the shadow of a fourteen-megabyte parity pass is a timing of the parity pass.

Nothing here changes a weight, a precision or an output. Every claim below was checked by running
the full parity suite afterwards and comparing the device's tensors byte for byte against the ones
it produced before: **41 of 41 identical, twice** — once after the inference changes and again
after eviction was added, so a model that gets closed and rebuilt mid-session answers the same.

**The per-inference overhead was the inference, for half the zoo.** Every call allocated two direct
`ByteBuffer`s per tensor, filled them an element at a time, and scanned the interpreter's tensor
names — twice per output, once to bind and once to read. All of it depends only on the model, so
all of it now happens once, at load, and lives on a bind plan. On a model that answers in twenty
microseconds this was most of the twenty:

| | before | after |
|---|---|---|
| `cva_probes_female` | 0.157 ms | **0.025 ms** (−84%) |
| `awhr_profile_head` | 0.145 ms | **0.030 ms** (−80%) |
| `halite_ppg_score` | 0.031 ms | **0.007 ms** (−78%) |
| `dhrv_imputation` | 0.021 ms | **0.007 ms** (−66%) |
| `step_head` | 0.018 ms | **0.008 ms** (−58%) |
| `whr_unet_head` | 16.90 ms | **14.18 ms** (−16%) |

27 of 41 models improved at p50 and 27 at p90, none regressed at p90. The compute-bound models
barely move — `pulse_ppg` goes 2565.6 ms to 2550.1 ms — which is the expected shape of the result:
this was overhead, and overhead only matters where there was not much else.

`activity_context_embedding` looks like a regression at p50 (2.23 ms to 3.64 ms) and is not. Its
baseline distribution was bimodal — p50 2.230 ms against p90 **5.818** ms — and the p50 happened to
sample the fast mode. Every run since is tightly clustered, and the honest comparison is the tail:
p90 5.818 ms → 3.699 ms, with CPU time per inference 5.33 ms → 4.00 ms.

**Cold load halved, and the reason was not where it looked.** Splitting the cold path into mapping,
hashing and building showed the integrity check is cheap — 88 ms for the whole zoo, 42 ms of it
Pulse-PPG's 57 MB, which is SHA-256 running at about 1.35 GB/s. So the hash stays exactly where it
is; the cost is the interpreter, at 1,338 ms, and 538 ms of that was one delegate compiling GPU
kernels. Those are now serialised to the app cache, keyed by the model's *admitted hash* so a
changed model can never be handed stale kernels:

| | before | after |
|---|---|---|
| `whr_unet_encoder` build | 538 ms every launch | **261 ms** after the first (−51%) |
| total cold load, 41 models | 2100.4 ms | **1034.5 ms** (−50.7%) |

**Threads are not a lever here, and that is a measurement rather than a shrug.** `THREAD_COUNT` was
2 with nothing behind it. Swept across 1, 2, 4 and 8 on the sixteen models heavy enough to care,
the zoo is flat: `pulse_ppg` moves 2553 ms → 2545 ms from one thread to two and is unchanged at
eight, because XNNPACK does not parallelise its 1-D convolutions. Eight threads is actively
harmful — `activity_detection` doubles, 11.1 ms → 22.1 ms, thrashing the four little cores. Two is
within noise of the best everywhere and never the worst, so it stays, now for a reason.

**The interpreter cache was unbounded, and the bundle size was badly misleading about what that
meant.** An 86.8 MB bundle costs **1.05 GB of native heap** to hold all at once, because the
interpreter builds tensor arenas and XNNPACK repacks the float16 weights to float32. The
distribution is long-tailed:

```
pulse_ppg        436 MB   (from a 57 MB asset)
whr_unet_head    165 MB   (from a 2.4 MB asset)
sleepnet_moonstone 98 MB
sleepnet_bdi       69 MB
awhr_imputation    46 MB
everything else  under 26 MB each
```

A process holding that gets killed, taking the model that was loaded to answer a question with it.
The cache is now bounded at 192 MB of *idle* models, least-recently-used first, and never evicts a
model that is mid-inference or the one just asked for — so a single model larger than the whole
budget still runs, it simply does not get to keep company. Loading all forty-one now peaks at
**178 MB of native heap instead of 1.15 GB**, a 84% reduction, and the parity suite still returns
41 of 41 byte-identical tensors through the reloads that causes.

No thermal throttling was observed in any of this: `PowerManager.getCurrentThermalStatus()` stayed
at `THERMAL_STATUS_NONE` throughout, and no model's sustained loop slowed by more than 15% between
its first tenth and its last. Power draw is **not measured** — the phone was on AC at 100%, so
battery current reports the charger rather than the workload.

### Four conversion defects worth remembering

Each was found by running the shipped artefacts rather than by reading the pipeline's own numbers,
and the pipeline reported success through every one of them.

**The parity number did not measure the file.** `tflite_export.py` measured parity by calling the
converter's returned handle, which answers from the source graph. It reported *exact* parity for
every model, including three whose written flatbuffers were wrong. Parity is now measured by
loading the `.tflite` with the interpreter the app uses.

**LiteRT's nearest-neighbour resize disagrees with PyTorch when it downscales.** The sleep models'
residual path calls `interpolate(mode="nearest")` with a scale below one, and the converted graph
picked different source indices — a whole-logit error, not rounding. Upscaling by an integer factor
is exact; downscaling is not. `tflite_export.py` now rewrites those nodes into the `index_select`
with `floor(i * in / out)` that PyTorch actually computes, which is exact because every shape here
is static.

**The repair was applied to a graph that was then thrown away.** The same resize rewrite was in the
Core ML path too, and did nothing: `ExportedProgram.module()` builds a *new* `GraphModule` on every
call, so rewriting one call's module and re-exporting from another's silently discards the change.
The three sleep models shipped still carrying the resize, disagreeing with themselves by 1.5
relative depending on which backend ran them, and the only visible symptom was a narrow compute-unit
admission that looked like a hardware quirk.

**Integer probes hid an embedding's rows.** Integer inputs were filled with a constant — the window
length — which is right for a sequence length and wrong for a table index: it measured one row of an
89-row embedding and called it the model. Integer inputs now come from the same probe file as the
float ones, and a spec that indexes a table declares its row count so the probe walks it.

The lasting change is the third gate. Each artefact matching PyTorch separately was not enough; the
two platforms are now also run against each other, and disagreement blocks the model.

### Folding the normalisation once, before either converter sees it

The largest *avoidable* source of cross-platform disagreement was not the arithmetic at all. Both
converters fold batch normalisation into the convolution before it — the standard inference
optimisation, and neither can be talked out of it — and both then store the folded weights at half
width. Independently. The fold is `w · γ/√(σ² + ε)`; each computes the same formula in a different
order and lands a float32 ulp apart, which is invisible until each rounds *its own* result to
float16, where a pair of values straddling a midpoint round to different half-precision numbers.
One weight in the wrong direction is a 1e-3 relative difference in that layer's output.

The measurement is unambiguous. Holding arithmetic width constant — Core ML on the CPU against
TensorFlow Lite on the CPU, both full width — the twenty-five models with no normalisation layer
agreed to 1e-6 or better, and **every one** of the sixteen with one sat between 2e-4 and 1e-2.

So `fold_norm.py` does the fold once, in the exported graph, before either converter sees it — into
a convolution or a dense layer, whichever feeds the normalisation — and rounds what folding
produces. Afterwards there is no normalisation node left to fold and the
convolution weights are already on the half-precision grid, so both toolchains receive bit-identical
constants. `tflite_export.convertible` now always exports and decomposes rather than handing LiteRT
the raw module, and the Core ML driver prefers the EXIR frontend for the same reason: it is the path
that shares the graph. The TorchScript frontend remains the fallback for the few cores whose
scripted control flow tracing cannot resolve.

What it cannot reach is a normalisation that does not *follow* an affine layer. Pulse-PPG's
ResNet1D is pre-activation — normalise, activate, then convolve — so a ReLU sits between each
normalisation and the next convolution and there is nothing ahead of it to fold into either.

That is not a guess about the architecture. Reading the shipped program back, every one of the
eleven surviving `batch_norm` operations is fed by a residual `add`, and every one of those adds
has two consumers:

```
what feeds each batch_norm, and how many users that producer has:
  <- add          users=2      (× 11)
```

Both halves of that line are disqualifying on their own. An `add` carries no weights, so there is
no affine layer to absorb the scale and shift into; and its output is read twice — once by the
normalisation and once by the skip connection — so folding would silently change the value the
residual path carries. Those eleven stay as scale-and-shift constants each converter derives for
itself.

So the encoder's floor is structural, and measured rather than asserted. On a Pixel 7, against
probes neither converter chose, the shipped encoder sits **1.46e-3** from eager PyTorch — which is
the cost of storing 28.5 M parameters at half width, not a conversion defect — Core ML's
half-precision artefact sits 2.48e-3, and the two platforms agree to 2.35e-3. Full-width weights
would close it and would also double the model from 57 MB to 114 MB, which the bundle cannot pay.
1.46e-3 is the accepted threshold for this model on Android, it clears the zoo's 5e-3 bar without
an exception written for it, and it is the number `ModelZooParityInstrumentedTest` holds it to on
the handset. The encoder keeps 131 of its 132 operations on the Neural Engine on iOS.

### Rounding the weights once, before either converter sees them

Both platforms store large constants at half width, and for a long time each rounded *its own*
constants after conversion. That is not the same thing. By the time a graph reaches a backend the
two converters have folded, fused and transposed different things, so they rounded different numbers
and the platforms computed with weights differing in the last bits of a float16 — 2.4e-3 apart on
the PulseNet encoder, against about 1e-6 when both carried full-width weights.

`tools/ml/fp16_align.py` rounds every parameter and buffer above the threshold through float16 and
back *on the PyTorch module*, before export, on both paths. Every value that will be stored at half
width is then already exactly representable at half width, the later storage passes are lossless,
and both platforms compute against the same numbers. The artefacts are the same size; only the
disagreement goes away. The reference each artefact is scored against is still the unrounded model,
deliberately — what the manifest records as deviation from PyTorch should include the cost of
half-width storage, not hide it by comparing the rounded model against itself.

## The deterministic archives

Twelve archives carry no learned parameters. They are TorchScript because that is what training
emits, but what they hold is arithmetic: threshold tables, weighted averages, medians, scaling
curves. There is no tensor for an accelerator to accelerate, so they are ported to Rust in
`mav_analytic::model_zoo::deterministic` rather than converted. Shipping a weighted average as an
`.mlpackage` would add a platform runtime, an FFI round trip and an artefact hash to guard in
exchange for nothing, and it would move the arithmetic somewhere neither platform can
golden-vector test it.

Each port reproduces the archive's exact tables and exact branch order, tested against vectors
generated by running the archive itself.

### `daytime_stress` — daytime stress from one HRV reading

| | |
|---|---|
| Inputs | daytime HRV value, the wearer's daytime HRV baseline, their night HRV baseline |
| Outputs | intensity, two thresholds, two saturation points, three scaled levels |
| From | `stress_daytime_sensing` 1.1.0 |

`intensity` is the raw difference from baseline. Around it sits a neutral zone whose half-width
steps 2 / 3 / 4 as the night baseline crosses 40 and 75, and beyond that two saturation points read
from sixteen-row tables — so the same millisecond drop is scored differently for someone whose
nights sit at 20 ms and someone at 100 ms.

`scaled_intensity` is in `-1.0 ..= 1.0`, negative for stress. It saturates: `-1.0` means "at or past
the stress saturation point for this wearer", not "maximally stressed". The scale is then
redistributed by two straight lines so 0.4 lands on 0.5, which keeps the mid-range readable without
letting the ends run past one.

### `short_term_baselines` — the baselines the stress path reads

| | |
|---|---|
| Inputs | daily medians for HRV, skin temperature and minimum heart rate; per-night sleep duration, lowest heart rate, highest temperature and average HRV |
| Outputs | three daily baselines, plus a night HRV baseline |
| From | `daily_short_term_baselines` 1.1.0 |

The three daily baselines are Gaussian-weighted averages over the history window, so the newest and
oldest days both count less than the middle. Fewer than five days is refused rather than estimated
from.

The night HRV baseline is a plain median over nights that pass a plausibility filter — at least four
hours asleep, lowest heart rate in 30–200, highest temperature in 28–40 °C, average HRV in 5–150. A
different estimator on purpose: one implausible night should be discarded, not down-weighted. It is
absent when no night qualifies.

Two deviations from the archive, both deliberate and both recorded in the module:

- It computes in `f64` where the archive casts to `f32`. The archive's own answers carry rounding of
  order 1e-6 — a baseline of exactly 46 comes back from it as 46.000004 — so the golden tests compare
  within that noise rather than bit for bit. Matching it exactly would mean reproducing torch's
  reduction order as well as its precision, and would make the port less accurate.
- A missing day is an error, not a skip. The archive multiplies the gap through and raises;
  renormalising over the days that are present would be a quieter but materially different estimator
  from the one the downstream thresholds were chosen against.

### `daily_medians` — the three medians the stress path reads

| | |
| --- | --- |
| Inputs | HRV and its accuracy, minimum heart rate, skin temperature, MET, sleep periods, each with timestamps |
| Outputs | one median each for HRV, minimum heart rate and skin temperature |
| From | `daily_medians` 1.1.0 |

Three exclusions and a median. A sample is dropped when its own HRV accuracy is below 20, when it
falls within a minute *after* a MET reading above 1.8, or when it falls inside a sleep period. The
window is closed at both ends and forward only — a sample a minute *before* the movement is kept —
and that asymmetry is the archive's.

Skin temperature is sampled on its own clock, so its accuracy exclusion is indirect: a
skin-temperature sample is dropped when it lands within a minute after any *HRV* sample whose
accuracy was poor. That is not the same as excluding poor skin-temperature samples, and the two
differ whenever the series are sampled at different rates.

The median here is NumPy's, not torch's — the mean of the middle pair for an even count. The archive
reaches it by averaging `torch.median` of the values with `torch.median` of the values plus their
maximum, which is the same thing; `numpy_median` computes it directly.

### `atlas_trendline` — the weighted trend through a body-composition history

| | |
| --- | --- |
| Inputs | day indices, values, confidences, a window and a metric |
| Outputs | slope with confidence interval, endpoint values, total change, significance |
| From | `atlas_trendline` 1.0.0 |

One weighted least-squares line. Each reading is weighted by `confidence^1.5 / (value·cv)²`, where
the coefficient of variation belongs to the metric: skeletal muscle is measured about three times as
precisely as fat, so a muscle reading of the same confidence carries roughly nine times the weight.

Three conditions make it decline to fit — fewer than three points, a span shorter than the window
demands, or weights summing to zero — and all three return a row of NaNs rather than an error,
because "no trend yet" is an ordinary state for a young history.

The port computes in `f32` because the archive does. A third-of-a-year span reads 358.20001 there
rather than 358.2, and an `f64` port would disagree with the reference in the sixth digit of
everything derived from it.

### The other five ports

| Archive | What it is |
| --- | --- |
| `astd_event_detection` | Sustained stress and recovery events from fifteen-minute bins. A window is four consecutive bins covering 55–65 minutes, with both ends present, at most one gap inside, and at least one bin past the extreme threshold rather than merely borderline. Overlapping windows of the same kind within thirty minutes merge; opposite kinds may never overlap, and the archive raises rather than resolving it. |
| `cva_calibrator` | The offset that makes cardiovascular age comparable across a hardware change, derived from the previous unit's smoothed reading and frozen at the median once fourteen days agree. Plus the per-sex cubic mapping calibrated age to pulse-wave velocity. |
| `steps_motion_decoder` | The other half of the strap's motion-feature encoding: per-column range, bit depth and transform. Amplitudes are encoded through `log10(x+1)` and fractions through `sqrt`, so codes are evenly spaced where the values are not; stride frequency reserves code zero for "no stride", which is not the same as its lowest value. |
| `meal_timing` | The hours a wearer habitually eats in. Meals go into 48 half-hour bins of local time, and the array is extended by twelve so a window across midnight is contiguous rather than split at both ends. |
| `training_stress_score` | How hard the last twelve hours were. A reading's weight halves every hour into the past, intensity is mapped so the hardest minute counts eight times the easiest, and the result is scaled by VO₂max band or — where none is known — by where the resting heart rate falls in its age-and-sex percentile table. |

### The last three, and the largest

| Archive | What it is |
| --- | --- |
| `pregnancy_biometrics` | Four biometrics per gestational day against their expected bands, for 350 days. The baseline is the first fifteen-day window holding eight readings taken without a fever, less the population's own median change over those same days — so a baseline established at week 30 is not read as if it were week 5. |
| `stress_resilience` | Where a fortnight's recovery sits against a curve fitted through its stress. Four bands either side of that curve give levels one to five, and the fraction inside a band gives the granular figure. |
| `cumulative_stress` | Thirty-one nights reduced to nine features, projected by a fitted factor model into a five-factor space, and scored by how much cluster probability falls on the two stressed centres. A Huber M-estimator handles the spiky got-up counts; medians handle the rest. |

`atlas_2_1_0` is here too, and it is the one archive in this section with learned parameters:
twelve linear regressions, sixty coefficients between them. Sixty coefficients across dot
products of at most five terms is not work for an accelerator, so it takes the same route as the
rest — [see below](#what-does-not-ship) for the hardware it is waiting on.

## The ledger: every archive

Thirty archives came out of training, plus the third-party Pulse-PPG checkpoint. This table is
generated by `tools/ml/build_ledger.py` from the conversion contracts and the manifest, so it
cannot drift from what is actually in the bundles. `artifacts/models/ledger.json` carries the same
data per core.

A thirty-first archive, the legacy `sleepstaging_2_6_0` path, is **withdrawn**. It held no learned
parameters of its own — the gradient-boosted classifier it called lived outside the archive — and
that classifier is no longer available, so there is nothing to convert and nothing to port. It is
excluded from the ledger rather than carried as a permanent gap. Sleep staging on Maverick is
`sleepnet_moonstone` and `sleepnet_bdi`.

An archive can be partly done. `whr_2_7_1` ships two cores and has a third that will not convert;
calling that either finished or blocked would be wrong, so it reads `partial`.

<!-- LEDGER-TABLE -->

| Archive | Params | Params shipped | Status | Detail |
|---|---|---|---|---|
| `automatic_activity_detection_3_0_8` | 1,338,910 | 100% | shipped |  |
| `automatic_activity_detection_3_1_11` | 3,560,987 | 100% | shipped | All 3,560,987 parameters ship as 8 cores. The composing parent does not convert: Core ML: inplace_ops pass doesn't yet support append op inside conditional; NotImplementedError: inplace_ops pass doesn't yet support append op inside conditio |
| `awhr_imputation_1_2_0` | 209,665 | 100% | shipped |  |
| `awhr_profile_selector_1_0_1` | 10,843 | 100% | shipped | All 10,843 parameters ship as 3 cores. The composing parent does not convert: TFLite: Array boolean indices must be concrete; got bool[1,60] |
| `cva_1_3_0` | 317,522 | 100% | shipped | All 317,522 parameters ship as 1 cores. The composing parent does not convert: Core ML: no core ml precision policy converted: ValueError: Input 23 has rank 0 != other inputs rank 1; ValueError: Input 23 has rank 0 != other inputs rank 1; |
| `cva_2_1_0` | 1,807,783 | 100% | shipped | All 1,807,783 parameters ship as 3 cores. The composing parent does not convert: Core ML: no core ml precision policy converted: ValueError: Incompatible dim 1 in shapes (1, 64, 512) vs. (1, 512, 1); ValueError: Incompatible dim 1 in shapes |
| `dhrv_imputation_1_1_0` | 4,577 | 100% | shipped |  |
| `energy_expenditure_1_0_0` | 318,650 | 100% | shipped |  |
| `halite_1_2_0` | 909,965 | 100% | shipped |  |
| `illness_detection_0_5_1` | 155,911 | 100% | shipped |  |
| `popsicle_1_6_0` | 254,020 | 100% | shipped | All 254,020 parameters ship as 4 cores. The composing parent does not convert: unknown |
| `popsicle_1_8_1` | 254,020 | 100% | shipped | All 254,020 parameters ship as 4 cores. The composing parent does not convert: Core ML: GuardOnDataDependentSymNode: Could not extract specialized integer from data-dependent expression u0 (unhinted: u0).  (Size-like symbols: u0) |
| `pulse_ppg` | 28,497,920 | 100% | shipped |  |
| `sleepnet_bdi_0_3_0` | 290,960 | 100% | shipped |  |
| `sleepnet_bdi_0_4_0` | 1,008,640 | 100% | shipped |  |
| `sleepnet_moonstone_1_2_0` | 1,075,955 | 100% | shipped |  |
| `step_counter_1_3_0` | 7,811 | 100% | shipped | All 7,811 parameters ship as 3 cores. The composing parent does not convert: TFLite: Array boolean indices must be concrete; got bool[1] |
| `whr_2_7_1` | 983,891 | 100% | shipped | All 983,891 parameters ship as 2 cores. The composing parent does not convert: Core ML: no core ml precision policy converted: TypeError: iteration over a 0-d tensor; TypeError: iteration over a 0-d tensor; TypeError: iteration over a 0-d |
| `astd_event_detection_0_1_0` | 0 | — | Rust | Deterministic: zero learned parameters. Implemented in model_zoo::deterministic::astd_event_detection. |
| `atlas_2_1_0` | 60 | — | Rust | Twelve linear regressions over five features — sixty parameters of dot product, which is not work for an accelerator. Implemented in model_zoo::deterministic::atlas. Its input needs a bioimpedance front end that no supported strap has, so the capability stays unavailable; see docs/ml.md. |
| `atlas_trendline_1_0_0` | 0 | — | Rust | Deterministic: zero learned parameters. Implemented in model_zoo::deterministic::atlas_trendline. |
| `cumulative_stress_1_2_2` | 0 | — | Rust | Deterministic: zero learned parameters. Implemented in model_zoo::deterministic::cumulative_stress. |
| `cva_calibrator_1_3_0` | 0 | — | Rust | Deterministic: zero learned parameters. Implemented in model_zoo::deterministic::cva_calibrator. |
| `daily_medians_1_1_0` | 0 | — | Rust | Deterministic: zero learned parameters. Implemented in model_zoo::deterministic::daily_medians. |
| `daily_short_term_baselines_1_1_0` | 0 | — | Rust | Deterministic: zero learned parameters. Implemented in model_zoo::deterministic::short_term_baselines. |
| `meal_timing_0_1_0` | 0 | — | Rust | Deterministic: zero learned parameters. Implemented in model_zoo::deterministic::meal_timing. |
| `pregnancy_biometrics_0_4_0` | 0 | — | Rust | Deterministic: zero learned parameters. Implemented in model_zoo::deterministic::pregnancy_biometrics. |
| `steps_motion_decoder_2_0_0` | 0 | — | Rust | Deterministic: zero learned parameters. Implemented in model_zoo::deterministic::steps_motion_decoder. |
| `stress_daytime_sensing_1_1_0` | 0 | — | Rust | Deterministic: zero learned parameters. Implemented in model_zoo::deterministic::daytime_stress. |
| `stress_resilience_2_2_1` | 0 | — | Rust | Deterministic: zero learned parameters. Implemented in model_zoo::deterministic::stress_resilience. |
| `training_stress_score_0_2_1` | 0 | — | Rust | Deterministic: zero learned parameters. Implemented in model_zoo::deterministic::training_stress_score. |

<!-- LEDGER-TABLE -->

Five statuses and what each means:

- **shipped** — every core converted on both platforms and is in both bundles.
- **partial** — some cores shipped, at least one blocked. The archive is usable to the extent its
  shipped cores cover it.
- **Rust** — zero learned parameters. There is no tensor to accelerate, so these are algorithms,
  and [the boundary](#the-boundary) puts deterministic computation in shared Rust rather than
  behind an FFI round-trip into a platform runtime. Shipping arithmetic as a `.mlpackage` would be
  the wrong artefact.
- **no sensor** — blocked on hardware, not on any amount of converter work.

## What does not ship

`tools/ml/build_manifest.py` drops any model that fails a gate and records why in the manifest
under `not_shipped`. The ledger above is the authoritative account; this is what is behind the rows
that are not `shipped`.

**Every learned parameter in every archive is implemented: 41,008,090 of 41,008,090.** The last
gaps closed were the popsicle heads — rebuilt whole rather than leaving a 161-parameter tail behind
each recurrent encoder — the 88-parameter provenance embedding in the activity archive, and
`atlas_2_1_0`'s sixty regression coefficients, which are a Rust port rather than an artefact.

**Shipping a model and using it are different claims, and only the first is true of any of these
yet.** All forty-one load, clear their SHA-256 admission check and run at their contracted shapes
on a Pixel 7 — `ModelZooInstrumentedTest` and `ModelZooParityInstrumentedTest` prove that on
hardware. None is invoked by a shipped feature. `ModelHost` holds the queue and `MavModelBridge`
drains it, both tested, but nothing calls `drain()` because the analytics that would read the
outputs are not admitted.

Sleep staging is the clearest case and worth stating precisely, because it is easy to read the
ledger and conclude the opposite: **all three sleepnet artefacts ship and run.** What does not
exist is the analytic. `docs/analytics.md` records `RestScorer` as **unavailable
(`SleepPerformance`)** — a sleep composite needs a staged hypnogram, and no admitted analytic
produces one yet. That is a metric-admission gap, scheduled for M4, not a conversion gap.
`atlas_2_1_0` is the other exclusion and is unrelated: a Rust port waiting on bioimpedance
hardware no supported strap has.

Six *parent modules* still do not convert, and none of them needs to: `whr_unet`, `step_counter`,
`awhr_profile_selector`, `activity_segments`, `cva_predictor` and `cva_predictor_v1` are each the
composition around cores that all ship. What fails in each is the glue — a boolean mask, a packed
sequence length, a list append inside a conditional — and glue belongs in Rust, which is where the
rest of every wrapper already went. `cva_predictor_v1` is additionally superseded: 2.1.0's encoder
and probe heads do the same job with more parameters, and both ship.

`atlas_2_1_0` is blocked on hardware, and the evidence is specific rather than assumed. Its
validator states the contract exactly: `bioz_signals` of shape `[2, 500]` — real and imaginary
impedance at one excitation frequency, 500 samples, bounded at ±2¹⁹ — plus per-frequency calibration
coefficients, three electrodermal values and a skin temperature. That is a bioimpedance front end:
something that injects a known current and measures the complex voltage response.

The straps Maverick supports do not have one. `docs/protocol/whoop-raw-afe.md` enumerates the AFE
from firmware console strings — red, IR and green LEDs plus ambient, and a single-lead ECG
electrode — and records no bioimpedance path. An ECG electrode senses voltage; it does not excite.
There is no configuration that turns one into the other, and no substitute signal that would be
anything but invented.

So the model is not fed rather than not built. The port exists, carries all sixty coefficients,
and is tested against vectors generated from the archive; `mav_analytic`'s capability negotiation
is where "no device can supply this" is expressed, which is the same mechanism any other
sensor-gated metric uses. What it is waiting for is hardware, not work.

## Runtime dependency rule

The iOS app uses the Core ML framework iOS already ships. The Android app admits exactly one
TensorFlow Lite dependency, and the zoo adds no second: every model in it shares the one
interpreter class the ECG classifier already required. Additional runtimes, delegates or model
variants need their own measured need, contract, bundle audit and admission review.

## How analytics are admitted

Whether an analytic is a learned model or a closed-form formula, it enters Maverick under the same
rule, stated in [testing.md](testing.md): a golden fixture derived from a real capture or a
published reference, or property tests that can genuinely fail. And the same validation distinction
applies. A model that agrees with itself on both platforms is *consistent*; only a model checked
against ground truth is *validated*, and anything less is provisional.

**Shipping a model is not admitting its output.** Every model in the manifest is in the bundle, in
the registry and reachable over the FFI. None feeds `DailySnapshot`, and the one surface that names
them — the on-device analysis screen — reports what ran and why anything absent is absent, never a
value. Wiring one into a reading is a separate decision per model, and it needs the thing none of
them has yet: a fixture set with ground truth that the model could fail against.

## Regenerating

The pipeline is documented in [tools/ml/README.md](../tools/ml/README.md):

```sh
python tools/ml/convert.py
python tools/ml/pulseppg_convert.py
python tools/ml/pulseppg_tflite.py
python tools/ml/pulseppg_crosscheck.py
python tools/ml/build_manifest.py --conversion-out <dir>
python tools/ml/generate_bindings.py
```

Conversion reads the training checkpoints, which are held outside this repository. Everything
downstream of the contracts — the manifest, the three generated registries, and every test — is
reproducible without them.
