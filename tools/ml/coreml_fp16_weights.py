#!/usr/bin/env python3
"""Half-width weights, full-width activations, for Core ML.

`compute_precision=FLOAT16` gives both: the weights *and* the arithmetic go to half
precision, and the arithmetic is what costs the accuracy. Measured on the PulseNet
encoder, FLOAT32 lands 1.6e-6 from the PyTorch reference and FLOAT16 lands 2.0e-2 —
four orders of magnitude, for a graph whose weights are identical.

TensorFlow Lite does not pay that: `fp16_weights.py` halves its constants and widens
each one back before the kernel reads it, so its activations stay `fp32`. This does the
same thing on the Core ML side, so the two platforms compute the same way and their
outputs can be held to a tight tolerance against each other rather than against the
looser of the two.

Applied as a MIL pass after conversion at FLOAT32: every large `const` becomes `fp16`
with a `cast` to `fp32` in front of it. Small constants — axes, shapes, strides, single
scalars — are left alone; halving them saves nothing and a `cast` on a `begin` index is
a good way to confuse a later pass.
"""
import numpy as np

# Below this many elements a constant is structural, not a weight.
MIN_ELEMENTS = 1024


def halve_weights(mil_program):
    """Rewrite `mil_program` in place. Returns the number of constants halved.

    Uses `constexpr_cast`, which is MIL's own compressed-weight representation: the value is
    *stored* at half width and widened when the model is loaded. A plain `const` plus a `cast`
    does not survive — the backend's constant folding collapses it straight back into a
    full-width constant, so the numerics take the rounding and the file keeps its size. A
    `constexpr` op is not a candidate for folding, which is the whole point of it.
    """
    from coremltools.converters.mil.mil import Builder as mb
    from coremltools.converters.mil.mil.passes.helper import block_context_manager

    halved = 0

    @block_context_manager
    def process(block):
        nonlocal halved
        for operation in list(block.operations):
            for nested in operation.blocks:
                process(nested)
            if operation.op_type != "const":
                continue
            output = operation.outputs[0]
            value = output.val
            if not isinstance(value, np.ndarray) or value.dtype != np.float32:
                continue
            if value.size < MIN_ELEMENTS:
                continue

            widened = mb.constexpr_cast(
                source_val=value.astype(np.float16),
                output_dtype="fp32",
                name=f"{operation.name}_fp16",
                before_op=operation,
            )
            if block.try_replace_uses_of_var_after_op(
                anchor_op=operation, old_var=output, new_var=widened
            ):
                halved += 1

    for function in mil_program.functions.values():
        process(function)
    return halved


def register():
    """Register the pass so a conversion pipeline can name it."""
    from coremltools.converters.mil.mil.passes.graph_pass import AbstractGraphPass
    from coremltools.converters.mil.mil.passes.pass_registry import PASS_REGISTRY, register_pass

    if "mav::fp16_weights_only" in PASS_REGISTRY.__dict__.get("passes", {}):
        return

    @register_pass(namespace="mav")
    class fp16_weights_only(AbstractGraphPass):  # noqa: N801 - the registry keys on this name
        """Store weights at half width; keep every computation at full width."""

        def apply(self, prog):
            halve_weights(prog)

    _ = fp16_weights_only


def pipeline():
    """A default pipeline with this pass appended after everything else.

    Appended last on purpose: an earlier slot and the constant-folding passes downstream
    would collapse each `const -> cast` straight back into an `fp32` constant, which is
    how the equivalent attempt on the TensorFlow Lite side failed.
    """
    import coremltools as ct

    register()
    passes = ct.PassPipeline.DEFAULT
    passes.append_pass("mav::fp16_weights_only")
    # The replaced constants are left with no consumers, and the serializer writes every
    # constant it finds — without this the weight blob keeps both widths and the package
    # does not shrink at all.
    passes.append_pass("common::dead_code_elimination")
    return passes
