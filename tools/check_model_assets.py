#!/usr/bin/env python3
"""Prove that each Maverick app ships exactly its one admitted ECG model."""

from __future__ import annotations

import argparse
import hashlib
import sys
import zipfile
from pathlib import Path


ANDROID_RELATIVE = Path(
    "apps/android/app/src/main/assets/ecg/nao_full_ecg_model_fp16.tflite"
)
ANDROID_SHA256 = "0be97329077d5d5d2791b8b7850baf8acaf8f12f96fdfad7bcdb4af37156ea21"
ANDROID_APK_ENTRY = "assets/ecg/nao_full_ecg_model_fp16.tflite"

IOS_RELATIVE = Path(
    "apps/ios/Maverick/Models/nao_full_ecg_model_fp16.mlpackage"
)
IOS_COMPILED_NAME = "nao_full_ecg_model_fp16.mlmodelc"
IOS_MEMBER_HASHES = {
    "Manifest.json": "2760ca6f4696a0519091fa43ee9ddbfae1bbda4e61fb85a5438d2cb3317ab288",
    "Data/com.apple.CoreML/model.mlmodel":
        "24230f1c635ab45fba02bb80c86f3b9a9dc2499436bda16a1d97b34ffb63f6d3",
    "Data/com.apple.CoreML/weights/weight.bin":
        "24111a56f73dc262cf600a73f18a647bf8ad623ecaa7336da5463e87325de0d9",
}
MODEL_SUFFIXES = (".tflite", ".onnx", ".mlmodel", ".mlpackage", ".mlmodelc")


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def source_failures(root: Path) -> list[str]:
    failures: list[str] = []
    android_root = root / "apps/android/app/src/main"
    ios_root = root / "apps/ios/Maverick"
    expected_android = root / ANDROID_RELATIVE
    expected_ios = root / IOS_RELATIVE

    android_models = sorted(
        path for path in android_root.rglob("*")
        if path.is_file() and path.suffix.lower() in MODEL_SUFFIXES
    )
    if android_models != [expected_android]:
        names = [str(path.relative_to(root)) for path in android_models]
        failures.append(f"Android source models are {names}, expected only {ANDROID_RELATIVE}")
    elif sha256(expected_android) != ANDROID_SHA256:
        failures.append(f"{ANDROID_RELATIVE} has an unadmitted SHA-256")

    ios_models = sorted(
        path for path in ios_root.rglob("*")
        if path.is_dir() and path.suffix.lower() in (".mlpackage", ".mlmodelc")
    )
    if ios_models != [expected_ios]:
        names = [str(path.relative_to(root)) for path in ios_models]
        failures.append(f"iOS source model containers are {names}, expected only {IOS_RELATIVE}")
    elif expected_ios.exists():
        for member, expected_hash in IOS_MEMBER_HASHES.items():
            member_path = expected_ios / member
            if not member_path.is_file() or sha256(member_path) != expected_hash:
                failures.append(f"{IOS_RELATIVE / member} has an unadmitted SHA-256")

    loose_ios_models = sorted(
        path for path in ios_root.rglob("*")
        if path.is_file() and path.suffix.lower() in (".tflite", ".onnx", ".mlmodel")
        and expected_ios not in path.parents
    )
    if loose_ios_models:
        names = [str(path.relative_to(root)) for path in loose_ios_models]
        failures.append(f"iOS contains additional loose model files: {names}")
    return failures


def android_apk_failures(apk: Path) -> list[str]:
    if not apk.is_file():
        return [f"Android APK does not exist: {apk}"]
    with zipfile.ZipFile(apk) as archive:
        models = sorted(
            name for name in archive.namelist()
            if Path(name).suffix.lower() in MODEL_SUFFIXES
        )
        if models != [ANDROID_APK_ENTRY]:
            return [
                f"Android APK models are {models}, expected only {ANDROID_APK_ENTRY}"
            ]
        digest = hashlib.sha256(archive.read(ANDROID_APK_ENTRY)).hexdigest()
        if digest != ANDROID_SHA256:
            return [f"{ANDROID_APK_ENTRY} has an unadmitted SHA-256 in {apk}"]
    return []


def ios_app_failures(app: Path) -> list[str]:
    if not app.is_dir():
        return [f"iOS app bundle does not exist: {app}"]
    compiled = sorted(
        path for path in app.rglob("*.mlmodelc")
        if path.is_dir()
    )
    names = [path.name for path in compiled]
    if names != [IOS_COMPILED_NAME]:
        return [
            f"iOS app compiled models are {names}, expected only {IOS_COMPILED_NAME}"
        ]
    other_models = sorted(
        str(path.relative_to(app))
        for path in app.rglob("*")
        if path.is_file() and path.suffix.lower() in (".tflite", ".onnx", ".mlmodel")
    )
    if other_models:
        return [f"iOS app contains additional model files: {other_models}"]
    return []


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path)
    parser.add_argument("--android-apk", type=Path)
    parser.add_argument("--ios-app", type=Path)
    arguments = parser.parse_args()
    root = (arguments.root or Path(__file__).resolve().parent.parent).resolve()

    failures = source_failures(root)
    if arguments.android_apk:
        failures.extend(android_apk_failures(arguments.android_apk.resolve()))
    if arguments.ios_app:
        failures.extend(ios_app_failures(arguments.ios_app.resolve()))

    if failures:
        for failure in failures:
            print(f"check_model_assets: {failure}", file=sys.stderr)
        return 1
    print("check_model_assets: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
