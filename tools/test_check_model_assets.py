import tempfile
import unittest
import zipfile
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_model_assets


class ModelAssetProofTest(unittest.TestCase):
    def test_apk_rejects_an_extra_model_variant(self) -> None:
        with tempfile.TemporaryDirectory(prefix="mav-model-apk-") as temporary:
            apk = Path(temporary) / "app.apk"
            with zipfile.ZipFile(apk, "w") as archive:
                archive.writestr(check_model_assets.ANDROID_APK_ENTRY, b"wrong")
                archive.writestr("assets/ecg/fp32.tflite", b"variant")

            self.assertEqual(
                check_model_assets.android_apk_failures(apk),
                [
                    "Android APK models are "
                    "['assets/ecg/fp32.tflite', "
                    "'assets/ecg/nao_full_ecg_model_fp16.tflite'], expected only "
                    "assets/ecg/nao_full_ecg_model_fp16.tflite"
                ],
            )

    def test_ios_bundle_rejects_an_extra_compiled_model(self) -> None:
        with tempfile.TemporaryDirectory(prefix="mav-model-ios-") as temporary:
            app = Path(temporary) / "Mav.app"
            (app / check_model_assets.IOS_COMPILED_NAME).mkdir(parents=True)
            (app / "experimental.mlmodelc").mkdir()

            self.assertEqual(
                check_model_assets.ios_app_failures(app),
                [
                    "iOS app compiled models are "
                    "['experimental.mlmodelc', 'nao_full_ecg_model_fp16.mlmodelc'], "
                    "expected only nao_full_ecg_model_fp16.mlmodelc"
                ],
            )


if __name__ == "__main__":
    unittest.main()
