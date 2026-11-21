#!/usr/bin/env python3
"""Fail-closed browser runtime closure gate for ordinary ARMv7 release packages.

This verifies a producer's immutable inventory; it does not itself qualify
WebKit sandbox operation, dynamic dlopen coverage or real-device pixels.
"""

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import subprocess

ROLES = {"engine", "webprocess", "networkprocess", "gpuprocess",
         "bwrap", "xdg-dbus-proxy", "loader", "library", "module",
         "resource"}
EXECUTABLE_ROLES = {"engine", "webprocess", "networkprocess", "gpuprocess",
                    "bwrap", "xdg-dbus-proxy", "loader"}
ELF_ROLES = EXECUTABLE_ROLES | {"library", "module"}
REQUIRED = EXECUTABLE_ROLES - {"gpuprocess"}  # GPUProcess may be disabled at build time.
PROCESS_NAMES = {"webprocess": "WPEWebProcess",
                 "networkprocess": "WPENetworkProcess",
                 "gpuprocess": "WPEGPUProcess"}
TARGETS = {
    "armv7-unknown-linux-musleabihf": "musl",
    "armv7-unknown-linux-gnueabihf": "glibc",
}
SHA = re.compile(r"[0-9a-f]{40}\Z")
DIGEST = re.compile(r"[0-9a-f]{64}\Z")


def require(ok, reason):
    if not ok:
        raise ValueError(reason)


def inspect_elf(path, abi):
    """Production inspector: verify ELF target, loader and basic ISA attributes."""
    def readelf(*options):
        return subprocess.run(["readelf", *options, str(path)], check=True,
                              capture_output=True, text=True, timeout=30).stdout

    header = readelf("-h")
    require("ELF32" in header and "ARM" in header and "hard-float ABI" in header,
            f"non-ARMv7 hardfloat ELF: {path}")
    attrs = readelf("-A")
    require("Tag_ABI_VFP_args: VFP registers" in attrs and
            "Tag_FP_arch: VFPv3-D16" in attrs and
            not re.search(r"NEON|Advanced_SIMD", attrs, re.I), f"wrong ARM ISA: {path}")
    program = readelf("-l")
    if "INTERP" in program:
        expected = "/lib/ld-musl-armhf.so.1" if abi == "musl" else "/lib/ld-linux-armhf.so.3"
        require(expected in program, f"wrong ELF interpreter: {path}")
    dynamic = readelf("-d")
    require("NEEDED" in dynamic or "There is no dynamic section" in dynamic,
            f"unreadable DT_NEEDED closure: {path}")
    disassembly = subprocess.run(["llvm-objdump", "-d", str(path)], check=True,
                                 capture_output=True, text=True, timeout=60).stdout
    require(not re.search(r"\b(?:sdiv|udiv|vld[1-4]|vst[1-4]|vdup|vext|vtbl|vtbx|vzip|vuzp|vtrn|vq[a-z0-9.]+)\b",
                          disassembly, re.I), f"forbidden ARM instruction: {path}")


def verify(root, source_sha, target, producer_revision, inspector=inspect_elf):
    root = Path(root)
    require(target in TARGETS and isinstance(source_sha, str) and SHA.fullmatch(source_sha) and
            isinstance(producer_revision, str) and SHA.fullmatch(producer_revision),
            "invalid target or pinned SHA")
    require(root.is_dir() and not root.is_symlink(), "missing runtime directory")
    document = root / "runtime.json"
    require(document.is_file() and not document.is_symlink(), "missing runtime.json")
    manifest = json.loads(document.read_text())
    require(set(manifest) == {"schema", "source_sha", "producer_revision", "target", "abi", "qualification", "files"},
            "runtime schema fields incomplete or unexpected")
    require(manifest["schema"] == "nuraloumi-browser-runtime-v1" and
            manifest["source_sha"] == source_sha and
            manifest["producer_revision"] == producer_revision and
            manifest["target"] == target and manifest["abi"] == TARGETS[target],
            "wrong browser source, producer, target or ABI")
    qualification = manifest["qualification"]
    require(isinstance(qualification, dict) and
            set(qualification) == {"sandbox", "instruction_closure", "dlopen_closure", "webprocess", "review"} and
            all(value == "PASS" for value in qualification.values()),
            "browser runtime qualifications missing or not PASS")
    entries = manifest["files"]
    require(isinstance(entries, list) and entries, "empty browser runtime")
    seen, roles, processes = set(), set(), {}
    for entry in entries:
        require(isinstance(entry, dict) and set(entry) == {"path", "sha256", "role"},
                "invalid browser file entry")
        name, digest, role = entry["path"], entry["sha256"], entry["role"]
        require(isinstance(name, str) and isinstance(digest, str),
                "invalid browser path/digest type")
        path = PurePosixPath(name)
        require(name and not path.is_absolute() and path.as_posix() == name and
                all(part not in (".", "..") for part in name.split("/")) and
                name not in seen and role in ROLES and DIGEST.fullmatch(digest),
                "unsafe or duplicate browser path/role/hash")
        seen.add(name)
        roles.add(role)
        if role in PROCESS_NAMES:
            require(name.endswith("/" + PROCESS_NAMES[role]) and role not in processes,
                    f"invalid or duplicate {role} executable")
            processes[role] = path.parent
        file = root.joinpath(*path.parts)
        require(file.is_file() and not file.is_symlink() and
                all(not parent.is_symlink() for parent in file.parents if parent != root),
                f"missing regular browser file: {name}")
        require(hashlib.sha256(file.read_bytes()).hexdigest() == digest,
                f"browser SHA256 mismatch: {name}")
        if role in EXECUTABLE_ROLES:
            require(bool(file.stat().st_mode & 0o111),
                    f"browser process or loader is not executable: {name}")
        elif role == "resource":
            require(not (file.stat().st_mode & 0o111),
                    f"browser resource must not be executable: {name}")
        if role in ELF_ROLES:
            inspector(file, TARGETS[target])
    require(REQUIRED <= roles and {"library", "module"} <= roles,
            "missing engine, WPE processes, sandbox helper, loader, library or dlopen module")
    require(any(e["role"] == "engine" and e["path"] == "nurabrowser-core" for e in entries),
            "engine must have stable nurabrowser-core location")
    require(len(set(processes.values())) == 1,
            "WPE process executables must share the compiled libexec directory")
    all_paths = list(root.rglob("*"))
    require(all(not path.is_symlink() for path in all_paths),
            "symlink in browser runtime tree")
    actual = {str(p.relative_to(root)) for p in all_paths if p.is_file()}
    require(actual == seen | {"runtime.json"}, "unlisted, missing or partial browser runtime")
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--producer-revision", required=True)
    args = parser.parse_args()
    try:
        verify(args.root, args.source_sha, args.target, args.producer_revision)
    except (ValueError, OSError, subprocess.SubprocessError, json.JSONDecodeError) as exc:
        parser.exit(1, f"BROWSER_RUNTIME=FAIL: {exc}\n")
    print("BROWSER_RUNTIME=PASS (static package contract only; device qualification separate)")


if __name__ == "__main__":
    main()
