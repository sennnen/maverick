#!/usr/bin/env python3
"""Find the first operation whose answer changes when the GPU runs it.

The three sleep networks are exact on the CPU and on the Neural Engine and wrong by more
than one whole relative unit on the GPU. That is too large to be precision; something in
the graph is being computed differently. Rather than guess from the op histogram, this
cuts the program at each operation in turn and compares the CPU's answer with the GPU's
at that cut, which names the culprit instead of describing the symptom.
"""
import os
import sys

import numpy as np

import coremltools as ct
from coremltools.converters.mil.debugging_utils import extract_submodel

MODELS = os.path.join(
    os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))),
    "apps/ios/Maverick/Models",
)


def operations(package):
    spec = ct.models.MLModel(package, compute_units=ct.ComputeUnit.CPU_ONLY).get_spec()
    function = spec.mlProgram.functions["main"]
    block = function.block_specializations[list(function.block_specializations)[0]]
    ordered = []
    for op in block.operations:
        if op.type in ("const", "constexpr_cast"):
            continue
        for output in op.outputs:
            ordered.append((op.type, output.name))
    return ordered


def feed_for(model):
    rng = np.random.default_rng(11)
    values = {}
    for name, kind in model.input_description._fd_spec:
        shape = tuple(d for d in kind.type.multiArrayType.shape)
        values[name] = rng.standard_normal(shape).astype(np.float32)
    return values


def feed_from_spec(package):
    spec = ct.models.MLModel(package, compute_units=ct.ComputeUnit.CPU_ONLY).get_spec()
    rng = np.random.default_rng(11)
    values = {}
    for descriptor in spec.description.input:
        shape = tuple(int(d) for d in descriptor.type.multiArrayType.shape)
        values[descriptor.name] = rng.standard_normal(shape).astype(np.float32)
    return values


def divergence(model, feed, name):
    cpu = ct.models.MLModel(model.get_spec(), weights_dir=model.weights_dir,
                            compute_units=ct.ComputeUnit.CPU_ONLY).predict(feed)[name]
    gpu = ct.models.MLModel(model.get_spec(), weights_dir=model.weights_dir,
                            compute_units=ct.ComputeUnit.CPU_AND_GPU).predict(feed)[name]
    cpu = np.asarray(cpu, dtype=np.float64)
    gpu = np.asarray(gpu, dtype=np.float64).reshape(cpu.shape)
    scale = float(np.max(np.abs(cpu)))
    error = float(np.max(np.abs(cpu - gpu)))
    return error / scale if scale > 1e-9 else error


def main():
    package = f"{MODELS}/{sys.argv[1]}.mlpackage"
    ordered = operations(package)
    feed = feed_from_spec(package)
    print(f"{len(ordered)} candidate cuts")
    low, high = 0, len(ordered) - 1
    # Binary search for the earliest cut that already disagrees. The graph is a chain for
    # these networks, so a cut that disagrees implies every later cut does too.
    cache = {}

    def diverges(index):
        if index in cache:
            return cache[index]
        kind, name = ordered[index]
        try:
            sub = extract_submodel(ct.models.MLModel(package, compute_units=ct.ComputeUnit.CPU_ONLY),
                                   outputs=[name])
            value = divergence(sub, feed, name)
        except Exception as exc:  # noqa: BLE001 - an uncuttable point is not evidence
            print(f"  [{index:3d}] {kind:28s} {name:24s} skip: {type(exc).__name__}")
            cache[index] = None
            return None
        print(f"  [{index:3d}] {kind:28s} {name:24s} rel {value:.2e}")
        cache[index] = value > 1e-3
        return cache[index]

    while low < high:
        middle = (low + high) // 2
        verdict = diverges(middle)
        if verdict is None:
            middle += 1
            if middle > high:
                break
            continue
        if verdict:
            high = middle
        else:
            low = middle + 1
    kind, name = ordered[low]
    print(f"\nfirst divergence at [{low}] {kind} -> {name}")
    for index in range(max(0, low - 4), min(len(ordered), low + 2)):
        print(f"   {index:3d} {ordered[index][0]:28s} {ordered[index][1]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
