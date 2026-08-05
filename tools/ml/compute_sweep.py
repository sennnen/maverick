#!/usr/bin/env python3
"""Measure every shipped Core ML package on every compute unit.

`computeUnits = .all` is what the apps want: it lets the OS pick the Neural Engine when
it is free and fall back when it is not. Narrowing it is a real cost — a model pinned to
CPU_AND_NE cannot use the GPU even when that is the only accelerator available — so a
narrow admission has to be earned by a measurement, not assumed.

This loads the artefact that actually ships, four times, once per unit, on identical
inputs, and reports each unit's deviation from CPU_ONLY. CPU_ONLY is the reference here
rather than PyTorch because the question is not "is the model right" — the manifest gate
already answers that — but "does the answer depend on which silicon ran it".
"""
import json
import os
import sys

import numpy as np

MAVERICK = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
MANIFEST = os.path.join(MAVERICK, "artifacts/models/manifest.json")
IOS = os.path.join(MAVERICK, "apps/ios/Maverick/Models")

UNITS = ("CPU_ONLY", "CPU_AND_NE", "CPU_AND_GPU", "ALL")

# Mirrors coreml_precision.FULL_WIDTH_DESCRIPTION; kept as a literal so this script stays
# runnable without the conversion environment.
FULL_WIDTH = "float16 weights, float32 arithmetic"

# A unit whose answer differs from the CPU's by more than this is not admitted.
#
# Two bars, for the same reason build_manifest has two. Under full-width arithmetic every
# backend is computing the same thing and any real gap is a backend defect — that is how the
# sleep models' 1.5-relative GPU divergence was caught. Under half-width arithmetic the CPU
# accumulates at full width where the accelerators do not, so a gap of this order is the
# policy working rather than a defect.
UNIT_BAR = 5e-3
HALF_UNIT_BAR = 3e-2


def probe(model, seed):
    rng = np.random.default_rng(seed)
    feed = {}
    for spec in model["inputs"]:
        shape = tuple(spec["shape"])
        if spec["name"] in ("ppg", "pulses", "vpgs", "apgs"):
            time = np.arange(shape[-1], dtype=np.float32) / 50.0
            phase = 2.0 * np.pi * 1.2 * time
            wave = np.sin(phase) + 0.25 * np.sin(2.0 * phase)
            feed[spec["name"]] = np.broadcast_to(wave, shape).astype(np.float32).copy()
        elif spec["dtype"] in ("int32", "int64"):
            feed[spec["name"]] = rng.integers(0, 8, size=shape).astype(np.int32)
        else:
            feed[spec["name"]] = rng.standard_normal(shape).astype(np.float32)
    return feed


def run(package, units, feed, model):
    import coremltools as ct

    loaded = ct.models.MLModel(package, compute_units=getattr(ct.ComputeUnit, units))
    predicted = loaded.predict(feed)
    return [np.asarray(predicted[s["name"]], dtype=np.float64) for s in model["outputs"]]


def deviation(reference, other):
    worst = 0.0
    for left, right in zip(reference, other):
        scale = float(np.max(np.abs(left)))
        error = float(np.max(np.abs(left - np.asarray(right).reshape(left.shape))))
        worst = max(worst, error / scale if scale > 1e-9 else error)
    return worst


def sweep(model, probes=3):
    package = os.path.join(IOS, model["coreml"]["artifact"])
    spread = {}
    for index in range(probes):
        feed = probe(model, seed=7000 + index)
        try:
            reference = run(package, "CPU_ONLY", feed, model)
        except Exception as exc:  # noqa: BLE001 - a unit that refuses the model is data
            return {"CPU_ONLY": f"error: {type(exc).__name__}"}
        for units in UNITS[1:]:
            try:
                got = run(package, units, feed, model)
            except Exception as exc:  # noqa: BLE001
                spread[units] = f"error: {type(exc).__name__}"
                continue
            value = deviation(reference, got)
            if not isinstance(spread.get(units), str):
                spread[units] = max(spread.get(units, 0.0), value)
    spread["CPU_ONLY"] = 0.0
    return spread


def main():
    manifest = json.load(open(MANIFEST))
    wanted = set(sys.argv[1:])
    results = {}
    for model in sorted(manifest["models"], key=lambda m: m["model"]):
        if wanted and model["model"] not in wanted:
            continue
        spread = sweep(model)
        results[model["model"]] = spread
        precision = model["coreml"].get("precision", "")
        bar = UNIT_BAR if precision == FULL_WIDTH else HALF_UNIT_BAR
        rendered = "  ".join(
            f"{unit}={spread[unit] if isinstance(spread[unit], str) else f'{spread[unit]:.2e}'}"
            for unit in UNITS
            if unit in spread
        )
        safe = [
            unit
            for unit in UNITS
            if isinstance(spread.get(unit), float) and spread[unit] <= bar
        ]
        verdict = "ALL" if "ALL" in safe else (safe[-1] if safe else "NONE")
        print(f"{model['model']:34s} {verdict:12s} {rendered}")
    # Beside the script rather than in the working directory: run from the repository root this
    # dropped a second, differently-measured copy at the top level that nothing read.
    out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "compute_sweep.json")
    json.dump(results, open(out, "w"), indent=1, sort_keys=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
