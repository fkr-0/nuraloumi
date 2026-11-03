#!/usr/bin/env python3
"""Discover exactly the runtime binary targets declared by Cargo (no device work)."""

import json
import pathlib
import subprocess
import sys

RUNTIME_CRATES = {"nuraloumi-shell", "nuraloumi-wayland", "nuraloumi-providers"}
BASELINE = {
    "nuraloumi-panel", "nuraloumi-menu", "nuraloumi-thumbnail-helper", "nuraloumi-probe"
}


def targets(metadata):
    if not isinstance(metadata, dict) or not isinstance(metadata.get("packages"), list):
        raise ValueError("metadata must contain a packages array")
    binaries = set()
    for package in metadata["packages"]:
        if package.get("name") not in RUNTIME_CRATES:
            continue
        for target in package.get("targets", []):
            if "bin" not in target.get("kind", []):
                continue
            name = target.get("name")
            if not isinstance(name, str) or not name.startswith("nuraloumi-") or not (
                name.replace("-", "").isascii() and name.replace("-", "").isalnum()
            ):
                raise ValueError(f"invalid binary name: {name!r}")
            if name in binaries:
                raise ValueError(f"duplicate binary name: {name}")
            binaries.add(name)
    if not BASELINE.issubset(binaries):
        raise ValueError(f"baseline binary targets missing: {sorted(BASELINE - binaries)}")
    return sorted(binaries)


def main():
    root = pathlib.Path(__file__).resolve().parent.parent
    try:
        process = subprocess.run(
            ["cargo", "metadata", "--locked", "--offline", "--no-deps",
             "--format-version", "1"],
            cwd=root, capture_output=True, text=True, timeout=30, check=True,
        )
        names = targets(json.loads(process.stdout))
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"runtime-binaries=FAIL reason={error}", file=sys.stderr)
        return 2
    print("\n".join(names))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
