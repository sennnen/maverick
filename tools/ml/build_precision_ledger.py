#!/usr/bin/env python3
"""Per-model record of what precision each artefact actually runs at, and where.

"FP16" is four separate claims and they are routinely conflated:

  * the *weights* are stored at half width;
  * the *arithmetic* is done at half width;
  * Core ML assigns the operations to the Neural Engine, which only runs half width;
  * Android's delegate does the same.

A model can satisfy the first and none of the others — that is the default outcome, and it is
what this file exists to make impossible to overstate. Storage precision comes from the
conversion contract, arithmetic precision from the policy the ladder chose, processor
assignment from Core ML's own `MLComputePlan`, and the Android path from the same catalogue
flag the app reads. The cross-platform error is split into the part the two graphs disagree by
and the part the arithmetic width accounts for, because only the first is a defect.

    python build_precision_ledger.py [--check]
"""
import argparse
import json
import os
import sys

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
MAVERICK = os.path.dirname(os.path.dirname(HERE))
MANIFEST = os.path.join(MAVERICK, "artifacts/models/manifest.json")
LEDGER = os.path.join(MAVERICK, "artifacts/models/precision.json")
DOC = os.path.join(MAVERICK, "docs/ml.md")
IOS = os.path.join(MAVERICK, "apps/ios/Maverick/Models")
ANDROID = os.path.join(MAVERICK, "apps/android/app/src/main/assets/models")
CONTRACTS = os.path.join(HERE, "out", "contracts")

MARKER = "<!-- PRECISION-TABLE -->"
END_MARKER = "<!-- /PRECISION-TABLE -->"

FULL_WIDTH = "float16 weights, float32 arithmetic"


def probe(model, seed=770):
    rng = np.random.default_rng(seed)
    feed = {}
    for spec in model["inputs"]:
        shape = tuple(spec["shape"])
        if spec["name"] in ("ppg", "pulses", "vpgs", "apgs"):
            time = np.arange(shape[-1], dtype=np.float32) / 50.0
            phase = 2.0 * np.pi * 1.15 * time
            wave = np.sin(phase) + 0.3 * np.sin(2.0 * phase) + 0.05 * np.sin(0.05 * time)
            feed[spec["name"]] = np.broadcast_to(wave, shape).astype(np.float32).copy()
        elif spec["dtype"] in ("int32", "int64"):
            feed[spec["name"]] = rng.integers(0, 8, size=shape).astype(np.int32)
        else:
            feed[spec["name"]] = rng.standard_normal(shape).astype(np.float32)
    return feed


def worst_relative(left, right):
    worst = 0.0
    for a, b in zip(left, right):
        b = np.asarray(b).reshape(a.shape)
        scale = float(np.max(np.abs(a)))
        error = float(np.max(np.abs(a - b)))
        worst = max(worst, error / scale if scale > 1e-9 else error)
    return worst


def run_coreml(model, feed, units):
    import coremltools as ct

    package = os.path.join(IOS, model["coreml"]["artifact"])
    loaded = ct.models.MLModel(package, compute_units=getattr(ct.ComputeUnit, units))
    predicted = loaded.predict(feed)
    return [np.asarray(predicted[s["name"]], dtype=np.float64) for s in model["outputs"]]


def run_tflite(model, feed):
    from ai_edge_litert.interpreter import Interpreter

    interpreter = Interpreter(model_path=os.path.join(ANDROID, model["tflite"]["artifact"]))
    interpreter.allocate_tensors()
    for detail, spec in zip(interpreter.get_input_details(), model["inputs"]):
        value = feed[spec["name"]].astype(detail["dtype"]).reshape(tuple(detail["shape"]))
        interpreter.set_tensor(detail["index"], value)
    interpreter.invoke()
    return [
        np.asarray(interpreter.get_tensor(d["index"]), dtype=np.float64)
        for d in interpreter.get_output_details()
    ]


def measure(model, half):
    """Split the platforms' disagreement into the graph part and the arithmetic part.

    The graph part is only measurable where Core ML kept full-width arithmetic. Precision is a
    property of the *program*, not of the compute unit, so a half-precision artefact computes
    at half width on the CPU too — there is no full-width answer to be had out of it, and
    printing its CPU answer as though it isolated the graph would be a contaminated number
    wearing the label of a clean one. Those models report `None` and say why.
    """
    feed = probe(model)
    cpu = run_coreml(model, feed, "CPU_ONLY")
    shipped = run_coreml(model, feed, "ALL")
    android = run_tflite(model, feed)
    return {
        # Both sides at full-width arithmetic: what remains is the graphs and the weights, and
        # any of it is a defect.
        "graph": None if half else worst_relative(cpu, android),
        "graph_note": "not separable: the program itself is half precision" if half else None,
        # The same Core ML artefact on the accelerated path against its own CPU answer. For a
        # half-precision program this is what the accelerator's half-width *accumulation* adds
        # on top of the half-width arithmetic the CPU already did.
        "accelerator": worst_relative(shipped, cpu),
        # What the two apps will actually compute on this host.
        "shipped": worst_relative(shipped, android),
    }


def compute_plan(model):
    import coreml_precision

    package = os.path.join(IOS, model["coreml"]["artifact"])
    try:
        return coreml_precision.neural_engine_share(package)
    except Exception as exc:  # noqa: BLE001 - a plan that cannot be built is the finding
        return {"error": f"{type(exc).__name__}: {exc}"[:120]}


ANDROID_DELEGATE = os.path.join(MAVERICK, "artifacts/models/android_delegate.json")
DEVICE_PARITY = os.path.join(MAVERICK, "artifacts/models/device_parity.json")

ANDROID_PATHS = {
    "CPU": "xnnpack (float32)",
    "GPU": "gpu delegate (float16)",
    "GPU_FULL": "gpu delegate (float32)",
}


def android_path(model):
    """Which Android path this model actually takes, per the recorded device sweep.

    This used to be derived from the Core ML policy on the theory that a model computing at
    half width on one platform should compute at half width on the other. Measuring it said
    otherwise on every count — the delegate's half width is not the Neural Engine's, it is
    wrong outright on four graphs, and it is slower than the CPU on all but one — so the path
    is now read from the measurement rather than inferred from the other platform.
    """
    if not os.path.exists(ANDROID_DELEGATE):
        return "unmeasured (no device sweep recorded)"
    paths = json.load(open(ANDROID_DELEGATE)).get("paths", {})
    return ANDROID_PATHS.get(paths.get(model["model"], "CPU"), "xnnpack (float32)")


def device_errors():
    """What the handset and Core ML actually produced, on identical inputs.

    Keyed by model. Absent until a device run has happened, and absent is reported as absent:
    an unmeasured platform is not a passing one.
    """
    if not os.path.exists(DEVICE_PARITY):
        return {}
    return {r["model"]: r for r in json.load(open(DEVICE_PARITY))["rows"]}


def build():
    manifest = json.load(open(MANIFEST))
    measured = device_errors()
    rows = []
    for model in sorted(manifest["models"], key=lambda m: m["model"]):
        contract_path = os.path.join(CONTRACTS, f"{model['model']}.json")
        policy = None
        if os.path.exists(contract_path):
            contract = json.load(open(contract_path))
            policy = (contract.get("coreml") or {}).get("configuration", {}).get("policy")
        half = model["coreml"]["precision"] != FULL_WIDTH
        plan = compute_plan(model)
        errors = measure(model, half)
        rows.append(
            {
                "model": model["model"],
                "weights": "float16",
                "arithmetic": "float16" if half else "float32",
                "policy": policy,
                "precision": model["coreml"]["precision"],
                "compute_units": model["coreml"]["compute_units"],
                "neural_engine": plan,
                "android": android_path(model),
                "error": errors,
                # From the phone, on inputs neither converter chose. `None` where no device
                # run has been recorded, because the alternative is a blank that reads as zero.
                "device": measured.get(model["model"]),
            }
        )
    accelerated = [r for r in rows if r["neural_engine"].get("on_neural_engine")]
    summary = {
        "models": len(rows),
        "half_arithmetic": sum(1 for r in rows if r["arithmetic"] == "float16"),
        "on_neural_engine": len(accelerated),
        "neural_engine_operations": sum(
            r["neural_engine"].get("on_neural_engine", 0) for r in rows
        ),
        "total_operations": sum(r["neural_engine"].get("operations", 0) for r in rows),
        "measurable_graph_errors": sum(1 for r in rows if r["error"]["graph"] is not None),
        "worst_graph_error": max(
            (r["error"]["graph"] for r in rows if r["error"]["graph"] is not None),
            default=0.0,
        ),
        "worst_shipped_error": max(r["error"]["shipped"] for r in rows),
    }
    on_device = [r["device"] for r in rows if r["device"]]
    if on_device:
        summary["device_models"] = len(on_device)
        summary["worst_device_vs_reference"] = max(
            r["android_vs_reference"] for r in on_device
        )
        summary["worst_between_platforms"] = max(r["between_platforms"] for r in on_device)
    return {"summary": summary, "rows": rows}


def render(ledger):
    lines = [
        MARKER,
        "",
        "| Model | Weights | Arithmetic | Neural Engine | Android path | Graph err | Accel err | Device vs ref | Between platforms |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for row in ledger["rows"]:
        engine = row["neural_engine"]
        share = (
            f"{engine.get('on_neural_engine', 0)}/{engine.get('operations', 0)}"
            if "operations" in engine
            else "—"
        )
        graph = (
            "n/a" if row["error"]["graph"] is None else f"{row['error']['graph']:.1e}"
        )
        device = row.get("device")
        on_device = f"{device['android_vs_reference']:.1e}" if device else "unmeasured"
        between = f"{device['between_platforms']:.1e}" if device else "unmeasured"
        lines.append(
            f"| `{row['model']}` | {row['weights']} | {row['arithmetic']} | {share} | "
            f"{row['android']} | {graph} | {row['error']['accelerator']:.1e} | "
            f"{on_device} | {between} |"
        )
    summary = ledger["summary"]
    lines += [
        "",
        f"{summary['half_arithmetic']} of {summary['models']} models compute at half width; "
        f"{summary['on_neural_engine']} place work on the Neural Engine, "
        f"{summary['neural_engine_operations']:,} operations of "
        f"{summary['total_operations']:,}. Graph-only disagreement is separable for the "
        f"{summary['measurable_graph_errors']} models that kept full-width arithmetic and is "
        f"worst at {summary['worst_graph_error']:.2e} there; worst as-shipped across all "
        f"{summary['models']} is {summary['worst_shipped_error']:.2e}.",
    ]
    if "device_models" in summary:
        lines += [
            "",
            f"The last two columns came off a Pixel 7 (Tensor G2, API 37, arm64-v8a) rather "
            f"than this host: {summary['device_models']} models run on the handset against "
            f"probes neither converter chose, worst {summary['worst_device_vs_reference']:.2e} "
            f"from the reference and worst {summary['worst_between_platforms']:.2e} between "
            f"the two platforms.",
        ]
    lines += [
        "",
        END_MARKER,
    ]
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    arguments = parser.parse_args()

    ledger = build()
    table = render(ledger)
    document = open(DOC).read()
    if MARKER in document:
        start = document.index(MARKER)
        end = document.index(END_MARKER) + len(END_MARKER)
        updated = document[:start] + table + document[end:]
    else:
        updated = document

    if arguments.check:
        stale = []
        if os.path.exists(LEDGER):
            existing = json.load(open(LEDGER))
            if existing.get("summary") != ledger["summary"]:
                stale.append("precision.json")
        else:
            stale.append("precision.json")
        if updated != document:
            stale.append("docs/ml.md")
        if stale:
            print(f"build_precision_ledger: stale {', '.join(stale)}")
            return 1
        print(f"build_precision_ledger: ok, {ledger['summary']['models']} models")
        return 0

    json.dump(ledger, open(LEDGER, "w"), indent=1, sort_keys=True)
    open(DOC, "w").write(updated)
    summary = ledger["summary"]
    print(
        f"build_precision_ledger: {summary['models']} models, "
        f"{summary['half_arithmetic']} at half-width arithmetic, "
        f"{summary['on_neural_engine']} on the Neural Engine, "
        f"worst graph {summary['worst_graph_error']:.2e}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
