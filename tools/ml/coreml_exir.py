#!/usr/bin/env python3
"""Core ML export through torch.export, run in the LiteRT environment.

Two Core ML frontends exist and they fail on different things. The TorchScript
frontend (convert.py, PyTorch 2.5) keeps the scripted control flow and needs the
dead-branch handlers in ct_ops.py; the EXIR frontend (here, PyTorch 2.12) gets a
flat aten graph with the branches already resolved, but has its own gaps. Neither
covers every core, so the driver tries the first and falls back to this.

Job file and output are the same shape as tflite_export.py's, so the driver treats
both backends identically.
"""
import json
import os
import shutil
import sys

import numpy as np
import torch


def get_core(model, path):
    node = model
    for part in [p for p in (path or "").split(".") if p]:
        node = getattr(node, part)
    return node


def materialise_const(value):
    """A const arg may be a plain Python value or a frozen tensor description.

    Two cores take a sequence-length tensor whose *value* steers the graph. Exported at a fixed
    shape the length is not a free input any more, so it is frozen into the graph rather than
    pretended to be one; the contract records the frozen value.
    """
    if isinstance(value, dict) and "tensor" in value:
        import torch as _torch

        dtype = _torch.int64 if value.get("dtype") == "int64" else _torch.float32
        return _torch.full(tuple(value["tensor"]), value.get("fill", 0), dtype=dtype)
    return value


def build_args(tensors, input_names, const_args, arg_template):
    """Assemble the positional arguments a core's forward expects.

    `const_args` appended at the end covers most cores. `arg_template` is for the ones whose
    non-tensor argument sits in the middle: CVA's probe head takes
    `(embeddings, gender: str, age, weight, bmi)`, and there is no way to reach that by
    appending. Each entry is either `"@name"` for the named input tensor, or a literal.
    """
    if not arg_template:
        return list(tensors) + list(const_args)
    by_name = dict(zip(input_names, tensors))
    args = []
    for entry in arg_template:
        if isinstance(entry, str) and entry.startswith("@"):
            args.append(by_name[entry[1:]])
        else:
            args.append(entry)
    return args


def _core_for(model, job):
    """The core named by the job, rebuilt first if it cannot be called where it sits."""
    if job.get("rebuild"):
        import rebuilt_cores

        return rebuilt_cores.rebuild(
            model, job["core"], job["rebuild"], job["rebuild_config"]
        )
    return get_core(model, job["core"])


class CoreWrapper(torch.nn.Module):
    def __init__(self, core, const_args, method=None, arg_template=None, input_names=None):
        super().__init__()
        object.__setattr__(self, "_core", core)
        self.const_args = [materialise_const(value) for value in const_args]
        self.method = method
        self.arg_template = arg_template
        self.input_names = list(input_names or [])

    def forward(self, *tensors):
        # Two cores write into their own input. Exporters reject a mutated graph input, and the
        # mutation is incidental to the arithmetic, so each call gets its own copy.
        cloned = [tensor.clone() for tensor in tensors]
        entry = getattr(self._core, self.method) if self.method else self._core
        return entry(*build_args(cloned, self.input_names, self.const_args, self.arg_template))


def flatten(value):
    if isinstance(value, torch.Tensor):
        return [value]
    if isinstance(value, (list, tuple)):
        out = []
        for item in value:
            out += flatten(item)
        return out
    return []


def main():
    import coremltools as ct

    import ct_ops

    ct_ops.install()

    job = json.load(open(sys.argv[1]))
    model = torch.jit.load(job["source"], map_location="cpu")
    model.eval()
    # Same rounding, same threshold, before the graph is built — see fp16_align. This runs in a
    # separate interpreter from convert.py, so it re-does the rounding rather than inheriting
    # it; the reference it is scored against was computed in the parent, unrounded.
    core = _core_for(model, job)
    import fp16_align

    fp16_align.round_to_half(core)
    wrapper = CoreWrapper(
        core,
        job["const_args"],
        job.get("core_method"),
        job.get("arg_template"),
        [name for name, _s, _d in job["inputs"]],
    ).eval()

    loaded = np.load(job["inputs_npz"])
    example = []
    for name, _shape, _dtype in job["inputs"]:
        # Every input comes from the parent's npz, integers included. Regenerating the integer
        # ones here would export against different indices from the ones the reference was
        # computed with, which is a silent parity error rather than a loud one.
        example.append(torch.from_numpy(loaded[name]))

    exported = torch.export.export(wrapper, tuple(example), strict=False)
    # The Core ML EXIR frontend only accepts the ATEN/EDGE dialects; core decompositions
    # also break composite ops such as unfold into gathers the frontend does implement.
    try:
        from torch._decomp import core_aten_decompositions

        exported = exported.run_decompositions(core_aten_decompositions())
    except Exception:  # noqa: BLE001 - fall back to the default table
        exported = exported.run_decompositions({})

    # The same repair the TensorFlow Lite path needs, for the same reason. Core ML's EXIR
    # frontend lowers a nearest-neighbour resize with a scale below one using a different index
    # convention from PyTorch, which put the three sleep models more than a whole relative unit
    # from their reference at every precision and on every compute unit. Upscaling by an integer
    # factor is exact; downscaling is not.
    #
    # It has to run after decomposition — the resize only appears as `upsample_nearest` once the
    # graph is lowered — and the re-export has to be decomposed again, or the frontend is handed
    # a TRAINING-dialect program it will not accept.
    from tflite_export import rewrite_nearest_downsampling

    # `ExportedProgram.module()` builds a *new* GraphModule on every call, so the rewrite has
    # to be held and re-exported — rewriting one call's module and re-exporting another's
    # silently discards the repair, which is how the three sleep models shipped still carrying
    # the resize and disagreeing with themselves by 1.5 relative on the GPU.
    module = exported.module()
    nearest_rewritten = rewrite_nearest_downsampling(module)
    if nearest_rewritten:
        exported = torch.export.export(module, tuple(example), strict=False)
        try:
            from torch._decomp import core_aten_decompositions

            exported = exported.run_decompositions(core_aten_decompositions())
        except Exception:  # noqa: BLE001
            exported = exported.run_decompositions({})

    # Fold normalisation and settle the constants *before* the converter does, so the Core ML
    # and TensorFlow Lite artefacts carry bit-identical weights. See fold_norm.
    import fold_norm

    exported, folding = fold_norm.prepare(exported, example)

    names = [name for name, _s, _d in job["inputs"]]
    output_path = job["output_path"]
    reference = np.load(job["reference_npz"])

    # The same measured ladder convert.py runs; see coreml_precision. Full-width arithmetic
    # takes the whole program off the Neural Engine, so half precision is the default and a
    # model only leaves it by failing a measurement.
    import coreml_policy
    import coreml_precision

    def convert_and_save(policy, destination):
        converted = ct.convert(
            exported,
            outputs=[ct.TensorType(name=name) for name in job["outputs"]],
            convert_to="mlprogram",
            compute_precision=coreml_precision.precision(policy),
            minimum_deployment_target=ct.target.iOS17,
            pass_pipeline=coreml_precision.pipeline(policy),
        )
        # torch.export names its placeholders after the forward signature, and CoreWrapper
        # takes `*tensors` — so every EXIR package arrives with inputs called `tensors_0`,
        # `tensors_1`. The apps bind by contract name, so without this the package loads and
        # then refuses the feed.
        for descriptor, wanted in zip(converted.get_spec().description.input, names):
            if descriptor.name != wanted:
                ct.utils.rename_feature(converted._spec, descriptor.name, wanted)
        if os.path.exists(destination):
            shutil.rmtree(destination)
        converted.save(destination)

    # Every probe; see tflite_export for why one is not enough.
    probes = job.get("probes") or [
        {"inputs_npz": job["inputs_npz"], "reference_npz": job["reference_npz"]}
    ]

    def measure(package, units):
        model = ct.models.MLModel(package, compute_units=coreml_policy.compute_unit(units))
        worst_abs = worst_rel = 0.0
        for probe in probes:
            loaded = np.load(probe["inputs_npz"])
            want_all = np.load(probe["reference_npz"])
            feed = {}
            for name, _shape, dtype in job["inputs"]:
                feed[name] = loaded[name].astype(
                    np.int32 if dtype == "int64" else np.float32
                )
            predicted = model.predict(feed)
            for index, name in enumerate(job["outputs"]):
                want = want_all[f"out{index}"].astype(np.float64)
                have = np.asarray(predicted[name], dtype=np.float64).reshape(want.shape)
                error = float(np.max(np.abs(have - want)))
                scale = float(np.max(np.abs(want)))
                worst_abs = max(worst_abs, error)
                worst_rel = max(worst_rel, error / scale if scale > 1e-9 else error)
        return {"max_abs": worst_abs, "max_rel": worst_rel}

    stem = output_path[: -len(".mlpackage")]
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
        raise RuntimeError(
            "no core ml precision policy converted: "
            + "; ".join(a.get("error", "") for a in policy_attempts)[:200]
        )
    if os.path.exists(output_path):
        shutil.rmtree(output_path)
    shutil.move(f"{stem}.{chosen}.mlpackage", output_path)
    for attempt in policy_attempts:
        shutil.rmtree(f"{stem}.{attempt['policy']}.mlpackage", ignore_errors=True)
    accelerated = next(
        a["neural_engine"] for a in policy_attempts if a["policy"] == chosen and "parity" in a
    )

    attempts = []
    for units in coreml_policy.COMPUTE_ORDER:
        try:
            attempts.append({"compute_units": units, "parity": measure(output_path, units)})
        except Exception as exc:  # noqa: BLE001
            attempts.append({"compute_units": units, "error": str(exc)[:160]})
    usable = [a for a in attempts if "parity" in a]
    if not usable:
        raise RuntimeError("no core ml compute unit produced a prediction")
    spread = max(a["parity"]["max_rel"] for a in usable) - min(
        a["parity"]["max_rel"] for a in usable
    )
    units = "ALL"
    if not any(a["compute_units"] == "ALL" for a in usable):
        units = min(usable, key=lambda a: a["parity"]["max_rel"])["compute_units"]
    parity = next(a["parity"] for a in usable if a["compute_units"] == units)
    precision_name = coreml_precision.storage(chosen)

    print(
        json.dumps(
            {
                **parity,
                "frontend": "exir",
                "inputs": names,
                "precision": precision_name,
                "compute_units": units,
                "configuration": {
                    "policy": chosen,
                    "policy_reason": why,
                    "policies": policy_attempts,
                    "neural_engine": accelerated,
                    "compute_units": attempts,
                    "unit_spread": spread,
                },
                "nearest_downsamples_rewritten": nearest_rewritten,
                **folding,
            }
        )
    )


if __name__ == "__main__":
    main()
