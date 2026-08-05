#!/usr/bin/env python3
"""Where a model's cross-platform error is actually made.

A single parity number says two artefacts disagree; it does not say where, and without that
the only available responses are to loosen the bar or to change something and hope. This cuts
the Core ML program at every operation and measures each cut against the same point in a
float32 PyTorch reference, so the error can be attributed to the operation that introduces it.

The output is a per-operation error curve. What matters in it is not the final number but the
*steps*: a curve that rises smoothly is accumulation, which is inherent to the arithmetic
width, and a curve with a jump at one operation is that operation, which is usually fixable.

    python parity_decompose.py <model> [--policy half]
"""
import argparse
import json
import os
import sys

import numpy as np

MAVERICK = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
IOS = os.path.join(MAVERICK, "apps/ios/Maverick/Models")
MANIFEST = os.path.join(MAVERICK, "artifacts/models/manifest.json")


def operations(model):
    """Every non-constant operation output, in program order."""
    spec = model.get_spec()
    function = spec.mlProgram.functions["main"]
    block = function.block_specializations[list(function.block_specializations)[0]]
    ordered = []
    for op in block.operations:
        if op.type in ("const", "constexpr_cast"):
            continue
        for output in op.outputs:
            ordered.append((op.type, output.name))
    return ordered


def reference_intermediates(traced, example, wanted):
    """Run the traced module in float64 and keep every intermediate it produces.

    Float64 rather than float32 so the reference is not itself a source of the error being
    attributed — the question is how far each artefact drifts from exact arithmetic.
    """
    import torch

    captured = {}
    handles = []

    def hook(name):
        def record(_module, _inputs, output):
            if isinstance(output, torch.Tensor):
                captured[name] = output.detach().to(torch.float64).numpy()

        return record

    for name, module in traced.named_modules():
        handles.append(module.register_forward_hook(hook(name)))
    with torch.no_grad():
        traced(*example)
    for handle in handles:
        handle.remove()
    return captured


def cut_error(package, name, feed, reference):
    """How far the Core ML program's value at `name` sits from the reference at that point."""
    import coremltools as ct
    from coremltools.converters.mil.debugging_utils import extract_submodel

    source = ct.models.MLModel(package, compute_units=ct.ComputeUnit.CPU_ONLY)
    submodel = extract_submodel(source, outputs=[name])
    loaded = ct.models.MLModel(
        submodel.get_spec(),
        weights_dir=submodel.weights_dir,
        compute_units=ct.ComputeUnit.ALL,
    )
    produced = np.asarray(loaded.predict(feed)[name], dtype=np.float64)
    want = np.asarray(reference, dtype=np.float64).reshape(produced.shape)
    scale = float(np.max(np.abs(want)))
    error = float(np.max(np.abs(produced - want)))
    return error / scale if scale > 1e-9 else error, produced


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("model")
    parser.add_argument("--stride", type=int, default=1, help="sample every Nth operation")
    parser.add_argument("--limit", type=int, default=0, help="stop after this many cuts")
    arguments = parser.parse_args()

    import coremltools as ct

    manifest = json.load(open(MANIFEST))
    model = next(m for m in manifest["models"] if m["model"] == arguments.model)
    package = os.path.join(IOS, model["coreml"]["artifact"])
    loaded = ct.models.MLModel(package, compute_units=ct.ComputeUnit.CPU_ONLY)

    rng = np.random.default_rng(4242)
    feed = {}
    for spec in model["inputs"]:
        shape = tuple(spec["shape"])
        if spec["name"] in ("ppg", "pulses", "vpgs", "apgs"):
            time = np.arange(shape[-1], dtype=np.float32) / 50.0
            phase = 2.0 * np.pi * (68.0 / 60.0) * time
            wave = np.sin(phase) + 0.3 * np.sin(2.0 * phase) + 0.05 * np.sin(0.05 * time)
            feed[spec["name"]] = np.broadcast_to(wave, shape).astype(np.float32).copy()
        elif spec["dtype"] in ("int32", "int64"):
            feed[spec["name"]] = rng.integers(0, 8, size=shape).astype(np.int32)
        else:
            feed[spec["name"]] = rng.standard_normal(shape).astype(np.float32)

    ordered = operations(loaded)
    if arguments.stride > 1:
        ordered = ordered[:: arguments.stride]
    if arguments.limit:
        ordered = ordered[: arguments.limit]

    # The reference at each cut is the *full-width* Core ML program, which is the same graph
    # computed exactly — so a difference is the arithmetic and not a different topology.
    full = os.path.join(IOS, model["coreml"]["artifact"])
    print(f"{len(ordered)} cuts on {arguments.model}")
    previous = 0.0
    rows = []
    for index, (kind, name) in enumerate(ordered):
        try:
            from coremltools.converters.mil.debugging_utils import extract_submodel

            source = ct.models.MLModel(full, compute_units=ct.ComputeUnit.CPU_ONLY)
            submodel = extract_submodel(source, outputs=[name])
            exact = np.asarray(
                ct.models.MLModel(
                    submodel.get_spec(),
                    weights_dir=submodel.weights_dir,
                    compute_units=ct.ComputeUnit.CPU_ONLY,
                ).predict(feed)[name],
                dtype=np.float64,
            )
            accelerated = np.asarray(
                ct.models.MLModel(
                    submodel.get_spec(),
                    weights_dir=submodel.weights_dir,
                    compute_units=ct.ComputeUnit.ALL,
                ).predict(feed)[name],
                dtype=np.float64,
            )
        except Exception as exc:  # noqa: BLE001 - an uncuttable point is not evidence
            continue
        scale = float(np.max(np.abs(exact)))
        error = float(np.max(np.abs(exact - accelerated)))
        relative = error / scale if scale > 1e-9 else error
        step = relative - previous
        rows.append({"index": index, "op": kind, "name": name, "relative": relative, "step": step})
        marker = "  <<< step" if step > max(1e-4, previous * 0.5) else ""
        print(f"  [{index:4d}] {kind:26s} rel {relative:.3e}  step {step:+.3e}{marker}")
        previous = relative
    json.dump(rows, open(f"parity_decompose_{arguments.model}.json", "w"), indent=1)
    if rows:
        worst = max(rows, key=lambda r: r["step"])
        print(f"\nlargest single step: {worst['op']} at [{worst['index']}] +{worst['step']:.3e}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
