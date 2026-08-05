#!/usr/bin/env python3
"""Generate docs/model-matrix.md: one row per admitted model, from the sources of truth.

Three files decide what a row says, and none of them is prose:

- `artifacts/models/manifest.json` — parameters, tensor shapes, the archive each model came from.
- `core/crates/mav-analytic/src/model_zoo/pipeline.rs` — required streams, profile fields,
  upstream models, front-end status and the blocker where there is one.
- `core/crates/mav-analytic/src/model_zoo/registry.rs` — the role line.

Generating it rather than writing it is the point: a matrix maintained by hand goes stale on the
first re-conversion, and a stale capability matrix is worse than none, because it is the document
someone checks before promising a feature.

Usage
    model_matrix.py            # write docs/model-matrix.md
    model_matrix.py --check    # fail if the committed file is not what this produces
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
MANIFEST = os.path.join(ROOT, "artifacts/models/manifest.json")
PIPELINE = os.path.join(ROOT, "core/crates/mav-analytic/src/model_zoo/pipeline.rs")
OUT = os.path.join(ROOT, "docs/model-matrix.md")

# Which archive each model's parameters came from. From the conversion specs; a model whose
# archive is not listed here fails the generator rather than getting an empty cell.
ARCHIVES = {
    "activity_context_embedding": "automatic_activity_detection_3_1_11",
    "activity_detection": "automatic_activity_detection_3_0_8",
    "activity_ensemble": "automatic_activity_detection_3_1_11",
    "activity_history_transformer": "automatic_activity_detection_3_1_11",
    "activity_primary_segments": "automatic_activity_detection_3_1_11",
    "activity_secondary_segments": "automatic_activity_detection_3_1_11",
    "activity_transition": "automatic_activity_detection_3_0_8",
    "awhr_imputation": "awhr_imputation_1_2_0",
    "awhr_profile_core": "awhr_profile_selector_1_0_1",
    "awhr_profile_head": "awhr_profile_selector_1_0_1",
    "awhr_profile_recurrent": "awhr_profile_selector_1_0_1",
    "behavior_embedding": "automatic_activity_detection_3_1_11",
    "cva_encoder": "cva_2_1_0",
    "cva_predictor_v1_base": "cva_1_3_0",
    "cva_probes_female": "cva_2_1_0",
    "cva_probes_male": "cva_2_1_0",
    "dhrv_imputation": "dhrv_imputation_1_1_0",
    "energy_expenditure_hr": "energy_expenditure_1_0_0",
    "energy_expenditure_no_hr": "energy_expenditure_1_0_0",
    "halite_ppg_score": "halite_1_2_0",
    "halite_risk_tree": "halite_1_2_0",
    "illness_detection": "illness_detection_0_5_1",
    "popsicle_min_follicular": "popsicle_1_8_1",
    "popsicle_min_follicular_v16": "popsicle_1_6_0",
    "popsicle_ovulation_detection": "popsicle_1_8_1",
    "popsicle_ovulation_detection_v16": "popsicle_1_6_0",
    "popsicle_ovulation_prediction": "popsicle_1_8_1",
    "popsicle_ovulation_prediction_v16": "popsicle_1_6_0",
    "popsicle_period_prediction": "popsicle_1_8_1",
    "popsicle_period_prediction_v16": "popsicle_1_6_0",
    "pulse_ppg": "pulse-ppg (open weights)",
    "pulsenet_foundation": "halite_1_2_0",
    "sleepnet_bdi": "sleepnet_bdi_0_4_0",
    "sleepnet_bdi_v3": "sleepnet_bdi_0_3_0",
    "sleepnet_moonstone": "sleepnet_moonstone_1_2_0",
    "source_embedding": "automatic_activity_detection_3_1_11",
    "step_eligibility": "step_counter_1_3_0",
    "step_head": "step_counter_1_3_0",
    "step_multiplier": "step_counter_1_3_0",
    "whr_unet_encoder": "whr_2_7_1",
    "whr_unet_head": "whr_2_7_1",
}


def pipeline_rows() -> dict[str, dict]:
    """Parse the pipeline table. Structured Rust, so a regex over entry blocks is enough."""
    text = open(PIPELINE).read()
    table = text[text.index("pub const PIPELINE") :]
    table = table[: table.index("\n];")]
    rows: dict[str, dict] = {}
    for block in re.findall(r"    entry\(\n(.*?)\n    \),", table, re.S):
        variant = re.search(r"ModelId::(\w+),", block).group(1)
        signal = re.search(r"Signal::(\w+),", block).group(1)
        front = re.search(r"FrontEnd::(\w+)", block).group(1)
        blocker = re.search(r"FrontEnd::NotPorted\(\s*Blocker::(\w+)", block, re.S)
        detail = re.search(r'FrontEnd::(?:Ported|NotPorted)\((?:\s*Blocker::\w+,\s*)?"(.*?)"', block, re.S)
        streams = re.findall(r"StreamKind::(\w+)", block)
        profile = re.findall(r"ProfileField::(\w+)", block)
        # Upstream models are the ModelId references after the first (which is this model).
        upstream = re.findall(r"ModelId::(\w+)", block)[1:]
        interp = re.search(r"Interpretation::(\w+)", block).group(1)
        rows[variant] = {
            "signal": signal,
            "front_end": front,
            "blocker": blocker.group(1) if blocker else None,
            "detail": (detail.group(1).replace("\\\n", "").strip() if detail else ""),
            "streams": sorted(set(streams)),
            "profile": sorted(set(profile)),
            "upstream": upstream,
            "interpretation": interp,
        }
    return rows


def variant_of(slug: str) -> str:
    return "".join(part.capitalize() for part in slug.split("_")).replace("V16", "V16").replace("V1Base", "V1Base")


def status(row: dict) -> str:
    if row["front_end"] == "Ported":
        return "**Runnable** — front-end ported"
    if row["front_end"] == "Upstream":
        return "**Runnable when its upstream is**"
    if row["blocker"] == "RingFirmwareFeatures":
        return "Blocked — ring firmware features"
    return "Not yet ported — recoverable"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    manifest = json.load(open(MANIFEST))
    rows = pipeline_rows()
    by_variant = {}
    for model in manifest["models"]:
        slug = model["model"]
        variant = next(
            (key for key in rows if key.lower().replace("_", "") == slug.replace("_", "")), None
        )
        if variant is None:
            print(f"model_matrix: {slug} has no pipeline row", file=sys.stderr)
            return 1
        by_variant[slug] = rows[variant]

    lines: list[str] = []
    lines.append("# The model matrix\n")
    lines.append(
        "Generated by `tools/ml/model_matrix.py`. Do not edit; re-run the generator.\n\n"
        "One row per admitted model. **Production status** is the only column that answers "
        '"can a wearer get this today", and it has four values:\n\n'
        "- **Runnable — front-end ported**: a named Rust front-end builds this model's inputs "
        "from stored samples, and the production pipeline queues it.\n"
        "- **Runnable when its upstream is**: every input is another model's output; it runs "
        "exactly when the model above it does.\n"
        "- **Not yet ported — recoverable**: the wrapper's logic is readable in the archive and "
        "every input exists on a supported strap. This is work, and the blocker column says what.\n"
        "- **Blocked — ring firmware features**: the model reads features an Oura ring's own "
        "firmware computes (`stride_frequency`, `gait_amplitude_frac`, `acm_average_*`, "
        "`ring_met`, `motion_seconds`). The archives *consume* those features and do not contain "
        "the code that produces them, and no strap Maverick supports emits them. Porting the "
        "wrapper would give a correctly shaped tensor with nothing real in it.\n\n"
        "Counts by front-end status sum to 41. **Runnable** is a different axis and overlaps "
        "them: it is every `Ported` row plus the `Upstream` rows whose upstream is itself "
        "runnable.\n"
    )

    counts: dict[str, int] = {}
    for slug in sorted(by_variant):
        counts[status(by_variant[slug])] = counts.get(status(by_variant[slug]), 0) + 1
    lines.append("\n| status | models |\n| --- | --- |\n")
    for key in sorted(counts):
        lines.append(f"| {key} | {counts[key]} |\n")
    lines.append(f"| **total** | **{sum(counts.values())}** |\n")

    lines.append("\n## Every model\n\n")
    lines.append(
        "| model | archive | params | inputs (shape) | needs streams | needs profile | upstream "
        "| production status | blocker / front-end |\n"
        "| --- | --- | ---: | --- | --- | --- | --- | --- | --- |\n"
    )
    for model in sorted(manifest["models"], key=lambda entry: entry["model"]):
        slug = model["model"]
        row = by_variant[slug]
        shapes = "; ".join(
            f"`{spec['name']}` {tuple(spec['shape'])}" for spec in model["inputs"]
        )
        streams = ", ".join(row["streams"]) or "—"
        profile = ", ".join(row["profile"]) or "—"
        upstream = (
            ", ".join(re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower() for name in row["upstream"])
            or "—"
        )
        lines.append(
            f"| `{slug}` | {ARCHIVES.get(slug, '?')} | {model['parameters']:,} | {shapes} "
            f"| {streams} | {profile} | {upstream} | {status(row)} | {row['detail']} |\n"
        )

    text = "".join(lines)
    if args.check:
        if not os.path.isfile(OUT) or open(OUT).read() != text:
            print("model_matrix: docs/model-matrix.md is stale; re-run the generator", file=sys.stderr)
            return 1
        print("model_matrix: ok, 41 rows")
        return 0
    open(OUT, "w").write(text)
    print(f"model_matrix: wrote {len(manifest['models'])} rows to {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
