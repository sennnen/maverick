#!/usr/bin/env python3
"""Reference vectors for the on-device parity test, one file per model.

Everything the conversion pipeline measures about Android it measures on this Mac, through
LiteRT's own interpreter on the CPU. That is a real measurement of the *flatbuffer*, and it is
no measurement at all of the *phone*: the delegate the device attaches, the driver behind it
and the arithmetic width that driver chooses are all decided on the handset, and none of them
exist here. A number that never left the host cannot say what the app computes.

So each model gets a vector file carrying, per probe:

  * the inputs;
  * `expected` — eager PyTorch at full width, before any rounding, which is the ground truth
    the whole zoo is measured against;
  * `host` — what this Mac's LiteRT gets from the *shipped* flatbuffer on the same inputs.

Two references rather than one because they answer different questions and only the pair
separates them. Device against `expected` is the number that matters — total Android error,
directly comparable to the manifest. Device against `host` isolates what the handset added on
top of the file: the delegate, the driver, and the half-width arithmetic. A regression in one
is a conversion defect and in the other is a device defect, and a single reference would leave
them indistinguishable.

The seeds are deliberately not the conversion's. `convert.py` probes at seeds 0-4 and picks
policies against them; measuring on those same points would ask the pipeline to mark its own
work. These start at 100.

    python device_vectors.py [model ...]
"""
import json
import os
import struct
import sys

import numpy as np
import torch

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import convert  # noqa: E402 - needs the path above
import specs  # noqa: E402 - needs the path above

MAVERICK = os.path.dirname(os.path.dirname(HERE))
MANIFEST = os.path.join(MAVERICK, "artifacts/models/manifest.json")
ANDROID = os.path.join(MAVERICK, "apps/android/app/src/main/assets/models")
OUT = os.path.join(MAVERICK, "apps/android/app/src/androidTest/assets/vectors")

# Independent of convert.py's 0..4.
FIRST_SEED = 100
PROBE_COUNT = 3

MAGIC = b"MAVVEC01"


def write_vectors(path, probes):
    """`MAVVEC01`, probe count, then per probe each tensor as count + little-endian floats.

    Names and order are not written: the catalogue already declares them, and a file that
    restated them could disagree with it. Reading is a forward scan with no seeking, which is
    what keeps the device side small enough to be obviously correct.
    """
    with open(path, "wb") as handle:
        handle.write(MAGIC)
        handle.write(struct.pack("<i", len(probes)))
        for inputs, expected, host in probes:
            for group in (inputs, expected, host):
                for tensor in group:
                    flat = np.asarray(tensor, dtype=np.float32).ravel()
                    handle.write(struct.pack("<i", flat.size))
                    handle.write(flat.tobytes())


# Heart rates the waveform probes are drawn at, one per probe.
#
# Deliberately none of `convert.PROBE_RATES`, for the same reason the seeds are none of the
# conversion's: a model whose input is purely a waveform has no other source of variation, so
# reusing a rate the ladder already chose its policy against would measure the pipeline on its
# own homework and call it an independent check.
PROBE_RATES = (57.0, 83.0, 110.0)


def make_probe(spec_inputs, int_bounds, index):
    """Inputs for one probe: a pulse at that probe's rate, an index walk, or seeded noise."""
    generator = torch.Generator().manual_seed(FIRST_SEED + index)
    tensors = []
    for name, shape, dtype in spec_inputs:
        if dtype == "int64" and name in (int_bounds or {}):
            walk = torch.arange(int(torch.tensor(shape).prod()), dtype=torch.int64) + index
            tensors.append((walk % int(int_bounds[name])).reshape(tuple(shape)))
        elif dtype == "int64":
            fill = shape[-1] if len(shape) > 1 else 40
            tensors.append(torch.full(tuple(shape), int(fill), dtype=torch.int64))
        elif name in convert.PULSATILE_INPUTS:
            tensors.append(convert.pulse_probe(tuple(shape), bpm=PROBE_RATES[index]))
        else:
            tensors.append(torch.randn(*shape, generator=generator))
    return tensors


def pulse_ppg_wrapper():
    """Pulse-PPG is converted by its own scripts and has no entry in `specs.SPECS`."""
    import pulseppg_convert
    import pulseppg_tflite

    net = pulseppg_convert.load_net_class()(**pulseppg_convert.NET_CONFIG)
    checkpoint = torch.load(pulseppg_convert.CHECKPOINT, map_location="cpu", weights_only=False)
    state = checkpoint["net"] if isinstance(checkpoint, dict) and "net" in checkpoint else checkpoint
    net.load_state_dict({k.replace("module.", "", 1): v for k, v in state.items()}, strict=True)
    net.eval()
    return pulseppg_tflite.PulsePpgEncoder(net).eval()


def host_tflite(model, feed):
    """The shipped flatbuffer, run through LiteRT here, at full-width CPU arithmetic."""
    from ai_edge_litert.interpreter import Interpreter

    interpreter = Interpreter(model_path=os.path.join(ANDROID, model["tflite"]["artifact"]))
    interpreter.allocate_tensors()
    for detail, spec in zip(interpreter.get_input_details(), model["inputs"]):
        value = feed[spec["name"]].astype(detail["dtype"]).reshape(tuple(detail["shape"]))
        interpreter.set_tensor(detail["index"], value)
    interpreter.invoke()
    return [
        np.asarray(interpreter.get_tensor(d["index"]), dtype=np.float32)
        for d in interpreter.get_output_details()
    ]


def main():
    wanted = set(sys.argv[1:])
    manifest = json.load(open(MANIFEST))
    os.makedirs(OUT, exist_ok=True)
    index = {}

    for model in sorted(manifest["models"], key=lambda m: m["model"]):
        key = model["model"]
        if wanted and key not in wanted:
            continue
        spec = specs.SPECS.get(key)
        if spec is None:
            if key != "pulse_ppg":
                raise KeyError(f"{key} has no spec and no special case")
            wrapper = pulse_ppg_wrapper()
            int_bounds = {}
        else:
            source = os.path.join(convert.MODELS_DIR, spec["source"])
            loaded = torch.jit.load(source, map_location="cpu")
            loaded.eval()
            if spec.get("rebuild"):
                import rebuilt_cores

                core = rebuilt_cores.rebuild(
                    loaded, spec["core"], spec["rebuild"], spec["rebuild_config"]
                )
            else:
                core = convert.get_core(loaded, spec["core"])
            wrapper = convert.CoreWrapper(
                core,
                spec["const_args"],
                spec.get("core_method"),
                spec.get("arg_template"),
                [name for name, _s, _d in spec["inputs"]],
            ).eval()
            int_bounds = spec.get("int_bounds") or {}

        # From the manifest rather than the spec, so the probe is shaped like the tensor the
        # app will actually bind and pulse_ppg needs no second description of itself.
        spec_inputs = [
            (s["name"], tuple(s["shape"]), s["dtype"]) for s in model["inputs"]
        ]

        # The reference is the *unrounded* module, deliberately, matching `convert.py`: what the
        # device is scored against should include the cost of half-width storage rather than
        # hide it by comparing a rounded model against itself.
        probes = []
        for offset in range(PROBE_COUNT):
            tensors = make_probe(spec_inputs, int_bounds, offset)
            with torch.no_grad():
                expected = convert.flatten(wrapper(*tensors))
            feed = {
                name: tensor.numpy().astype(np.float32)
                for (name, _s, _d), tensor in zip(spec_inputs, tensors)
            }
            probes.append(
                (
                    [t.numpy() for t in tensors],
                    [t.numpy() for t in expected],
                    host_tflite(model, feed),
                )
            )

        path = os.path.join(OUT, f"{key}.vec")
        write_vectors(path, probes)
        index[key] = {"bytes": os.path.getsize(path), "probes": PROBE_COUNT}
        print(f"  {key:34s} {os.path.getsize(path):>10,} B")

    if not wanted:
        json.dump(
            {"probes": PROBE_COUNT, "first_seed": FIRST_SEED, "models": index},
            open(os.path.join(OUT, "index.json"), "w"),
            indent=1,
            sort_keys=True,
        )
    total = sum(entry["bytes"] for entry in index.values())
    print(f"device_vectors: {len(index)} models, {total:,} B")
    return 0


if __name__ == "__main__":
    sys.exit(main())
