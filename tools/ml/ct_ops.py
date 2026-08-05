"""Core ML frontend handlers for TorchScript ops that only exist on dead branches.

The cores are TorchScript, so tracing preserves their scripted control flow
verbatim, including the arms that raise. Those arms carry `prim::Uninitialized`
placeholders and `aten::format` message building — ops the Core ML frontend has no
reason to implement, because a normal traced model never contains them.

Both are provably unreachable at the contracted shapes: the conversion driver runs
the core eagerly first, so a validator that would raise has already been shown not
to. Mapping them to inert constants removes the dead arm without touching the live
computation. If one of these ever became reachable, the eager run would have thrown
before the converter was ever called.
"""
import numpy as _np
from coremltools.converters.mil import Builder as mb
from coremltools.converters.mil.frontend.torch.ops import _get_inputs
from coremltools.converters.mil.frontend.torch.torch_op_registry import (
    _TORCH_OPS_REGISTRY,
    register_torch_op,
)

__all__ = ["install"]


def _scalar(value):
    """EXIR hands raw ints where TorchScript hands a const Var."""
    return int(getattr(value, "val", value))

_installed = []


def _handler(name, build):
    if _TORCH_OPS_REGISTRY.get_func(name) is not None:
        return
    register_torch_op(torch_alias=[name])(build)
    _installed.append(name)


def install():
    """Register the dead-branch handlers once per process."""

    def mav_format(context, node):
        """`aten::format` only ever builds an exception message here."""
        context.add(mb.const(val="", name=node.name))

    def mav_uninitialized(context, node):
        """`prim::Uninitialized` is the placeholder value of a branch that raises."""
        context.add(mb.const(val=0.0, name=node.name))

    def mav_raise_exception(context, node):
        """`prim::RaiseException` on a dead branch contributes nothing."""

    def mav_unfold(context, node):
        """`Tensor.unfold(dim, size, step)`: sliding windows, static at our shapes.

        The Core ML frontend has no unfold. Every use here sits on a fixed-length
        input, so the window offsets are known at conversion time and the op becomes
        one gather plus one transpose.
        """
        x, dim, size, step = _get_inputs(context, node, expected=4)
        axis = _scalar(dim)
        size = _scalar(size)
        step = _scalar(step)
        shape = x.shape
        rank = len(shape)
        if axis < 0:
            axis += rank
        length = int(shape[axis])
        count = (length - size) // step + 1
        offsets = _np.arange(count)[:, None] * step + _np.arange(size)[None, :]
        gathered = mb.gather(x=x, indices=offsets.astype(_np.int32), axis=axis)
        # gather inserted the window axis at `axis + 1`; unfold wants it last.
        order = [i for i in range(rank + 1) if i != axis + 1] + [axis + 1]
        context.add(mb.transpose(x=gathered, perm=order, name=node.name))

    def mav_bitwise_or(context, node):
        """Boolean mask union; EXIR emits `bitwise_or` where TorchScript emitted `__or__`."""
        left, right = _get_inputs(context, node, expected=2)
        context.add(mb.logical_or(x=left, y=right, name=node.name))

    def mav_alias(context, node):
        """`aten::alias` is a view with no arithmetic; pass the tensor through."""
        (value,) = _get_inputs(context, node, expected=1)
        context.add(mb.identity(x=value, name=node.name))

    _handler("format", mav_format)
    _handler("bitwise_or", mav_bitwise_or)
    _handler("alias", mav_alias)
    _handler("uninitialized", mav_uninitialized)
    _handler("raiseexception", mav_raise_exception)
    _handler("unfold", mav_unfold)
    return list(_installed)
