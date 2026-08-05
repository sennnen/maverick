#!/usr/bin/env python3
"""Fold batch normalisation into the convolution before it, once, for both platforms.

This is the repair for the largest *avoidable* source of cross-platform disagreement. Both
converters fold batch normalisation into the preceding convolution — it is the standard
inference optimisation and neither can be talked out of it — and both then store the folded
weights at half width. The trouble is that they do it independently. The fold is
`w · γ/√(σ² + ε)`, and although each converter computes the same formula from the same inputs,
they do it in different orders and land a float32 ulp apart. That difference is invisible until
each rounds its own result to float16, where a pair of values straddling a midpoint round to
*different* half-precision numbers. One weight in the wrong direction is a 1e-3 relative
difference in the layer's output.

Measured across the zoo, that is exactly the split: the twenty-five models with no
normalisation agree between platforms to 1e-6 or better, and every one of the sixteen with a
normalisation layer sits between 2e-4 and 1e-2 — with the arithmetic width held constant, so
none of it is the half-precision compute.

So the fold happens here instead: once, in the exported graph, before either converter sees it.
Afterwards there is no normalisation node left to fold and the convolution weights are already
on the half-precision grid, so both toolchains receive bit-identical constants and have nothing
left to disagree about.

`fp16_align` rounds the module's *parameters* for the same reason; this rounds what folding
makes out of them, which is a different set of tensors and the one that was still drifting.
"""

# Matches fp16_align.MIN_ELEMENTS: the storage passes on both platforms halve constants at
# least this large, so these are the constants whose rounding has to be settled here.
from fp16_align import MIN_ELEMENTS


def _constant(graph_module, node):
    """The tensor a graph node refers to, if it refers to one at all."""
    if node is None or not hasattr(node, "op"):
        return None
    if node.op == "get_attr":
        return getattr(graph_module, node.target, None)
    return None


def fold_batch_norm(graph_module):
    """Fold every batch normalisation into the affine layer feeding it.

    Convolutions and dense layers both, because a normalisation left standing is one each
    converter turns into its own scale-and-shift constants — the same divergence in a smaller
    place. Returns the number folded; a graph with none is left untouched.
    """
    import torch

    folded = 0
    for node in list(graph_module.graph.nodes):
        if node.op != "call_function":
            continue
        if "native_batch_norm" not in str(node.target):
            continue
        producer = node.args[0]
        if not hasattr(producer, "op") or producer.op != "call_function":
            continue
        target = str(producer.target)
        # A normalisation folds into whatever affine layer feeds it — a convolution or a dense
        # layer, which after decomposition is an `addmm`. Only into that layer, though: one
        # feeding anything else as well wants its unfolded activation there.
        is_convolution = "convolution" in target
        is_dense = "addmm" in target
        if not (is_convolution or is_dense):
            continue
        if len(producer.users) != 1:
            continue
        convolution = producer

        # `addmm(bias, x, weight_t)` puts its weight last and its bias first; a convolution
        # puts the weight second and the bias third.
        weight_slot, bias_slot = (2, 0) if is_dense else (1, 2)
        weight = _constant(graph_module, convolution.args[weight_slot])
        gamma = _constant(graph_module, node.args[1])
        beta = _constant(graph_module, node.args[2])
        mean = _constant(graph_module, node.args[3])
        variance = _constant(graph_module, node.args[4])
        if any(tensor is None for tensor in (weight, gamma, beta, mean, variance)):
            continue
        epsilon = node.args[6] if len(node.args) > 6 else 1e-5
        bias = _constant(graph_module, convolution.args[bias_slot])

        with torch.no_grad():
            scale = gamma / torch.sqrt(variance + epsilon)
            if is_dense:
                # `addmm`'s weight is already transposed, so the output feature is its *last*
                # dimension and the scale broadcasts along that.
                new_weight = weight * scale
            else:
                # A convolution's output channel is its first dimension, whatever its rank.
                shape = [scale.numel()] + [1] * (weight.dim() - 1)
                new_weight = weight * scale.reshape(shape)
            existing = bias if bias is not None else torch.zeros_like(gamma)
            new_bias = (existing - mean) * scale + beta

        weight_name = f"_folded_weight_{folded}"
        bias_name = f"_folded_bias_{folded}"
        graph_module.register_buffer(weight_name, new_weight, persistent=False)
        graph_module.register_buffer(bias_name, new_bias, persistent=False)
        with graph_module.graph.inserting_before(convolution):
            weight_node = graph_module.graph.get_attr(weight_name)
            bias_node = graph_module.graph.get_attr(bias_name)
        weight_node.meta["val"] = new_weight
        bias_node.meta["val"] = new_bias
        if is_dense:
            convolution.args = (bias_node, convolution.args[1], weight_node)
        else:
            convolution.args = (
                convolution.args[0],
                weight_node,
                bias_node,
                *convolution.args[3:],
            )
        convolution.meta["val"] = node.meta["val"][0] if isinstance(
            node.meta.get("val"), (list, tuple)
        ) else convolution.meta.get("val")

        # The normalisation returns a tuple and its consumer takes element zero; both go, and
        # the convolution takes their place.
        for user in list(node.users):
            if user.op == "call_function" and user.target is __import__("operator").getitem:
                user.replace_all_uses_with(convolution)
                graph_module.graph.erase_node(user)
        node.replace_all_uses_with(convolution)
        graph_module.graph.erase_node(node)
        folded += 1

    if folded:
        graph_module.graph.eliminate_dead_code()
        graph_module.graph.lint()
        graph_module.recompile()
    return folded


def round_constants(graph_module, min_elements=MIN_ELEMENTS):
    """Round every large float constant in the graph onto the half-precision grid.

    After folding there are new constants that `fp16_align` never saw — the folded weights and
    biases — and they are exactly the ones both platforms will store at half width. Rounding
    them here means both round a value that is already on the grid, which is a no-op, rather
    than each rounding a slightly different float32.
    """
    import torch

    rounded = 0
    with torch.no_grad():
        for name, tensor in list(graph_module.named_buffers()) + list(
            graph_module.named_parameters()
        ):
            if not isinstance(tensor, torch.Tensor) or tensor.dtype != torch.float32:
                continue
            if tensor.numel() < min_elements:
                continue
            tensor.data.copy_(tensor.data.to(torch.float16).to(torch.float32))
            rounded += 1
        # Lifted constants are held as plain attributes rather than buffers, and the folded
        # graph still reads them for anything that was not folded.
        for name in dir(graph_module):
            if not name.startswith("lifted_tensor"):
                continue
            tensor = getattr(graph_module, name, None)
            if not isinstance(tensor, torch.Tensor) or tensor.dtype != torch.float32:
                continue
            if tensor.numel() < min_elements:
                continue
            tensor.data.copy_(tensor.data.to(torch.float16).to(torch.float32))
            rounded += 1
    return rounded


def prepare(exported, example):
    """Fold, round, and re-export so both converters receive the same graph.

    Returns `(exported_program, {"folded": n, "rounded": n})`. Re-exporting is what makes the
    change stick: `ExportedProgram.module()` hands out a fresh graph each call, so a pass
    applied to one and a conversion driven from another silently does nothing — the mistake
    that left three sleep models shipping a resize their GPU backend computed wrong.
    """
    import torch

    module = exported.module()
    folded = fold_batch_norm(module)
    rounded = round_constants(module)
    if not folded and not rounded:
        return exported, {"norms_folded": 0, "constants_rounded": 0}
    exported = torch.export.export(module, tuple(example), strict=False)
    try:
        from torch._decomp import core_aten_decompositions

        exported = exported.run_decompositions(core_aten_decompositions())
    except Exception:  # noqa: BLE001 - fall back to the default table
        exported = exported.run_decompositions({})
    return exported, {"norms_folded": folded, "constants_rounded": rounded}
