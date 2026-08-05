#!/usr/bin/env python3
"""Rewrite a TensorFlow Lite flatbuffer to carry float16 weights.

LiteRT's torch converter has no float16 recipe, and the two ways to get one from the
PyTorch side both fail: casting the whole graph to half does not legalise `tfl.pad` or
`tfl.strided_slice`, and inserting the cast into the exported graph gets folded straight
back to float32 — which costs the accuracy of half precision and saves none of its bytes.

So it is done here instead, on the finished flatbuffer, in exactly the representation
TensorFlow Lite's own float16 post-training quantisation produces: each constant weight
stored as FLOAT16, and a DEQUANTIZE op widening it back to FLOAT32 before the kernel that
reads it. Activations stay float32, so numerics change only by the weights' rounding.

Nothing here is trusted on faith. The conversion pipeline measures parity by loading the
rewritten file and running it, so a mistake in this pass fails the admission gate rather
than shipping.
"""
import numpy as np

# Only large constants are halved, and the threshold matches coreml_fp16_weights exactly.
#
# Both numbers matter. Small constants — biases, batch-norm scales, shapes, axes — cost almost
# nothing to store and their rounding lands on every output, so halving them is a bad trade:
# dropping from 16 to 1,024 on the PulseNet encoder costs 19 kB and improves parity from
# 6.6e-3 to 1.9e-3. And the two platforms have to halve the *same* tensors, or they round
# different values and disagree with each other for no reason beyond a mismatched constant.
MIN_ELEMENTS = 1024

_DEQUANTIZE_BUILTIN = 6


def _tensor_elements(tensor):
    count = 1
    for side in tensor.shape if tensor.shape is not None else []:
        count *= int(side)
    return count


def halve_weights(path, min_elements=MIN_ELEMENTS):
    """Halve every large constant in `path`, in place. Returns (halved, before, after)."""
    import flatbuffers
    from ai_edge_litert import schema_py_generated as schema

    with open(path, "rb") as handle:
        original = handle.read()
    model = schema.ModelT.InitFromObj(schema.Model.GetRootAs(bytearray(original), 0))

    # DEQUANTIZE has to exist in the operator table before any op can reference it.
    dequantize_code = None
    for index, code in enumerate(model.operatorCodes):
        if code.builtinCode == _DEQUANTIZE_BUILTIN:
            dequantize_code = index
            break
    if dequantize_code is None:
        code = schema.OperatorCodeT()
        code.builtinCode = _DEQUANTIZE_BUILTIN
        code.deprecatedBuiltinCode = _DEQUANTIZE_BUILTIN
        code.version = 1
        model.operatorCodes.append(code)
        dequantize_code = len(model.operatorCodes) - 1

    halved = 0
    for subgraph in model.subgraphs:
        # flatbuffers hands these back as numpy arrays, which are not falsy-testable.
        protected = {int(i) for i in (subgraph.inputs if subgraph.inputs is not None else [])}
        protected |= {int(i) for i in (subgraph.outputs if subgraph.outputs is not None else [])}
        inserted = []
        for index, tensor in enumerate(list(subgraph.tensors)):
            if index in protected or tensor.type != schema.TensorType.FLOAT32:
                continue
            buffer = model.buffers[tensor.buffer]
            if buffer.data is None or len(buffer.data) == 0:
                continue  # an activation, not a weight
            if _tensor_elements(tensor) < min_elements:
                continue

            values = np.frombuffer(bytes(buffer.data), dtype=np.float32)
            buffer.data = np.frombuffer(values.astype(np.float16).tobytes(), dtype=np.uint8)
            tensor.type = schema.TensorType.FLOAT16

            widened = schema.TensorT()
            widened.shape = [int(side) for side in (tensor.shape if tensor.shape is not None else [])]
            widened.type = schema.TensorType.FLOAT32
            widened.name = (tensor.name or b"") + b"_dequantized"
            empty = schema.BufferT()
            model.buffers.append(empty)
            widened.buffer = len(model.buffers) - 1
            subgraph.tensors.append(widened)
            widened_index = len(subgraph.tensors) - 1

            operator = schema.OperatorT()
            operator.opcodeIndex = dequantize_code
            operator.inputs = [index]
            operator.outputs = [widened_index]
            inserted.append(operator)

            for consumer in subgraph.operators:
                if consumer.inputs is None:
                    continue
                consumer.inputs = [
                    widened_index if int(value) == index else int(value)
                    for value in consumer.inputs
                ]
            halved += 1

        # Every DEQUANTIZE reads a constant and writes a value its consumers need, so the
        # whole batch belongs ahead of the graph the converter emitted.
        if inserted:
            subgraph.operators = inserted + list(subgraph.operators)

    builder = flatbuffers.Builder(len(original) + 1024)
    builder.Finish(model.Pack(builder), b"TFL3")
    rewritten = bytes(builder.Output())
    with open(path, "wb") as handle:
        handle.write(rewritten)
    return halved, len(original), len(rewritten)


if __name__ == "__main__":
    import sys

    for target in sys.argv[1:]:
        count, before, after = halve_weights(target)
        print(f"{target}: halved {count} tensors, {before:,} -> {after:,} bytes")
