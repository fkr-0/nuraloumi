#!/usr/bin/env python3
"""Read-only ARMv7/musl WPE canary closure observation, NEVER a runtime.json producer.

Scan only the explicit browser/runtime/overlay trees, not user profiles or
previous test results. Reachability is a conservative DT_NEEDED graph starting
at all WPE process executables and known plugin/dlopen seeds. Runtime loads,
seccomp behavior and the final installed layout require separate evidence.
"""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess


SOURCE_SHA = re.compile(r"[0-9a-f]{40}\Z")
NEEDED_RE = re.compile(r"Shared library: \[([^\]]+)\]")
SONAME_RE = re.compile(r"Library soname: \[([^\]]+)\]")
RUNPATH_RE = re.compile(r"(?:Library runpath|Library rpath): \[([^\]]+)\]")
TREE_NAMES = ("browser", "bin", "lib", "target-wayland", "modules/gio",
              "resources/icu", "mesa-d16-gnu-tls-candidate-20261124")
EXECUTABLES = (
    "browser/nurabrowser-core",
    "browser/libexec/WPEWebProcess",
    "browser/libexec/WPENetworkProcess",
    "bin/bwrap",
    "bin/xdg-dbus-proxy",
    "lib/ld-musl-armhf.so.1",
)
ICU_LIBRARIES = ("libicuuc.so.76", "libicui18n.so.76", "libicudata.so.76")
MESA_DIR = "mesa-d16-gnu-tls-candidate-20261124/"


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def marker_source(root):
    marker = root / "CANDIDATE-ONLY.txt"
    if not marker.is_file() or marker.is_symlink():
        raise ValueError("missing regular CANDIDATE-ONLY.txt")
    pairs = {}
    for line in marker.read_text().splitlines():
        if "=" not in line:
            raise ValueError("invalid candidate marker")
        key, value = line.split("=", 1)
        if key in pairs:
            raise ValueError("duplicate marker key")
        pairs[key] = value
    if (not SOURCE_SHA.fullmatch(pairs.get("source_sha", ""))
            or pairs.get("target") != "armv7-unknown-linux-musleabihf"
            or pairs.get("upstream_wpewebkit") != "2.54.2"
            or pairs.get("runtime_qualification") != "UNQUALIFIED"
            or pairs.get("device_deployment") != "FORBIDDEN"):
        raise ValueError("unexpected candidate identity or promotion flags")
    return pairs["source_sha"]


def enumerate_files(root):
    files = {}
    symlinks = []
    for name in TREE_NAMES:
        tree = root / name
        if not tree.is_dir() or tree.is_symlink():
            continue
        for item in sorted(tree.rglob("*")):
            rel = item.relative_to(root).as_posix()
            if item.is_symlink():
                symlinks.append(rel)
            elif item.is_file():
                files[rel] = item
    return files, symlinks


def parse_dynamic(output):
    return {
        "needed": NEEDED_RE.findall(output),
        "soname": (SONAME_RE.search(output).group(1)
                   if SONAME_RE.search(output) else None),
        "runpath": RUNPATH_RE.findall(output),
    }


def readelf(path, flag):
    return subprocess.run(["readelf", flag, str(path)], check=True,
                          text=True, capture_output=True, timeout=30).stdout


def inspect_elf(path):
    header = readelf(path, "-h")
    attrs = readelf(path, "-A")
    program = readelf(path, "-l")
    dynamic = parse_dynamic(readelf(path, "-d"))
    issues = []
    if "ELF32" not in header or not re.search(r"Machine:\s+ARM\b", header):
        issues.append("ELF_NOT_ARM32")
    if "hard-float ABI" not in header or "Tag_ABI_VFP_args: VFP registers" not in attrs:
        issues.append("ABI_NOT_ARM_HARDFLOAT")
    if "Tag_FP_arch: VFPv3-D16" not in attrs:
        issues.append("ISA_NOT_VFPV3_D16")
    if re.search(r"NEON|Advanced_SIMD", attrs, flags=re.I):
        issues.append("ISA_NEON_ATTRIBUTE")
    if "INTERP" in program and "/lib/ld-musl-armhf.so.1" not in program:
        issues.append("WRONG_INTERPRETER")
    if any(dep.startswith("libc.so.6") or "ld-linux" in dep
           for dep in dynamic["needed"]):
        issues.append("GLIBC_IN_MUSL_TREE")
    dynamic["issues"] = issues
    return dynamic


def seeds_for(files):
    seeds = [name for name in EXECUTABLES if name in files]
    injected = "lib/libWPEInjectedBundle.so"
    if injected in files:
        seeds.append(injected)
    for name in sorted(files):
        if (name.startswith("modules/gio/") and name.endswith(".so")
                or name.startswith("target-wayland/") and ".so" in name
                or name.startswith(MESA_DIR) and ".so" in name):
            seeds.append(name)
    return sorted(set(seeds))


def resolve_graph(elf, seeds):
    """Resolve private names; a SONAME-only alias is NOT an installed path."""
    exact = {}
    sonames = {}
    for name, info in elf.items():
        basename = Path(name).name
        exact.setdefault(basename, []).append(name)
        if info["soname"]:
            sonames.setdefault(info["soname"], []).append(name)
    # Multiple candidate files with the same basename/SONAME are ambiguous,
    # even if their content happens to match; do not choose a host path.
    reached = set()
    pending = list(seeds)
    issues = []
    edges = []
    while pending:
        name = pending.pop()
        if name in reached or name not in elf:
            continue
        reached.add(name)
        info = elf[name]
        issues.extend({"path": name, "code": code} for code in info["issues"])
        for dep in info["needed"]:
            candidates = sorted(exact.get(dep, []))
            # A same-named file in the overlay can shadow an ordinary library;
            # an ambiguous cross-directory name cannot prove runtime selection.
            if len(candidates) > 1:
                issues.append({"path": name, "code": "AMBIGUOUS_PRIVATE_NAME",
                               "library": dep, "candidates": candidates})
                continue
            if not candidates:
                aliases = sorted(sonames.get(dep, []))
                if len(aliases) == 1:
                    issues.append({"path": name, "code": "SONAME_ALIAS_NOT_MATERIALIZED",
                                   "library": dep, "candidate": aliases[0]})
                    candidates = aliases
                else:
                    issues.append({"path": name, "code": "DT_NEEDED_NOT_PRIVATELY_RESOLVED",
                                   "library": dep, "candidates": aliases})
                    continue
            edges.append({"from": name, "needed": dep, "to": candidates[0]})
            pending.append(candidates[0])
    return sorted(reached), sorted(edges, key=lambda x: (x["from"], x["needed"])), issues


def inventory(root):
    if root.is_symlink():
        raise ValueError("candidate root must not be a symlink")
    root = root.resolve(strict=True)
    if not root.is_dir():
        raise ValueError("candidate root must be a directory")
    source = marker_source(root)
    files, symlinks = enumerate_files(root)
    issues = [{"path": item, "code": "SYMLINK_IN_PRIVATE_RUNTIME"}
              for item in symlinks]
    for name in EXECUTABLES:
        if name not in files:
            issues.append({"path": name, "code": "REQUIRED_EXECUTABLE_MISSING"})
    for name in ICU_LIBRARIES:
        if f"lib/{name}" not in files:
            issues.append({"path": f"lib/{name}", "code": "ICU_76_LIBRARY_MISSING"})
    if not any(n.startswith("resources/icu/") for n in files):
        issues.append({"path": "resources/icu", "code": "ICU_DATA_MISSING"})
    if not any(n.startswith("modules/gio/") and n.endswith(".so") for n in files):
        issues.append({"path": "modules/gio", "code": "GIO_MODULE_MISSING"})

    hashes = []
    elf = {}
    for name, path in sorted(files.items()):
        hashes.append({"path": name, "sha256": sha256(path)})
        with path.open("rb") as stream:
            if stream.read(4) != b"\x7fELF":
                continue
        elf[name] = inspect_elf(path)
    seeds = seeds_for(files)
    reached, edges, graph_issues = resolve_graph(elf, seeds)
    issues.extend(graph_issues)
    issues = sorted(issues, key=lambda x: (x["path"], x["code"],
                                         x.get("library", "")))
    return {
        "schema": "nuraloumi-wpe-live-closure-observation-v1",
        "qualification": "UNQUALIFIED",
        "build_source_sha": source,
        "target": "armv7-unknown-linux-musleabihf",
        "files": hashes,
        "elf_count": len(elf),
        "reachable_elf_count": len(reached),
        "seeds": seeds,
        "reachable": reached,
        "dt_needed_edges": edges,
        "issues": issues,
        "unproven": [
            "resolved_loader_link_maps_and_symbol_versions",
            "dynamic_dlopen_and_transitive_plugins",
            "external_host_mount_and_soname_alias_independence",
            "webprocess_seccomp_and_negative_controls",
            "networkprocess_mount_socket_dbus_policy",
            "browser_chrome_tls_touch_thermal_recovery",
            "source_matched_final_release_layout_and_independent_review",
        ],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("canary_root", type=Path)
    parser.add_argument("--output", type=Path, help="JSON report outside the canary tree")
    args = parser.parse_args()
    try:
        if (args.output and args.output.resolve().is_relative_to(
                args.canary_root.resolve())):
            raise ValueError("report must be outside the candidate tree")
        result = inventory(args.canary_root)
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        parser.exit(2, f"WPE_LIVE_CLOSURE=ERROR: {error}\n")
    if args.output:
        args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(f"WPE_LIVE_CLOSURE={'BLOCKED' if result['issues'] else 'STATIC_PASS'} "
          f"files={len(result['files'])} elf={result['elf_count']} "
          f"reachable={result['reachable_elf_count']} issues={len(result['issues'])} "
          "qualification=UNQUALIFIED")
    for issue in result["issues"]:
        print(f"  {issue['code']}: {issue['path']}"
              + (f" -> {issue['library']}" if "library" in issue else ""))
    if result["issues"]:
        parser.exit(1)


if __name__ == "__main__":
    main()
