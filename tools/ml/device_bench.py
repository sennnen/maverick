#!/usr/bin/env python3
"""Compare two on-device benchmark runs, per model.

`ModelBenchmarkInstrumentedTest` writes one JSON per run. This diffs a baseline against a
later run and prints what moved, so an optimisation is reported as a measurement rather than
as an intention.

Timings on a phone are noisy in one direction — thermal state only ever makes them worse — so
a change is only called out when it clears a threshold that ordinary run-to-run variation does
not. Anything inside that band is printed as unchanged rather than dressed up.

    python device_bench.py baseline.json after.json
    python device_bench.py run.json                  # single run, no comparison
"""
import json
import sys

# Below this relative change a difference is run-to-run noise, not a result. Measured: repeat
# runs of the same build on a settled device moved warm medians by a few percent, and a cold
# path that maps and hashes fifty-seven megabytes moves more.
NOISE = 0.10

FIELDS = (
    ("cold_ms", "cold load"),
    ("first_ms", "first inference"),
    ("warm_p50_ms", "warm p50"),
    ("warm_p90_ms", "warm p90"),
    ("cpu_ms_per_inference", "cpu per inference"),
)


def load(path):
    with open(path) as handle:
        return json.load(handle)


def show_single(run):
    print(f"device: {run['device']}")
    print(
        "%-34s %-9s %9s %9s %9s %9s %8s %7s"
        % ("model", "path", "cold_ms", "first_ms", "p50_ms", "p90_ms", "cpu_ms", "iters")
    )
    for slug, row in sorted(run["models"].items()):
        print(
            "%-34s %-9s %9.2f %9.2f %9.3f %9.3f %8.2f %7d"
            % (
                slug,
                row["path"],
                row["cold_ms"],
                row["first_ms"],
                row["warm_p50_ms"],
                row["warm_p90_ms"],
                row["cpu_ms_per_inference"],
                row.get("warm_samples", 0),
            )
        )
    total = sum(r["warm_p50_ms"] for r in run["models"].values())
    cold = sum(r["cold_ms"] for r in run["models"].values())
    print(f"\nwarm p50 summed over {len(run['models'])} models: {total:.1f} ms")
    print(f"cold load summed: {cold:.1f} ms")
    throttled = [
        s
        for s, r in run["models"].items()
        if r["sustained_iterations"] >= 4
        and r["sustained_late_ms"] > r["sustained_early_ms"] * 1.15
    ]
    print(f"models slowing under sustained load: {throttled or 'none'}")


def show_diff(before, after):
    print(f"device: {after['device']}\n")
    header = "%-34s %-9s" % ("model", "path")
    for _key, label in FIELDS:
        header += " %22s" % label
    print(header)
    improved = {key: 0 for key, _ in FIELDS}
    regressed = {key: 0 for key, _ in FIELDS}
    for slug in sorted(after["models"]):
        old = before["models"].get(slug)
        new = after["models"][slug]
        if old is None:
            continue
        line = "%-34s %-9s" % (slug, new["path"])
        for key, _label in FIELDS:
            a, b = old[key], new[key]
            if a <= 0:
                line += " %22s" % "-"
                continue
            change = (b - a) / a
            mark = ""
            if change <= -NOISE:
                mark = "*"
                improved[key] += 1
            elif change >= NOISE:
                mark = "!"
                regressed[key] += 1
            line += " %9.3f>%9.3f%+3.0f%%%s" % (a, b, change * 100, mark)
        print(line)

    print()
    for key, label in FIELDS:
        print(f"{label:20s} better {improved[key]:3d}   worse {regressed[key]:3d}")
    for key, label in FIELDS:
        a = sum(r[key] for r in before["models"].values())
        b = sum(r[key] for s, r in after["models"].items() if s in before["models"])
        if a > 0:
            print(f"total {label:16s} {a:10.1f} ms -> {b:10.1f} ms  ({(b - a) / a * 100:+.1f}%)")


def main():
    if len(sys.argv) == 2:
        show_single(load(sys.argv[1]))
        return 0
    if len(sys.argv) == 3:
        show_diff(load(sys.argv[1]), load(sys.argv[2]))
        return 0
    print(__doc__)
    return 1


if __name__ == "__main__":
    sys.exit(main())
