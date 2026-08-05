# The model conversion pipeline

This directory turns Maverick's trained checkpoints into the two artefacts the apps can run, and
generates everything downstream: the manifest, and the registries Rust, Swift and Kotlin read.

The boundary it serves is [docs/ml.md](../../docs/ml.md); the decision behind it is
[ADR-035](../../docs/adr/ADR-035.md).

## What you need

Conversion reads the training checkpoints. Those are held outside this repository — they are large,
they change on a training cadence rather than a code one, and nothing in the app build needs them.
Point `convert.py` at wherever they live.

Everything *downstream* of the contracts is reproducible without them:
[artifacts/models/manifest.json](../../artifacts/models/manifest.json) and the committed per-model
contracts beside it are what the manifest builder, the three generated registries, the asset gate
and every test read.

## Two environments, on purpose

The two converters need incompatible PyTorch versions, so the pipeline runs across two virtual
environments and passes work between them as job files.

```sh
uv venv --python 3.11 .venv
uv pip install --python .venv/bin/python torch==2.5.1 numpy==1.26.4 coremltools onnx

uv venv --python 3.11 .venv-tf
uv pip install --python .venv-tf/bin/python torch litert-torch coremltools numpy
```

- `.venv` — PyTorch 2.5 with coremltools. Loads the TorchScript archives and drives the Core ML
  TorchScript frontend.
- `.venv-tf` — PyTorch 2.12 with `litert-torch`. Drives the TensorFlow Lite converter, and the
  Core ML **EXIR** frontend, which needs the newer `torch.export`.

`convert.py` runs in the first and shells out to the second. That is not incidental complexity:
neither Core ML frontend converts every model, and the fallback between them is what takes Core ML
coverage from four models to thirteen.

## Running it

```sh
# every model in specs.py: both backends, parity, cross-platform check, contracts under out/
.venv/bin/python convert.py

# one or more models by key
.venv/bin/python convert.py sleepnet_bdi pulsenet_foundation

# Pulse-PPG, in three invocations: coremltools' prediction runtime and the LiteRT converter
# deadlock on the GIL if they share one interpreter
.venv-tf/bin/python pulseppg_convert.py
.venv-tf/bin/python pulseppg_tflite.py
.venv/bin/python pulseppg_crosscheck.py

# conversion output -> manifest + app artefacts
python build_manifest.py --conversion-out out

# manifest -> Rust, Swift and Kotlin registries, and the parity table in docs/ml.md
python generate_bindings.py
```

CI runs the last two with `--check`, which fails on a stale file rather than writing one.

## The files

| File | Role |
|---|---|
| `specs.py` | Per-model spec: which archive, which submodule is the neural core, the contracted input shapes, the output names. Includes the models that do not currently convert, whose errors are recorded in the manifest. |
| `convert.py` | The driver. Loads, runs eagerly, traces, converts to Core ML, shells out for TensorFlow Lite, measures parity against the eager reference, runs the two shipped artefacts against each other, writes one contract per model. |
| `ct_ops.py` | Core ML frontend handlers for the ops a TorchScript wrapper leaves behind on dead branches (`format`, `uninitialized`, `raiseexception`), plus `unfold`, `bitwise_or` and `alias`. |
| `coreml_exir.py` | The second Core ML attempt, through `torch.export`, run in `.venv-tf`. |
| `tflite_export.py` | The TensorFlow Lite converter, run in `.venv-tf`, including the nearest-downsample repair. |
| `fp16_weights.py` | Rewrites a finished flatbuffer to carry float16 weights with float32 activations. |
| `coreml_fp16_weights.py` | The Core ML half of the same policy, through MIL `constexpr_cast`. |
| `coreml_policy.py` | The compute-unit sweep: every backend, on the artefact that ships, recorded as the evidence for admitting `ALL`. |
| `coreml_precision.py` | The precision ladder. Half-width arithmetic first, and a policy is only kept if the compute plan shows it leaving work on the Neural Engine. |
| `compute_plan.py` | Reads Core ML's own `MLComputePlan` and reports which processor each operation is assigned to. This is what showed that full-width arithmetic meant no accelerator at all. |
| `fp16_align.py` | Rounds the weights to the float16 grid once, on the PyTorch module, so both converters see the same numbers instead of each rounding its own. |
| `fold_norm.py` | Folds batch normalisation into the convolution before it, once, in the shared exported graph — and rounds what folding produces. The largest avoidable source of cross-platform disagreement was each converter folding and rounding its own copy. |
| `parity_decompose.py` | Cuts a converted program at every operation and measures each cut, so an error can be attributed to the operation that introduces it rather than guessed at. |
| `build_precision_ledger.py` | Per-model record of weight precision, arithmetic precision, actual Core ML processor assignment, Android delegate path, and the cross-platform error split into its graph and arithmetic parts. |
| `gpu_bisect.py` | Cuts a converted program at each operation and compares the CPU against the GPU, to name the operation a backend computes differently rather than describe the symptom. |
| `compute_sweep.py` | Every shipped package on every compute unit, measured against the CPU. What a narrow admission has to be earned by. |
| `deterministic_vectors.py` | Runs the zero-parameter archives on seeded inputs and writes what they returned, so the Rust ports are tested against the archive rather than against a reading of its decompilation. |
| `verify_shipped.py` | Independent check: loads the bundles' own bytes on their admitted compute units, with its own probes and no shared code. |
| `device_vectors.py` | Writes the reference vectors the instrumented tests read: inputs, eager PyTorch, and this host's LiteRT answer, at seeds and pulse rates the conversion never uses. |
| `device_compare.py` | Puts Core ML's answer beside a real handset's on identical inputs. The cross-platform number measured rather than composed from two host runs. |
| `android_delegate.py` | Turns a device delegate sweep into the execution path each model ships with. The CPU is the default; a model leaves it only where the GPU measured both faster and no less accurate. |
| `check_claims.py` | Asserts the load-bearing figures in `docs/ml.md` against the artefacts that settle them, including the parameter reconciliation. The generated tables cannot go stale; the prose around them did. |
| `device_bench.py` | Compares two on-device benchmark runs per model, and refuses to call a change a result until it clears the run-to-run noise band. |
| `device_test.sh` | Build, verify the build, install, run. In that order — installing after a failed build silently re-measures the previous binary. |
| `pulseppg_*.py` | Pulse-PPG, a plain `nn.Module` rather than a TorchScript archive, so it takes its own path in three invocations. |
| `discover.py` | Error-driven shape discovery: calls a core with candidate shapes and reads PyTorch's own complaints until one is accepted. How the shapes in `specs.py` were found. |
| `build_manifest.py` | Applies the both-platforms rule and the three parity gates, writes the manifest, copies artefacts into the two app bundles, deletes stale ones. |
| `generate_bindings.py` | Renders the manifest into the Rust, Swift and Kotlin registries and into the parity table in `docs/ml.md`. |
| `test_build_manifest.py` | The admission gate's own tests. They exist because three models once shipped wrong. |

## Precision

**Storage is unconditional: float16 weights, both platforms, every model.** Core ML writes them
itself under a half-precision policy and through MIL `constexpr_cast` under the full-width one, so
the bytes do not change when a model's arithmetic does. `fp16_weights.py` does the TensorFlow Lite
half, rewriting the finished flatbuffer into TFLite's own float16 form — FLOAT16 constants with a
`DEQUANTIZE` before each kernel.

Two routes that look simpler and both fail: casting the module to `.half()` before conversion
(`tfl.pad` and `tfl.strided_slice` will not legalise in half precision), and halving constants in
the exported graph with a widening cast (folded straight back to full width, so the rounding is paid
for and the bytes are not saved).

`fp16_align.py` rounds the weights to the float16 grid *before* either converter runs, on the
PyTorch module. Without it each converter rounds its own post-fusion constants, which are not the
same constants, and the platforms end up computing with different numbers from what should be the
same weights. Both use the same 1,024-element threshold; below it a constant is a bias or a
normalisation scale, cheap to store and with rounding that reaches every output.

**Arithmetic is chosen per model, by measurement.** `compute_precision=FLOAT32` looks like the safe
default and is not: the Neural Engine is half-precision hardware, and `compute_plan.py` — which
reads Core ML's own `MLComputePlan` — shows that under full width *zero* operations in the zoo are
assigned to it. `coreml_precision.py` runs a ladder from half width to full and keeps the first rung
that clears the parity bar **and** leaves work on the accelerator. That second condition matters:
exempting a few operations from half precision does not partition the graph, it takes the whole
program off the accelerator, which full width already does with less error.

Parity is measured after every pass, on the bytes that ship, through `ComputeUnit.ALL`, over five
probes. The accelerated path is the one the app runs, and it is not the flattering one — the Neural
Engine accumulates at half width where Core ML's CPU path accumulates at full width.

## Adding a model

1. Find the neural core. `discover.py <archive> <dotted.path> <n_args>` will search shapes for you;
   a core with weights usually sits under a name like `trained_model`, `predictor` or
   `_model_runner`.
2. Add a spec entry. Name the outputs in the order the core returns them, flattened.
3. Run `convert.py <key>`. The driver executes the core eagerly first, so a wrong shape fails there
   rather than producing a mismatched artefact.
4. Port the wrapper's preprocessing into `mav_analytic::model_zoo::ppg` (or a sibling module) with
   tests. The TorchScript source is readable: unzip the `.pt` and look under `code/__torch__/`.
5. Rebuild the manifest and the bindings, then run `cargo test --workspace` and
   `tools/check_model_assets.py`.

## Why conversion fails, when it fails

Four causes so far, and the distinction matters because only three are fixable here:

- **A missing operator.** Fixable: add a handler to `ct_ops.py`. `unfold` was this, and it
  unblocked all three sleep models.
- **A mutated graph input.** Fixable: the wrappers clone their inputs before calling the core.
- **A converter that lowers an operator differently from PyTorch.** Fixable: rewrite the node in the
  exported graph. LiteRT's nearest-neighbour resize disagrees with PyTorch when it downscales, which
  is why `tflite_export.py` replaces those nodes with the `index_select` PyTorch actually computes.
  This is the class of failure that produces a *working* model with wrong numbers, so measure the
  written file, never the converter's handle on the source.
- **Data-dependent control flow.** Not fixable at this layer. `whr_unet` compares tensors to decide
  a branch; `cva_predictor` guards on a computed length. Converting these means splitting the core
  so the branch happens in Rust, not adding a shim that bakes in whichever branch the tracing input
  took.
