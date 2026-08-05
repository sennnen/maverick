#!/usr/bin/env python3
"""Round the weights to half precision once, before either converter sees them.

Both platforms store large constants at half width — Core ML through MIL `constexpr_cast`,
TensorFlow Lite through a `DEQUANTIZE` ahead of each consumer — and both widen them back at
load. That gets the bytes. What it does not get, on its own, is agreement: each converter
rounds *its own* constants, and by the time a graph reaches the backend the two converters
have folded, fused and transposed different things. So they round different numbers, and the
two platforms end up computing with weights that differ in the last few bits of a float16.
On a deep convolutional encoder that compounds — 2.4e-3 apart on PulseNet, against about
1e-6 when both sides carry full-width weights.

Rounding here, on the PyTorch module, fixes the cause rather than the symptom. Every value
that will end up stored at half width is already exactly representable at half width before
export, so the later halving passes are lossless and both platforms compute against the same
numbers. The artefacts stay the same size; only the disagreement goes away.

The threshold matches `fp16_weights.MIN_ELEMENTS` and `coreml_fp16_weights.MIN_ELEMENTS`,
and it has to: a tensor rounded here but stored full-width there, or the reverse, reopens
exactly the gap this closes. Small tensors — biases, normalisation scales, the odd learned
scalar — are left alone on all three sides. They cost nothing to keep at full width, and
they are the tensors where a half-precision step is largest relative to the value.
"""

# One threshold for the whole pipeline. Imported by the two storage passes so it cannot drift.
MIN_ELEMENTS = 1024


def round_to_half(module, min_elements=MIN_ELEMENTS):
    """Round every float parameter and buffer of `module` to the float16 grid, in place.

    Returns `(rounded, skipped)` so callers can record that a model with no large tensors
    was genuinely left alone rather than silently missed.
    """
    import torch

    rounded = 0
    skipped = 0
    with torch.no_grad():
        for _name, tensor in list(module.named_parameters()) + list(module.named_buffers()):
            if not isinstance(tensor, torch.Tensor):
                continue
            if tensor.dtype not in (torch.float32, torch.float64):
                continue
            if tensor.numel() < min_elements:
                skipped += 1
                continue
            tensor.data.copy_(tensor.data.to(torch.float16).to(tensor.dtype))
            rounded += 1
    return rounded, skipped


def residual(module, min_elements=MIN_ELEMENTS):
    """Largest remaining distance to the float16 grid, over the tensors that were rounded.

    Zero after `round_to_half`. Non-zero means something re-materialised a weight after the
    rounding ran — the failure this whole approach is guarding against — so the pipeline
    asserts on it rather than trusting the call happened.
    """
    import torch

    worst = 0.0
    with torch.no_grad():
        for _name, tensor in list(module.named_parameters()) + list(module.named_buffers()):
            if not isinstance(tensor, torch.Tensor):
                continue
            if tensor.dtype not in (torch.float32, torch.float64):
                continue
            if tensor.numel() < min_elements:
                continue
            gap = (tensor - tensor.to(torch.float16).to(tensor.dtype)).abs().max().item()
            worst = max(worst, float(gap))
    return worst
