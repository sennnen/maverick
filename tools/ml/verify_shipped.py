#!/usr/bin/env python3
"""Independent check of the artefacts that actually shipped.

Reads the manifest, loads the bytes out of the two app bundles — not the conversion
output directory — and runs both on a fresh input the pipeline never saw. Core ML is
loaded on the compute units the manifest admitted it under, because that is what
`MavModelRunner` will do.

Separate from the pipeline on purpose. The pipeline's own numbers were wrong once, for
three models, because parity was read off the converter's handle instead of the file; a
check that shares no code with it is the one that can catch that class of mistake again.
"""
import json
import os
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import coreml_precision

MAVERICK = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
MANIFEST = os.path.join(MAVERICK, "artifacts/models/manifest.json")
IOS = os.path.join(MAVERICK, "apps/ios/Maverick/Models")
ANDROID = os.path.join(MAVERICK, "apps/android/app/src/main/assets/models")

# Mirrors tools/ml/build_manifest.py, including its two tiers. Where Core ML kept full-width
# arithmetic the host comparison is like for like; where it runs half width, this interpreter
# does not, so the number carries the width difference on top of any real disagreement.
CROSS_PLATFORM_MAX_REL = 5e-3
CROSS_PLATFORM_HALF_MAX_REL = 3e-2

COMPUTE_UNITS = {
    "ALL": "ALL",
    "CPU_AND_NE": "CPU_AND_NE",
    "CPU_AND_GPU": "CPU_AND_GPU",
    "CPU_ONLY": "CPU_ONLY",
}


def probe(model, seed):
    """A fresh input per model: a pulse waveform for waveform inputs, noise otherwise."""
    rng = np.random.default_rng(seed)
    feed = {}
    for spec in model["inputs"]:
        shape = tuple(spec["shape"])
        if spec["name"] in ("ppg", "pulses", "vpgs", "apgs"):
            length = shape[-1]
            time = np.arange(length, dtype=np.float32) / 50.0
            phase = 2.0 * np.pi * (72.0 / 60.0) * time
            wave = np.sin(phase) + 0.25 * np.sin(2.0 * phase)
            feed[spec["name"]] = np.broadcast_to(wave, shape).astype(np.float32).copy()
        elif spec["dtype"] in ("int32", "int64"):
            # Whole numbers only, and small: one of these inputs is an embedding index, and a
            # negative or out-of-range value is a lookup fault rather than a parity result.
            feed[spec["name"]] = rng.integers(0, 8, size=shape).astype(np.float32)
        else:
            feed[spec["name"]] = rng.standard_normal(shape).astype(np.float32)
    return feed


def run_coreml(model, feed):
    import coremltools as ct

    units = getattr(ct.ComputeUnit, COMPUTE_UNITS[model["coreml"].get("compute_units", "ALL")])
    package = os.path.join(IOS, model["coreml"]["artifact"])
    loaded = ct.models.MLModel(package, compute_units=units)
    typed = {}
    for spec in model["inputs"]:
        value = feed[spec["name"]]
        typed[spec["name"]] = value.astype(np.int32 if spec["dtype"] == "int32" else np.float32)
    predicted = loaded.predict(typed)
    return [np.asarray(predicted[spec["name"]], dtype=np.float64) for spec in model["outputs"]]


def run_tflite(model, feed):
    from ai_edge_litert.interpreter import Interpreter

    interpreter = Interpreter(model_path=os.path.join(ANDROID, model["tflite"]["artifact"]))
    interpreter.allocate_tensors()
    for detail, spec in zip(interpreter.get_input_details(), model["inputs"]):
        value = feed[spec["name"]].astype(detail["dtype"]).reshape(tuple(detail["shape"]))
        interpreter.set_tensor(detail["index"], value)
    interpreter.invoke()
    return [
        np.asarray(interpreter.get_tensor(detail["index"]), dtype=np.float64)
        for detail in interpreter.get_output_details()
    ]


def main():
    manifest = json.load(open(MANIFEST))
    worst = 0.0
    worst_model = None
    failures = []
    for index, model in enumerate(sorted(manifest["models"], key=lambda m: m["model"])):
        # Three probes, not one. A single probe measures the model on one point of its input
        # space, and relative error on a near-zero output is dominated by cancellation rather
        # than by precision: the linear PPG-score head reads 4e-4 on one input and 7e-3 on
        # another, with the same weights, because the second output happens to sit near zero.
        relative = 0.0
        absolute = 0.0
        broken = None
        for attempt in range(3):
            feed = probe(model, seed=1000 + index * 10 + attempt)
            try:
                apple = run_coreml(model, feed)
                android = run_tflite(model, feed)
            except Exception as exc:  # noqa: BLE001 - a model that cannot run is the finding
                broken = f"{type(exc).__name__}: {exc}"[:140]
                break
            for left, right in zip(apple, android):
                right = right.reshape(left.shape)
                scale = float(np.max(np.abs(left)))
                error = float(np.max(np.abs(left - right)))
                absolute = max(absolute, error)
                relative = max(relative, error / scale if scale > 1e-9 else error)
        if broken:
            failures.append((model["model"], broken))
            print(f"{model['model']:32s} FAILED  {broken[:80]}")
            continue
        units = model["coreml"].get("compute_units", "ALL")
        half = coreml_precision.runs_half_arithmetic(model["coreml"].get("precision", ""))
        bar = CROSS_PLATFORM_HALF_MAX_REL if half else CROSS_PLATFORM_MAX_REL
        # The same bar `build_manifest.CROSS_PLATFORM_MAX_REL` gates on, so this check cannot
        # be stricter than the gate it is checking. Either measure passing counts: a tiny
        # absolute difference on an output whose scale is tiny is agreement, not disagreement.
        verdict = "" if (relative <= bar or absolute <= 1e-3) else "  <<< OVER"
        print(
            f"{model['model']:33s} {units:5s} {'fp16' if half else 'fp32':5s} "
            f"abs {absolute:.2e} rel {relative:.2e}{verdict}"
        )
        if relative > worst:
            worst, worst_model = relative, model["model"]
    print(f"\nworst: {worst:.3e} ({worst_model}) across {len(manifest['models'])} models")
    if failures:
        print(f"{len(failures)} model(s) could not be run: {[f[0] for f in failures]}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
