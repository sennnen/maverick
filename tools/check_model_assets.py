#!/usr/bin/env python3
"""Prove that each Maverick app ships exactly the admitted model set, and nothing else.

The admitted set is `artifacts/models/manifest.json` plus the one ECG classifier that predates
the zoo. Anything else under an app's source tree, or inside a built APK or `.app`, is a model
this build never validated — so the check is an equality, not a subset.

    tools/check_model_assets.py [--android-apk APK] [--ios-app APP]
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "artifacts/models/manifest.json"

# The provisional ECG classifier landed before the zoo and keeps its own contract in docs/ml.md.
# It is listed here rather than folded into the manifest because its conversion is not
# reproducible from tools/ml: the source weights are not in this repository.
ECG_SLUG = "nao_full_ecg_model_fp16"
ECG_ANDROID_RELATIVE = Path("apps/android/app/src/main/assets/ecg") / f"{ECG_SLUG}.tflite"
ECG_ANDROID_SHA256 = "0be97329077d5d5d2791b8b7850baf8acaf8f12f96fdfad7bcdb4af37156ea21"
ECG_IOS_RELATIVE = Path("apps/ios/Maverick/Models") / f"{ECG_SLUG}.mlpackage"
ECG_IOS_MEMBER_HASHES = {
    "Manifest.json": "2760ca6f4696a0519091fa43ee9ddbfae1bbda4e61fb85a5438d2cb3317ab288",
    "Data/com.apple.CoreML/model.mlmodel":
        "24230f1c635ab45fba02bb80c86f3b9a9dc2499436bda16a1d97b34ffb63f6d3",
    "Data/com.apple.CoreML/weights/weight.bin":
        "24111a56f73dc262cf600a73f18a647bf8ad623ecaa7336da5463e87325de0d9",
}

ANDROID_MODELS = Path("apps/android/app/src/main/assets/models")
IOS_MODELS = Path("apps/ios/Maverick/Models")
MODEL_SUFFIXES = (".tflite", ".onnx", ".mlmodel", ".mlpackage", ".mlmodelc", ".pt", ".pte")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_manifest() -> list[dict]:
    if not MANIFEST.is_file():
        raise SystemExit(f"check_model_assets: {MANIFEST} is missing")
    return json.loads(MANIFEST.read_text())["models"]


def source_failures(root: Path, models: list[dict]) -> list[str]:
    failures: list[str] = []

    expected_android = {root / ECG_ANDROID_RELATIVE}
    for model in models:
        expected_android.add(root / ANDROID_MODELS / f"{model['model']}.tflite")
    found_android = {
        path
        for path in (root / "apps/android/app/src/main").rglob("*")
        if path.is_file() and path.suffix.lower() in MODEL_SUFFIXES
    }
    for path in sorted(found_android - expected_android):
        failures.append(f"Android ships an unadmitted model: {path.relative_to(root)}")
    for path in sorted(expected_android - found_android):
        failures.append(f"Android is missing an admitted model: {path.relative_to(root)}")

    for model in models:
        path = root / ANDROID_MODELS / f"{model['model']}.tflite"
        if path.is_file() and sha256(path) != model["tflite"]["sha256"]:
            failures.append(f"{path.relative_to(root)} has an unadmitted SHA-256")
    if (root / ECG_ANDROID_RELATIVE).is_file():
        if sha256(root / ECG_ANDROID_RELATIVE) != ECG_ANDROID_SHA256:
            failures.append(f"{ECG_ANDROID_RELATIVE} has an unadmitted SHA-256")

    expected_ios = {root / ECG_IOS_RELATIVE}
    for model in models:
        expected_ios.add(root / IOS_MODELS / f"{model['model']}.mlpackage")
    found_ios = {
        path
        for path in (root / "apps/ios/Maverick").rglob("*")
        if path.is_dir() and path.suffix.lower() in (".mlpackage", ".mlmodelc")
    }
    for path in sorted(found_ios - expected_ios):
        failures.append(f"iOS ships an unadmitted model container: {path.relative_to(root)}")
    for path in sorted(expected_ios - found_ios):
        failures.append(f"iOS is missing an admitted model container: {path.relative_to(root)}")

    for model in models:
        package = root / IOS_MODELS / f"{model['model']}.mlpackage"
        if not package.is_dir():
            continue
        for member, expected in model["coreml"]["members"].items():
            path = package / member
            if not path.is_file() or sha256(path) != expected:
                failures.append(f"{package.relative_to(root)}/{member} has an unadmitted SHA-256")
        failures.extend(feature_name_failures(package, model, root))
    ecg_package = root / ECG_IOS_RELATIVE
    if ecg_package.is_dir():
        for member, expected in ECG_IOS_MEMBER_HASHES.items():
            path = ecg_package / member
            if not path.is_file() or sha256(path) != expected:
                failures.append(f"{ECG_IOS_RELATIVE / member} has an unadmitted SHA-256")

    loose_ios = sorted(
        path
        for path in (root / "apps/ios/Maverick").rglob("*")
        if path.is_file()
        and path.suffix.lower() in (".tflite", ".onnx", ".mlmodel", ".pt")
        and not any(parent.suffix.lower() == ".mlpackage" for parent in path.parents)
    )
    for path in loose_ios:
        failures.append(f"iOS contains a loose model file: {path.relative_to(root)}")

    failures.extend(ios_target_failures(root, models))
    return failures


# The Swift that reads the zoo. A package can be in the target and still unreachable if the
# runner is not compiled, which is the state this check was added to catch.
IOS_ZOO_SOURCES = (
    "MavModelCatalog.swift",
    "MavModelRunner.swift",
    "MavModelBridge.swift",
)


def ios_target_failures(root: Path, models: list[dict]) -> list[str]:
    """Every admitted package, and the Swift that loads it, must be in the Xcode target.

    Sitting in `apps/ios/Maverick/Models/` is not the same as shipping. Xcode builds what its
    project file lists, and for a long time this one listed the ECG model and nothing else: all
    forty-one zoo packages were on disk, hashed by the check above, and absent from the app. The
    iOS tests could not have caught it either, because the test target did not compile the
    runner that would have loaded them.
    """
    project = root / "apps/ios/Maverick.xcodeproj/project.pbxproj"
    if not project.is_file():
        return [f"iOS project file is missing: {project.relative_to(root)}"]
    text = project.read_text(encoding="utf-8", errors="replace")
    failures: list[str] = []
    for model in models:
        name = f"{model['model']}.mlpackage"
        if f"/* {name} in Sources */" not in text:
            failures.append(f"iOS target does not build {name}; it is on disk and not shipped")
    for source in IOS_ZOO_SOURCES:
        if f"/* {source} in Sources */" not in text:
            failures.append(f"iOS target does not compile {source}, so no model can be loaded")
    return failures


def android_apk_failures(apk: Path, models: list[dict]) -> list[str]:
    if not apk.is_file():
        return [f"Android APK does not exist: {apk}"]
    expected = {f"assets/ecg/{ECG_SLUG}.tflite": ECG_ANDROID_SHA256}
    for model in models:
        expected[f"assets/models/{model['model']}.tflite"] = model["tflite"]["sha256"]
    failures: list[str] = []
    with zipfile.ZipFile(apk) as archive:
        found = sorted(
            name for name in archive.namelist() if Path(name).suffix.lower() in MODEL_SUFFIXES
        )
        for name in found:
            if name not in expected:
                failures.append(f"Android APK ships an unadmitted model: {name}")
        for name in expected:
            if name not in found:
                failures.append(f"Android APK is missing an admitted model: {name}")
        for name, digest in expected.items():
            if name in found:
                actual = hashlib.sha256(archive.read(name)).hexdigest()
                if actual != digest:
                    failures.append(f"{name} has an unadmitted SHA-256 in {apk.name}")
    return failures


def ios_app_failures(app: Path, models: list[dict]) -> list[str]:
    if not app.is_dir():
        return [f"iOS app bundle does not exist: {app}"]
    expected = {f"{ECG_SLUG}.mlmodelc"} | {f"{model['model']}.mlmodelc" for model in models}
    found = {path.name for path in app.rglob("*.mlmodelc") if path.is_dir()}
    failures: list[str] = []
    for name in sorted(found - expected):
        failures.append(f"iOS app ships an unadmitted compiled model: {name}")
    for name in sorted(expected - found):
        failures.append(f"iOS app is missing an admitted compiled model: {name}")
    others = sorted(
        str(path.relative_to(app))
        for path in app.rglob("*")
        if path.is_file() and path.suffix.lower() in (".tflite", ".onnx", ".mlmodel", ".pt")
    )
    for name in others:
        failures.append(f"iOS app contains an additional model file: {name}")
    return failures


def feature_name_failures(package: Path, model: dict, root: Path) -> list[str]:
    """The package's own input and output names must be the contract's names.

    Twelve packages once shipped with inputs called `tensors_0`, `tensors_1` — `torch.export`
    names placeholders after the forward signature, and the conversion wrapper takes `*tensors`.
    They loaded fine and then refused every feed, because the apps bind by contract name. The
    conversion pipeline missed it by reading the names off the package instead of the contract,
    so the check belongs here, where the bytes are.
    """
    manifest_path = package / "Data/com.apple.CoreML/model.mlmodel"
    if not manifest_path.is_file():
        return []
    blob = manifest_path.read_bytes()
    failures = []
    for kind in ("inputs", "outputs"):
        for spec in model[kind]:
            # The names live in the protobuf as length-prefixed UTF-8; a substring test is
            # enough to catch a wholesale rename without parsing the schema here.
            if spec["name"].encode() not in blob:
                failures.append(
                    f"{package.relative_to(root)} does not declare {kind[:-1]} "
                    f"'{spec['name']}' from its contract"
                )
    return failures


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path)
    parser.add_argument("--android-apk", type=Path)
    parser.add_argument("--ios-app", type=Path)
    arguments = parser.parse_args()
    root = (arguments.root or ROOT).resolve()

    models = load_manifest()
    failures = source_failures(root, models)
    if arguments.android_apk:
        failures.extend(android_apk_failures(arguments.android_apk.resolve(), models))
    if arguments.ios_app:
        failures.extend(ios_app_failures(arguments.ios_app.resolve(), models))

    if failures:
        for failure in failures:
            print(f"check_model_assets: {failure}", file=sys.stderr)
        return 1
    print(f"check_model_assets: ok, {len(models) + 1} admitted models")
    return 0


if __name__ == "__main__":
    sys.exit(main())
