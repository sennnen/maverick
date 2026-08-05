"""The admission gate has to reject the things that actually shipped wrong.

One of them already did. Three sleep models converted, hashed cleanly, passed every other
check, and computed the wrong answer, because the parity number being measured came from the
converter's handle on the source module rather than from the written flatbuffer. The parity
thresholds and the cross-platform check exist because of that, so they get tests that fail if
either is ever relaxed by accident.
"""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import build_manifest


def contract(**overrides) -> dict:
    base = {
        "model": "example",
        "coreml": {"sha256": "a" * 64, "parity": {"max_rel": 1e-3, "max_abs": 1e-3}},
        "tflite": {"sha256": "b" * 64, "parity": {"max_rel": 1e-7, "max_abs": 1e-7}},
        "cross_platform": {"max_rel": 1e-3, "max_abs": 1e-3},
    }
    base.update(overrides)
    return base


class AdmissionGateTest(unittest.TestCase):
    def test_a_clean_conversion_is_admitted(self) -> None:
        self.assertIsNone(build_manifest.rejection(contract()))

    def test_a_core_ml_failure_is_rejected_with_its_error(self) -> None:
        reason = build_manifest.rejection(
            contract(coreml={"error": "Unsupported fx node unfold"})
        )
        self.assertEqual(reason, "core ml: Unsupported fx node unfold")

    def test_a_tflite_failure_is_rejected_with_its_error(self) -> None:
        reason = build_manifest.rejection(contract(tflite={"error": "boolean index"}))
        self.assertEqual(reason, "tflite: boolean index")

    def test_a_tflite_artefact_that_does_not_match_pytorch_is_rejected(self) -> None:
        # The exact failure the sleep models had: converted, hashed, and wrong by whole logits.
        reason = build_manifest.rejection(
            contract(tflite={"sha256": "b" * 64, "parity": {"max_rel": 1.54, "max_abs": 5.8}})
        )
        self.assertIsNotNone(reason)
        self.assertIn("tflite parity", reason)

    def test_a_model_whose_weights_do_not_survive_halving_is_rejected(self) -> None:
        # Every artefact is half-width now, so this is the check that a model whose weights do
        # not tolerate that rounding cannot ship on the strength of its hash alone.
        rough = {"sha256": "b" * 64, "parity": {"max_rel": 4e-2, "max_abs": 5.0}}
        self.assertIn("tflite parity", build_manifest.rejection(contract(tflite=rough)))

    def test_reference_parity_admits_half_precision_rounding_but_not_a_broken_graph(self) -> None:
        # FP16 storage against an FP32 reference is a real difference; a percent is expected,
        # ten is not.
        # 2e-3 is the band half-width storage actually lands in and has to pass.
        self.assertIsNone(
            build_manifest.rejection(
                contract(coreml={"sha256": "a" * 64, "parity": {"max_rel": 2e-3, "max_abs": 1}})
            )
        )
        reason = build_manifest.rejection(
            contract(coreml={"sha256": "a" * 64, "parity": {"max_rel": 0.2, "max_abs": 1}})
        )
        self.assertIn("core ml parity", reason)

    def test_platforms_that_disagree_are_rejected_even_when_each_matched_pytorch(self) -> None:
        reason = build_manifest.rejection(contract(cross_platform={"max_rel": 1.2, "max_abs": 5}))
        self.assertIn("platforms disagree", reason)

    def test_a_half_precision_model_gets_the_wider_cross_platform_bar(self) -> None:
        # The comparison is not like for like when Core ML runs half-precision arithmetic and
        # the host interpreter runs full width, so the bound is the reference bar rather than
        # the tight one. A model that keeps full width does not get that allowance.
        disagreement = {"max_rel": 1.2e-2, "max_abs": 1.0}
        self.assertIsNone(
            build_manifest.rejection(
                contract(
                    coreml={
                        "sha256": "a" * 64,
                        "precision": "float16",
                        "parity": {"max_rel": 1e-3, "max_abs": 1e-3},
                    },
                    cross_platform=disagreement,
                )
            )
        )
        reason = build_manifest.rejection(
            contract(
                coreml={
                    "sha256": "a" * 64,
                    "precision": "float16 weights, float32 arithmetic",
                    "parity": {"max_rel": 1e-3, "max_abs": 1e-3},
                },
                cross_platform=disagreement,
            )
        )
        self.assertIsNotNone(reason)
        self.assertIn("0.012", reason)

    def test_a_near_zero_output_is_judged_on_absolute_error_not_the_ratio(self) -> None:
        # The PPG-score head reads 1.0 relative on a probe whose score sits a hair from zero.
        # That is agreement, and rejecting it would drive the precision ladder to a policy
        # that costs the accelerator for no measurable gain.
        self.assertIsNone(
            build_manifest.rejection(
                contract(
                    coreml={
                        "sha256": "a" * 64,
                        "parity": {"max_rel": 5.6e-1, "max_abs": 5e-5},
                    },
                    cross_platform={"max_rel": 5.6e-1, "max_abs": 5e-5},
                )
            )
        )

    def test_a_model_that_was_never_cross_checked_is_rejected(self) -> None:
        broken = contract()
        del broken["cross_platform"]
        self.assertEqual(
            build_manifest.rejection(broken),
            "the two platforms were never compared to each other",
        )

    def test_the_shipped_manifest_still_passes_its_own_gate(self) -> None:
        import json

        import coreml_precision

        def bar_for(model: dict) -> float:
            """The cross-platform bar this model is actually held to.

            A model computing at half width on Core ML is compared here against a full-width
            interpreter, so its number carries the arithmetic-width difference. Holding it to
            the like-for-like bar would fail models that agree exactly on the weights.
            """
            if coreml_precision.runs_half_arithmetic(model["coreml"].get("precision", "")):
                return build_manifest.CROSS_PLATFORM_HALF_MAX_REL
            return build_manifest.CROSS_PLATFORM_MAX_REL

        manifest = json.loads(build_manifest.MANIFEST.read_text())
        self.assertGreater(len(manifest["models"]), 0)
        for model in manifest["models"]:
            self.assertLessEqual(
                model["coreml"]["parity"]["max_rel"], build_manifest.REFERENCE_MAX_REL, model["model"]
            )
            self.assertLessEqual(
                model["tflite"]["parity"]["max_rel"],
                build_manifest.REFERENCE_MAX_REL,
                model["model"],
            )
            self.assertLessEqual(
                model["cross_platform"]["max_rel"],
                bar_for(model),
                model["model"],
            )


if __name__ == "__main__":
    unittest.main()
