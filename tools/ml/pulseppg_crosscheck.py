#!/usr/bin/env python3
"""Third and last stage of the Pulse-PPG conversion: do the two platforms agree?

Both artefacts were measured against PyTorch in the two stages before this one. Agreeing
with the reference separately is not the same as agreeing with each other — a converter can
produce a graph that is right for the tensor it was measured on and wrong for the next — so
this runs the shipped Core ML package and the shipped flatbuffer on identical input and
merges the deviation into the contract.

It is a separate invocation for the same reason the first two are: coremltools' prediction
runtime and the LiteRT converter deadlock on the GIL in one interpreter.

    python pulseppg_convert.py && python pulseppg_tflite.py && python pulseppg_crosscheck.py
"""
import json
import os

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
OUT_DIR = os.path.join(HERE, "out")
KEY = "pulse_ppg"


def main():
    import coremltools as ct
    from ai_edge_litert.interpreter import Interpreter

    data = np.load(os.path.join(OUT_DIR, f"{KEY}_reference.npz"))
    # Every rate the other two stages measured on. Agreement at 68 bpm is agreement at 68 bpm,
    # and this encoder's whole job is to behave the same across the range a wearer covers.
    probes = data["probes"] if "probes" in data else data["ppg"]

    package = os.path.join(OUT_DIR, "coreml", f"{KEY}.mlpackage")
    model = ct.models.MLModel(package, compute_units=ct.ComputeUnit.CPU_ONLY)
    input_name = model.get_spec().description.input[0].name

    interpreter = Interpreter(model_path=os.path.join(OUT_DIR, "tflite", f"{KEY}.tflite"))
    interpreter.allocate_tensors()
    detail = interpreter.get_input_details()[0]
    output_index = interpreter.get_output_details()[0]["index"]

    parity = {"max_abs": 0.0, "max_rel": 0.0}
    for index in range(probes.shape[0]):
        example = probes[index : index + 1]
        apple = np.asarray(
            model.predict({input_name: example})["embeddings"], dtype=np.float64
        )
        interpreter.set_tensor(detail["index"], example.astype(detail["dtype"]))
        interpreter.invoke()
        android = np.asarray(
            interpreter.get_tensor(output_index), dtype=np.float64
        ).reshape(apple.shape)
        error = float(np.max(np.abs(apple - android)))
        scale = max(float(np.max(np.abs(android))), 1e-9)
        parity = {
            "max_abs": max(parity["max_abs"], error),
            "max_rel": max(parity["max_rel"], error / scale),
        }

    contract_path = os.path.join(OUT_DIR, "contracts", f"{KEY}.json")
    with open(contract_path) as handle:
        record = json.load(handle)
    record["cross_platform"] = parity
    with open(contract_path, "w") as handle:
        json.dump(record, handle, indent=2, sort_keys=True)
    print("    cross_platform: abs {max_abs:.3g}  rel {max_rel:.3g}".format(**parity))


if __name__ == "__main__":
    main()
