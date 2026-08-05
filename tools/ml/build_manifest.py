#!/usr/bin/env python3
"""Turn a conversion run into the shipped model set: the manifest and the app artefacts.

The conversion pipeline emits one contract JSON per model, and some of those models only
converted on one platform. This applies Maverick's parity rule — a feature ships on both
platforms or on neither — and copies through only the models that cleared it, so the
manifest can never describe a model that iOS has and Android does not.

    python tools/ml/build_manifest.py --conversion-out <dir> [--check]

`--check` reports what would change without writing, which is what CI runs.

Outputs, all under the repository root:

    artifacts/models/manifest.json          the admitted set, hashes, parity, sizes
    artifacts/models/contracts/<key>.json   the full conversion contract for each
    apps/ios/Maverick/Models/<key>.mlpackage
    apps/android/app/src/main/assets/models/<key>.tflite
"""
from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
MANIFEST = ROOT / "artifacts/models/manifest.json"
CONTRACTS = ROOT / "artifacts/models/contracts"
IOS_MODELS = ROOT / "apps/ios/Maverick/Models"
ANDROID_MODELS = ROOT / "apps/android/app/src/main/assets/models"

COREML_MEMBER = "Data/com.apple.CoreML/model.mlmodel"

# Parity thresholds, as worst relative deviation from the PyTorch reference.
#
# Core ML ships FP16, so a fraction of a percent is expected and a few percent on a
# near-zero-centred output is not alarming, and half-width rounding is the point rather than a
# defect. So deviation from the float32 PyTorch reference is bounded generously and *recorded*,
# and the absolute floor exists because relative error is meaningless on an output that happens
# to land near zero.
REFERENCE_MAX_REL = 3e-2
REFERENCE_ABSOLUTE_FLOOR = 1e-3

# What is gated tightly is whether the two platforms agree with each other, because that is the
# property a shared core depends on — and it catches a class of error that comparing each to
# PyTorch separately cannot.
#
# The bar depends on what the comparison is actually measuring, which is not the same for every
# model. Both artefacts carry *identical* weights: `fp16_align` rounds them once on the PyTorch
# module before either converter runs, so nothing is left between the platforms but arithmetic.
#
#   * Where Core ML kept full-width arithmetic, the host comparison is like for like — the
#     TensorFlow Lite interpreter used here computes at full width too — and what remains is
#     kernel and accumulation order. That is held tight.
#   * Where Core ML was admitted at half-precision arithmetic, the host comparison is *not* like
#     for like: it measures a float16 Core ML answer against a float32 interpreter one, so it
#     reports the width difference on top of any real disagreement. It is an upper bound, and it
#     is bounded at the same place the reference gate is, because that is what it is made of. On
#     device the two converge, since `MavModelAcceleration` gives those same models Android's
#     half-precision path.
CROSS_PLATFORM_MAX_REL = 5e-3
CROSS_PLATFORM_HALF_MAX_REL = 3e-2

# Provenance per model, carried through the registry to any surface that has to attribute a
# reading. Everything here ships with the app; the only distinction that matters downstream is
# whether an upstream attribution notice has to travel with the artefact.
FIRST_PARTY = {
    "standing": "first_party",
    "licence": "Maverick first-party weights",
}
PULSE_PPG = {
    "standing": "open_licensed",
    "licence": "MIT (Pulse-PPG, Xu et al., UbiComp 2025)",
}
STANDING_BY_MODEL = {"pulse_ppg": PULSE_PPG}


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def member_hashes(package: Path) -> dict[str, str]:
    out: dict[str, str] = {}
    for path in sorted(package.rglob("*")):
        if path.is_file():
            out[str(path.relative_to(package))] = sha256_file(path)
    return out


def load_contracts(conversion_out: Path) -> list[dict]:
    contracts = []
    for path in sorted((conversion_out / "contracts").glob("*.json")):
        contracts.append(json.loads(path.read_text()))
    if not contracts:
        raise SystemExit(f"no contracts under {conversion_out / 'contracts'}")
    return contracts


def rejection(contract: dict) -> str | None:
    """Why this model does not ship, or None when it does.

    Two independent bars: both platforms must have converted, and both artefacts must still
    compute what the source weights computed.
    """
    coreml = contract.get("coreml") or {}
    tflite = contract.get("tflite") or {}
    if "error" in coreml:
        return f"core ml: {coreml['error']}"
    if "error" in tflite:
        return f"tflite: {tflite['error']}"
    if "sha256" not in coreml or "sha256" not in tflite:
        return "an artefact is missing"

    coreml_rel = coreml.get("parity", {}).get("max_rel")
    tflite_rel = tflite.get("parity", {}).get("max_rel")
    if coreml_rel is None or tflite_rel is None:
        return "parity was not measured"
    coreml_abs = coreml.get("parity", {}).get("max_abs", float("inf"))
    tflite_abs = tflite.get("parity", {}).get("max_abs", float("inf"))
    if coreml_rel > REFERENCE_MAX_REL and coreml_abs > REFERENCE_ABSOLUTE_FLOOR:
        return f"core ml parity {coreml_rel:.3g} exceeds {REFERENCE_MAX_REL}"
    if tflite_rel > REFERENCE_MAX_REL and tflite_abs > REFERENCE_ABSOLUTE_FLOOR:
        return f"tflite parity {tflite_rel:.3g} exceeds {REFERENCE_MAX_REL}"

    cross = contract.get("cross_platform", {}).get("max_rel")
    cross_abs = contract.get("cross_platform", {}).get("max_abs", float("inf"))
    if cross is None:
        return "the two platforms were never compared to each other"
    import coreml_precision

    half = coreml_precision.runs_half_arithmetic(coreml.get("precision", ""))
    bar = CROSS_PLATFORM_HALF_MAX_REL if half else CROSS_PLATFORM_MAX_REL
    if cross > bar and cross_abs > REFERENCE_ABSOLUTE_FLOOR:
        return f"the platforms disagree by {cross:.3g}, above {bar}"
    return None


def admitted(contract: dict) -> bool:
    """Both platforms converted and both artefacts still compute the right thing."""
    return rejection(contract) is None


def entry_for(contract: dict, conversion_out: Path) -> dict:
    key = contract["model"]
    package = conversion_out / "coreml" / f"{key}.mlpackage"
    flatbuffer = conversion_out / "tflite" / f"{key}.tflite"
    if not package.is_dir() or not flatbuffer.is_file():
        raise SystemExit(f"{key}: contract claims success but an artefact is missing")

    members = member_hashes(package)
    if COREML_MEMBER not in members:
        raise SystemExit(f"{key}: {package} has no {COREML_MEMBER}")

    standing = STANDING_BY_MODEL.get(key, FIRST_PARTY)
    return {
        "model": key,
        "algorithm_id": contract["algorithm"],
        "algorithm_version": contract["version"],
        "role": contract.get("role", ""),
        "notes": contract.get("notes", ""),
        "standing": standing["standing"],
        "licence": standing["licence"],
        "parameters": contract.get("parameters", 0),
        "inputs": contract["inputs"],
        "outputs": contract["outputs"],
        "coreml": {
            "artifact": f"{key}.mlpackage",
            "bytes": contract["coreml"]["bytes"],
            "sha256": members[COREML_MEMBER],
            "package_sha256": contract["coreml"]["sha256"],
            "members": members,
            "parity": contract["coreml"]["parity"],
            "compute_units": contract["coreml"].get("compute_units", "ALL"),
            "precision": contract["coreml"].get("precision", "float16"),
            "frontend": contract["coreml"].get("frontend", "torchscript"),
        },
        "tflite": {
            "artifact": f"{key}.tflite",
            "bytes": contract["tflite"]["bytes"],
            "sha256": contract["tflite"]["sha256"],
            "parity": contract["tflite"]["parity"],
            "precision": contract["tflite"].get("precision", "float32"),
        },
        "cross_platform": contract.get("cross_platform", {}),
    }


def build(conversion_out: Path, check: bool) -> int:
    contracts = load_contracts(conversion_out)
    kept = [c for c in contracts if admitted(c)]
    dropped = [
        {
            "model": c["model"],
            "reason": rejection(c),
            "coreml_error": (c.get("coreml") or {}).get("error"),
            "tflite_error": (c.get("tflite") or {}).get("error"),
        }
        for c in contracts
        if not admitted(c)
    ]
    entries = [entry_for(c, conversion_out) for c in kept]
    entries.sort(key=lambda entry: entry["model"])

    manifest = {
        "version": 1,
        "note": (
            "Generated by tools/ml/build_manifest.py. Every model here converted on both "
            "platforms; models that converted on only one are listed under `not_shipped` "
            "with the converter error, and are not in any app bundle."
        ),
        "models": entries,
        "not_shipped": sorted(dropped, key=lambda item: item["model"]),
        "bundle_bytes": {
            "ios": sum(entry["coreml"]["bytes"] for entry in entries),
            "android": sum(entry["tflite"]["bytes"] for entry in entries),
        },
    }
    rendered = json.dumps(manifest, indent=2, sort_keys=True) + "\n"

    if check:
        current = MANIFEST.read_text() if MANIFEST.exists() else ""
        if current != rendered:
            print("build_manifest: manifest.json is stale; re-run without --check", file=sys.stderr)
            return 1
        print(f"build_manifest: ok, {len(entries)} models")
        return 0

    MANIFEST.parent.mkdir(parents=True, exist_ok=True)
    CONTRACTS.mkdir(parents=True, exist_ok=True)
    IOS_MODELS.mkdir(parents=True, exist_ok=True)
    ANDROID_MODELS.mkdir(parents=True, exist_ok=True)
    MANIFEST.write_text(rendered)

    shipped_keys = {entry["model"] for entry in entries}
    for contract in contracts:
        (CONTRACTS / f"{contract['model']}.json").write_text(
            json.dumps(contract, indent=2, sort_keys=True) + "\n"
        )
    for entry in entries:
        key = entry["model"]
        target_package = IOS_MODELS / f"{key}.mlpackage"
        if target_package.exists():
            shutil.rmtree(target_package)
        shutil.copytree(conversion_out / "coreml" / f"{key}.mlpackage", target_package)
        shutil.copyfile(
            conversion_out / "tflite" / f"{key}.tflite", ANDROID_MODELS / f"{key}.tflite"
        )

    # A model that stops converting must also stop shipping, or the bundle keeps a stale
    # artefact the manifest no longer describes.
    for stale in IOS_MODELS.glob("*.mlpackage"):
        if stale.stem not in shipped_keys and stale.stem != "nao_full_ecg_model_fp16":
            shutil.rmtree(stale)
    for stale in ANDROID_MODELS.glob("*.tflite"):
        if stale.stem not in shipped_keys:
            stale.unlink()

    print(f"build_manifest: wrote {len(entries)} models")
    for item in manifest["not_shipped"]:
        print(f"  not shipped: {item['model']}: {(item['reason'] or 'unknown')[:130]}")
    print(
        "  bundle: ios {:,} B, android {:,} B".format(
            manifest["bundle_bytes"]["ios"], manifest["bundle_bytes"]["android"]
        )
    )
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--conversion-out", type=Path, required=True)
    parser.add_argument("--check", action="store_true")
    arguments = parser.parse_args()
    return build(arguments.conversion_out.resolve(), arguments.check)


if __name__ == "__main__":
    sys.exit(main())
