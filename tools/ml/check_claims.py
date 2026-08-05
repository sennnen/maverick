#!/usr/bin/env python3
"""Check the numbers `docs/ml.md` states against the artefacts they describe.

The generated tables in that document cannot go stale — a generator rewrites them and CI runs
it with `--check`. The *prose* around them can, and did: the delegate sweep's margin changed,
one model stopped taking the GPU, and six sentences across the docs and the tooling went on
saying "all but two" until someone read them.

So the load-bearing figures are asserted here instead of trusted. Each entry is a claim the
document makes in prose, paired with the artefact that settles it. A claim that no longer
matches is a failure with both numbers printed, which is the difference between finding this
in CI and finding it in a review six months later.

Only figures with a single source of truth belong here. A measured latency has no artefact to
check it against and is not a candidate; the counts, totals and decisions are.

    python check_claims.py
"""
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
MAVERICK = os.path.dirname(os.path.dirname(HERE))
DOC = os.path.join(MAVERICK, "docs/ml.md")

# Every document that describes the zoo. The count moved from thirteen to forty-one and four
# sentences across three of these files went on saying thirteen, so the phrase ban runs over all
# of them rather than only over the one that has a generator pointed at it.
ZOO_DOCS = [
    "docs/ml.md",
    "docs/architecture.md",
    "docs/adr/ADR-035.md",
    "docs/plans/README.md",
    "docs/plans/active/model-zoo.md",
]


def load(name):
    with open(os.path.join(MAVERICK, "artifacts/models", name)) as handle:
        return json.load(handle)


def claims():
    manifest = load("manifest.json")
    ledger = load("ledger.json")
    precision = load("precision.json")
    delegate = load("android_delegate.json")
    device = load("device_parity.json")

    summary = precision["summary"]
    paths = delegate["paths"]
    accelerated = sorted(slug for slug, path in paths.items() if path != "CPU")
    half = sum(1 for row in precision["rows"] if row["arithmetic"] == "float16")

    return [
        ("shipped models", len(manifest["models"]), 41),
        ("archives", ledger["archives"], 31),
        ("parameters covered", ledger["parameters_covered"], 41008090),
        ("parameters total", ledger["parameters_total"], 41008090),
        ("models at half arithmetic", half, 11),
        ("models placing work on the ANE", summary["on_neural_engine"], 9),
        ("operations on the ANE", summary["neural_engine_operations"], 547),
        ("operations total", summary["total_operations"], 31090),
        ("models on the Android CPU", sum(1 for p in paths.values() if p == "CPU"), 40),
        ("models on the Android GPU", len(accelerated), 1),
        ("the accelerated model", accelerated, ["whr_unet_encoder"]),
        ("device rows", len(device["rows"]), 41),
        # The three worst-case figures the prose quotes, to two significant figures — enough to
        # catch a stale sentence, loose enough not to fail on the last bit of a re-measurement.
        ("worst device vs reference", round(summary["worst_device_vs_reference"], 5), 0.00538),
        ("worst between platforms", round(summary["worst_between_platforms"], 5), 0.00623),
        ("worst graph error", round(summary["worst_graph_error"], 5), 0.00365),
    ]


# Phrases no document about the zoo may still contain, each with what replaced it. A count
# spelled out in words is the one thing a generator cannot keep honest, so the words are banned
# and the figure is left to `manifest.json`.
BANNED_PHRASES = [
    ("all but two of", "the GPU is slower on all but ONE model now"),
    ("two models keep the GPU", "one model keeps the GPU"),
    ("Two models leave the CPU", "one model leaves the CPU"),
    ("NNAPI is second", "NNAPI was removed"),
    ("thirteen models", "the zoo outgrew thirteen; cite manifest.json, not a word"),
    ("Thirteen models", "the zoo outgrew thirteen; cite manifest.json, not a word"),
    ("fourteen models", "the zoo outgrew fourteen; cite manifest.json, not a word"),
    ("All thirteen", "the zoo outgrew thirteen; cite manifest.json, not a word"),
    ("Seven models did not convert", "six wrapper archives did not, and none is a lost core"),
]


def prose_checks():
    """Banned phrases still present, as (document, phrase, why)."""
    found = []
    for relative in ZOO_DOCS:
        path = os.path.join(MAVERICK, relative)
        if not os.path.exists(path):
            found.append((relative, "<missing>", "listed in ZOO_DOCS but not on disk"))
            continue
        with open(path) as handle:
            text = handle.read()
        found.extend(
            (relative, phrase, why) for phrase, why in BANNED_PHRASES if phrase in text
        )
    return found


# The integration's own numbers, read out of the Rust that decides them rather than out of prose.
#
# `pipeline.rs` is the single source of truth for which models this build can feed, and the two
# figures below are the ones a reader is most likely to take away from docs/ml.md. Both should
# move only when someone ports a front-end and says so in the same commit.
PIPELINE_RS = "core/crates/mav-analytic/src/model_zoo/pipeline.rs"


def pipeline_claims():
    with open(os.path.join(MAVERICK, PIPELINE_RS)) as handle:
        lines = handle.read().splitlines()
    # Only the argument position inside an `entry(...)` row — eight spaces of indent. Counting
    # bare occurrences would also catch the enum's own definition and the tests that name it.
    rows = [line.strip() for line in lines if line.startswith("        FrontEnd::")]
    ported = sum(1 for row in rows if row.startswith("FrontEnd::Ported("))
    upstream = sum(1 for row in rows if row.startswith("FrontEnd::Upstream"))
    not_ported = sum(1 for row in rows if row.startswith("FrontEnd::NotPorted("))
    return [
        ("ported front-ends", ported, 9),
        ("models fed entirely by upstream models", upstream, 11),
        ("models whose front-end is not ported", not_ported, 21),
        ("pipeline rows", ported + upstream + not_ported, 41),
    ]


def main():
    failures = []
    for label, actual, expected in list(claims()) + list(pipeline_claims()):
        if actual != expected:
            failures.append(f"{label}: artefacts say {actual!r}, docs claim {expected!r}")
    for document, phrase, why in prose_checks():
        failures.append(f"{document} still says {phrase!r} — {why}")

    # The parameter reconciliation the document spells out, checked rather than asserted.
    manifest = load("manifest.json")
    ledger = load("ledger.json")
    per_model = sum(model["parameters"] for model in manifest["models"])
    # cva_probes_female and cva_probes_male are one core exported twice; atlas is a Rust port
    # with no manifest row.
    shared_core = 2471
    rust_only = 60
    if per_model != ledger["parameters_total"] + shared_core - rust_only:
        failures.append(
            f"parameter reconciliation broke: manifest sums to {per_model:,}, "
            f"ledger {ledger['parameters_total']:,} + {shared_core:,} - {rust_only} "
            f"= {ledger['parameters_total'] + shared_core - rust_only:,}"
        )

    if failures:
        print("check_claims: FAILED")
        for failure in failures:
            print(f"  {failure}")
        return 1
    print(
        f"check_claims: ok, {len(claims()) + len(pipeline_claims())} claims "
        "and the parameter reconciliation"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
