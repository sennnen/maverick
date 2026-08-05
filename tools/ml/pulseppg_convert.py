#!/usr/bin/env python3
"""Convert Pulse-PPG, the open-weight PPG foundation encoder, to Core ML and TFLite.

Pulse-PPG (Xu et al., UbiComp 2025, MIT licence) is the open counterpart to Maverick's own
PulseNet encoder: the same job — raw PPG in, a general-purpose embedding out — but with
third-party weights, which is why it is the one model here carrying an attribution
requirement rather than shipping as first-party work. Unlike
the clinically-trained open models, it was pre-trained on roughly 200 million seconds of
uncurated wrist PPG from a 100-day field study, which is the regime Maverick actually
operates in: a strap on a moving wrist, not a finger clip in a lab.

Architecture: ResNet1D, twelve residual blocks, 128 base filters, kernel 11, stride 2,
instance-normalised input, max pooling over time. 28.5 M parameters, 512-d embedding.
Input: one channel at 50 Hz. The published pre-training window is four minutes, so that
is the window contracted here.

The checkpoint carries optimiser state as well as weights; only the `net` state dict is
converted, which is why the shipped artefact is a fraction of the 342 MB checkpoint.
"""
import hashlib
import json
import os
import shutil
import sys
import types

import numpy as np
import torch

HERE = os.path.dirname(os.path.abspath(__file__))
CHECKPOINT = os.path.join(HERE, "pulseppg_weights", "pulseppg_checkpoint_best.pkl")
REPO = os.path.join(HERE, "pulseppg_repo")
OUT_DIR = os.path.join(HERE, "out")
KEY = "pulse_ppg"

SAMPLE_RATE_HZ = 50
WINDOW_SECONDS = 240
INPUT_LEN = SAMPLE_RATE_HZ * WINDOW_SECONDS
EMBEDDING_DIM = 512

NET_CONFIG = dict(
    in_channels=1,
    base_filters=128,
    kernel_size=11,
    stride=2,
    groups=1,
    n_block=12,
    finalpool="max",
)


def load_net_class():
    """Import the upstream ResNet1D without dragging in its plotting dependencies.

    `ResNet1D_Net.py` imports matplotlib and sklearn at module scope purely for the
    training scripts. Stubbing them keeps the conversion environment to torch.
    """
    for name, attrs in (
        ("matplotlib", {}),
        ("matplotlib.pyplot", {}),
        ("sklearn", {}),
        ("sklearn.metrics", {"classification_report": lambda *a, **k: None}),
        ("tqdm", {"tqdm": lambda iterable=None, **k: iterable}),
    ):
        if name not in sys.modules:
            module = types.ModuleType(name)
            for key, value in attrs.items():
                setattr(module, key, value)
            sys.modules[name] = module
    sys.modules["matplotlib"].pyplot = sys.modules["matplotlib.pyplot"]
    sys.modules["sklearn"].metrics = sys.modules["sklearn.metrics"]

    sys.path.insert(0, REPO)
    import importlib.util

    path = os.path.join(REPO, "pulseppg", "nets", "ResNet1D", "ResNet1D_Net.py")
    spec = importlib.util.spec_from_file_location("pulseppg_resnet1d", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.Net


class PulsePpgEncoder(torch.nn.Module):
    """Fixed-shape wrapper: one 240-second window in, one 512-d embedding out."""

    def __init__(self, net):
        super().__init__()
        self.net = net

    def forward(self, ppg):
        return self.net(ppg)


def load_encoder():
    net_class = load_net_class()
    net = net_class(**NET_CONFIG)
    checkpoint = torch.load(CHECKPOINT, map_location="cpu", weights_only=False)
    state = checkpoint["net"] if isinstance(checkpoint, dict) and "net" in checkpoint else checkpoint
    cleaned = {key.replace("module.", "", 1): value for key, value in state.items()}
    missing, unexpected = net.load_state_dict(cleaned, strict=True)
    if missing or unexpected:
        raise RuntimeError(f"Pulse-PPG weights do not match the architecture: {missing} {unexpected}")
    net.eval()
    return PulsePpgEncoder(net).eval(), checkpoint


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sha256_tree(path):
    if os.path.isfile(path):
        return sha256_file(path)
    digest = hashlib.sha256()
    for root, dirs, files in os.walk(path):
        dirs.sort()
        for name in sorted(files):
            full = os.path.join(root, name)
            digest.update(os.path.relpath(full, path).encode())
            digest.update(sha256_file(full).encode())
    return digest.hexdigest()


def member_hashes(path):
    out = {}
    for root, dirs, files in os.walk(path):
        dirs.sort()
        for name in sorted(files):
            full = os.path.join(root, name)
            out[os.path.relpath(full, path)] = sha256_file(full)
    return out


def directory_size(path):
    return sum(
        os.path.getsize(os.path.join(root, name))
        for root, _, files in os.walk(path)
        for name in files
    )


def deviation(produced, reference):
    worst_abs = worst_rel = 0.0
    for got, expected in zip(produced, reference):
        want = np.asarray(expected, dtype=np.float64)
        have = np.asarray(got, dtype=np.float64).reshape(want.shape)
        error = float(np.max(np.abs(have - want)))
        scale = float(np.max(np.abs(want)))
        worst_abs = max(worst_abs, error)
        worst_rel = max(worst_rel, error / scale if scale > 1e-9 else error)
    return {"max_abs": worst_abs, "max_rel": worst_rel}


# The rates the parity number is the worst of. Mirrors `convert.PROBE_RATES`.
#
# This model took only one probe for a long time, at 68 bpm, which is the weakest measurement
# in the zoo attached to its largest artefact: 28.5 M parameters whose embedding every
# downstream head reads. A single waveform cannot distinguish a network that converted well
# from one that converted well *at 68 bpm*, and the encoder's whole job is to behave the same
# across the range a wearer's heart actually covers.
PROBE_RATES = (68.0, 52.0, 96.0, 44.0, 120.0, 61.0, 78.0, 150.0)


def synthetic_ppg(samples, bpm=68.0):
    """A pulse-shaped probe rather than white noise.

    Parity measured on noise understates a network whose activations are tuned to
    pulsatile input; this is the same waveform family the Rust fixtures use.
    """
    time = np.arange(samples) / SAMPLE_RATE_HZ
    phase = 2.0 * np.pi * (bpm / 60.0) * time
    signal = np.sin(phase) + 0.3 * np.sin(2.0 * phase) + 0.05 * np.sin(0.05 * time)
    return signal.astype(np.float32).reshape(1, 1, samples)


def main():
    import coremltools as ct

    os.makedirs(os.path.join(OUT_DIR, "coreml"), exist_ok=True)
    os.makedirs(os.path.join(OUT_DIR, "tflite"), exist_ok=True)
    os.makedirs(os.path.join(OUT_DIR, "contracts"), exist_ok=True)

    encoder, checkpoint = load_encoder()
    # One waveform per rate. The graph is shape-static so the export only needs the first;
    # every parity number below is the worst across all of them.
    probes = [torch.from_numpy(synthetic_ppg(INPUT_LEN, bpm=rate)) for rate in PROBE_RATES]
    example = probes[0]
    with torch.no_grad():
        references = [[encoder(probe).numpy()] for probe in probes]
    reference = references[0]

    # Reference first, then round; see fp16_align and convert.py. This is the model where the
    # rounding matters most — 28.5 M parameters feeding every downstream head, so a weight the
    # two platforms disagree about propagates into every comparison made after it.
    import fp16_align

    fp16_align.round_to_half(encoder)
    if reference[0].shape != (1, EMBEDDING_DIM):
        raise RuntimeError(f"unexpected embedding shape {reference[0].shape}")

    # The upstream net computes its SAME padding from `tensor.shape[-1]`, which TorchScript
    # tracing turns into tensor arithmetic the Core ML frontend then tries to cast to a Python
    # int. torch.export resolves those shapes to constants first, so the graph reaching the
    # converter has plain integer padding.
    from tflite_export import decompose

    exported = decompose(torch.export.export(encoder, (example,), strict=False))

    # Fold the normalisation and settle the constants here, so this artefact and the
    # TensorFlow Lite one carry bit-identical weights. Without it each converter folds and
    # rounds its own copy and the two sit 6.1e-3 apart on the graph alone — on the encoder
    # every downstream head reads. See fold_norm.
    import fold_norm

    exported, folding = fold_norm.prepare(exported, (example,))
    coreml_path = os.path.join(OUT_DIR, "coreml", f"{KEY}.mlpackage")

    # The same measured ladder every other model runs; see coreml_precision. This is the model
    # that most needs to be treated identically — 28.5 M parameters, the default PPG front-end,
    # and every downstream head reads its embedding — so it gets no exemption in either
    # direction, including no exemption from having its precision decided by measurement.
    import coreml_policy
    import coreml_precision

    def convert_and_save(policy, destination):
        converted = ct.convert(
            exported,
            outputs=[ct.TensorType(name="embeddings")],
            convert_to="mlprogram",
            compute_precision=coreml_precision.precision(policy),
            minimum_deployment_target=ct.target.iOS17,
            pass_pipeline=coreml_precision.pipeline(policy),
        )
        for descriptor in converted.get_spec().description.input:
            if descriptor.name != "ppg":
                ct.utils.rename_feature(converted._spec, descriptor.name, "ppg")
        if os.path.exists(destination):
            shutil.rmtree(destination)
        converted.save(destination)

    def measure(package, units):
        model = ct.models.MLModel(package, compute_units=coreml_policy.compute_unit(units))
        worst = {"max_abs": 0.0, "max_rel": 0.0}
        for probe, want in zip(probes, references):
            got = deviation([model.predict({"ppg": probe.numpy()})["embeddings"]], want)
            worst = {key: max(worst[key], got[key]) for key in worst}
        return worst

    stem = coreml_path[: -len(".mlpackage")]
    policy_attempts = []
    chosen = None
    for policy in coreml_precision.POLICIES:
        candidate = f"{stem}.{policy}.mlpackage"
        try:
            convert_and_save(policy, candidate)
            parity = measure(candidate, "ALL")
            accelerated = coreml_precision.neural_engine_share(candidate)
        except Exception as exc:  # noqa: BLE001 - a policy that will not convert is data
            policy_attempts.append(
                {"policy": policy, "error": f"{type(exc).__name__}: {exc}"[:200]}
            )
            shutil.rmtree(candidate, ignore_errors=True)
            continue
        policy_attempts.append(
            {"policy": policy, "parity": parity, "neural_engine": accelerated}
        )

    chosen, why = coreml_precision.choose(policy_attempts)
    if chosen is None:
        raise RuntimeError("no core ml precision policy converted Pulse-PPG")
    if os.path.exists(coreml_path):
        shutil.rmtree(coreml_path)
    shutil.move(f"{stem}.{chosen}.mlpackage", coreml_path)
    for attempt in policy_attempts:
        shutil.rmtree(f"{stem}.{attempt['policy']}.mlpackage", ignore_errors=True)

    attempts = []
    for units in coreml_policy.COMPUTE_ORDER:
        try:
            attempts.append({"compute_units": units, "parity": measure(coreml_path, units)})
        except Exception as exc:  # noqa: BLE001
            attempts.append({"compute_units": units, "error": str(exc)[:160]})
    usable = [a for a in attempts if "parity" in a]
    if not usable:
        raise RuntimeError("no core ml compute unit produced a prediction")
    spread = max(a["parity"]["max_rel"] for a in usable) - min(
        a["parity"]["max_rel"] for a in usable
    )
    compute_units = "ALL"
    if not any(a["compute_units"] == "ALL" for a in usable):
        compute_units = min(usable, key=lambda a: a["parity"]["max_rel"])["compute_units"]
    coreml_parity = next(
        a["parity"] for a in usable if a["compute_units"] == compute_units
    )
    precision_name = coreml_precision.storage(chosen)
    input_name = "ppg"

    record = {
        "model": KEY,
        "algorithm": "pulse_ppg_embeddings",
        "version": "1.0.0",
        "licence": "MIT, Max Xu and contributors",
        "citation": "Pulse-PPG: An Open-Source Field-Trained PPG Foundation Model, UbiComp 2025",
        "source_weights": "checkpoint_best.pkl",
        "source_weights_sha256": sha256_file(CHECKPOINT),
        "source_url": "https://github.com/maxxu05/pulseppg (weights: Zenodo 10.5281/zenodo.17270930)",
        "source_epoch": checkpoint.get("epoch") if isinstance(checkpoint, dict) else None,
        "architecture": "ResNet1D",
        "architecture_config": NET_CONFIG,
        "sample_rate_hz": SAMPLE_RATE_HZ,
        "window_seconds": WINDOW_SECONDS,
        "role": (
            "Open-weight PPG foundation encoder: 240 s at 50 Hz to a 512-d embedding, "
            "pre-trained on uncurated field PPG"
        ),
        "inputs": [{"name": "ppg", "shape": [1, 1, INPUT_LEN], "dtype": "float32"}],
        "outputs": [{"name": "embeddings", "shape": [1, EMBEDDING_DIM], "dtype": "float32"}],
        "parameters": int(sum(p.numel() for p in encoder.parameters())),
        "coreml": {
            "artifact": os.path.basename(coreml_path),
            "bytes": directory_size(coreml_path),
            "sha256": sha256_tree(coreml_path),
            "members": member_hashes(coreml_path),
            "parity": coreml_parity,
            "precision": precision_name,
            "compute_units": compute_units,
            "configuration": {
                **folding,
                "policy": chosen,
                "policy_reason": why,
                "policies": policy_attempts,
                "neural_engine": next(
                    a["neural_engine"]
                    for a in policy_attempts
                    if a["policy"] == chosen and "parity" in a
                ),
                "compute_units": attempts,
                "unit_spread": spread,
            },
            "frontend": "exir",
            "input_name": input_name,
        },
    }

    # The TFLite half runs as a separate invocation, not a child process: coremltools'
    # prediction runtime and a forked LiteRT converter in the same interpreter deadlock the
    # GIL. `pulseppg_tflite.py` merges its result into this same contract file.
    # Every probe, not just the traced one: the TensorFlow Lite half has to be measured over
    # the same set or the two platforms' numbers describe different questions.
    np.savez(
        os.path.join(OUT_DIR, "pulse_ppg_reference.npz"),
        ppg=example.numpy(),
        out0=reference[0],
        probes=np.concatenate([probe.numpy() for probe in probes], axis=0),
        probe_outputs=np.concatenate([want[0] for want in references], axis=0),
    )

    with open(os.path.join(OUT_DIR, "contracts", f"{KEY}.json"), "w") as handle:
        json.dump(record, handle, indent=2, sort_keys=True)
    info = record["coreml"]
    print(
        "    coreml: {:,} B  abs {:.3g}  rel {:.3g}".format(
            info["bytes"], info["parity"]["max_abs"], info["parity"]["max_rel"]
        )
    )
    print("    tflite: run pulseppg_tflite.py to complete the contract")


if __name__ == "__main__":
    main()
