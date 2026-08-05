#!/usr/bin/env python3
"""Ask Core ML which processor it will actually run each operation on.

Admitting a model under `computeUnits = .all` says the OS *may* use the Neural Engine. It
does not say it will. Core ML decides per operation, and one of the things it decides on is
arithmetic precision: the Neural Engine is half-precision hardware, so a program that asks
for float32 arithmetic can be handed entirely to the CPU while still reporting `.all`.

`MLComputePlan` is Core ML's own answer to that question. It reports, for every operation,
the device it is preferred on and the devices it is supported on. That is evidence rather
than inference, and it is what decides whether the float16-arithmetic rework below is worth
its accuracy risk on any given model.
"""
import json
import os
import sys
from collections import Counter

MAVERICK = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
IOS = os.path.join(MAVERICK, "apps/ios/Maverick/Models")


def device_name(device):
    return type(device).__name__.replace("ML", "").replace("ComputeDevice", "") or str(device)


def plan_for(package, compute_units="ALL"):
    import coremltools as ct
    from coremltools.models.compute_plan import MLComputePlan

    # The plan is read off the compiled model, not the package: the assignment is made by the
    # compiler, so asking before compilation asks the wrong thing.
    compiled = ct.utils.compile_model(package)
    plan = MLComputePlan.load_from_path(
        path=compiled, compute_units=getattr(ct.ComputeUnit, compute_units)
    )
    structure = plan.model_structure
    program = structure.program
    if program is None:
        raise RuntimeError("not an ML program")
    function = program.functions.get("main")
    counts = Counter()
    for operation in function.block.operations:
        if operation.operator_name in ("const", "constexpr_cast"):
            continue
        usage = plan.get_compute_device_usage_for_mlprogram_operation(operation)
        if usage is None:
            counts["unassigned"] += 1
            continue
        counts[device_name(usage.preferred_compute_device)] += 1
    return counts


def main():
    wanted = sys.argv[1:]
    packages = sorted(
        name for name in os.listdir(IOS) if name.endswith(".mlpackage")
    )
    if wanted:
        packages = [p for p in packages if p[: -len(".mlpackage")] in wanted]
    report = {}
    for package in packages:
        model = package[: -len(".mlpackage")]
        try:
            counts = plan_for(os.path.join(IOS, package))
        except Exception as exc:  # noqa: BLE001 - a plan that cannot be built is the finding
            print(f"{model:36s} error: {type(exc).__name__}: {exc}"[:160])
            continue
        report[model] = dict(counts)
        total = sum(counts.values())
        rendered = "  ".join(f"{name} {count}" for name, count in counts.most_common())
        neural = counts.get("NeuralEngine", 0)
        print(f"{model:36s} {neural:4d}/{total:<4d} on ANE   {rendered}")
    json.dump(report, open(os.path.join(os.path.dirname(__file__), "compute_plan.json"), "w"),
              indent=1, sort_keys=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
