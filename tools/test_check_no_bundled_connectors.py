import tempfile
import unittest
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_no_bundled_connectors


class ArchitectureProofTest(unittest.TestCase):
    def test_seeded_native_and_build_references_fail_exactly(self) -> None:
        with tempfile.TemporaryDirectory(prefix="mav-stale-proof-") as temporary:
            root = Path(temporary)
            source = root / "apps/android/app/src/main/java/Stale.kt"
            build = root / "apps/ios/project.yml"
            source.parent.mkdir(parents=True)
            build.parent.mkdir(parents=True)
            source.write_text("class WhoopConnectionService\n")
            build.write_text("dependency: mav-connector-whoop5\n")

            self.assertEqual(
                check_no_bundled_connectors.architecture_failures(root),
                [
                    "apps/android/app/src/main/java/Stale.kt contains WhoopConnectionService",
                    "apps/ios/project.yml contains mav-connector-whoop",
                ],
            )


if __name__ == "__main__":
    unittest.main()
