#!/usr/bin/env python3
"""Every generated file that is also committed still matches the source it is generated from.

A file that says "Do not edit" at the top and is checked into Git is a promise, and the only thing
that keeps a promise like that is a gate. Each of these generators already had a `--check` mode, but
they were wired into CI one step at a time and not into the local pre-commit list, so the answer to
"did I regenerate that?" was whoever remembered.

    tools/check_generated.py            fail if anything is stale
    tools/check_generated.py --write    regenerate everything in place

`apps/ios/Maverick.xcodeproj` is deliberately not here. It is generated too, and it used to be
committed, but it cannot be regenerated identically outside a fully built tree — the spec pulls in
`build/mav-core/Sources`, which the core build writes and Git ignores, so the same spec yields a
different project before and after that build. A generated file nobody can reproduce is not
something a gate can defend, so it is gitignored and rebuilt instead.
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

#: Generators that own their own `--check`. Each is (label, check command, write command).
PYTHON_GENERATORS = [
    (
        "design tokens (tokens/aura.json)",
        [sys.executable, "tools/gen_design_tokens.py", "--check"],
        [sys.executable, "tools/gen_design_tokens.py"],
    ),
    (
        "model bindings (artifacts/models/manifest.json)",
        [sys.executable, "tools/ml/generate_bindings.py", "--check"],
        [sys.executable, "tools/ml/generate_bindings.py"],
    ),
    (
        "precision ledger (artifacts/models/manifest.json)",
        [sys.executable, "tools/ml/build_ledger.py", "--check"],
        [sys.executable, "tools/ml/build_ledger.py"],
    ),
]

def run(command: list[str], cwd: Path = ROOT) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, cwd=cwd, text=True, capture_output=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--write", action="store_true", help="regenerate in place instead of checking"
    )
    args = parser.parse_args()

    failures: list[str] = []
    for label, check, write in PYTHON_GENERATORS:
        result = run(write if args.write else check)
        if result.returncode != 0:
            detail = (result.stderr or result.stdout).strip().splitlines()
            failures.append(f"{label}: {detail[0] if detail else 'failed'}")

    for line in failures:
        print(f"check_generated: {line}", file=sys.stderr)
    if failures:
        return 1
    print(f"check_generated: ok, {len(PYTHON_GENERATORS)} generated outputs match their sources")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
