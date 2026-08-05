"""The asset gate has to fail on the things that actually go wrong.

Three of them: a model in the bundle that the manifest never admitted, an admitted model that
did not make it into the bundle, and an admitted model whose bytes changed. Each is checked
against a synthetic bundle rather than the real one, so the test keeps failing for the right
reason after the manifest is regenerated.
"""

import hashlib
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_model_assets


def one_model() -> list[dict]:
    payload = b"admitted-bytes"
    return [
        {
            "model": "example_model",
            "tflite": {"sha256": hashlib.sha256(payload).hexdigest()},
            "coreml": {"members": {}},
        }
    ]


PAYLOAD = b"admitted-bytes"
ECG_ENTRY = f"assets/ecg/{check_model_assets.ECG_SLUG}.tflite"


class ModelAssetProofTest(unittest.TestCase):
    def test_apk_rejects_a_model_the_manifest_never_admitted(self) -> None:
        with tempfile.TemporaryDirectory(prefix="mav-model-apk-") as temporary:
            apk = Path(temporary) / "app.apk"
            with zipfile.ZipFile(apk, "w") as archive:
                archive.writestr(ECG_ENTRY, b"")
                archive.writestr("assets/models/example_model.tflite", PAYLOAD)
                archive.writestr("assets/models/experimental_fp32.tflite", b"variant")

            failures = check_model_assets.android_apk_failures(apk, one_model())
            self.assertIn(
                "Android APK ships an unadmitted model: assets/models/experimental_fp32.tflite",
                failures,
            )

    def test_apk_rejects_changed_bytes_for_an_admitted_model(self) -> None:
        with tempfile.TemporaryDirectory(prefix="mav-model-apk-") as temporary:
            apk = Path(temporary) / "app.apk"
            with zipfile.ZipFile(apk, "w") as archive:
                archive.writestr(ECG_ENTRY, b"")
                archive.writestr("assets/models/example_model.tflite", b"different")

            failures = check_model_assets.android_apk_failures(apk, one_model())
            self.assertIn(
                "assets/models/example_model.tflite has an unadmitted SHA-256 in app.apk",
                failures,
            )

    def test_apk_rejects_a_missing_admitted_model(self) -> None:
        with tempfile.TemporaryDirectory(prefix="mav-model-apk-") as temporary:
            apk = Path(temporary) / "app.apk"
            with zipfile.ZipFile(apk, "w") as archive:
                archive.writestr(ECG_ENTRY, b"")

            failures = check_model_assets.android_apk_failures(apk, one_model())
            self.assertIn(
                "Android APK is missing an admitted model: assets/models/example_model.tflite",
                failures,
            )

    def test_ios_bundle_rejects_an_extra_compiled_model(self) -> None:
        with tempfile.TemporaryDirectory(prefix="mav-model-ios-") as temporary:
            app = Path(temporary) / "Mav.app"
            (app / f"{check_model_assets.ECG_SLUG}.mlmodelc").mkdir(parents=True)
            (app / "example_model.mlmodelc").mkdir()
            (app / "experimental.mlmodelc").mkdir()

            failures = check_model_assets.ios_app_failures(app, one_model())
            self.assertEqual(
                failures,
                ["iOS app ships an unadmitted compiled model: experimental.mlmodelc"],
            )

    def test_ios_bundle_rejects_a_missing_compiled_model(self) -> None:
        with tempfile.TemporaryDirectory(prefix="mav-model-ios-") as temporary:
            app = Path(temporary) / "Mav.app"
            (app / f"{check_model_assets.ECG_SLUG}.mlmodelc").mkdir(parents=True)

            failures = check_model_assets.ios_app_failures(app, one_model())
            self.assertEqual(
                failures,
                ["iOS app is missing an admitted compiled model: example_model.mlmodelc"],
            )

    def test_the_repository_itself_passes(self) -> None:
        models = check_model_assets.load_manifest()
        self.assertGreater(len(models), 0)
        self.assertEqual(check_model_assets.source_failures(check_model_assets.ROOT, models), [])


if __name__ == "__main__":
    unittest.main()
