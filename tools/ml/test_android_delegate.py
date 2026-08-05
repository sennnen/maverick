#!/usr/bin/env python3
"""Tests for the rule that decides where a model runs on Android.

The sweep this reads needs a phone; the decision it drives does not, and the decision is the
part that can be wrong quietly. A rule that lets a faster-but-worse delegate through does not
fail loudly — it ships a model that returns a slightly different embedding, and the head
reading it has no way to say so.

    python test_android_delegate.py
"""
import json
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import android_delegate

RECORDED = android_delegate.OUT


def row(attached, relative, median_ms):
    return {"attached": attached, "relative": relative, "median_ms": median_ms}


class DecideTest(unittest.TestCase):
    def test_the_cpu_is_the_default_when_nothing_else_was_measured(self):
        path, why = android_delegate.decide({"CPU": row("CPU", 1e-4, 10.0)})
        self.assertEqual("CPU", path)
        self.assertEqual("the CPU is the fastest accurate path", why)

    def test_a_faster_and_equally_accurate_gpu_wins(self):
        path, _why = android_delegate.decide(
            {
                "CPU": row("CPU", 3.67e-4, 10.34),
                "GPU_FULL": row("GPU_FULL", 3.67e-4, 5.14),
            }
        )
        self.assertEqual("GPU_FULL", path)

    def test_a_faster_but_less_accurate_gpu_is_refused(self):
        # The whole reason the sweep exists: half width on this delegate is quicker and moves
        # the answer, and quicker is not the property being bought.
        path, _why = android_delegate.decide(
            {
                "CPU": row("CPU", 1.37e-3, 2563.61),
                "GPU": row("GPU", 2.70e-2, 547.31),
            }
        )
        self.assertEqual("CPU", path)

    def test_an_accurate_gpu_that_is_barely_faster_is_refused(self):
        # Within the margin: a second code path and a driver dependency have to be earned, and
        # a single timing run is not trustworthy to a hair — `activity_context_embedding`'s CPU
        # baseline moved five-fold between sweeps on thermal state alone.
        path, _why = android_delegate.decide(
            {
                "CPU": row("CPU", 1e-4, 10.0),
                "GPU_FULL": row("GPU_FULL", 1e-4, 6.0),
            }
        )
        self.assertEqual("CPU", path)

    def test_a_gpu_request_that_fell_back_to_the_cpu_is_not_a_gpu_measurement(self):
        # `activity_history_transformer` does this: the delegate attaches and then refuses the
        # graph, so the interpreter runs on the CPU. Counting that as a GPU result would admit
        # the GPU for a model that never ran on it.
        path, _why = android_delegate.decide(
            {
                "CPU": row("CPU", 4.6e-4, 3.22),
                "GPU": row("CPU", 4.6e-4, 0.5),
            }
        )
        self.assertEqual("CPU", path)

    def test_a_catastrophically_wrong_gpu_is_refused_at_both_widths(self):
        # cva_encoder, measured: 1.4e+1 away at either width. Not a precision effect.
        measured = {
            "CPU": row("CPU", 1.84e-3, 16.55),
            "GPU": row("GPU", 1.41e1, 25.08),
            "GPU_FULL": row("GPU_FULL", 1.42e1, 25.61),
        }
        self.assertEqual("CPU", android_delegate.decide(measured)[0])

    def test_a_model_with_zero_cpu_error_still_admits_an_exact_gpu(self):
        # The epsilon exists for this: a ratio bar alone would refuse everything when the CPU
        # deviation is exactly zero, including a delegate that is also exactly zero.
        path, _why = android_delegate.decide(
            {
                "CPU": row("CPU", 0.0, 10.0),
                "GPU_FULL": row("GPU_FULL", 0.0, 1.0),
            }
        )
        self.assertEqual("GPU_FULL", path)


class RecordedSweepTest(unittest.TestCase):
    """The decision as it stands in the tree, so a re-swept file cannot change it unnoticed."""

    def setUp(self):
        path = os.path.normpath(RECORDED)
        if not os.path.exists(path):
            self.skipTest("no device sweep recorded")
        with open(path) as handle:
            self.recorded = json.load(handle)

    def test_every_model_has_a_path(self):
        self.assertEqual(41, len(self.recorded["paths"]))

    def test_only_the_one_measured_model_leaves_the_cpu(self):
        accelerated = {s: p for s, p in self.recorded["paths"].items() if p != "CPU"}
        self.assertEqual({"whr_unet_encoder": "GPU_FULL"}, accelerated)

    def test_no_model_ships_the_half_width_gpu_path(self):
        # Measured on a Tensor G2, it moved every model it ran. If a future sweep admits one,
        # that is a decision to make deliberately rather than to inherit from this file.
        self.assertNotIn("GPU", set(self.recorded["paths"].values()))

    def test_the_decision_is_reproducible_from_the_measurements_it_kept(self):
        for slug, measured in self.recorded["measurements"].items():
            self.assertEqual(
                self.recorded["paths"][slug],
                android_delegate.decide(measured)[0],
                f"{slug} disagrees with its own recorded measurements",
            )


if __name__ == "__main__":
    unittest.main(verbosity=2)
