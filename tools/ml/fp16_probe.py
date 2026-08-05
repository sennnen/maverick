#!/usr/bin/env python3
"""What float16 arithmetic actually costs, per model, measured rather than assumed.

The pipeline converted at float32 arithmetic for a reason: the PulseNet encoder was measured
1.6e-1 from its reference at float16 on the accelerated path. But `compute_plan.py` shows the
price of that choice is total — under float32 arithmetic Core ML puts *zero* operations on the
Neural Engine, on every model in the zoo. Half-precision hardware will not run a
full-precision program, so "float32 for safety" is really "no accelerator at all".

So the question is not whether float16 arithmetic is free. It is which operations inside a
given model cannot survive it, and whether pinning those few back to float32 recovers the
accuracy while leaving the rest on the accelerator.

This script answers that for one model at a time: convert under each policy, measure the
artefact on each compute unit, and report. `coreml_precision.py` holds the policies.
"""
import json
import os
import sys

import numpy as np
import torch

import coreml_precision
from convert import CoreWrapper, flatten, get_core, make_inputs
from specs import SPECS

MODELS_DIR = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "decrypted_models")
UNITS = ("CPU_ONLY", "CPU_AND_NE", "ALL")


def build(key):
    spec = SPECS[key]
    model = torch.jit.load(os.path.join(MODELS_DIR, spec["source"]), map_location="cpu")
    model.eval()
    if spec.get("rebuild"):
        import rebuilt_cores

        core = rebuilt_cores.rebuild(model, spec["core"], spec["rebuild"], spec["rebuild_config"])
    else:
        core = get_core(model, spec["core"])
    wrapper = CoreWrapper(
        core,
        spec["const_args"],
        spec.get("core_method"),
        spec.get("arg_template"),
        [name for name, _s, _d in spec["inputs"]],
    ).eval()
    probes = [make_inputs(spec, seed=index) for index in range(3)]
    with torch.no_grad():
        references = [flatten(wrapper(*probe)) for probe in probes]
    import fp16_align

    fp16_align.round_to_half(core)
    with torch.no_grad():
        traced = torch.jit.trace(wrapper, tuple(probes[0]), strict=False, check_trace=False)
        traced.eval()
        try:
            traced = torch.jit.freeze(traced)
        except Exception:  # noqa: BLE001
            pass
    return spec, traced, probes, references


def measure(package, spec, probes, references, units):
    import coremltools as ct

    loaded = ct.models.MLModel(package, compute_units=getattr(ct.ComputeUnit, units))
    worst = 0.0
    for probe, reference in zip(probes, references):
        feed = {}
        for (name, _shape, dtype), tensor in zip(spec["inputs"], probe):
            feed[name] = tensor.numpy().astype(np.int32 if dtype == "int64" else np.float32)
        predicted = loaded.predict(feed)
        for name, want in zip(spec["outputs"], reference):
            want = want.numpy().astype(np.float64)
            have = np.asarray(predicted[name], dtype=np.float64).reshape(want.shape)
            error = float(np.max(np.abs(have - want)))
            scale = float(np.max(np.abs(want)))
            worst = max(worst, error / scale if scale > 1e-9 else error)
    return worst


def main():
    import coremltools as ct

    import ct_ops

    ct_ops.install()
    results = {}
    for key in sys.argv[1:]:
        spec, traced, probes, references = build(key)
        inputs = [
            ct.TensorType(
                name=name,
                shape=tuple(shape),
                dtype=np.int32 if dtype == "int64" else np.float32,
            )
            for name, shape, dtype in spec["inputs"]
        ]
        for policy in coreml_precision.POLICIES:
            path = f"/tmp/fp16probe_{key}_{policy}.mlpackage"
            try:
                converted = ct.convert(
                    traced,
                    inputs=inputs,
                    outputs=[ct.TensorType(name=name) for name in spec["outputs"]],
                    convert_to="mlprogram",
                    compute_precision=coreml_precision.precision(policy),
                    minimum_deployment_target=ct.target.iOS16,
                    pass_pipeline=coreml_precision.pipeline(policy),
                )
                import shutil

                shutil.rmtree(path, ignore_errors=True)
                converted.save(path)
            except Exception as exc:  # noqa: BLE001
                print(f"{key:28s} {policy:16s} convert failed: {type(exc).__name__}: {exc}"[:150])
                continue
            row = {}
            for units in UNITS:
                try:
                    row[units] = measure(path, spec, probes, references, units)
                except Exception as exc:  # noqa: BLE001
                    row[units] = f"error: {type(exc).__name__}"
            size = sum(
                os.path.getsize(os.path.join(base, name))
                for base, _dirs, names in os.walk(path)
                for name in names
            )
            rendered = "  ".join(
                f"{unit}={value if isinstance(value, str) else f'{value:.2e}'}"
                for unit, value in row.items()
            )
            print(f"{key:28s} {policy:16s} {size/1e6:6.2f} MB  {rendered}")
            results.setdefault(key, {})[policy] = {"bytes": size, "parity": row}
    # Beside the script, for the same reason `compute_sweep.py` is: where the probe is run from
    # is not supposed to decide where its record lands.
    out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fp16_probe.json")
    json.dump(results, open(out, "w"), indent=1, sort_keys=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
