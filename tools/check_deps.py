#!/usr/bin/env python3
"""Fail if any internal crate dependency exists that docs/architecture.md does not allow.

Reads the [dependencies] section of every core/crates/*/Cargo.toml with a deliberately dumb
line parser (no tomllib on older macOS pythons). Dev-dependencies are ignored: the table in
architecture.md governs the runtime graph.
"""

import re
import sys
from pathlib import Path

ALLOWED = {
    "mav-model": set(),
    "mav-connector-abi": set(),
    "mav-connector-sdk": {"mav-connector-abi"},
    "mav-connector-runtime": {"mav-model", "mav-connector-abi"},
    "mav-connector-tool": {"mav-connector-abi", "mav-connector-runtime"},
    "mav-frame": {"mav-model"},
    "mav-store": {"mav-model"},
    "mav-obs": {"mav-model", "mav-store"},
    "mav-codec": {"mav-model", "mav-frame"},
    "mav-timeline": {"mav-model"},
    "mav-sqi": {"mav-model"},
    "mav-feature": {"mav-model"},
    "mav-analytic": {"mav-model", "mav-feature"},
    "mav-engine": {
        "mav-model", "mav-frame", "mav-codec", "mav-timeline", "mav-sqi",
        "mav-feature", "mav-analytic", "mav-store", "mav-obs",
    },
    "mav-ffi": {"mav-model", "mav-obs", "mav-engine", "mav-connector-whoop"},
    "mav-replay": {"mav-model", "mav-obs", "mav-engine", "mav-connector-whoop"},
}

# Device codec crates under core/connectors/ (ADR-016): each may reach only the connector
# contract, never storage, analytics, the engine, or another connector. Only the two edge
# crates above may link them.
CONNECTOR_ALLOWED = {"mav-model", "mav-frame", "mav-codec"}

SECTION = re.compile(r"^\s*\[(?P<name>[^\]]+)\]\s*$")
DEP_KEY = re.compile(r"^\s*(?P<key>[A-Za-z0-9_-]+)\s*=")


def internal_deps(manifest: Path) -> set[str]:
    deps: set[str] = set()
    section = ""
    for line in manifest.read_text().splitlines():
        m = SECTION.match(line)
        if m:
            section = m.group("name").strip()
            continue
        if section != "dependencies":
            continue
        m = DEP_KEY.match(line)
        if m and m.group("key").startswith("mav-"):
            deps.add(m.group("key"))
    return deps


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    crates_dir = root / "core" / "crates"
    failures: list[str] = []

    found = {p.name for p in crates_dir.iterdir() if (p / "Cargo.toml").is_file()}
    for missing in sorted(set(ALLOWED) - found):
        failures.append(f"{missing}: listed in architecture.md but missing from core/crates/")
    for unknown in sorted(found - set(ALLOWED)):
        failures.append(f"{unknown}: exists in core/crates/ but architecture.md has no row for it")

    for crate in sorted(found & set(ALLOWED)):
        deps = internal_deps(crates_dir / crate / "Cargo.toml")
        for dep in sorted(deps - ALLOWED[crate]):
            failures.append(
                f"{crate}: depends on {dep}, which architecture.md does not allow "
                f"(allowed: {sorted(ALLOWED[crate]) or 'none'})"
            )

    connectors_dir = root / "core" / "connectors"
    connectors = (
        {p.name for p in connectors_dir.iterdir() if (p / "Cargo.toml").is_file()}
        if connectors_dir.is_dir()
        else set()
    )
    for crate in sorted(connectors):
        if not crate.startswith("mav-connector-"):
            failures.append(f"{crate}: connector crates must be named mav-connector-<family>")
        for dep in sorted(internal_deps(connectors_dir / crate / "Cargo.toml") - CONNECTOR_ALLOWED):
            failures.append(
                f"{crate}: depends on {dep}, which architecture.md does not allow a "
                f"connector crate (allowed: {sorted(CONNECTOR_ALLOWED)})"
            )

    if failures:
        print("dependency edges do not match docs/architecture.md:")
        for f in failures:
            print(f"  {f}")
        return 1
    print(
        f"check_deps: {len(found)} crates + {len(connectors)} connector crates, "
        "all edges match docs/architecture.md"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
