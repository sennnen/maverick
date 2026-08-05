#!/usr/bin/env python3
"""Build the model ledger: every archive, every core, what happened to it and why.

The manifest answers "what ships". This answers the harder question — of everything
that exists, what is implemented, what is not, and what is blocking each thing that
is not. Without it, "13 of 31" and "24 of 31" look the same from inside the repo.

One row per *archive*, because that is the unit the training side produces, plus the
per-core detail underneath. An archive can be partly implemented: `whr_2_7_1` ships
two cores and has a third that does not convert, and calling that either "done" or
"blocked" would be wrong.

    python3 tools/ml/build_ledger.py --inventory <core_inventory.json>
    python3 tools/ml/build_ledger.py --check
"""
import argparse
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "artifacts/models/manifest.json"
CONTRACTS = ROOT / "artifacts/models/contracts"
LEDGER = ROOT / "artifacts/models/ledger.json"
ML_DOC = ROOT / "docs/ml.md"

LEDGER_MARKER = "<!-- LEDGER-TABLE -->"

# Archives implemented in Rust rather than converted, whose learned parameters are therefore
# covered by a port rather than by an artefact. `module` is the mav-analytic module; `note`
# says why it is not a conversion target.
RUST_PARAMETRIC = {
    "atlas_2_1_0": (
        "model_zoo::deterministic::atlas",
        "Twelve linear regressions over five features — sixty parameters of dot product, "
        "which is not work for an accelerator. Implemented in {module}. Its input needs a "
        "bioimpedance front end that no supported strap has, so the capability stays "
        "unavailable; see docs/ml.md.",
    ),
}

# Why an archive is not a conversion target at all. Everything else is expected to
# either ship or carry a converter error.
STANDING_REASONS = {}

# Deterministic archives: zero learned parameters, so there is no tensor to accelerate.
# These are algorithms, and docs/ml.md puts deterministic computation in Rust.
# `module` is the mav-analytic module once ported, or None while pending.
DETERMINISTIC = {
    "astd_event_detection_0_1_0": "model_zoo::deterministic::astd_event_detection",
    "atlas_trendline_1_0_0": "model_zoo::deterministic::atlas_trendline",
    "cumulative_stress_1_2_2": "model_zoo::deterministic::cumulative_stress",
    "cva_calibrator_1_3_0": "model_zoo::deterministic::cva_calibrator",
    "daily_medians_1_1_0": "model_zoo::deterministic::daily_medians",
    "daily_short_term_baselines_1_1_0": "model_zoo::deterministic::short_term_baselines",
    "meal_timing_0_1_0": "model_zoo::deterministic::meal_timing",
    "pregnancy_biometrics_0_4_0": "model_zoo::deterministic::pregnancy_biometrics",
    "steps_motion_decoder_2_0_0": "model_zoo::deterministic::steps_motion_decoder",
    "stress_daytime_sensing_1_1_0": "model_zoo::deterministic::daytime_stress",
    "stress_resilience_2_2_1": "model_zoo::deterministic::stress_resilience",
    "training_stress_score_0_2_1": "model_zoo::deterministic::training_stress_score",
}

STATUS_ORDER = ["shipped", "partial", "rust", "rust_pending", "blocked", "no_sensor"]

# Withdrawn: the legacy sleep-staging path held no learned parameters, and the gradient-boosted
# classifier it called lived outside the archive. It is not a conversion target and not a Rust
# port either, so it is excluded from the ledger rather than carried as a permanent gap.
EXCLUDED = {"sleepstaging_2_6_0"}


def load_contracts():
    """Every conversion attempt, keyed by model slug."""
    out = {}
    if CONTRACTS.is_dir():
        for path in sorted(CONTRACTS.glob("*.json")):
            out[path.stem] = json.loads(path.read_text())
    return out


def short_reason(contract):
    """The one clause of a converter error that says what actually went wrong."""
    for backend, label in (("coreml", "Core ML"), ("tflite", "TFLite")):
        error = (contract.get(backend) or {}).get("error")
        if not error:
            continue
        clause = max((p.strip() for p in error.split("|")), key=len)
        for marker in ("NotImplementedError:", "RuntimeError:", "ValueError:", "Error:"):
            if marker in clause:
                clause = clause.split(marker, 1)[1].strip()
                break
        return f"{label}: {clause[:150]}"
    return "unknown"


def build(inventory_path):
    inventory = json.loads(pathlib.Path(inventory_path).read_text())
    manifest = json.loads(MANIFEST.read_text())
    shipped = {model["model"] for model in manifest["models"]}
    contracts = load_contracts()

    # Which archive did each attempted core come from?
    by_archive = {}
    for slug, contract in contracts.items():
        archive = (contract.get("source_asset") or "").removesuffix(".pt")
        by_archive.setdefault(archive, []).append((slug, contract))

    rows = []
    for archive in sorted(inventory):
        if archive in EXCLUDED:
            continue
        detail = inventory[archive]
        params = detail.get("total_params")
        cores = []
        for slug, contract in sorted(by_archive.get(archive, [])):
            cores.append(
                {
                    "model": slug,
                    "core": contract.get("core_path"),
                    "parameters": contract.get("parameters"),
                    "shipped": slug in shipped,
                    "reason": None if slug in shipped else short_reason(contract),
                }
            )

        if archive in RUST_PARAMETRIC:
            module, note = RUST_PARAMETRIC[archive]
            status = "rust"
            reason = note.format(module=module)
            # Its parameters are implemented, so they count as covered — by Rust rather than
            # by a converted artefact.
            for core in cores:
                core["shipped"] = True
        elif archive in STANDING_REASONS:
            status, reason = STANDING_REASONS[archive]
        elif archive in DETERMINISTIC:
            module = DETERMINISTIC[archive]
            status = "rust" if module else "rust_pending"
            reason = (
                f"Deterministic: zero learned parameters. Implemented in {module}."
                if module
                else "Deterministic: zero learned parameters. Belongs in Rust; not yet ported."
            )
        elif cores and all(core["shipped"] for core in cores):
            status, reason = "shipped", None
        elif cores and any(core["shipped"] for core in cores):
            covered_now = sum(
                {c["core"]: c["parameters"] or 0 for c in cores if c["shipped"]}.values()
            )
            blocked = [c for c in cores if not c["shipped"]]
            if params and covered_now >= params:
                # Every parameter ships; what failed is the parent that composed the cores,
                # and its composition is Rust's job. Calling this partial would be misleading.
                status = "shipped"
                reason = (
                    f"All {params:,} parameters ship as {len(cores) - len(blocked)} cores. "
                    f"The composing parent does not convert: {blocked[0]['reason']}"
                )
            else:
                status = "partial"
                missing = params - covered_now if params else 0
                reason = (
                    f"{covered_now:,} of {params:,} parameters ship; {missing:,} blocked: "
                    f"{blocked[0]['reason']}"
                )
        elif cores:
            status, reason = "blocked", cores[0]["reason"]
        else:
            status, reason = "blocked", "no conversion attempted"

        # Parameter coverage, not core count. "Two of three cores shipped" says nothing about
        # how much of the model works: whr_2_7_1's two shipped cores are all 983,891 of its
        # parameters, while the third is their unconvertible parent.
        #
        # Counted per distinct core path, not per model. Two models can convert the *same*
        # submodule under different constants — the CVA probe head ships once per sex — and
        # counting it twice would put an archive above 100%.
        counted = {}
        for core in cores:
            if core["shipped"]:
                counted[core["core"]] = core["parameters"] or 0
        covered = sum(counted.values())
        # An archive implemented in Rust has no converted cores to count, so its coverage is
        # its whole parameter count: the weights are in the port, not in an artefact.
        if archive in RUST_PARAMETRIC:
            covered = params
        rows.append(
            {
                "archive": archive,
                "parameters": params,
                "parameters_covered": covered,
                "coverage": (covered / params) if params else None,
                "status": status,
                "reason": reason,
                "shipped_cores": sum(1 for c in cores if c["shipped"]),
                "total_cores": len(cores),
                "cores": cores,
            }
        )

    # Pulse-PPG has no archive in the inventory; it is a plain checkpoint.
    if "pulse_ppg" in shipped:
        rows.append(
            {
                "archive": "pulse_ppg",
                "parameters": 28_497_920,
                "parameters_covered": 28_497_920,
                "coverage": 1.0,
                "status": "shipped",
                "reason": None,
                "shipped_cores": 1,
                "total_cores": 1,
                "cores": [
                    {
                        "model": "pulse_ppg",
                        "core": "ResNet1D",
                        "parameters": 28_497_920,
                        "shipped": True,
                        "reason": None,
                    }
                ],
            }
        )

    counts = {status: sum(1 for r in rows if r["status"] == status) for status in STATUS_ORDER}
    weighted = [r for r in rows if r["parameters"]]
    return {
        "archives": len(rows),
        "shipped_models": len(shipped),
        "counts": counts,
        "weighted_archives": len(weighted),
        "parameters_total": sum(r["parameters"] for r in weighted),
        "parameters_covered": sum(r["parameters_covered"] for r in weighted),
        "rows": rows,
    }


def render_table(ledger):
    label = {
        "shipped": "shipped",
        "partial": "partial",
        "rust": "Rust",
        "rust_pending": "Rust (pending)",
        "blocked": "blocked",
        "no_sensor": "no sensor",
    }
    lines = [
        "| Archive | Params | Params shipped | Status | Detail |",
        "|---|---|---|---|---|",
    ]
    for row in sorted(ledger["rows"], key=lambda r: (STATUS_ORDER.index(r["status"]), r["archive"])):
        params = f"{row['parameters']:,}" if row["parameters"] else "0"
        if not row["total_cores"]:
            covered = "—"
        elif row["coverage"] is None:
            covered = f"{row['parameters_covered']:,}"
        else:
            covered = f"{row['coverage'] * 100:.0f}%"
        detail = (row["reason"] or "").replace("|", "/")
        lines.append(
            f"| `{row['archive']}` | {params} | {covered} | {label[row['status']]} | {detail} |"
        )
    return "\n".join(lines)


def render_doc(ledger):
    text = ML_DOC.read_text()
    start = text.index(LEDGER_MARKER)
    end = text.index(LEDGER_MARKER, start + len(LEDGER_MARKER))
    return text[: start + len(LEDGER_MARKER)] + "\n\n" + render_table(ledger) + "\n\n" + text[end:]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--inventory", default=str(ROOT / "artifacts/models/core_inventory.json"))
    parser.add_argument("--check", action="store_true")
    arguments = parser.parse_args()

    ledger = build(arguments.inventory)
    rendered = {
        LEDGER: json.dumps(ledger, indent=1, sort_keys=True) + "\n",
        ML_DOC: render_doc(ledger),
    }
    stale = [path for path, text in rendered.items() if not path.exists() or path.read_text() != text]
    if arguments.check:
        for path in stale:
            print(f"build_ledger: stale {path.relative_to(ROOT)}", file=sys.stderr)
        if stale:
            return 1
        print(f"build_ledger: ok, {ledger['archives']} archives")
        return 0
    for path, text in rendered.items():
        path.write_text(text)
    counts = ", ".join(f"{k} {v}" for k, v in ledger["counts"].items() if v)
    print(f"build_ledger: {ledger['archives']} archives ({counts})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
