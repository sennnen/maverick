#!/usr/bin/env python3
"""Read a training archive's wrapper: its source, its attributes, its constants.

`specs.py` names the tensor-in / tensor-out core inside each archive and `convert.py` converts
exactly that. Everything *around* the core — column selection, merging, windowing, masking,
scaling — stays in the archive and has to be ported to Rust by hand. This is the tool for
reading it.

A TorchScript `.pt` is a zip. `code/` holds the scripted source of every module that survived
tracing, which is the wrapper's logic verbatim rather than a reconstruction. The attributes it
closes over — column indices, shift constants, thresholds, feature-name tables — are in the
pickled state, reachable through `torch.jit.load`.

Both halves matter and neither is enough alone: the source says *what* the wrapper does and the
attributes say *with which numbers*. Porting from one without the other is how a feature vector
ends up in the right shape and the wrong order.

Usage
    wrapper_source.py --list
    wrapper_source.py ARCHIVE [--code] [--attrs] [--grep PATTERN]
    wrapper_source.py --survey        # one line per archive: wrapper modules and entry points
"""

from __future__ import annotations

import argparse
import os
import sys
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
MODELS_DIR = os.path.join(os.path.dirname(HERE), "decrypted_models")

# Everything under these prefixes is stock PyTorch, not the archive's own logic.
STOCK_PREFIXES = ("__torch__/torch/", "__torch__/collections", "__torch__/typing")


def archives() -> list[str]:
    return sorted(name for name in os.listdir(MODELS_DIR) if name.endswith(".pt"))


def wrapper_files(path: str) -> list[str]:
    """The archive's own scripted modules, stock PyTorch excluded."""
    with zipfile.ZipFile(path) as archive:
        return sorted(
            name
            for name in archive.namelist()
            if "/code/" in name
            and name.endswith(".py")
            and not any(f"/code/{prefix}" in name for prefix in STOCK_PREFIXES)
        )


def read(path: str, member: str) -> str:
    with zipfile.ZipFile(path) as archive:
        return archive.read(member).decode("utf-8", errors="replace")


def attributes(path: str) -> list[tuple[str, str]]:
    """Every non-tensor attribute the wrapper closes over, and small tensors in full.

    Large tensors are the weights, which are already converted and hashed; printing them would
    bury the constants that are not recoverable any other way.
    """
    import torch

    module = torch.jit.load(path, map_location="cpu")
    found: list[tuple[str, str]] = []

    def describe(value) -> str | None:
        if isinstance(value, torch.Tensor):
            if value.numel() <= 128:
                return f"tensor{list(value.shape)} = {value.tolist()}"
            return f"tensor{list(value.shape)} <{value.numel()} elements>"
        if isinstance(value, (int, float, bool, str)):
            return repr(value)
        if isinstance(value, (list, tuple, dict)):
            text = repr(value)
            return text if len(text) < 4000 else f"{type(value).__name__} <{len(value)} entries>"
        return None

    # The scripted class body is the reliable list of attribute names: `torch.jit` exposes no
    # attribute dict, and the C++ type introspection differs between releases. The body lines
    # look like `  max_delta_ms : float`, so the names are read from there and pulled by getattr.
    declared: dict[str, list[str]] = {}
    for member in wrapper_files(path):
        if member.endswith(".debug_pkl"):
            continue
        current: str | None = None
        for line in read(path, member).splitlines():
            if line.startswith("class "):
                current = line.split()[1].split("(")[0]
                declared.setdefault(current, [])
            elif current and line.startswith("  ") and " : " in line and "def " not in line:
                name = line.strip().split(" : ", 1)[0]
                if name.isidentifier():
                    declared[current].append(name)

    def walk(node, prefix: str) -> None:
        try:
            type_name = node._c._type().name()
        except Exception:
            type_name = None
        for name in declared.get(type_name or "", []):
            try:
                value = getattr(node, name)
            except Exception:
                continue
            if isinstance(value, torch.jit.RecursiveScriptModule):
                continue
            described = describe(value)
            if described is not None:
                found.append((f"{prefix}{name}", described))
        for name, child in node.named_children():
            walk(child, f"{prefix}{name}.")

    walk(module, "")
    return found


def survey() -> int:
    """One line per archive: its own modules and the entry points on each."""
    for name in archives():
        path = os.path.join(MODELS_DIR, name)
        own = [member for member in wrapper_files(path) if not member.endswith(".debug_pkl")]
        print(f"\n=== {name} ===")
        for member in own:
            short = member.split("/code/__torch__/", 1)[-1]
            text = read(path, member)
            defs = [
                line.strip()[4:].split("(")[0]
                for line in text.splitlines()
                if line.strip().startswith("def ")
            ]
            classes = [
                line.split()[1].split("(")[0]
                for line in text.splitlines()
                if line.startswith("class ")
            ]
            print(f"  {short}")
            if classes:
                print(f"      classes: {', '.join(classes)}")
            if defs:
                print(f"      defs:    {', '.join(sorted(set(defs)))}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", nargs="?", help="archive file name under decrypted_models")
    parser.add_argument("--list", action="store_true", help="list the archives")
    parser.add_argument("--survey", action="store_true", help="modules and entry points, all archives")
    parser.add_argument("--code", action="store_true", help="print the archive's own scripted source")
    parser.add_argument("--attrs", action="store_true", help="print the wrapper's attributes")
    parser.add_argument("--grep", help="only print source lines matching this substring")
    args = parser.parse_args()

    if args.list:
        for name in archives():
            print(name)
        return 0
    if args.survey:
        return survey()
    if not args.archive:
        parser.print_help()
        return 2

    path = os.path.join(MODELS_DIR, args.archive)
    if not os.path.isfile(path):
        print(f"no such archive: {args.archive}", file=sys.stderr)
        return 1

    if args.code or not args.attrs:
        for member in wrapper_files(path):
            if member.endswith(".debug_pkl"):
                continue
            text = read(path, member)
            if args.grep:
                hits = [line for line in text.splitlines() if args.grep in line]
                if not hits:
                    continue
                print(f"\n--- {member} ---")
                print("\n".join(hits))
            else:
                print(f"\n--- {member} ---")
                print(text)
    if args.attrs:
        print("\n--- attributes ---")
        for key, value in attributes(path):
            print(f"  {key} = {value}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
