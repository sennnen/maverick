# MZ — Model zoo

**Exit criterion:** at least one zoo model produces a value that reaches `DailySnapshot`, admitted
under the rule in [testing.md](../../testing.md) — a fixture with ground truth it could fail
against — and the rest are either admitted the same way or removed from the bundle.

That criterion is deliberately not "the models ship". They already ship. Every artefact in
`artifacts/models/manifest.json` is in both app bundles, contract-checked, hash-gated and reachable
over the FFI, and not one of them has produced a number anyone should believe yet. Closing the gap
between *runs* and *is trusted* is the whole of the remaining milestone.

The decision that opened it is [ADR-035](../../adr/ADR-035.md); the contracts are in
[ml.md](../../ml.md).

## Landed

| Packet | What it did |
|---|---|
| MZ-P1 | Conversion pipeline in `tools/ml`: per-model specs, two-environment driver, Core ML TorchScript and EXIR frontends with a fallback between them, LiteRT for TensorFlow Lite, parity against the eager PyTorch reference, one contract per model. |
| MZ-P2 | `mav_analytic::model_zoo`: tensor vocabulary, contract validation, admission by artefact hash, and the ported PPG front-ends (`pulsenet_input`, `cva_pulse`, `pulse_ppg_input`) with tests. |
| MZ-P3 | `mav_engine::model_host`: the bounded pull-based queue, rejecting unknown ids, unadmitted hashes and out-of-contract tensors. |
| MZ-P4 | The FFI surface in `mav-ffi/src/models.rs`: catalogue, queue, and the two `prepare_*` calls that are the only way a raw signal enters. |
| MZ-P5 | `MavModelRunner` and `MavModelBridge` on both platforms, replacing the one-class-per-model pattern; generated catalogues; Swift and Kotlin tests; an instrumented test that runs every bundled model on a device. |
| MZ-P6 | `build_manifest.py`, `generate_bindings.py`, the generalised `tools/check_model_assets.py`, and the manifest all of them read. |
| MZ-P7 | Three parity gates and the two defects that made them necessary: parity measured on the written flatbuffer rather than the converter's handle, and LiteRT's nearest-neighbour downscaling rewritten to match PyTorch. |
| MZ-P8 | Float16 weights on both platforms for every model, via `fp16_weights.py` rewriting the finished flatbuffer the way TensorFlow Lite's own float16 quantisation does. |
| MZ-P9 | Coverage from 13 models to 24, by splitting every blocked core at its branch: the four tensor heads of the 3.1.11 predictor, both halves of the WHR U-Net, the CVA transformer encoder via `get_embeddings_1`, the no-heart-rate energy branch, and the masked step and profile cores. Plus a rewrite of `aten._convolution.default` to the modern overload LiteRT lowers. |
| MZ-P10 | The ledger: `build_ledger.py`, `artifacts/models/ledger.json` and the generated table in ml.md, covering all 31 archives with a status and a reason each. |
| MZ-P11 | The first two deterministic ports, `daytime_stress` and `short_term_baselines`, against vectors generated from the archives. |
| MZ-P12 | Coverage from 24 models to 32, and from 76% to 98% of all learned parameters: the 3.1.11 ensemble head, both CVA probe branches, the step head and multiplier, the awake-HR profile head, and both follicular heads. `whr_2_7_1`, `step_counter_1_3_0` and `cva_2_1_0` now ship every parameter they have. |
| MZ-P13 | Precision and compute units chosen per model by measurement (`coreml_policy.py`). Core ML FLOAT16 halves the arithmetic as well as the weights, and Core ML's backends disagree with each other on some graphs — the sleep models are exact on CPU and Neural Engine and wrong by a whole relative unit on the GPU. Worst cross-platform disagreement fell from 2.7e-2 to 9.3e-4. |
| MZ-P14 | Parity measured over five probes instead of one, after three artefacts passed a single-probe gate and then disagreed by up to 6e-3 on unseen inputs. |
| MZ-P15 | Three shipping defects the platforms would have hit: twelve Core ML packages named their inputs `tensors_0`; the Android runner wrote 32-bit values into a 64-bit tensor; the iOS runner built float arrays for an integer input. Each now has a gate or a test. |
| MZ-P16 | Float16 storage over float32 arithmetic, on both platforms, for every model — no float32 artefacts. Core ML through MIL `constexpr_cast`, TensorFlow Lite through the flatbuffer, both at the same 1,024-element threshold. Bundles fell from 174.8/154.0 MB to 104.9/86.8 MB while cross-platform disagreement stayed under 3e-3. |
| MZ-P17 | The last blocked cores: scripted `nn.LSTM` layers rebuilt as equivalent callable layers under `strict=True` weight loading (`rebuilt_cores.py`), unblocking the awake-HR recurrent layer and all three popsicle recurrent encoders; and `cva_1_3_0`, whose window arithmetic resolved to a 256-sample triple plus eight exogenous values. Coverage reached 99.997% of all learned parameters across 40 models. |
| MZ-P18 | Sleep staging withdrawn. The legacy path held no learned parameters and the classifier it called is gone, so it is excluded from the ledger rather than carried as a permanent gap. |
| MZ-P19 | The Neural Engine, which nothing had been using. `compute_plan.py` reads Core ML's own `MLComputePlan`: under the full-width arithmetic policy, **zero** operations across the whole zoo were assigned to it, because the Neural Engine is half-precision hardware and will not run a full-precision program. Arithmetic precision is now chosen per model by a ladder that requires both parity *and* measured accelerator use, and the mixed policies were measured to buy nothing — exempting a few operations takes the whole program off the accelerator rather than partitioning it. |
| MZ-P20 | Weights rounded once, on the PyTorch module, before either converter runs (`fp16_align.py`). Each converter had been rounding its own post-fusion constants, so the platforms computed with different numbers; now they carry bit-identical weights and nothing is left between them but arithmetic width. |
| MZ-P21 | The resize repair that was being thrown away. `ExportedProgram.module()` returns a new graph per call, so the Core ML path rewrote one module and re-exported another — the three sleep models shipped still carrying a nearest-neighbour downscale their GPU backend computed wrong by 1.5 relative. `gpu_bisect.py` found it by cutting the program at each operation. All three are now exact on every backend and admitted on `ALL`. |
| MZ-P22 | The last 1,114 parameters. The popsicle heads ship whole — recurrent encoder, scalar branch and the layer joining them, rebuilt together rather than leaving a 161-parameter tail stranded — and the activity archive's 88-parameter provenance embedding is a model of its own. |
| MZ-P23 | Android's half-precision path. `MavModelAcceleration` offers the GPU delegate, gated on the vendor `CompatibilityList`, to exactly the models Core ML admitted at half-precision arithmetic, with NNAPI second and not attempted above API 34. Shipping the same bytes was never parity; computing the same arithmetic is. |
| MZ-P24 | Two more deterministic ports, `daily_medians` and `atlas_trendline`, and a generator (`deterministic_vectors.py`) that runs each archive to produce the golden vectors the ports are tested against — refusal codes included. |
| MZ-P25 | Five more: `astd_event_detection`, `cva_calibrator`, `steps_motion_decoder`, `meal_timing` and `training_stress_score`. Two defects the golden vectors caught that a reading of the decompilation had not: `cva_calibrator`'s VO₂max table keys female as zero and everything else as one, not the reverse; and its score floor is applied *after* the fitness scaling, not before. |
| MZ-P26 | The last three deterministic archives — `pregnancy_biometrics`, `stress_resilience`, `cumulative_stress` — and `atlas_2_1_0`'s sixty regression coefficients. Every archive is now implemented and the parameter ledger closes at 41,008,090 of 41,008,090. Three more defects the vectors caught: `torch.slice`'s four-argument form is `[:n]` and not `[::n]`, which had the pregnancy filter restoring every third day instead of the opening three; `cumulative_stress` matches its temperature clock in seconds, not milliseconds; and `argmax` takes the *first* maximum, where taking the last moved atlas's settled impedance by most of a kernel width. |
| MZ-P27 | The scheduler and the passes around it: `mav_engine::analytics` decides which models are worth running on a device, in what order, and what is already known, against a fingerprint of the inputs *and* the artefact hash that produced them. Each platform contributes only its own half — when a pass happens, how hard it pushes, and getting all of it off the thread that draws. |
| MZ-P28 | Lifecycle, and the surface. `releaseCache()` and `close()` had no caller and no bound on iOS at all, so a pass left every model it touched resident; both are wired to backgrounding behind the pass lock, and iOS gained an LRU. Background windows on both platforms — `BGTaskScheduler` and `WorkManager` — with the Android worker fixed to reach a process-wide engine, because it used to wake a cold process, find a null static the activity was supposed to have filled in, and report success without running anything. And the screen: reachable from Today, built from `MavKit`, one honest state per signal, no model output rendered as a reading. |

## Open

### MZ-P29 — Golden vectors for the ported front-ends

The front-ends in `model_zoo::ppg` are tested against synthesised signals and their own properties,
which catches a broken filter but not a subtly wrong one. What they are missing is the thing
[ml.md](../../ml.md) demands of preprocessing: a stored input signal and a byte-identical expected
tensor, generated from the training wrapper.

This is the highest-value open packet, because every model downstream of a PPG front-end inherits
whatever the front-end gets wrong, and a 1% embedding difference from float16 is indistinguishable
from a 1% error caused by an off-by-one in the reflect padding.

Owned files: `fixtures/ml/`, `core/crates/mav-analytic/src/model_zoo/ppg.rs`.

### MZ-P30 — Admit one model end to end

Pick the model with the clearest ground truth available and take it all the way: fixture with
labels, output decoding into a typed result with provenance, a `MetadataId`, storage, and a surface
that states its standing. `sleepnet_bdi` is the candidate — interbeat intervals are a stream
Maverick already decodes, and polysomnography-labelled data exists to score against.

Blocked on MZ-P29 for the input side, and on a decision about the staging class order, which has not
been mapped onto Maverick's sleep-stage vocabulary and must not be guessed.

## Decisions taken here

- **Convert the core, port the wrapper.** ADR-035 §1. The alternative freezes data-dependent
  behaviour into a static graph.
- **Both platforms or neither.** `build_manifest.py` enforces it mechanically rather than by
  review.
- **Float16 everywhere.** Same precision on both platforms, for every model: the best of the
  size, accuracy and speed trades on mobile, and what the Neural Engine computes in regardless.
- **Measure the file, not the converter.** A parity number taken from the converter's handle
  reported exact agreement for three artefacts that were wrong. Everything is measured by loading
  the shipped bytes, and the two platforms are additionally compared to each other.
- **Standing is provenance, not quality.** `first_party` versus `open_licensed` answers only
  whether an attribution notice travels with the artefact. Trustworthiness is a separate axis, and
  today every model sits at the same point on it.
- **Shipping is not admitting.** No zoo model touches `DailySnapshot`, and the analysis surface
  reports state rather than values. This is what stops a registry of models from becoming a screen
  of unvalidated numbers.
