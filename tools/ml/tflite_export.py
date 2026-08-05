#!/usr/bin/env python3
"""TensorFlow Lite half of the conversion, run in its own interpreter.

LiteRT's torch converter needs a newer PyTorch than coremltools supports, so the
two backends live in separate virtual environments and talk through a job file:

    {"source": "...pt", "core": "a.b", "const_args": [...],
     "inputs": [["name", [shape], "dtype"], ...],
     "reference_npz": "...", "output_path": "..."}

The job carries the reference outputs PyTorch produced in the Core ML process, so
the parity number compares the shipped flatbuffer against the same tensors the
Core ML artefact was measured on.
"""
import json
import os
import shutil
import sys

import numpy as np
import torch

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

# Matches convert.py: above this relative deviation a half-width artefact is not worth its
# saving, and the full-width one ships instead.
FP16_PARITY_BAR = 1e-3


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
    """Hides the ScriptModule from the module tree.

    `torch.export` rejects a TorchScript submodule because its tensors are plain
    tensors rather than `nn.Parameter`. Holding the core outside the registry lets
    export lift the weights as constants instead.
    """

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


def rewrite_nearest_downsampling(graph_module):
    """Replace nearest-neighbour resizes with the gather PyTorch actually computes.

    LiteRT lowers `upsample_nearest` to a resize whose index convention differs from
    PyTorch's when the scale is below one: upscaling by an integer factor is exact, and
    downscaling is not. The sleep models' residual path downsamples that way, which made
    their flatbuffers disagree with PyTorch by whole logits while every other model was
    exact to 1e-7.

    Every shape here is static, so the resize is just an index_select with
    `floor(i * in / out)` — the same indices PyTorch picks. Returns the number of nodes
    rewritten so the caller can leave a graph that needed no change alone.
    """
    import torch as _torch

    rewritten = 0
    for node in list(graph_module.graph.nodes):
        if node.op != "call_function" or "upsample_nearest" not in str(node.target):
            continue
        source = node.args[0]
        source_shape = source.meta["val"].shape
        output_shape = node.meta["val"].shape
        with graph_module.graph.inserting_before(node):
            current = source
            for axis in range(len(output_shape) - 2):
                dim = 2 + axis
                in_size = int(source_shape[dim])
                out_size = int(output_shape[dim])
                if in_size == out_size:
                    continue
                indices = (_torch.arange(out_size) * in_size // out_size).to(_torch.int64)
                name = f"_nearest_index_{rewritten}_{axis}"
                graph_module.register_buffer(name, indices, persistent=False)
                constant = graph_module.graph.get_attr(name)
                constant.meta["val"] = indices
                current = graph_module.graph.call_function(
                    _torch.ops.aten.index_select.default, (current, dim, constant)
                )
        node.replace_all_uses_with(current)
        graph_module.graph.erase_node(node)
        rewritten += 1
    if rewritten:
        graph_module.graph.lint()
        graph_module.recompile()
    return rewritten


def rewrite_legacy_convolution(graph_module):
    """Retarget `aten._convolution.default` at `aten.convolution.default`.

    TorchScript emits the legacy thirteen-argument overload, which carries four trailing
    backend hints — benchmark, deterministic, cudnn_enabled, allow_tf32 — that mean nothing
    off a CUDA device. LiteRT only has a lowering for the nine-argument form. Same
    convolution, same arguments up to those hints, so the extras are dropped.

    Returns the number of nodes retargeted.
    """
    import torch as _torch

    rewritten = 0
    for node in list(graph_module.graph.nodes):
        if node.op != "call_function" or node.target is not _torch.ops.aten._convolution.default:
            continue
        node.target = _torch.ops.aten.convolution.default
        node.args = tuple(node.args[:9])
        rewritten += 1
    if rewritten:
        graph_module.graph.lint()
        graph_module.recompile()
    return rewritten


def decompose(exported):
    """Lower to the core ATen set, the same table the Core ML EXIR path uses.

    The two paths have to agree here or nothing downstream of them can: a different
    decomposition produces different constants to fold and different constants to round, which
    is precisely the divergence `fold_norm` exists to remove.
    """
    try:
        from torch._decomp import core_aten_decompositions

        return exported.run_decompositions(core_aten_decompositions())
    except Exception:  # noqa: BLE001 - fall back to the default table
        return exported.run_decompositions({})


def convertible(wrapper, example):
    """The module LiteRT should convert: exported, repaired, folded and rounded.

    Always exported rather than only when a repair is needed. Handing the raw wrapper to LiteRT
    lets *it* decompose and fold, which is the one thing the Core ML side must not have to guess
    about — see fold_norm for what that cost.
    """
    exported = decompose(torch.export.export(wrapper, tuple(example), strict=False))
    graph_module = exported.module()
    passes = {
        "nearest_downsamples_rewritten": rewrite_nearest_downsampling(graph_module),
        "legacy_convolutions_rewritten": rewrite_legacy_convolution(graph_module),
    }
    if any(passes.values()):
        exported = decompose(torch.export.export(graph_module, tuple(example), strict=False))

    import fold_norm

    exported, folding = fold_norm.prepare(exported, example)
    passes.update(folding)
    return exported.module(), passes


def run_flatbuffer(path, example):
    """Execute the written .tflite through the interpreter the app will use."""
    from ai_edge_litert.interpreter import Interpreter

    interpreter = Interpreter(model_path=path)
    interpreter.allocate_tensors()
    details = interpreter.get_input_details()
    if len(details) != len(example):
        raise RuntimeError(
            f"flatbuffer takes {len(details)} inputs, the contract supplies {len(example)}"
        )
    for detail, tensor in zip(details, example):
        array = tensor.numpy().astype(detail["dtype"])
        if tuple(detail["shape"]) != array.shape:
            raise RuntimeError(
                f"flatbuffer input {detail['name']} is {list(detail['shape'])}, "
                f"the contract supplies {list(array.shape)}"
            )
        interpreter.set_tensor(detail["index"], array)
    interpreter.invoke()
    return [
        interpreter.get_tensor(detail["index"]) for detail in interpreter.get_output_details()
    ]


def main():
    import litert_torch

    job = json.load(open(sys.argv[1]))
    model = torch.jit.load(job["source"], map_location="cpu")
    model.eval()
    core = _core_for(model, job)
    # Same rounding, same threshold, as the Core ML side; see fp16_align. Doing it here rather
    # than only in `halve_weights` below is what makes the two platforms compute against the
    # same numbers instead of each rounding whatever its own converter happened to fold.
    import fp16_align

    fp16_align.round_to_half(core)
    wrapper = CoreWrapper(
        core,
        job["const_args"],
        job.get("core_method"),
        job.get("arg_template"),
        [name for name, _s, _d in job["inputs"]],
    ).eval()

    # Every input comes from the parent's npz, integers included. Regenerating the integer ones
    # here would export against different indices from the ones the reference was computed with,
    # which is a silent parity error rather than a loud one.
    loaded = np.load(job["inputs_npz"])
    example = [torch.from_numpy(loaded[name]) for name, _s, _d in job["inputs"]]

    convertible_module, passes = convertible(wrapper, example)
    edge = litert_torch.convert(convertible_module, tuple(example))
    edge.export(job["output_path"])

    # Measure the file, not the converter's handle on the module it was built from.
    #
    # This is not a stylistic preference. Measuring `edge(...)` reported exact parity for the
    # three sleep models whose written flatbuffers were in fact wrong by several logits: the
    # handle answered from the source graph, so the number proved nothing about the bytes that
    # ship. Loading the artefact is the only measurement that can fail for the right reason.
    # Every probe, not just the one the graph was traced on. One probe measures a model at one
    # point of its input space, which let three artefacts through a single-probe gate and then
    # disagree across platforms by up to 6e-3 on inputs the pipeline had never tried.
    probes = job.get("probes") or [
        {"inputs_npz": job["inputs_npz"], "reference_npz": job["reference_npz"]}
    ]

    def measure(path):
        worst_abs = 0.0
        worst_rel = 0.0
        for probe in probes:
            loaded = np.load(probe["inputs_npz"])
            probe_example = [
                torch.from_numpy(loaded[name]) for name, _shape, _dtype in job["inputs"]
            ]
            want_all = np.load(probe["reference_npz"])
            produced = run_flatbuffer(path, probe_example)
            for index, tensor in enumerate(produced):
                want = want_all[f"out{index}"].astype(np.float64)
                have = np.asarray(tensor, dtype=np.float64).reshape(want.shape)
                error = float(np.max(np.abs(have - want)))
                scale = float(np.max(np.abs(want)))
                worst_abs = max(worst_abs, error)
                worst_rel = max(worst_rel, error / scale if scale > 1e-9 else error)
        return {"max_abs": worst_abs, "max_rel": worst_rel}

    # Half-width weights, full-width activations, for every model. Applied to the finished
    # flatbuffer because neither route from the PyTorch side works; see fp16_weights.py. The
    # threshold matches the Core ML side so both platforms round the same tensors.
    from fp16_weights import halve_weights

    full_bytes = os.path.getsize(job["output_path"])
    halved, _before, halved_bytes = halve_weights(job["output_path"])
    parity = measure(job["output_path"])
    precision = "float16 weights, float32 activations"

    passes["weights_halved"] = halved
    passes["bytes_full_width"] = full_bytes
    passes["bytes_halved"] = halved_bytes
    passes["precision"] = precision
    print(json.dumps({**parity, **passes}))


if __name__ == "__main__":
    main()
