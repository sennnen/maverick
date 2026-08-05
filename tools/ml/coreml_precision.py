#!/usr/bin/env python3
"""Which arithmetic runs at half width, and which few operations do not.

Core ML offers one knob — `compute_precision` — and it is coarser than it looks.
`FLOAT16` halves every intermediate; `FLOAT32` halves none and, because the Neural Engine is
half-precision hardware, takes the whole program off the accelerator. `compute_plan.py`
measures that directly: under FLOAT32 not one operation in the zoo is assigned to the ANE.

Between the two sits `FP16ComputePrecision(op_selector=...)`, which decides per operation.
That is the mechanism for the exception the policy allows: a model whose arithmetic is fine
at half width except in a handful of places runs at half width except in those places, and
the places are named rather than guessed at.

The policies below are tried in order and the first that clears the parity bar is the one that
ships. Half width is the default and a model only leaves it by failing a measurement — the
accelerator share is recorded as evidence, not used as a veto. An earlier version did use it as
one, reasoning that a policy which buys no Neural Engine is not worth its accuracy cost; that
was wrong twice over. Half-precision arithmetic is faster and lower-power on the GPU and the
CPU too, not only on the accelerator, and it is what makes the two platforms compute the same
thing. They are ordered by how much of the graph they leave at half width:

  half            every operation at half width. The default, and what most models get.
  half_pooled     only the global pooling reductions stay full width. The narrowest useful
                  exemption: it fixes the models whose squeeze-and-excite mean is the problem
                  without taking anything else off the accelerator.
  half_reduced    every accumulation stays full width. A reduction sums many terms into one, so
                  its rounding error grows with the number of terms rather than staying at
                  one ulp; normalisation then divides by that sum and propagates it. Whether
                  the convolutions around them still reach the accelerator depends on the
                  graph and is measured, not assumed.
  half_stable     the above, plus the transcendental and reciprocal operations. `rsqrt` and
                  `exp` at half width lose accuracy fastest where their input is small,
                  which is exactly where a normalisation lands.
  full            no half-precision arithmetic anywhere, and therefore no Neural Engine.
                  Last resort, recorded per model together with the parity that forced it and
                  what each earlier rung measured.

Weight *storage* is a separate question and is not decided here: under the half policies
Core ML stores half-width weights as a matter of course, and `coreml_fp16_weights.py` does it
for the full policy. Either way the bytes on disk are the same.
"""

# Reductions and the normalisations built on them: error grows with the number of terms
# summed, not with one rounding step.
ACCUMULATING = {
    "reduce_sum",
    "reduce_mean",
    "reduce_prod",
    "reduce_l2_norm",
    "reduce_log_sum_exp",
    "cumsum",
    "layer_norm",
    "batch_norm",
    "instance_norm",
    "l2_norm",
    "softmax",
}

# Transcendentals and reciprocals, which lose the most relative accuracy exactly where a
# normalisation puts them: near zero.
UNSTABLE = {
    "rsqrt",
    "sqrt",
    "inverse",
    "real_div",
    "exp",
    "log",
    "erf",
    "pow",
}

# The single operation a global pool is: one mean over the whole time axis. It is the
# narrowest exemption in the ladder and the one that most often suffices, because a mean over
# thousands of elements is where half precision's accumulator actually runs out — a
# squeeze-and-excite block averaging 1,500 samples per channel is summing 1,500 terms into a
# format with three decimal digits.
POOLING = {"reduce_mean", "reduce_sum"}

POLICIES = ("half", "half_pooled", "half_reduced", "half_stable", "full")

# How far a converted model may sit from the float32 PyTorch reference, measured on the
# artefact that ships, loaded the way the app loads it. Half-precision arithmetic is not free
# and this bar says how much of it is tolerable: 1e-2 relative is under a tenth of the spread
# between the zoo's own model generations on the same input, so a head reading the output
# cannot tell which policy produced it.
PARITY_BAR = 1e-2

# Relative error is the wrong measure when an output happens to land near zero: the PPG-score
# head reads 4e-4 relative on one probe and 1.0 on another with the same weights, because the
# second probe's score is a hair from zero and the scale it is divided by goes with it. A
# deviation this small in absolute terms is agreement whatever the ratio says.
ABSOLUTE_FLOOR = 1e-3


# The tighter bar a policy must clear to be *preferred for the accelerator*.
#
# `PARITY_BAR` says what an artefact may deviate by. This says how much of that allowance a
# policy may already have spent before the pipeline is willing to choose it for the Neural
# Engine, and it is half, because the ladder measures on eight probes and eight probes
# underestimate.
#
# Measured rather than guessed. Running the shipped artefacts against three probes with seeds
# and pulse rates the ladder never uses, the independent number came out higher than the
# pipeline's own on the models that were close to the bar — `activity_transition` by 2.16x
# (5.10e-3 measured here, 1.10e-2 there), `step_head` by 3.30x, `cva_predictor_v1_base` by
# 1.61x — while the median across the zoo sat at 0.95x. A gate calibrated on its own probes
# admits at the bar and ships past it.
#
# Full width is not held to this. It is the fallback, it is what a model gets when nothing
# else qualifies, and refusing it on margin would leave nothing to fall back to.
ACCELERATED_BAR = PARITY_BAR / 2


def acceptable(parity, bar=PARITY_BAR):
    """Whether a measured parity clears `bar`, on either measure."""
    return parity["max_rel"] <= bar or parity["max_abs"] <= ABSOLUTE_FLOOR


def choose(attempts):
    """Pick the policy to ship from the measured attempts, and say why.

    Half-precision arithmetic is what the Neural Engine runs and what Android's GPU delegate
    runs, so where a policy both clears the bar and reaches the accelerator it wins outright —
    take the earliest such rung, which is the one leaving most of the graph at half width.

    That first branch holds a policy to `ACCELERATED_BAR` rather than `PARITY_BAR` — half the
    allowance — because choosing the accelerator is a choice made from eight sampled probes,
    and eight probes were measured underestimating the worst case by up to 2.16x. The fallback
    branch keeps the full bar; it is what a model gets when nothing else qualifies.

    Where *no* policy reaches the accelerator the trade is different. Half width then buys
    nothing on iOS that full width does not, and it costs twice: accuracy against the
    reference, and agreement with Android — which a device sweep has since put on a full-width
    CPU path for all but one model, so half width on iOS is now a divergence rather than a
    match. So the most accurate passing policy wins instead. Both branches are recorded per
    model, with every rung's measurement, because "this model is float16" and "this model is
    float16 *at runtime*" are different claims and the second is the one that has to be earned.
    """
    usable = [a for a in attempts if "parity" in a]
    if not usable:
        return None, "no policy converted"
    accelerated = [
        a
        for a in usable
        if acceptable(a["parity"], ACCELERATED_BAR) and a["neural_engine"]["fraction"] > 0
    ]
    if accelerated:
        first = min(accelerated, key=lambda a: POLICIES.index(a["policy"]))
        return first["policy"], "half-width arithmetic on the Neural Engine"
    passing = [a for a in usable if acceptable(a["parity"])] or usable
    best = min(passing, key=lambda a: a["parity"]["max_rel"])
    return (
        best["policy"],
        "no policy reaches the Neural Engine; the most accurate one ships",
    )


EXEMPT = {
    "half": frozenset(),
    "half_pooled": frozenset(POOLING),
    "half_reduced": frozenset(ACCUMULATING),
    "half_stable": frozenset(ACCUMULATING | UNSTABLE),
    "full": None,
}


def precision(policy):
    """The `compute_precision` argument for `ct.convert` under `policy`."""
    import coremltools as ct

    if policy == "full":
        return ct.precision.FLOAT32
    if policy == "half":
        return ct.precision.FLOAT16
    exempt = EXEMPT[policy]
    return ct.transform.FP16ComputePrecision(op_selector=lambda op: op.op_type not in exempt)


def pipeline(policy):
    """The pass pipeline: half-width weight storage is only needed where the policy is full.

    Under any half policy the converter already writes half-width weights. Under `full` it
    does not, and `coreml_fp16_weights` puts them back — the artefact is the same size either
    way, which is what lets a model change policy without changing the bundle budget.
    """
    if policy != "full":
        return None
    import coreml_fp16_weights

    return coreml_fp16_weights.pipeline()


def neural_engine_share(package):
    """How much of the converted program Core ML will actually put on the Neural Engine.

    This is the measurement that stops the ladder from choosing a policy that costs accuracy
    and buys nothing. Exempting a few operations from half precision partitions some graphs
    cleanly — `sleepnet_bdi` keeps 99 of 182 operations on the accelerator that way — and takes
    others off it entirely, which is what the full-width policy does with less error. Nothing in
    the operation list predicts which, so it is measured per model.
    """
    import compute_plan

    counts = compute_plan.plan_for(package)
    total = sum(counts.values())
    on_engine = counts.get("NeuralEngine", 0)
    return {
        "operations": total,
        "on_neural_engine": on_engine,
        "fraction": (on_engine / total) if total else 0.0,
        "devices": dict(counts),
    }


# What `storage("full")` returns. Named so the downstream gates can recognise the one policy
# that computes entirely at full width, rather than pattern-matching a sentence.
FULL_WIDTH_DESCRIPTION = "float16 weights, float32 arithmetic"


def runs_half_arithmetic(description):
    """Whether a contract's recorded precision means any arithmetic happens at half width.

    The mixed policies still say "float32" — they name the operations kept at full width — so
    a substring test gets them backwards. Only the `full` policy computes nothing at half
    width, and it is the only one this returns false for.
    """
    return description != FULL_WIDTH_DESCRIPTION


def storage(policy):
    """How the contract should describe this policy's weights and arithmetic."""
    if policy == "full":
        return FULL_WIDTH_DESCRIPTION
    if policy == "half":
        return "float16"
    exempted = {
        "half_pooled": "pooling reductions",
        "half_reduced": "accumulations",
        "half_stable": "accumulations and transcendentals",
    }[policy]
    return f"float16, {exempted} at float32"
