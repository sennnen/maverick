#!/usr/bin/env python3
"""Reject the compiled connector architecture removed by WC-P12."""

import sys
from pathlib import Path


TOKENS = (
    "mav-connector-whoop",
    "mav_connector_whoop",
    "register_codec",
    "CodecFactory",
    "codec_for",
    "DeviceCodec",
    "ManifestCodec",
    "register_with_codec",
    "validate_against_codec",
    "DECODE_CODEC_UNAVAILABLE",
    "DECODE_NO_MANIFEST_FOR_MODEL",
    "DECODE_UNKNOWN_RECORD_VERSION",
)
def main() -> int:
    root = Path(__file__).resolve().parent.parent
    failures: list[str] = []
    connector_dir = root / "core" / "connectors"
    if connector_dir.exists() and any(connector_dir.rglob("Cargo.toml")):
        failures.append("core/connectors still contains compiled connector crates")

    roots = [
        root / "core" / "Cargo.toml",
        root / "core" / "crates",
        root / "tools" / "check_deps.py",
    ]
    for candidate in roots:
        files = [candidate] if candidate.is_file() else candidate.rglob("*")
        for path in files:
            if not path.is_file() or "target" in path.parts:
                continue
            try:
                text = path.read_text()
            except UnicodeDecodeError:
                continue
            for token in TOKENS:
                if token in text:
                    failures.append(f"{path.relative_to(root)} contains {token}")

    if failures:
        print("bundled connector architecture remains:")
        for failure in failures:
            print(f"  {failure}")
        return 1
    print("check_no_bundled_connectors: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
