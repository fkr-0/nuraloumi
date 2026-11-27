#!/usr/bin/env python3
"""Fail-closed host preflight; only actual target playback can qualify media."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

FEATURES = ("ENABLE_VIDEO", "ENABLE_WEB_AUDIO", "USE_GSTREAMER")
PLUGINS = ("libgstcoreelements.so", "libgstplayback.so", "libgsttypefindfunctions.so")


def evaluate(root: Path, cache: Path) -> dict:
    failures = []
    lines = cache.read_text().splitlines() if cache.is_file() else []
    for flag in FEATURES:
        if f"{flag}:BOOL=ON" not in lines:
            failures.append(f"feature-disabled-or-cache-missing:{flag}")
    lib = root / "usr/lib"
    wpe = [p for p in sorted(lib.glob("libWPEWebKit-2.0.so.*"))
           if p.is_file() and not p.is_symlink()]
    if not wpe:
        failures.append("missing-wpe-elf")
    else:
        result = subprocess.run(["readelf", "-h", "-d", str(wpe[0])],
                                capture_output=True, text=True, check=False)
        if result.returncode or "Machine:                           ARM" not in result.stdout:
            failures.append("non-arm-elf-or-readelf-unavailable")
        if "libgstreamer-1.0.so" not in result.stdout and "libgst" not in result.stdout:
            failures.append("no-direct-gstreamer-needed-edge")
    plugins = root / "usr/lib/gstreamer-1.0"
    for name in PLUGINS:
        if not (plugins / name).is_file():
            failures.append(f"missing-plugin:{name}")
    if not (plugins / "libgstlibav.so").is_file():
        failures.append("missing-libav-h264-aac-decoder-candidate")
    if not (plugins / "libgstisomp4.so").is_file():
        failures.append("missing-isomp4-demux-candidate")
    failures.append("unverified-dlopen-soname-sha-isa-codec-and-device-playback")
    return {"MEDIA_CANDIDATE_READY": "no", "root": str(root),
            "cmake_cache": str(cache), "failures": failures,
            "wpe_sha256": hashlib.sha256(wpe[0].read_bytes()).hexdigest() if wpe else None}


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--cmake-cache", type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(evaluate(args.root, args.cmake_cache), sort_keys=True, indent=2))
    raise SystemExit(1)
