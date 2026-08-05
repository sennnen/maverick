#!/usr/bin/env python3
"""TensorFlow Lite export for Pulse-PPG, the second half of its conversion.

Reads the .npz pulseppg_convert.py wrote (the probe input and the PyTorch reference
embedding), converts the encoder, measures parity, and merges the result into the same
contract file. Run it after pulseppg_convert.py, in the same environment but a separate
invocation: coremltools' prediction runtime and the LiteRT converter cannot share one
interpreter without deadlocking on the GIL.

    python pulseppg_convert.py && python pulseppg_tflite.py
"""
import json
import os
import sys

import numpy as np
import torch

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

from pulseppg_convert import NET_CONFIG, CHECKPOINT, load_net_class  # noqa: E402


class PulsePpgEncoder(torch.nn.Module):
    def __init__(self, net):
        super().__init__()
        self.net = net

    def forward(self, ppg):
        return self.net(ppg)


def sha256_file(path):
    import hashlib

    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


# Matches convert.py and tflite_export.py.
FP16_PARITY_BAR = 1e-3


def main():
    import litert_torch

    out_dir = os.path.join(HERE, "out")
    reference_npz = sys.argv[1] if len(sys.argv) > 1 else os.path.join(out_dir, "pulse_ppg_reference.npz")
    output_path = sys.argv[2] if len(sys.argv) > 2 else os.path.join(out_dir, "tflite", "pulse_ppg.tflite")
    data = np.load(reference_npz)

    net = load_net_class()(**NET_CONFIG)
    checkpoint = torch.load(CHECKPOINT, map_location="cpu", weights_only=False)
    state = checkpoint["net"] if isinstance(checkpoint, dict) and "net" in checkpoint else checkpoint
    net.load_state_dict({k.replace("module.", "", 1): v for k, v in state.items()}, strict=True)
    net.eval()
    encoder = PulsePpgEncoder(net).eval()
    # Same rounding as the Core ML side; see fp16_align.
    import fp16_align

    fp16_align.round_to_half(encoder)

    example = torch.from_numpy(data["ppg"])

    # The identical export, decomposition and fold the Core ML side runs; see fold_norm. The
    # module is converted through the folded graph rather than directly, so LiteRT has no
    # normalisation left to fold on its own.
    from fold_norm import prepare
    from tflite_export import decompose

    exported = decompose(torch.export.export(encoder, (example,), strict=False))
    exported, folding = prepare(exported, (example,))
    edge = litert_torch.convert(exported.module(), (example,))
    edge.export(output_path)

    # Same half-width pass every other model gets; see fp16_weights.py.
    # Halved only if the halved file still tracks the reference, matching tflite_export.
    from fp16_weights import halve_weights

    import shutil as _shutil

    full_width = output_path + ".fp32"
    _shutil.copyfile(output_path, full_width)
    halved, _before, _after = halve_weights(output_path)

    # Measure the written file, not the converter's handle: see tflite_export.run_flatbuffer.
    from ai_edge_litert.interpreter import Interpreter

    interpreter = Interpreter(model_path=output_path)
    interpreter.allocate_tensors()
    detail = interpreter.get_input_details()[0]
    output_index = interpreter.get_output_details()[0]["index"]

    # Every probe the Core ML half was measured on, taken from the same npz so the two numbers
    # answer the same question. A single 68 bpm waveform used to stand in for the whole input
    # range of the largest model in the zoo.
    probes = data["probes"] if "probes" in data else data["ppg"]
    wants = data["probe_outputs"] if "probe_outputs" in data else data["out0"]

    def deviation_of(values, want):
        want = np.asarray(want, dtype=np.float64)
        error = float(
            np.max(np.abs(np.asarray(values, dtype=np.float64).reshape(want.shape) - want))
        )
        scale = float(np.max(np.abs(want)))
        return {"max_abs": error, "max_rel": error / scale if scale > 1e-9 else error}

    # Half width always; there is no full-width fallback any more.
    parity = {"max_abs": 0.0, "max_rel": 0.0}
    for index in range(probes.shape[0]):
        probe = probes[index : index + 1]
        interpreter.set_tensor(detail["index"], probe.astype(detail["dtype"]))
        interpreter.invoke()
        got = deviation_of(
            interpreter.get_tensor(output_index), wants[index : index + 1]
        )
        parity = {key: max(parity[key], got[key]) for key in parity}
    precision = "float16 weights, float32 activations"
    os.remove(full_width)

    contract_path = os.path.join(out_dir, "contracts", "pulse_ppg.json")
    with open(contract_path) as handle:
        record = json.load(handle)
    record["tflite"] = {
        "artifact": os.path.basename(output_path),
        "bytes": os.path.getsize(output_path),
        "sha256": sha256_file(output_path),
        "parity": parity,
        "precision": precision,
        "weights_halved": halved,
        **folding,
    }
    with open(contract_path, "w") as handle:
        json.dump(record, handle, indent=2, sort_keys=True)
    print(
        "    tflite: {:,} B  abs {:.3g}  rel {:.3g}".format(
            record["tflite"]["bytes"], parity["max_abs"], parity["max_rel"]
        )
    )


if __name__ == "__main__":
    main()
