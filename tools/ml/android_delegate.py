#!/usr/bin/env python3
"""Turn a device delegate sweep into the execution path each model ships with.

Android's delegate was chosen for a long time by inheritance: Core ML admitted the model at
half precision, therefore Android's half-precision delegate must be equivalent. Measuring it
on a Pixel 7 showed that reasoning fails in both directions at once.

  * The GPU delegate at half width moved *every* model it ran, because "float16" is not one
    behaviour — Apple's Neural Engine accumulates a half-width matmul into a wider register
    and this delegate accumulates in half width. Pulse-PPG is 3.9e-3 from its reference under
    Core ML and 2.7e-2 under this delegate, from identical weights.
  * On four models it was not a precision effect at all but a wrong answer: `activity_detection`
    and `cva_encoder` come back 7.2e-1 and 1.4e+1 away at *either* width, and `step_head`
    returns a whole relative unit at half width.
  * And it was slower than the CPU on all but one of the forty-one. The delegate costs a few
    hundred microseconds to dispatch, which most of this zoo's graphs never earn back.

So the path is measured rather than inferred, and the default is the CPU. A model leaves it
only by being at least twice as fast on the GPU *and* no less accurate there — accuracy first,
because a faster wrong embedding is worth nothing to the head reading it, and twice because a
single timing run on a phone is not trustworthy to a hair (see SPEED_MARGIN).

Full width on the GPU is what makes that possible where it happens: it keeps the accelerator
and gives up only the arithmetic width, which is the half of the trade that was costing
accuracy.

    python android_delegate.py [sweep.txt]
"""
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
MAVERICK = os.path.dirname(os.path.dirname(HERE))
SWEEP = os.path.join(MAVERICK, "artifacts/models/device/sweep.txt")
OUT = os.path.join(MAVERICK, "artifacts/models/android_delegate.json")

LINE = re.compile(
    r"SWEEP (?P<slug>\S+) path=(?P<path>\S+) attached=(?P<attached>\S+)"
    r"(?: rel=(?P<rel>\S+) median_ms=(?P<ms>\S+))?"
)

# How much worse than the CPU's own deviation an accelerated path may be before it is refused.
# A ratio rather than an absolute, because the models span six orders of magnitude of output
# scale; the small epsilon keeps a model whose CPU error is exactly zero from refusing
# everything.
ACCURACY_TOLERANCE = 1.10
ACCURACY_EPSILON = 1e-9

# How much faster an accelerated path has to be to be worth leaving the CPU for: twice.
#
# A delegate that wins by a hair has not earned the extra surface — it is a second code path, a
# driver dependency, and a class of wrong answer the CPU does not have. But the number is 0.5
# rather than something nearer 1.0 for a second reason, which is that a single timing run is
# not trustworthy to a hair.
#
# `activity_context_embedding` measured 10.34 ms and 11.83 ms on the CPU in two sweeps run
# straight after a long parity pass, and 2.13 ms in one run on a cool device — a five-fold
# spread in the *baseline*, while its GPU time sat at 5.1-5.5 ms throughout. At a 20% margin
# that model flipped in and out of the accelerator between runs on nothing but thermal state.
# At 2x it is refused in every run, which is the right answer: on an unloaded phone the CPU
# beats the GPU for it outright.
#
# `whr_unet_encoder` clears this comfortably and in every run — 61.8 ms against 28.9 ms — which
# is what a real win looks like next to a measurement artefact.
SPEED_MARGIN = 0.50


def parse(path):
    rows = {}
    for line in open(path):
        match = LINE.match(line.strip())
        if not match:
            continue
        entry = match.groupdict()
        rows.setdefault(entry["slug"], {})[entry["path"]] = {
            "attached": entry["attached"],
            "relative": float(entry["rel"]) if entry["rel"] else None,
            "median_ms": float(entry["ms"]) if entry["ms"] else None,
        }
    return rows


def decide(measured):
    """The fastest path that is no less accurate than the CPU, defaulting to the CPU."""
    cpu = measured.get("CPU")
    if not cpu or cpu["relative"] is None:
        return "CPU", "no CPU measurement"
    bar = cpu["relative"] * ACCURACY_TOLERANCE + ACCURACY_EPSILON
    best, best_ms, why = "CPU", cpu["median_ms"], "the CPU is the fastest accurate path"
    for name in ("GPU", "GPU_FULL"):
        row = measured.get(name)
        if not row or row["relative"] is None:
            continue
        # The sweep asks for a delegate and reports what it got; a request that fell back to
        # the CPU has measured the CPU twice, not the delegate.
        if not row["attached"].startswith("GPU"):
            continue
        if row["relative"] > bar:
            continue
        if row["median_ms"] < best_ms * SPEED_MARGIN:
            best, best_ms = name, row["median_ms"]
            why = (
                f"{cpu['median_ms']:.2f} ms on the CPU against {row['median_ms']:.2f} ms here, "
                f"at {row['relative']:.2e} against {cpu['relative']:.2e}"
            )
    return best, why


def main():
    sweep = sys.argv[1] if len(sys.argv) > 1 else SWEEP
    rows = parse(sweep)
    if not rows:
        print(f"android_delegate: no SWEEP lines in {sweep}", file=sys.stderr)
        return 1

    paths = {}
    reasons = {}
    for slug, measured in sorted(rows.items()):
        path, why = decide(measured)
        paths[slug] = path
        reasons[slug] = why

    record = {
        "device": {
            "model": "Pixel 7",
            "soc": "GS201",
            "api": 37,
            "abi": "arm64-v8a",
        },
        "accuracy_tolerance": ACCURACY_TOLERANCE,
        "speed_margin": SPEED_MARGIN,
        "paths": paths,
        "reasons": reasons,
        # Kept as the field the binding generator reads, so the Kotlin side needs no
        # knowledge of how the decision was reached.
        "gpu_full_width": sorted(s for s, p in paths.items() if p == "GPU_FULL"),
        "measurements": rows,
    }
    json.dump(record, open(OUT, "w"), indent=1, sort_keys=True)

    counts = {}
    for path in paths.values():
        counts[path] = counts.get(path, 0) + 1
    print(f"android_delegate: {len(paths)} models, {counts}")
    for slug, path in sorted(paths.items()):
        if path != "CPU":
            print(f"  {slug}: {path} — {reasons[slug]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
