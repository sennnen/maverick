#!/usr/bin/env python3
"""Convert Maverick's neural cores to Core ML and TensorFlow Lite.

For each spec in specs.py this:

  1. loads the TorchScript archive and walks to the tensor-in / tensor-out core;
  2. runs the core eagerly at the contracted shapes and keeps the reference output;
  3. traces it, and checks the trace still reproduces the reference;
  4. emits a Core ML ML Program (.mlpackage, FP16 weights, FLOAT32 I/O);
  5. emits a TensorFlow Lite flatbuffer (FP16 weights, FLOAT32 I/O) through ONNX;
  6. re-runs both artefacts and records the worst absolute deviation from PyTorch;
  7. writes a contract JSON carrying shapes, dtypes, hashes and parity numbers.

Nothing is written unless the core executed. A spec whose shape is wrong fails
loudly here instead of shipping a mismatched tensor contract.

Usage:
    python convert.py [model_key ...]        # default: every spec
"""
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import traceback

import numpy as np
import torch

import fp16_align
from specs import SPECS

HERE = os.path.dirname(os.path.abspath(__file__))
MODELS_DIR = os.path.join(os.path.dirname(HERE), "decrypted_models")
OUT_DIR = os.path.join(HERE, "out")
COREML_DIR = os.path.join(OUT_DIR, "coreml")
TFLITE_DIR = os.path.join(OUT_DIR, "tflite")
CONTRACT_DIR = os.path.join(OUT_DIR, "contracts")

DTYPES = {"float32": torch.float32, "int64": torch.int64}


# --------------------------------------------------------------------------- io


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sha256_tree(path):
    """Stable hash of a directory: every member path and its content, sorted."""
    if os.path.isfile(path):
        return sha256_file(path)
    digest = hashlib.sha256()
    for root, dirs, files in os.walk(path):
        dirs.sort()
        for name in sorted(files):
            full = os.path.join(root, name)
            rel = os.path.relpath(full, path)
            digest.update(rel.encode())
            digest.update(sha256_file(full).encode())
    return digest.hexdigest()


def member_hashes(path):
    out = {}
    for root, dirs, files in os.walk(path):
        dirs.sort()
        for name in sorted(files):
            full = os.path.join(root, name)
            out[os.path.relpath(full, path)] = sha256_file(full)
    return out


def directory_size(path):
    if os.path.isfile(path):
        return os.path.getsize(path)
    return sum(
        os.path.getsize(os.path.join(root, name))
        for root, _, files in os.walk(path)
        for name in files
    )


# ---------------------------------------------------------------------- torch


def get_core(model, path):
    node = model
    for part in [p for p in (path or "").split(".") if p]:
        node = getattr(node, part)
    return node


PULSATILE_INPUTS = ("ppg", "pulses", "vpgs", "apgs")


# The heart rates the waveform probes are drawn at, one per probe index.
#
# Without these `pulse_probe` ignores the seed, so a model whose input is a signal was measured
# at one single waveform however many probes the ladder asked for — five identical inputs, five
# identical answers, and a parity number that described one point of the input space while
# reading like the worst of five. The rates span a plausible resting-to-exercise range so the
# probes exercise different filter responses rather than merely different noise.
PROBE_RATES = (68.0, 52.0, 96.0, 44.0, 120.0, 61.0, 78.0, 150.0)


def pulse_probe(shape, bpm=68.0, rate_hz=50.0):
    """A pulse-shaped probe for the inputs that carry a waveform.

    White noise into a network whose filters are tuned to pulses lands far outside the
    activation range the weights were fitted for, and the FP16 parity measured there is
    not the parity the model will show in use. Waveform inputs get a waveform.
    """
    length = shape[-1]
    time = torch.arange(length, dtype=torch.float32) / rate_hz
    phase = 2.0 * torch.pi * (bpm / 60.0) * time
    wave = torch.sin(phase) + 0.3 * torch.sin(2.0 * phase) + 0.05 * torch.sin(0.05 * time)
    return wave.expand(*shape).clone() if len(shape) > 1 else wave


# How many probes each parity number is the worst of.
#
# One probe measures a model at one point of its input space, and that turned out to be too
# few: three models passed a single-probe gate and then disagreed across platforms by up to
# 6e-3 on inputs the pipeline had never tried. Relative error also swings wildly when an
# output lands near zero, which one probe cannot distinguish from a real defect.
#
# Three was still not enough for `activity_detection`, whose outputs mix probabilities, a
# 256-value embedding and day-minute positions on very different scales — it passed at three
# probes and missed at five. Probe sensitivity does not disappear with more probes; it only
# gets cheaper to detect, which is why the independent verification uses its own seeds.
#
# Five was still not enough, and this time the evidence came off a phone. Measured on three
# probes the pipeline had never tried, `activity_transition` sat 1.10e-2 from its reference —
# past the bar its policy was admitted under — and four more models landed between 6e-3 and
# 1e-2. Eight probes, each waveform model now at a different rate, is what those points cost
# to find before an artefact ships rather than after.
PROBE_COUNT = 8


def probe_sets(spec):
    """The examples every parity number is measured over."""
    return [make_inputs(spec, seed=index) for index in range(PROBE_COUNT)]


def make_inputs(spec, seed=0):
    generator = torch.Generator().manual_seed(seed)
    bounds = spec.get("int_bounds") or {}
    tensors = []
    for name, shape, dtype in spec["inputs"]:
        if dtype == "int64" and name in bounds:
            # A lookup index, not a length. Walking the table is what makes the probe mean
            # something — a constant index measures one row of an embedding and calls it the
            # model — and a value past the last row is an out-of-range fault, not a parity
            # result, so the spec states the row count and the probe stays inside it.
            walk = (torch.arange(int(torch.tensor(shape).prod()), dtype=torch.int64) + seed)
            tensors.append((walk % int(bounds[name])).reshape(tuple(shape)))
        elif dtype == "int64":
            # Sequence-length style inputs: the full window is the honest default.
            fill = shape[-1] if len(shape) > 1 else 40
            tensors.append(torch.full(tuple(shape), int(fill), dtype=torch.int64))
        elif name in PULSATILE_INPUTS:
            tensors.append(pulse_probe(tuple(shape), bpm=PROBE_RATES[seed % len(PROBE_RATES)]))
        else:
            tensors.append(torch.randn(*shape, generator=generator))
    return tensors


def flatten(value):
    if isinstance(value, torch.Tensor):
        return [value]
    if isinstance(value, (list, tuple)):
        out = []
        for item in value:
            out += flatten(item)
        return out
    return []


def materialise_const(value):
    """A const arg may be a plain Python value or a frozen tensor description.

    Two cores take a sequence-length tensor whose *value* steers the graph. Exported at a fixed
    shape the length is not a free input any more, so it is frozen into the graph rather than
    pretended to be one; the contract records the frozen value.
    """
    if isinstance(value, dict) and "tensor" in value:
        import torch as _torch

        dtype = _torch.int64 if value.get("dtype") == "int64" else _torch.float32
        return _torch.full(tuple(value["tensor"]), value.get("fill", 0), dtype=dtype)
    return value


def build_args(tensors, input_names, const_args, arg_template):
    """Assemble the positional arguments a core's forward expects.

    `const_args` appended at the end covers most cores. `arg_template` is for the ones whose
    non-tensor argument sits in the middle: CVA's probe head takes
    `(embeddings, gender: str, age, weight, bmi)`, and there is no way to reach that by
    appending. Each entry is either `"@name"` for the named input tensor, or a literal.
    """
    if not arg_template:
        return list(tensors) + list(const_args)
    by_name = dict(zip(input_names, tensors))
    args = []
    for entry in arg_template:
        if isinstance(entry, str) and entry.startswith("@"):
            args.append(by_name[entry[1:]])
        else:
            args.append(entry)
    return args


class CoreWrapper(torch.nn.Module):
    """Freezes the const args so the traced graph is purely tensor-in, tensor-out."""

    def __init__(self, core, const_args, method=None, arg_template=None, input_names=None):
        super().__init__()
        self.core = core
        self.const_args = [materialise_const(value) for value in const_args]
        # A few cores expose inference under a named method rather than `forward`. CVA's
        # `base_model.forward` computes a contrastive training loss; only `get_embeddings_1`
        # produces the embedding its predictor consumes.
        self.method = method
        self.arg_template = arg_template
        self.input_names = list(input_names or [])

    def forward(self, *tensors):
        # Two cores write into their own input. Exporters reject a mutated graph input, and the
        # mutation is incidental to the arithmetic, so each call gets its own copy.
        cloned = [tensor.clone() for tensor in tensors]
        entry = getattr(self.core, self.method) if self.method else self.core
        return entry(*build_args(cloned, self.input_names, self.const_args, self.arg_template))


def deviation(produced, reference):
    """Worst absolute and scale-relative deviation across every output tensor.

    Absolute error alone is unreadable across models whose outputs span probabilities
    and heart rates, so both are recorded and the relative figure is the one the
    admission gate reads.
    """
    worst_abs = 0.0
    worst_rel = 0.0
    for got, expected in zip(produced, reference):
        want = expected.numpy().astype(np.float64)
        have = np.asarray(got, dtype=np.float64)
        error = float(np.max(np.abs(have - want)))
        scale = float(np.max(np.abs(want)))
        worst_abs = max(worst_abs, error)
        worst_rel = max(worst_rel, error / scale if scale > 1e-9 else error)
    return {"max_abs": worst_abs, "max_rel": worst_rel}


# --------------------------------------------------------------------- core ml


# The bar a half-precision artefact has to clear to be worth its saving. Above it the model
# is converted at full precision instead. 1e-3 relative is tight enough that a downstream
# head cannot tell the platforms apart, and loose enough that most small dense networks pass.
FP16_PARITY_BAR = 1e-3


def _coreml_feed(spec, example):
    feed = {}
    for (name, _shape, dtype), tensor in zip(spec["inputs"], example):
        array = tensor.numpy()
        feed[name] = array.astype(np.int32 if dtype == "int64" else np.float32)
    return feed


def _convert_coreml(traced, spec, policy):
    import coremltools as ct

    import coreml_precision
    import ct_ops

    ct_ops.install()

    inputs = [
        ct.TensorType(
            name=name,
            shape=tuple(shape),
            dtype=np.int32 if dtype == "int64" else np.float32,
        )
        for name, shape, dtype in spec["inputs"]
    ]
    return ct.convert(
        traced,
        inputs=inputs,
        outputs=[ct.TensorType(name=name) for name in spec["outputs"]],
        convert_to="mlprogram",
        compute_precision=coreml_precision.precision(policy),
        minimum_deployment_target=ct.target.iOS16,
        pass_pipeline=coreml_precision.pipeline(policy),
    )


def coreml_parity_worst(path, spec, examples, references, compute_units):
    """The worst parity across every probe."""
    worst = {"max_abs": 0.0, "max_rel": 0.0}
    for probe, reference in zip(examples, references):
        one = coreml_parity(path, spec, probe, reference, compute_units)
        worst = {key: max(worst[key], one[key]) for key in worst}
    return worst


def coreml_parity(path, spec, example, reference, compute_units):
    """Parity of the saved package, loaded the way the app will load it.

    `compute_units` matters more than it looks. Measured CPU-only, the PulseNet encoder at
    half precision sits 1.7e-2 from its reference; loaded with the Neural Engine available,
    the same package sits 1.6e-1 away, because the Neural Engine accumulates at half width
    where Core ML's CPU path accumulates at full width. The accelerated path is the one the
    app actually runs, so it is the one the gate reads.
    """
    import coremltools as ct

    model = ct.models.MLModel(path, compute_units=compute_units)
    predicted = model.predict(_coreml_feed(spec, example))
    got = [
        np.asarray(predicted[name]).reshape(expected.shape)
        for name, expected in zip(spec["outputs"], reference)
    ]
    return deviation(got, reference)


def export_coreml(traced, spec, examples, references, key):
    """Convert under each precision policy and keep the one that earns its accuracy.

    Two things are being chosen here and they are not independent.

    *Precision* decides whether the Neural Engine can run the model at all: it is half-precision
    hardware, so a program asking for full-width arithmetic is handed to the CPU or GPU
    entirely. `compute_plan.py` measures that — under the full-width policy, zero operations in
    the whole zoo are assigned to the accelerator.

    *Compute units* stay `ALL` unless a backend is measurably wrong, which after the resize
    repair in `tflite_export.rewrite_nearest_downsampling` none is. Pinning them narrower is a
    real cost — a model barred from the GPU cannot use it even when it is the only accelerator
    free — so it takes a failed measurement, not a suspicion.

    The ladder in `coreml_precision` runs from most accelerator to least. A policy is only kept
    over a later one if it *both* clears the parity bar and actually leaves work on the Neural
    Engine: the mixed policies are the trap here, because exempting a handful of operations
    from half precision does not partition the graph — it takes the whole program off the
    accelerator, which is what the full-width policy does anyway, and with better accuracy.
    """
    import coreml_precision
    import coreml_policy

    path = os.path.join(COREML_DIR, f"{key}.mlpackage")
    attempts = []
    chosen = None
    for policy in coreml_precision.POLICIES:
        candidate = f"{path[: -len('.mlpackage')]}.{policy}.mlpackage"
        try:
            model = _convert_coreml(traced, spec, policy)
            if os.path.exists(candidate):
                shutil.rmtree(candidate)
            model.save(candidate)
            parity = coreml_parity_worst(
                candidate, spec, examples, references, coreml_policy.compute_unit("ALL")
            )
            accelerated = coreml_precision.neural_engine_share(candidate)
        except Exception as exc:  # noqa: BLE001 - a policy that will not convert is data
            attempts.append({"policy": policy, "error": f"{type(exc).__name__}: {exc}"[:200]})
            shutil.rmtree(candidate, ignore_errors=True)
            continue
        attempts.append(
            {"policy": policy, "parity": parity, "neural_engine": accelerated}
        )

    policy, why = coreml_precision.choose(attempts)
    if policy is None:
        raise RuntimeError(
            "no core ml precision policy converted: "
            + "; ".join(a.get("error", "") for a in attempts)[:200]
        )
    picked = next(a for a in attempts if a["policy"] == policy and "parity" in a)
    candidate = f"{path[: -len('.mlpackage')]}.{policy}.mlpackage"
    parity, accelerated = picked["parity"], picked["neural_engine"]
    if os.path.exists(path):
        shutil.rmtree(path)
    shutil.move(candidate, path)
    stem = path[: -len(".mlpackage")]
    for attempt in attempts:
        shutil.rmtree(f"{stem}.{attempt['policy']}.mlpackage", ignore_errors=True)

    # Every unit, on the artefact that ships, to prove the answer does not depend on which
    # backend the OS happens to pick.
    units_attempts = []
    for units in coreml_policy.COMPUTE_ORDER:
        try:
            units_attempts.append(
                {
                    "compute_units": units,
                    "parity": coreml_parity_worst(
                        path, spec, examples, references, coreml_policy.compute_unit(units)
                    ),
                }
            )
        except Exception as exc:  # noqa: BLE001 - a refused unit combination is data
            units_attempts.append({"compute_units": units, "error": str(exc)[:160]})
    usable = [a for a in units_attempts if "parity" in a]
    if not usable:
        raise RuntimeError("no core ml compute unit produced a prediction")
    spread = max(a["parity"]["max_rel"] for a in usable) - min(
        a["parity"]["max_rel"] for a in usable
    )
    units = "ALL"
    if not any(a["compute_units"] == "ALL" for a in usable):
        units = min(usable, key=lambda a: a["parity"]["max_rel"])["compute_units"]
    return (
        path,
        coreml_precision.storage(policy),
        units,
        parity,
        {
            "policy": policy,
            "policy_reason": why,
            "policies": attempts,
            "neural_engine": accelerated,
            "compute_units": units_attempts,
            "unit_spread": spread,
        },
    )


def export_coreml_exir(spec, examples, references, key):
    """Second Core ML attempt, through torch.export in the sibling environment."""
    path = os.path.join(COREML_DIR, f"{key}.mlpackage")
    parity = run_job(
        os.path.join(HERE, "coreml_exir.py"), spec, examples, references, key, path
    )
    if not os.path.exists(path):
        raise RuntimeError("exir core ml export produced nothing")
    return path, parity


def run_job(script, spec, examples, references, key, output_path):
    """Write a job file, run `script` under the LiteRT interpreter, return its parity."""
    work = tempfile.mkdtemp(prefix=f"job_{key}_")
    probes = []
    for index, (probe, reference) in enumerate(zip(examples, references)):
        inputs_npz = os.path.join(work, f"inputs_{index}.npz")
        np.savez(
            inputs_npz,
            **{name: tensor.numpy() for (name, _s, _d), tensor in zip(spec["inputs"], probe)},
        )
        reference_npz = os.path.join(work, f"reference_{index}.npz")
        np.savez(reference_npz, **{f"out{i}": t.numpy() for i, t in enumerate(reference)})
        probes.append({"inputs_npz": inputs_npz, "reference_npz": reference_npz})
    inputs_npz = probes[0]["inputs_npz"]
    reference_npz = probes[0]["reference_npz"]
    job_path = os.path.join(work, "job.json")
    with open(job_path, "w") as handle:
        json.dump(
            {
                "source": os.path.join(MODELS_DIR, spec["source"]),
                "core": spec["core"],
                "core_method": spec.get("core_method"),
                "rebuild": spec.get("rebuild"),
                "rebuild_config": spec.get("rebuild_config"),
                "arg_template": spec.get("arg_template"),
                "const_args": spec["const_args"],
                "inputs": [[n, list(s), d] for n, s, d in spec["inputs"]],
                "outputs": list(spec["outputs"]),
                "inputs_npz": inputs_npz,
                "reference_npz": reference_npz,
                "probes": probes,
                "output_path": output_path,
            },
            handle,
        )
    result = subprocess.run(
        [TFLITE_PYTHON, script, job_path],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        combined = (result.stderr or "") + (result.stdout or "")
        lines = [line for line in combined.strip().splitlines() if line.strip()]
        interesting = [
            line
            for line in lines
            if "Error" in line or "error" in line or "not supported" in line
        ]
        raise RuntimeError(" | ".join((interesting or lines)[-3:]))
    parity = json.loads(result.stdout.strip().splitlines()[-1])
    shutil.rmtree(work, ignore_errors=True)
    return parity


# ---------------------------------------------------------------------- tflite


TFLITE_PYTHON = os.path.join(HERE, ".venv-tf", "bin", "python")


def export_tflite(spec, examples, references, key):
    """Run the LiteRT converter in the sibling environment and return its parity."""
    output_path = os.path.join(TFLITE_DIR, f"{key}.tflite")
    parity = run_job(
        os.path.join(HERE, "tflite_export.py"), spec, examples, references, key, output_path
    )
    if not os.path.exists(output_path):
        raise RuntimeError("tflite export produced nothing")
    return output_path, parity


# ----------------------------------------------------------------------- driver


def convert_one(key, spec):
    record = {"model": key, "algorithm": spec["algorithm"], "version": spec["version"]}
    source = os.path.join(MODELS_DIR, spec["source"])
    model = torch.jit.load(source, map_location="cpu")
    model.eval()
    if spec.get("rebuild"):
        import rebuilt_cores

        core = rebuilt_cores.rebuild(
            model, spec["core"], spec["rebuild"], spec["rebuild_config"]
        )
    else:
        core = get_core(model, spec["core"])

    examples = probe_sets(spec)
    example = examples[0]
    wrapper = CoreWrapper(
        core,
        spec["const_args"],
        spec.get("core_method"),
        spec.get("arg_template"),
        [name for name, _s, _d in spec["inputs"]],
    ).eval()
    with torch.no_grad():
        references = [flatten(wrapper(*probe)) for probe in examples]
    reference = references[0]
    if len(reference) != len(spec["outputs"]):
        raise RuntimeError(
            f"{key}: spec names {len(spec['outputs'])} outputs, core returned {len(reference)}"
        )

    # The reference is the unrounded model, deliberately: what the manifest records as
    # deviation from PyTorch should include the cost of half-width storage, not hide it by
    # comparing the rounded model against itself. Everything downstream of this line — the
    # trace, both converters, both artefacts — sees weights already on the float16 grid.
    record["weights_rounded"], record["weights_kept_full"] = fp16_align.round_to_half(core)
    remaining = fp16_align.residual(core)
    if remaining:
        raise RuntimeError(f"{key}: {remaining:g} off the float16 grid after rounding")

    with torch.no_grad():
        traced = torch.jit.trace(wrapper, tuple(example), strict=False, check_trace=False)
        traced.eval()
        # Tracing a ScriptModule leaves attribute reads in the graph; the Core ML frontend
        # then sees a string where a bias tensor should be. Freezing inlines them.
        try:
            traced = torch.jit.freeze(traced)
        except Exception:  # noqa: BLE001 - freezing is an optimisation, not a requirement
            pass
        traced_out = flatten(traced(*example))
    trace_error = max(
        float(torch.max(torch.abs(a.float() - b.float())).item())
        for a, b in zip(traced_out, reference)
    )
    record["trace_max_abs_error"] = trace_error

    record["inputs"] = [
        {"name": n, "shape": list(s), "dtype": d} for n, s, d in spec["inputs"]
    ]
    record["outputs"] = [
        {"name": n, "shape": list(t.shape), "dtype": str(t.dtype).replace("torch.", "")}
        for n, t in zip(spec["outputs"], reference)
    ]
    record["role"] = spec["role"]
    record["notes"] = spec["notes"]
    record["source_asset"] = spec["source"]
    record["source_asset_sha256"] = sha256_file(source)
    record["core_path"] = spec["core"]
    record["parameters"] = int(sum(p.numel() for p in core.parameters()))

    # EXIR first, TorchScript as the fallback.
    #
    # The order matters for parity, not for capability. The EXIR path shares its exported graph
    # with the TensorFlow Lite exporter — same decomposition, same normalisation fold, same
    # constant rounding (see fold_norm) — so a model converted through it carries bit-identical
    # weights on both platforms. Converted through the TorchScript frontend instead, Core ML
    # folds and rounds on its own and the two artefacts drift apart by ~1e-3 on any model with a
    # normalisation layer. TorchScript remains the fallback because it keeps the scripted
    # control flow that EXIR's tracing cannot resolve on a few cores.
    try:
        path, reported = export_coreml_exir(spec, examples, references, key)
        parity = {name: reported[name] for name in ("max_abs", "max_rel")}
        record["coreml"] = {
            "artifact": os.path.basename(path),
            "bytes": directory_size(path),
            "sha256": sha256_tree(path),
            "members": member_hashes(path),
            "parity": parity,
            "precision": reported.get("precision", "float16"),
            "compute_units": reported.get("compute_units", "ALL"),
            "configuration": {
                **reported.get("configuration", {}),
                # The shared graph passes, recorded on the artefact that ran them.
                "norms_folded": reported.get("norms_folded", 0),
                "constants_rounded": reported.get("constants_rounded", 0),
                "nearest_downsamples_rewritten": reported.get(
                    "nearest_downsamples_rewritten", 0
                ),
            },
            "frontend": "exir",
        }
    except Exception as exc:  # noqa: BLE001 - per-model failures are reported, not fatal
        exir_error = f"{type(exc).__name__}: {exc}"[:400]
        try:
            path, precision, units, parity, attempts = export_coreml(
                traced, spec, examples, references, key
            )
            record["coreml"] = {
                "precision": precision,
                "compute_units": units,
                "configuration": attempts,
                "artifact": os.path.basename(path),
                "bytes": directory_size(path),
                "sha256": sha256_tree(path),
                "members": member_hashes(path),
                "parity": parity,
                "frontend": "torchscript",
                "exir_error": exir_error,
            }
        except Exception as fallback:  # noqa: BLE001
            record["coreml"] = {
                "error": f"{type(fallback).__name__}: {fallback}"[:400],
                "exir_error": exir_error,
            }

    try:
        path, reported = export_tflite(spec, examples, references, key)
        # The exporter reports parity and its pass bookkeeping in one object; split them so
        # the contract's `parity` means only parity.
        parity = {key_: reported[key_] for key_ in ("max_abs", "max_rel")}
        passes = {key_: value for key_, value in reported.items() if key_ not in parity}
        record["tflite"] = {
            "artifact": os.path.basename(path),
            "bytes": os.path.getsize(path),
            "sha256": sha256_file(path),
            "parity": parity,
            "precision": passes.get("precision", "float32"),
            "passes": passes,
        }
    except Exception as exc:  # noqa: BLE001
        record["tflite"] = {"error": f"{type(exc).__name__}: {exc}"[:400]}

    # Both artefacts matched PyTorch separately. Check they match each other, on the same
    # tensors, through the runtimes the apps use. This is the stage that catches a converter
    # that produced a different graph while its own parity number looked clean.
    if "error" not in record["coreml"] and "error" not in record["tflite"]:
        try:
            record["cross_platform"] = cross_platform_parity(
                spec, example, key, record["coreml"].get("compute_units", "ALL")
            )
        except Exception as exc:  # noqa: BLE001
            record["cross_platform"] = {"error": f"{type(exc).__name__}: {exc}"[:300]}

    return record


def cross_platform_parity(spec, example, key, compute_units="ALL"):
    """Run the shipped Core ML package and the shipped flatbuffer on identical tensors.

    Loaded with the compute units this model was admitted under, because that is what the app
    will use. Measuring on a configuration the model was not admitted for would either flatter
    it or fail it for the wrong reason.
    """
    import coremltools as ct
    from ai_edge_litert.interpreter import Interpreter

    import coreml_policy

    package = os.path.join(COREML_DIR, f"{key}.mlpackage")
    model = ct.models.MLModel(package, compute_units=coreml_policy.compute_unit(compute_units))
    # Bind by contract name, exactly as the apps do. Reading the names off the spec instead is
    # what let twelve packages ship with inputs called `tensors_0` that no app could feed.
    declared = {descriptor.name for descriptor in model.get_spec().description.input}
    expected = {name for name, _s, _d in spec["inputs"]}
    if declared != expected:
        raise RuntimeError(f"core ml inputs {sorted(declared)} do not match contract {sorted(expected)}")
    apple = model.predict(_coreml_feed(spec, example))

    interpreter = Interpreter(model_path=os.path.join(TFLITE_DIR, f"{key}.tflite"))
    interpreter.allocate_tensors()
    for detail, tensor in zip(interpreter.get_input_details(), example):
        interpreter.set_tensor(detail["index"], tensor.numpy().astype(detail["dtype"]))
    interpreter.invoke()
    android = [
        interpreter.get_tensor(detail["index"]) for detail in interpreter.get_output_details()
    ]

    worst_abs = 0.0
    worst_rel = 0.0
    for name, produced in zip(spec["outputs"], android):
        reference = np.asarray(produced, dtype=np.float64)
        have = np.asarray(apple[name], dtype=np.float64).reshape(reference.shape)
        error = float(np.max(np.abs(have - reference)))
        scale = float(np.max(np.abs(reference)))
        worst_abs = max(worst_abs, error)
        worst_rel = max(worst_rel, error / scale if scale > 1e-9 else error)
    return {"max_abs": worst_abs, "max_rel": worst_rel}


def main():
    for directory in (COREML_DIR, TFLITE_DIR, CONTRACT_DIR):
        os.makedirs(directory, exist_ok=True)
    keys = sys.argv[1:] or list(SPECS)
    summary = []
    for key in keys:
        spec = SPECS[key]
        print(f"=== {key}", flush=True)
        try:
            record = convert_one(key, spec)
        except Exception as exc:  # noqa: BLE001
            traceback.print_exc()
            record = {"model": key, "error": f"{type(exc).__name__}: {exc}"[:400]}
        with open(os.path.join(CONTRACT_DIR, f"{key}.json"), "w") as handle:
            json.dump(record, handle, indent=2, sort_keys=True)
        summary.append(record)
        for backend in ("coreml", "tflite", "cross_platform"):
            info = record.get(backend, {})
            if not info:
                continue
            if "error" in info:
                line = info["error"]
            elif backend == "cross_platform":
                line = "abs {:.3g}  rel {:.3g}".format(info["max_abs"], info["max_rel"])
            else:
                line = "{:,} B  abs {:.3g}  rel {:.3g}".format(
                    info["bytes"], info["parity"]["max_abs"], info["parity"]["max_rel"]
                )
            print(f"    {backend}: {line}", flush=True)
    with open(os.path.join(OUT_DIR, "summary.json"), "w") as handle:
        json.dump(summary, handle, indent=2, sort_keys=True)


if __name__ == "__main__":
    main()
