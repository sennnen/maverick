"""Error-driven input-shape discovery for the neural cores.

PyTorch's shape errors name the dimension they wanted ("mat1 and mat2 shapes
cannot be multiplied (1x8 and 19x32)", "expected input[1, 1, 64] to have 29
channels"). This walks that feedback until a core's forward call succeeds, so the
contracted shapes in specs.py are discovered from the weights rather than guessed.
"""
import itertools
import re
import sys

import torch

RANKS = (2, 3, 4)
LENGTHS = (64, 128, 256, 512, 1024, 1499, 1500, 40, 32, 20, 16, 10, 8, 4, 1)


def getcore(model, path):
    node = model
    for part in [p for p in (path or "").split(".") if p]:
        node = getattr(node, part)
    return node


HINT_PATTERNS = (
    r"\(\d+x\d+ and (\d+)x\d+\)",
    r"to have (\d+) channels",
    r"running_mean should contain (\d+) elements",
    r"Expected input_size (\d+)",
    r"must be equal to input_size\. Expected (\d+)",
    r"expected input.*?to have size (\d+)",
    r"but got size \d+ .*?expected (\d+)",
)


def parse_hint(message):
    """Return a dimension the error explicitly asked for, if it named one."""
    for pattern in HINT_PATTERNS:
        m = re.search(pattern, message)
        if m:
            return int(m.group(1))
    return None


def try_call(core, shapes, dtypes, const_args):
    args = []
    for shape, dtype in zip(shapes, dtypes):
        if dtype == torch.int64:
            args.append(torch.full(shape, max(1, shape[-1]), dtype=torch.int64))
        else:
            args.append(torch.randn(*shape))
    with torch.no_grad():
        return core(*args, *const_args)


def solve(core, n_args, const_args=(), dtypes=None, max_tries=400):
    dtypes = dtypes or [torch.float32] * n_args
    seeds = []
    for rank in RANKS:
        for length in LENGTHS[:8]:
            if rank == 2:
                seeds.append((1, length))
            elif rank == 3:
                seeds.append((1, 1, length))
            else:
                seeds.append((1, 1, length, length))

    tried = set()
    tries = 0
    candidates = [list(c) for c in itertools.islice(itertools.product(seeds, repeat=n_args), 0, None)]
    for combo in candidates:
        tries += 1
        if tries > max_tries:
            break
        key = tuple(tuple(s) for s in combo)
        if key in tried:
            continue
        tried.add(key)
        for _ in range(6):
            try:
                out = try_call(core, combo, dtypes, const_args)
                return combo, out
            except Exception as exc:  # noqa: BLE001 - discovery is exploratory
                hint = parse_hint(str(exc))
                if hint is None:
                    break
                changed = False
                for i, shape in enumerate(combo):
                    for axis in range(len(shape) - 1, 0, -1):
                        if shape[axis] != hint:
                            trial = list(shape)
                            trial[axis] = hint
                            combo[i] = tuple(trial)
                            changed = True
                            break
                    if changed:
                        break
                if not changed:
                    break
    return None, None


def describe(out, prefix="out"):
    if isinstance(out, torch.Tensor):
        return [f"{prefix}: {tuple(out.shape)} {out.dtype}"]
    if isinstance(out, (list, tuple)):
        lines = []
        for i, item in enumerate(out):
            lines += describe(item, f"{prefix}[{i}]")
        return lines
    return [f"{prefix}: {type(out).__name__}"]


def main():
    source, path = sys.argv[1], sys.argv[2]
    n_args = int(sys.argv[3])
    const_args = []
    for extra in sys.argv[4:]:
        const_args.append({"true": True, "false": False, "none": None}.get(extra, extra))
    model = torch.jit.load(f"../decrypted_models/{source}", map_location="cpu")
    model.eval()
    core = getcore(model, path)
    shapes, out = solve(core, n_args, const_args)
    if shapes is None:
        print("NO SHAPE FOUND")
        return
    print("INPUTS:", [tuple(s) for s in shapes])
    for line in describe(out):
        print(line)


if __name__ == "__main__":
    main()
