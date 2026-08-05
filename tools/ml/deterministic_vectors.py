#!/usr/bin/env python3
"""Generate golden vectors for the zero-parameter archives.

The Rust ports in `mav_analytic::model_zoo::deterministic` are read off TorchScript's
decompiled output, and a port read off a decompilation is a hypothesis until something
executes the original and disagrees. This runs each archive on seeded inputs and writes what
it returned, so the Rust tests assert against the archive rather than against the reading.

Inputs are built per archive by a recipe below rather than at random: these are not networks
with a tolerant input distribution, they are validators followed by arithmetic, and random
tensors mostly reach the validator's exception. Each recipe produces inputs that get through,
and a few that deliberately do not, because the error codes are part of the contract too.

    python deterministic_vectors.py [archive ...]
"""
import json
import os
import sys

import numpy as np
import torch

HERE = os.path.dirname(os.path.abspath(__file__))
MODELS_DIR = os.path.join(os.path.dirname(HERE), "decrypted_models")
OUT = os.path.join(HERE, "out", "deterministic")

RECIPES = {}


def recipe(name):
    def register(builder):
        RECIPES[name] = builder
        return builder

    return register


def as_json(value):
    """Tensors and arrays to nested lists, non-finite values to null, so the file is readable."""
    if isinstance(value, torch.Tensor):
        value = value.detach().cpu().numpy()
    if isinstance(value, np.ndarray):
        if value.dtype.kind != "f":
            return value.tolist()
        return [as_json(item) for item in value] if value.ndim else as_json(value.item())
    if isinstance(value, (list, tuple)):
        return [as_json(item) for item in value]
    if isinstance(value, (np.floating, float)):
        return float(value) if np.isfinite(value) else None
    if isinstance(value, (np.integer, int)):
        return int(value)
    return value


# --------------------------------------------------------------- daily_medians


@recipe("daily_medians_1_1_0")
def daily_medians(generator):
    """Five-hour night: HRV every five minutes, skin temperature every minute.

    The met series has to contain a value below 1.8 or the validator refuses it, and the
    sleep timestamps have to come in pairs. Both are contract, not convenience.
    """
    cases = []
    for index in range(6):
        rng = np.random.default_rng(400 + index)
        start = 1_700_000_000.0
        hrv_timestamps = start + np.arange(60, dtype=np.float64) * 300.0
        hrv = 40.0 + rng.normal(0.0, 8.0, 60)
        accuracy = rng.integers(5, 60, 60).astype(np.float64)
        hr_min = 52.0 + rng.normal(0.0, 4.0, 60)
        skin_timestamps = start + np.arange(300, dtype=np.float64) * 60.0
        skin_temp = 34.0 + rng.normal(0.0, 0.5, 300)
        met_timestamps = start + np.arange(120, dtype=np.float64) * 150.0
        met = np.clip(1.0 + rng.exponential(0.6, 120), 0.9, 6.0)
        # One sleep period in the middle, plus a short one near the end.
        sleep = np.array(
            [start + 3600.0, start + 9000.0, start + 12000.0, start + 13200.0],
            dtype=np.float64,
        )
        if index == 4:
            met = np.full(120, 2.5)  # every value above the threshold: error 2
        if index == 5:
            sleep = sleep[:3]  # odd count: error 4
        cases.append(
            {
                "hrv": hrv,
                "hrv_accuracy": accuracy,
                "hrv_timestamps": hrv_timestamps,
                "hr_min": hr_min,
                "skin_temp": skin_temp,
                "skin_temp_timestamps": skin_timestamps,
                "met": met,
                "met_timestamps": met_timestamps,
                "sleep_timestamps": sleep,
            }
        )
    return cases, [
        "hrv",
        "hrv_accuracy",
        "hrv_timestamps",
        "hr_min",
        "skin_temp",
        "skin_temp_timestamps",
        "met",
        "met_timestamps",
        "sleep_timestamps",
    ]


# ------------------------------------------------------------- atlas_trendline


@recipe("atlas_trendline_1_0_0")
def atlas_trendline(generator):
    """Body-composition trends over a window: too few points, too short a span, and real fits.

    The window argument picks the minimum span — three days, ten days, a hundred and twenty —
    and the metric argument picks the coefficient of variation that becomes the weight. Both
    are swept, along with the three refusal paths, because each returns a differently shaped
    row of NaNs rather than an error.
    """
    cases = []
    # (points, window, metric, day spacing, base value). The day axis is bounded by the
    # window — seven, thirty-one, three hundred and sixty-six — and the value range by the
    # metric, so each row stays inside both or is deliberately outside one.
    for index, (count, window, metric, span, base) in enumerate(
        [
            (7, 0, 0, 1.0, 55.0),  # weekly, fat-free mass
            (30, 1, 1, 1.0, 30.0),  # monthly, skeletal muscle
            (200, 2, 2, 1.8, 20.0),  # yearly, fat mass
            (2, 0, 0, 1.0, 55.0),  # too few points
            (5, 2, 3, 1.0, 25.0),  # enough points, span far short of a year
            (6, 0, 4, 0.1, 30.0),  # six points inside half a day: span below three
            (8, 0, 5, 1.0, 30.0),  # metric past the table: refused by the validator
        ]
    ):
        rng = np.random.default_rng(700 + index)
        days = np.arange(count, dtype=np.float64) * span
        values = base + 0.005 * days + rng.normal(0.0, 0.2, count)
        confidences = np.clip(rng.uniform(0.3, 1.0, count), 0.0, 1.0)
        cases.append(
            {
                "days": days,
                "values": values,
                "confidences": confidences,
                "window": np.array(float(window)),
                "metric": np.array(float(metric)),
            }
        )
    return cases, ["days", "values", "confidences", "window", "metric"], torch.float32


# --------------------------------------------------------- astd_event_detection


@recipe("astd_event_detection_0_1_0")
def astd_event_detection(generator):
    """Fifteen-minute stress bins: sustained stretches, gaps, merges and the refusals.

    A window is four consecutive bins spanning 55 to 65 minutes, so the timestamps are laid on
    a fifteen-minute grid and some cases skip a bin to break the span. The value bands are
    fully stressed at or below -0.5, borderline down to -0.4, and the mirror image above zero.
    """
    quarter = 900_000
    base = 1_700_000_000_000

    def grid(count, skip=()):
        # Integers, not floats: these are millisecond epochs the archive reads back as ints,
        # and writing them through a float loses the low digits in the vector file as well.
        return np.array(
            [base + index * quarter for index in range(count) if index not in skip],
            dtype=np.int64,
        )

    cases = []
    # A clean stressed run of six bins, then neutral, then a restored run.
    values = np.array([-0.8, -0.7, -0.6, -0.9, -0.45, 0.0, 0.1, 0.7, 0.8, 0.6, 0.9, 0.55])
    cases.append({"dsa_values": values, "dsa_timestamps_ms": grid(len(values))})
    # One missing bin inside the stressed run: at most one NaN is allowed per window.
    holed = values.copy()
    holed[2] = np.nan
    cases.append({"dsa_values": holed, "dsa_timestamps_ms": grid(len(holed))})
    # Two missing bins in the same window: no window should survive there.
    holed2 = values.copy()
    holed2[1] = np.nan
    holed2[2] = np.nan
    cases.append({"dsa_values": holed2, "dsa_timestamps_ms": grid(len(holed2))})
    # Borderline only, never reaching the extreme threshold: rejected by the fully-count rule.
    cases.append(
        {
            "dsa_values": np.array([-0.45, -0.42, -0.48, -0.41, -0.44, -0.43]),
            "dsa_timestamps_ms": grid(6),
        }
    )
    # A gap in the timestamps stretches the window past sixty-five minutes.
    stretched = np.array([-0.8, -0.7, -0.6, -0.9, -0.8, -0.7])
    cases.append({"dsa_values": stretched, "dsa_timestamps_ms": grid(8, skip={2, 5})})
    # Two stressed runs separated by more than the merge gap, so they stay separate.
    split = np.array([-0.8, -0.7, -0.6, -0.9, 0.0, 0.0, 0.0, -0.8, -0.7, -0.6, -0.9])
    cases.append({"dsa_values": split, "dsa_timestamps_ms": grid(len(split))})
    # Fewer bins than the minimum: refused.
    cases.append({"dsa_values": np.array([-0.8, -0.7, -0.6]), "dsa_timestamps_ms": grid(3)})
    # A value outside [-1, 1]: refused.
    cases.append({"dsa_values": np.array([-1.4, -0.7, -0.6, -0.9]), "dsa_timestamps_ms": grid(4)})
    # Timestamps that do not increase: refused.
    cases.append(
        {
            "dsa_values": np.array([-0.8, -0.7, -0.6, -0.9]),
            "dsa_timestamps_ms": np.array(
                [base, base + quarter, base + quarter, base + 2 * quarter], dtype=np.int64
            ),
        }
    )
    return (
        cases,
        ["dsa_values", "dsa_timestamps_ms"],
        {"dsa_values": torch.float64, "dsa_timestamps_ms": torch.int64},
    )


# ------------------------------------------------------------- cva_calibrator


@recipe("cva_calibrator_1_3_0")
def cva_calibrator(generator):
    """Cardiovascular-age calibration: the offset paths, the freeze, and the reset.

    Every branch here turns on how much history there is and where the hardware change sits,
    so the cases differ in exactly those: no history at all, history too short to smooth,
    enough to smooth, a hardware change inside the last thirty days with enough readings
    before it to carry an offset across, an already-frozen offset, and a baseline reset.
    """
    day = 86_400.0
    base = 1_700_000_000.0

    def case(
        count,
        *,
        hw_offset_days,
        daily=45.0,
        sex=1.0,
        frozen_last=0.0,
        offsets=None,
        reset=False,
        freeze_days=14,
        seed=0,
    ):
        rng = np.random.default_rng(900 + seed)
        timestamps = (base + np.arange(count, dtype=np.float64) * day).reshape(count, 1)
        values = (45.0 + rng.normal(0.0, 1.5, count)).reshape(count, 1)
        offset_column = (
            np.zeros((count, 1)) if offsets is None else np.asarray(offsets, dtype=np.float64).reshape(count, 1)
        )
        frozen = np.zeros((count, 1))
        if count:
            frozen[-1, 0] = frozen_last
        return {
            "daily_cva_value": np.array([[daily]]),
            "hw_serial_change": np.array([[base + hw_offset_days * day]]),
            "calibrated_cva_values": values,
            "offsets": offset_column,
            "is_offset_frozen": frozen,
            "timestamps": timestamps,
            "sex_at_birth": np.array([[sex]]),
            "freeze_offset_days": np.array([[freeze_days]]),
            "reset_baseline": np.array([[reset]]),
        }

    cases = [
        # No history: the daily value passes through and nothing is smoothed.
        case(0, hw_offset_days=-1, seed=0),
        # Some history, but fewer than the fourteen readings smoothing needs.
        case(8, hw_offset_days=-1, seed=1),
        # Enough history on the current hardware to smooth, male and female curves.
        case(30, hw_offset_days=-1, seed=2),
        case(30, hw_offset_days=-1, sex=-1.0, seed=3),
        # A hardware change twenty days ago, with a long run before it to carry an offset over.
        case(40, hw_offset_days=20, seed=4),
        # The same, but the last row already froze its offset, so it is reused as it stands.
        case(40, hw_offset_days=20, frozen_last=1.0, offsets=[2.5] * 40, seed=5),
        # Enough non-zero offsets on the current hardware to trigger the freeze.
        case(40, hw_offset_days=20, offsets=[1.0 + 0.05 * i for i in range(40)], seed=6),
        # A baseline reset discards the offset entirely.
        case(30, hw_offset_days=-1, reset=True, seed=7),
    ]
    order = [
        "daily_cva_value",
        "hw_serial_change",
        "calibrated_cva_values",
        "offsets",
        "is_offset_frozen",
        "timestamps",
        "sex_at_birth",
        "freeze_offset_days",
        "reset_baseline",
    ]
    dtypes = {name: torch.float32 for name in order}
    dtypes["freeze_offset_days"] = torch.int64
    dtypes["reset_baseline"] = torch.bool
    return cases, order, dtypes


# --------------------------------------------------------- steps_motion_decoder


@recipe("steps_motion_decoder_2_0_0")
def steps_motion_decoder(generator):
    """Quantised motion features off the strap: whole-range codes, zeros, and the tick spacing.

    The codes are integers in each column's own bit range, so the cases sweep the range rather
    than sample it — the endpoints are where a scale or a transform is wrong by the most.
    Column three onwards repeats in three thirty-second sub-windows, and the timestamp spacing
    decides how those are laid back out, so one case uses an irregular spacing the archive
    replaces with its nominal thirty seconds.
    """
    # Bit depth per column, in the archive's column order.
    bits = [10, 8, 8] + [9, 9, 9, 7, 8, 8, 8, 8] * 3
    cases = []
    for index, (rows, spacing) in enumerate(
        [(4, 30_000), (1, 30_000), (6, 30_000), (5, 40_000), (3, 30_000)]
    ):
        rng = np.random.default_rng(1100 + index)
        data = np.zeros((rows, 27), dtype=np.float64)
        for column, width in enumerate(bits):
            top = 2**width - 1
            if index == 0:
                # Sweep: minimum, maximum, and two interior codes.
                data[:, column] = np.linspace(0, top, rows)
            elif index == 2:
                # Zeros throughout, which is the encode_zero path for stride frequency.
                data[:, column] = 0.0
            else:
                data[:, column] = rng.integers(0, top + 1, rows)
        timestamps = (
            1_700_000_000_000 + np.arange(rows, dtype=np.int64) * spacing
        ).reshape(rows, 1)
        cases.append({"timestamps": timestamps, "data": data})
    return (
        cases,
        ["timestamps", "data"],
        {"timestamps": torch.int64, "data": torch.float32},
    )


# ----------------------------------------------------------------- meal_timing


@recipe("meal_timing_0_1_0")
def meal_timing(generator):
    """Logged meal times over a fortnight: tight windows, scattered ones, and the wrap-around.

    Meals land in 48 half-hour bins of local time, and the bin array is extended by twelve so a
    window spanning midnight is contiguous rather than split. The cases are built from explicit
    meal hours so each exercises one behaviour: three tidy windows, one late-night window that
    wraps, a scattered day that should not form clusters, and too few meals to score.
    """
    day = 86_400
    base = 1_700_000_000
    # A timezone offset in seconds, added to the timestamp before binning.
    offset = 3600

    def days(hours, count):
        stamps = []
        for index in range(count):
            for hour in hours:
                stamps.append(base + index * day + int(hour * 3600))
        return np.array(stamps, dtype=np.float64)

    cases = []
    # Breakfast, lunch and dinner, held to within a half hour for a fortnight.
    cases.append(days([7.5, 12.5, 19.0], 14))
    # The same, plus a late-night meal that wraps past midnight into the extension.
    cases.append(days([7.5, 12.5, 19.0, 23.5], 14))
    # Scattered: every meal at a different hour, so no bin dominates.
    scattered = np.array(
        [base + index * day + int((6 + (index * 37) % 16) * 3600) for index in range(28)],
        dtype=np.float64,
    )
    cases.append(scattered)
    # One long grazing window across the afternoon.
    cases.append(days([13.0, 13.5, 14.0, 14.5, 15.0, 15.5, 16.0], 10))
    # Too few meals logged to score consistency at all.
    cases.append(days([8.0], 6))
    # A single meal: one bin, which is the half-bin-widening path.
    cases.append(days([8.0], 1))

    built = []
    for stamps in cases:
        built.append(
            {
                "unix_timestamps": stamps,
                "unix_timezones": np.full(len(stamps), offset, dtype=np.float64),
            }
        )
    # Mismatched lengths: refused.
    built.append(
        {
            "unix_timestamps": days([8.0, 12.0], 3),
            "unix_timezones": np.full(4, offset, dtype=np.float64),
        }
    )
    return built, ["unix_timestamps", "unix_timezones"]


# --------------------------------------------------- training_stress_score


@recipe("training_stress_score_0_2_1")
def training_stress_score(generator):
    """Twelve hours of MET readings and the demographics that scale them.

    The score needs a full 720-minute window before it produces anything, so every case
    carries at least that. The cases vary what the scaling depends on: sex, age band, resting
    heart rate against its percentile table, and whether a VO2max is available at all — with
    one absent, since that switches the weighting from VO2max to resting heart rate.
    """
    cases = []
    for index, (minutes, age, sex, rhr, vo2max, readiness, tz_change, no_ots) in enumerate(
        [
            (800, 35.0, 1.0, 55.0, 45.0, 80.0, 0.0, 0.0),
            (800, 35.0, -1.0, 62.0, 38.0, 80.0, 0.0, 0.0),
            (800, 65.0, 0.0, 70.0, 30.0, 80.0, 0.0, 0.0),
            # No VO2max: the weighting falls back to the resting-heart-rate percentile.
            (760, 28.0, 1.0, 48.0, float("nan"), 80.0, 0.0, 0.0),
            # Readiness below sixty lowers the high-score threshold.
            (760, 45.0, -1.0, 58.0, 42.0, 50.0, 0.0, 0.0),
            # A quiet window: most readings below the movement floor, so the score is absent.
            (760, 45.0, 1.0, 58.0, 42.0, 80.0, 0.0, 0.0),
            # A timezone change in the last twelve hours: refused.
            (760, 45.0, 1.0, 58.0, 42.0, 80.0, 1.0, 0.0),
        ]
    ):
        rng = np.random.default_rng(1300 + index)
        if index == 5:
            mets = np.full(minutes, 0.5)
            mets[:100] = 3.0
        else:
            mets = np.clip(1.0 + rng.gamma(2.0, 0.8, minutes), 0.5, 12.0)
        cases.append(
            {
                "start_timestamp": np.array([1_700_000_000_000], dtype=np.int64),
                "mets": mets,
                "age": np.array([age]),
                "biological_sex": np.array([sex]),
                "rhr": np.array([rhr]),
                "no_ots": np.array([no_ots]),
                "tz_change": np.array([tz_change]),
                "readiness": np.array([readiness]),
                "vo2max": np.array([vo2max]),
            }
        )
    order = [
        "start_timestamp",
        "mets",
        "age",
        "biological_sex",
        "rhr",
        "no_ots",
        "tz_change",
        "readiness",
        "vo2max",
    ]
    dtypes = {name: torch.float32 for name in order}
    dtypes["start_timestamp"] = torch.int64
    return cases, order, dtypes


# ------------------------------------------------------- pregnancy_biometrics


@recipe("pregnancy_biometrics_0_4_0")
def pregnancy_biometrics(generator):
    """A pregnancy's biometric series, and the ways a baseline fails to establish.

    Each series is 350 days, one row per gestational day, and the baseline is found by
    searching for the first fifteen-day window holding eight usable readings. Days where the
    temperature deviation reaches 1 degree are masked out of *every* biometric, so the cases
    vary how much fever there is as well as how much data.
    """
    days = 350
    cases = []
    for index, (coverage, fever_days, age, gestational_day, backfill) in enumerate(
        [
            (0.9, 0, 28.0, 200.0, 0.0),
            (0.9, 0, 32.0, 120.0, 1.0),
            (0.9, 0, 40.0, 300.0, 0.0),
            # A fever stretch early on, so the baseline window has to move past it.
            (0.9, 40, 31.0, 250.0, 0.0),
            # Sparse: no fifteen-day window holds eight readings, so no personal baseline.
            (0.15, 0, 33.0, 180.0, 0.0),
            # Nothing at all.
            (0.0, 0, 29.0, 100.0, 0.0),
        ]
    ):
        rng = np.random.default_rng(1500 + index)
        present = rng.random(days) < coverage
        def series(mean, spread):
            values = rng.normal(mean, spread, days).astype(np.float64)
            values[~present] = np.nan
            return values.reshape(days, 1)

        temperature = rng.normal(0.0, 0.2, days)
        temperature[:fever_days] = 1.4
        temperature[~present] = np.nan
        cases.append(
            {
                "average_heart_rate": series(68.0, 3.0),
                "average_hrv": series(48.0, 6.0),
                "average_breath": series(15.5, 0.8),
                "temperature_deviation": temperature.reshape(days, 1),
                "age": np.array([age]),
                "gestational_day": np.array([gestational_day]),
                "is_backfill": np.array([backfill]),
            }
        )
    # A gestational day past the table: refused.
    rejected = dict(cases[0])
    rejected["gestational_day"] = np.array([400.0])
    cases.append(rejected)
    order = [
        "average_heart_rate",
        "average_hrv",
        "average_breath",
        "temperature_deviation",
        "age",
        "gestational_day",
        "is_backfill",
    ]
    return cases, order, {name: torch.float32 for name in order}


# ----------------------------------------------------------- stress_resilience


@recipe("stress_resilience_2_2_1")
def stress_resilience(generator):
    """A day of stress readings plus thirteen days of history, across the resilience levels.

    Resilience is a level from one to five, read off where the fortnight's recovery sits
    against a curve fitted through its stress. The cases move the history up and down that
    curve so each level is exercised, and add the two refusal paths that matter: too little
    daytime stress to score the day at all, and too short a history to score the fortnight.
    """
    day = 86_400
    base = 1_700_000_000

    def build(
        *,
        stress_mean,
        history_stress,
        history_recovery,
        history_sleep,
        hours=10.0,
        hrv=60.0,
        history=13,
        seed=0,
    ):
        rng = np.random.default_rng(1700 + seed)
        count = int(hours * 6)
        # Readings every ten minutes through the day, starting after the night.
        stamps = (base + 9 * 3600 + np.arange(count) * 600) * 1000
        values = rng.normal(stress_mean, 0.4, count)
        return {
            "sleep_start_timestamps": np.array([(base + 1 * 3600) * 1000], dtype=np.float64),
            "sleep_end_timestamps": np.array([(base + 8 * 3600) * 1000], dtype=np.float64),
            "sleep_score": np.array([[78.0]]),
            "hrv_balance": np.array([[hrv]]),
            "recovery_index": np.array([[72.0]]),
            "resting_heart_rate": np.array([[80.0]]),
            "stress_lim": np.array([[-0.5]]),
            "saturation_stress_deviation": np.array([[-2.0]]),
            "saturation_recovery_deviation": np.array([[2.0]]),
            "recovery_lim": np.array([[0.5]]),
            "stress": values.reshape(-1, 1),
            "stress_timestamps": stamps.reshape(-1, 1).astype(np.float64),
            "daily_stress_list": np.full((history, 1), history_stress),
            "daily_restorative_time_list": np.full((history, 1), history_recovery),
            "daily_sleep_recovery_list": np.full((history, 1), history_sleep),
        }

    cases = [
        # A calm fortnight: high recovery against low stress, the top of the scale.
        build(stress_mean=0.9, history_stress=20.0, history_recovery=60.0, history_sleep=85.0, seed=0),
        # A hard one: high stress, little recovery.
        build(stress_mean=-1.2, history_stress=85.0, history_recovery=8.0, history_sleep=25.0, seed=1),
        # Two in between, to land on the middle levels.
        build(stress_mean=-0.2, history_stress=50.0, history_recovery=30.0, history_sleep=55.0, seed=2),
        build(stress_mean=0.2, history_stress=40.0, history_recovery=42.0, history_sleep=68.0, seed=3),
        # No HRV balance: the sleep-recovery mean drops that term and renormalises.
        build(stress_mean=0.0, history_stress=45.0, history_recovery=35.0, history_sleep=60.0,
              hrv=float("nan"), seed=4),
        # Two hours of readings: below the four-hour daytime minimum.
        build(stress_mean=0.0, history_stress=45.0, history_recovery=35.0, history_sleep=60.0,
              hours=2.0, seed=5),
        # A history that is mostly missing: too few days to score the fortnight.
        build(stress_mean=0.0, history_stress=float("nan"), history_recovery=float("nan"),
              history_sleep=float("nan"), seed=6),
    ]
    # A history of the wrong length: refused.
    cases.append(
        build(stress_mean=0.0, history_stress=40.0, history_recovery=40.0, history_sleep=60.0,
              history=10, seed=7)
    )
    order = [
        "sleep_start_timestamps",
        "sleep_end_timestamps",
        "sleep_score",
        "hrv_balance",
        "recovery_index",
        "resting_heart_rate",
        "stress_lim",
        "saturation_stress_deviation",
        "saturation_recovery_deviation",
        "recovery_lim",
        "stress",
        "stress_timestamps",
        "daily_stress_list",
        "daily_restorative_time_list",
        "daily_sleep_recovery_list",
    ]
    return cases, order, {name: torch.float32 for name in order}


# ----------------------------------------------------------- cumulative_stress


CUMULATIVE_STRESS_ORDER = [
    "got_ups",
    "lowest_heart_rate",
    "sleep_phase_30_sec",
    "hrv_items",
    "average_hrv",
    "resting_hr_average",
    "temperature_avg",
    "average_met_minutes",
    "long_sleep_hrv",
    "hrv_medianHR_5min",
    "hrv_quality_5min",
    "temp_skin",
    "sleep_fragmentation_index",
    "norm_hrv_medianHR_5min",
    "median_hrv_quality_5min",
    "normalised_iqr",
    "norm_temp_wake",
    "highest_temperature",
    "temperature_dev",
    "temperature_dev_baseline",
    "total_sleep_duration",
    "n_days_to_ovulation",
    "n_days_to_period",
    "cycle_phase",
    "interpreted_cycle_phase",
    "bedtime_start",
    "temp_skin_timestamps",
]

# Column lengths the validator insists on: thirty-one nights of history for most series,
# thirty for the ones whose latest value this call is computing, and one for the scalars.
CUMULATIVE_STRESS_LENGTHS = {
    "sleep_phase_30_sec": 960,
    "hrv_items": 480,
    "temp_skin": 480,
    "temp_skin_timestamps": 480,
    # The validator ties these three together: whatever the names suggest, it requires one
    # entry per thirty-second sleep epoch for all of them.
    "hrv_medianHR_5min": 960,
    "hrv_quality_5min": 960,
    "temperature_avg": 1,
    "n_days_to_ovulation": 1,
    "n_days_to_period": 1,
    "bedtime_start": 1,
    "average_met_minutes": 30,
    "sleep_fragmentation_index": 30,
    "norm_hrv_medianHR_5min": 30,
    "median_hrv_quality_5min": 30,
    "normalised_iqr": 30,
    "norm_temp_wake": 30,
    "interpreted_cycle_phase": 30,
}


@recipe("cumulative_stress_1_2_2")
def cumulative_stress(generator):
    """A month of nights, then the clustering that turns them into one chronic-stress score.

    Thirty-one nights of summaries plus the newest night's raw sleep, HRV and skin
    temperature. The cases vary the two things that stop a score being produced — a fever on
    the latest night, and a night too short to summarise — and the cycle-phase inputs, which
    move the temperature threshold a fever is judged against.
    """
    nights = 31
    base_ms = 1_700_000_000_000

    def build(*, seed, fever=False, short_sleep=False, ovulation=5.0, period=12.0, gaps=0):
        rng = np.random.default_rng(1900 + seed)

        def column(name, low, high):
            length = CUMULATIVE_STRESS_LENGTHS.get(name, nights)
            return rng.uniform(low, high, length).reshape(length, 1)

        temperature = column("highest_temperature", 36.2, 37.2)
        deviation = column("temperature_dev", -0.25, 0.25)
        if fever:
            temperature[-1, 0] = 39.0
        sleep = column("total_sleep_duration", 25_000.0, 30_000.0)
        if short_sleep:
            sleep[-1, 0] = 7_000.0
        got_ups = column("got_ups", 1.0, 4.0)
        if gaps:
            # A stretch of missing nights, which is what pushes a contributor below the
            # twenty-one usable days the score needs.
            got_ups[:gaps, 0] = np.nan
        return {
            "got_ups": got_ups,
            "lowest_heart_rate": column("lowest_heart_rate", 48.0, 60.0),
            "sleep_phase_30_sec": rng.integers(
                1, 5, CUMULATIVE_STRESS_LENGTHS["sleep_phase_30_sec"]
            ).astype(np.float64).reshape(-1, 1),
            "hrv_items": column("hrv_items", 30.0, 80.0),
            "average_hrv": column("average_hrv", 40.0, 70.0),
            "resting_hr_average": column("resting_hr_average", 50.0, 62.0),
            "temperature_avg": np.array([[35.5]]),
            "average_met_minutes": column("average_met_minutes", 1.2, 1.8),
            "long_sleep_hrv": column("long_sleep_hrv", 40.0, 70.0),
            "hrv_medianHR_5min": column("hrv_medianHR_5min", 50.0, 65.0),
            "hrv_quality_5min": column("hrv_quality_5min", 60.0, 100.0),
            "temp_skin": column("temp_skin", 33.0, 35.0),
            "sleep_fragmentation_index": column("sleep_fragmentation_index", 10.0, 40.0),
            "norm_hrv_medianHR_5min": column("norm_hrv_medianHR_5min", 0.9, 1.1),
            "median_hrv_quality_5min": column("median_hrv_quality_5min", 0.6, 1.0),
            "normalised_iqr": column("normalised_iqr", 0.2, 0.5),
            "norm_temp_wake": column("norm_temp_wake", 0.95, 1.0),
            "highest_temperature": temperature,
            "temperature_dev": deviation,
            "temperature_dev_baseline": np.full((nights, 1), 0.4),
            "total_sleep_duration": sleep,
            "n_days_to_ovulation": np.array([[ovulation]]),
            "n_days_to_period": np.array([[period]]),
            "cycle_phase": rng.integers(0, 2, nights).astype(np.float64).reshape(nights, 1),
            "interpreted_cycle_phase": rng.integers(0, 2, 30)
            .astype(np.float64)
            .reshape(30, 1),
            "bedtime_start": np.array([[float(base_ms)]]),
            "temp_skin_timestamps": (
                np.arange(CUMULATIVE_STRESS_LENGTHS["temp_skin"]) * 60_000.0 + base_ms
            ).reshape(-1, 1),
        }

    cases = [
        build(seed=0),
        build(seed=1, ovulation=-3.0),
        build(seed=2, ovulation=float("nan"), period=float("nan")),
        build(seed=3, fever=True),
        build(seed=4, short_sleep=True),
        build(seed=5, gaps=15),
    ]
    return (
        cases,
        CUMULATIVE_STRESS_ORDER,
        {name: torch.float32 for name in CUMULATIVE_STRESS_ORDER},
    )


# ----------------------------------------------------------------------- atlas


@recipe("atlas_2_1_0")
def atlas(generator):
    """Body composition from a bioimpedance sweep, across both sexes and the history paths.

    Two 500-sample rows carry the in-phase and quadrature response at one excitation
    frequency; the settled value of each is its mode over the last three seconds. The cases
    sweep sex, body size, and how much prior history there is to check the estimate against —
    none, one entry, and a full ten — because the confidence figure is built from that.
    """
    samples = 500
    cases = []
    for index, (sex, age, weight, height, history) in enumerate(
        [
            (1.0, 34.0, 82.0, 180.0, 10),
            (0.0, 29.0, 62.0, 165.0, 10),
            (1.0, 55.0, 95.0, 175.0, 0),
            (0.0, 41.0, 58.0, 158.0, 1),
            (1.0, 22.0, 70.0, 188.0, 4),
        ]
    ):
        rng = np.random.default_rng(2100 + index)
        # The settled value is a mode, so each row is a steady level with a little noise on it.
        in_phase = rng.normal(120_000.0, 400.0, samples)
        quadrature = rng.normal(-18_000.0, 300.0, samples)
        historical = np.full((10, 3), np.nan)
        for row in range(history):
            historical[row] = [
                60.0 + rng.normal(0.0, 1.0),
                float(row * 3 + 1),
                0.8,
            ]
        cases.append(
            {
                "bioz_signals": np.stack([in_phase, quadrature]),
                "eda_vals": rng.uniform(1000.0, 5000.0, 3),
                "temperature": np.array([33.5]),
                "demographics": np.array([sex, age, weight, height]),
                "calibration_coeffs": np.array([[500.0, -200.0, 1.05, 3.0]]),
                "historical_data": historical,
            }
        )
    order = [
        "bioz_signals",
        "eda_vals",
        "temperature",
        "demographics",
        "calibration_coeffs",
        "historical_data",
    ]
    return cases, order, {name: torch.float32 for name in order}


# ------------------------------------------- the two ports that predate this generator


@recipe("stress_daytime_sensing_1_1_0")
def daytime_stress(generator):
    """One daytime HRV reading against its baselines, awake and asleep, moving and still."""
    base = 1_700_000_000
    cases = []
    for index, (dhrv, offset_hours, baseline, night_baseline, met) in enumerate(
        [
            (46.0, 14.0, 46.0, 60.0, 1.1),
            (30.0, 14.0, 46.0, 60.0, 1.1),
            (70.0, 14.0, 46.0, 60.0, 1.1),
            # Inside the sleep window, so the reading is not a daytime one.
            (46.0, 3.0, 46.0, 60.0, 1.1),
            # Moving: above the MET floor.
            (46.0, 14.0, 46.0, 60.0, 3.5),
            (52.0, 20.0, 40.0, 55.0, 1.0),
        ]
    ):
        cases.append(
            {
                "dhrv_value": np.array([dhrv]),
                "dhrv_value_timestamp": np.array([base + offset_hours * 3600.0]),
                "bedtime_start": np.array([base + 1.0 * 3600.0]),
                "bedtime_end": np.array([base + 8.0 * 3600.0]),
                "dhrv_baseline": np.array([baseline]),
                "night_hrv_baseline": np.array([night_baseline]),
                "ring_met": np.array([met]),
            }
        )
    return cases, [
        "dhrv_value",
        "dhrv_value_timestamp",
        "bedtime_start",
        "bedtime_end",
        "dhrv_baseline",
        "night_hrv_baseline",
        "ring_met",
    ]


@recipe("daily_short_term_baselines_1_1_0")
def short_term_baselines(generator):
    """A history of nights, from a full one down to the lengths that refuse to produce one."""
    cases = []
    for index, (days, implausible) in enumerate([(14, 0), (7, 0), (5, 0), (4, 0), (14, 4)]):
        rng = np.random.default_rng(2300 + index)
        sleep = rng.uniform(20_000.0, 30_000.0, days)
        lowest = rng.uniform(48.0, 60.0, days)
        highest = rng.uniform(35.0, 37.0, days)
        hrv = rng.uniform(35.0, 65.0, days)
        # Nights the plausibility filter should discard: too short, and out of range.
        for night in range(implausible):
            sleep[night] = 3_000.0
            lowest[night] = 250.0
        cases.append(
            {
                "dhrv_medians": rng.uniform(35.0, 60.0, days),
                "skin_temp_medians": rng.uniform(33.0, 35.0, days),
                "hr_min_medians": rng.uniform(45.0, 58.0, days),
                "total_sleep_durations": sleep,
                "lowest_heart_rates": lowest,
                "highest_temperatures": highest,
                "average_hrvs": hrv,
            }
        )
    return cases, [
        "dhrv_medians",
        "skin_temp_medians",
        "hr_min_medians",
        "total_sleep_durations",
        "lowest_heart_rates",
        "highest_temperatures",
        "average_hrvs",
    ]


def run(archive, cases, order, dtype=torch.float64):
    """`dtype` is either one dtype for every input, or a per-input mapping.

    The archives do not agree on this and each validates what it was traced for: the trendline
    refuses anything but float32, the medians want float64, and the event detector reads its
    millisecond timestamps as integers — passed as float they overflow the conversion back to
    int rather than being rounded.
    """
    model = torch.jit.load(os.path.join(MODELS_DIR, f"{archive}.pt"), map_location="cpu")
    model.eval()
    vectors = []
    for case in cases:
        args = [
            torch.as_tensor(
                case[name],
                dtype=dtype[name] if isinstance(dtype, dict) else dtype,
            )
            for name in order
        ]
        record = {"inputs": {name: as_json(case[name]) for name in order}}
        try:
            with torch.no_grad():
                output = model(*args)
            record["outputs"] = as_json(output)
        except Exception as exc:  # noqa: BLE001 - a refused input is part of the contract
            record["error"] = str(exc).strip().splitlines()[-1][:200]
        vectors.append(record)
    return vectors


def main():
    os.makedirs(OUT, exist_ok=True)
    wanted = sys.argv[1:] or sorted(RECIPES)
    for archive in wanted:
        if archive not in RECIPES:
            print(f"{archive}: no recipe")
            continue
        built = RECIPES[archive](None)
        cases, order = built[0], built[1]
        dtype = built[2] if len(built) > 2 else torch.float64
        vectors = run(archive, cases, order, dtype)
        path = os.path.join(OUT, f"{archive}.json")
        with open(path, "w") as handle:
            json.dump({"archive": archive, "order": order, "vectors": vectors}, handle, indent=1)
        good = sum(1 for v in vectors if "outputs" in v)
        print(f"{archive}: {good}/{len(vectors)} produced output -> {path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
