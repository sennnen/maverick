#!/usr/bin/env python3
"""Core ML and a real Android handset, measured on the same inputs.

The manifest's cross-platform number compares two artefacts that both ran on this Mac. That is
a statement about the two *files*, and the thing it leaves out is the half of the system that
only exists on a phone: which delegate attached, what arithmetic width its driver chose, and
what it did to the answer. Comparing a Core ML result against a LiteRT result obtained on a
laptop cannot see any of that.

So the device writes its tensors out (`ModelZooParityInstrumentedTest`), `adb pull` brings
them back, and this puts them beside Core ML's own answer on the identical probes that
`device_vectors.py` generated. Three numbers per model:

    coreml   Core ML on the compute units the app admits, against eager PyTorch
    android  the handset, against the same PyTorch
    between  the two platforms against each other — the real cross-platform figure

`between` is the one the parity claim rests on, and it is the only one of the three that no
amount of host-side testing can produce.

    python device_compare.py [--json path]
"""
import argparse
import json
import os
import struct
import sys

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

MAVERICK = os.path.dirname(os.path.dirname(HERE))
MANIFEST = os.path.join(MAVERICK, "artifacts/models/manifest.json")
IOS = os.path.join(MAVERICK, "apps/ios/Maverick/Models")
VECTORS = os.path.join(MAVERICK, "apps/android/app/src/androidTest/assets/vectors")
PULLED = os.path.join(MAVERICK, "artifacts/models/device")
LEDGER = os.path.join(MAVERICK, "artifacts/models/device_parity.json")

MAGIC = b"MAVVEC01"


def read_groups(handle, counts):
    """`counts` tensors, each a little-endian length followed by that many floats."""
    out = []
    for _ in range(counts):
        (length,) = struct.unpack("<i", handle.read(4))
        out.append(np.frombuffer(handle.read(4 * length), dtype="<f4").astype(np.float64))
    return out


def read_vectors(slug, inputs, outputs):
    with open(os.path.join(VECTORS, f"{slug}.vec"), "rb") as handle:
        assert handle.read(8) == MAGIC, slug
        (probes,) = struct.unpack("<i", handle.read(4))
        return [
            (read_groups(handle, inputs), read_groups(handle, outputs), read_groups(handle, outputs))
            for _ in range(probes)
        ]


def read_device(slug, outputs):
    path = os.path.join(PULLED, f"{slug}.bin")
    if not os.path.exists(path):
        return None
    with open(path, "rb") as handle:
        assert handle.read(8) == MAGIC, slug
        (probes,) = struct.unpack("<i", handle.read(4))
        return [read_groups(handle, outputs) for _ in range(probes)]


def worst(produced, reference):
    """Largest absolute deviation and that deviation over the reference's own scale."""
    error = float(np.max(np.abs(produced - reference)))
    scale = float(np.max(np.abs(reference)))
    return error, (error / scale if scale > 1e-9 else error)


def run_coreml(model, feed, units="ALL"):
    import coremltools as ct

    loaded = ct.models.MLModel(
        os.path.join(IOS, model["coreml"]["artifact"]),
        compute_units=getattr(ct.ComputeUnit, units),
    )
    predicted = loaded.predict(feed)
    return [
        np.asarray(predicted[s["name"]], dtype=np.float64).ravel() for s in model["outputs"]
    ]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--json", default=LEDGER)
    arguments = parser.parse_args()

    manifest = json.load(open(MANIFEST))
    rows = []
    missing = []
    for model in sorted(manifest["models"], key=lambda m: m["model"]):
        slug = model["model"]
        probes = read_vectors(slug, len(model["inputs"]), len(model["outputs"]))
        device = read_device(slug, len(model["outputs"]))
        if device is None:
            missing.append(slug)
            continue
        worst_coreml = worst_android = worst_between = 0.0
        abs_between = 0.0
        for (inputs, expected, _host), produced in zip(probes, device):
            feed = {}
            for spec, flat in zip(model["inputs"], inputs):
                array = flat.reshape(tuple(spec["shape"]))
                feed[spec["name"]] = array.astype(
                    np.int32 if spec["dtype"] in ("int32", "int64") else np.float32
                )
            coreml = run_coreml(model, feed)
            for want, ios, android in zip(expected, coreml, produced):
                worst_coreml = max(worst_coreml, worst(ios, want)[1])
                worst_android = max(worst_android, worst(android, want)[1])
                between_abs, between_rel = worst(android, ios)
                worst_between = max(worst_between, between_rel)
                abs_between = max(abs_between, between_abs)
        rows.append(
            {
                "model": slug,
                "coreml_vs_reference": worst_coreml,
                "android_vs_reference": worst_android,
                "between_platforms": worst_between,
                "between_platforms_abs": abs_between,
            }
        )
        print(
            f"{slug:34s} coreml {worst_coreml:.3e}  android {worst_android:.3e}  "
            f"between {worst_between:.3e}"
        )

    if missing:
        print(f"\nno device output for {len(missing)}: {', '.join(missing)}", file=sys.stderr)
        return 1
    summary = {
        "models": len(rows),
        "worst_coreml_vs_reference": max(r["coreml_vs_reference"] for r in rows),
        "worst_android_vs_reference": max(r["android_vs_reference"] for r in rows),
        "worst_between_platforms": max(r["between_platforms"] for r in rows),
    }
    json.dump(
        {"summary": summary, "rows": rows}, open(arguments.json, "w"), indent=1, sort_keys=True
    )
    print(f"\n{json.dumps(summary, indent=1)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
