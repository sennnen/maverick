#!/usr/bin/env python3
"""Choose each Core ML model's precision and compute units by measurement.

Two knobs, and neither has a right answer that holds across models.

**Precision.** `compute_precision=FLOAT16` halves the weights and the arithmetic together,
and coremltools offers no way to separate them — the backend folds a widening cast straight
back into a full-width constant. On the PulseNet encoder that costs 1.6e-1 relative on the
accelerated path against 9e-7 at FLOAT32, so it cannot be a blanket choice.

**Compute units.** These are not interchangeable either, and not in the way one would guess.
The sleep models are exact on the CPU and on the Neural Engine, and wrong by more than one
whole relative unit on the *GPU* — at every precision. A model that ships without saying
which units are safe for it is a model that computes something different depending on what
else the phone is doing.

So both are searched, and the first configuration that tracks the PyTorch reference wins.
The order prefers the smaller artefact first and the more accelerated path second, which is
the trade the apps want when it is available and the gate refuses when it is not.
"""

# Above this relative deviation from the float32 PyTorch reference, a configuration is
# rejected and the next one tried.
PARITY_BAR = 1e-3

# Ranked preference. Half precision first because it halves the bundle; within a precision,
# the most accelerated path first.
COMPUTE_ORDER = ("ALL", "CPU_AND_NE", "CPU_AND_GPU", "CPU_ONLY")
PRECISION_ORDER = ("float16", "float32")


def compute_unit(name):
    import coremltools as ct

    return getattr(ct.ComputeUnit, name)


def precision(name):
    import coremltools as ct

    return ct.precision.FLOAT16 if name == "float16" else ct.precision.FLOAT32


def select(convert_and_save, measure):
    """Search the configurations and return the first that clears the bar.

    `convert_and_save(precision_name)` converts at that precision, saves to the artefact
    path, and returns nothing. `measure(compute_unit_name)` loads what was saved and returns
    a parity dict with `max_rel`.

    Returns `(precision_name, compute_units_name, parity, attempts)`. When nothing clears the
    bar, the best attempt is returned so the caller can record a model that converted but
    could not be made to agree — that is a reportable outcome, not a crash.
    """
    attempts = []
    best = None
    for precision_name in PRECISION_ORDER:
        convert_and_save(precision_name)
        for units in COMPUTE_ORDER:
            try:
                parity = measure(units)
            except Exception as exc:  # noqa: BLE001 - a refused unit combination is data
                attempts.append(
                    {"precision": precision_name, "compute_units": units, "error": str(exc)[:160]}
                )
                continue
            attempts.append(
                {"precision": precision_name, "compute_units": units, "parity": parity}
            )
            if best is None or parity["max_rel"] < best[2]["max_rel"]:
                best = (precision_name, units, parity)
            if parity["max_rel"] <= PARITY_BAR:
                return precision_name, units, parity, attempts
        # A precision that cannot pass on any unit will not pass at a wider one either; keep
        # searching precisions rather than stopping, because full width often fixes it.
    if best is None:
        raise RuntimeError("no core ml configuration produced a prediction")
    # The artefact on disk is from the last precision converted. Re-convert to the best one so
    # the file and the recorded configuration agree.
    if best[0] != PRECISION_ORDER[-1]:
        convert_and_save(best[0])
    return best[0], best[1], best[2], attempts
