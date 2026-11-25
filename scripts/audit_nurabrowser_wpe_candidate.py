#!/usr/bin/env python3
"""Read-only preflight for a locally assembled ARMv7/musl WPE candidate.

This is deliberately NOT a runtime.json producer. An ELF inventory and
source-tree comparison cannot certify sandbox behavior, dynamic dlopen paths,
device pixels, or independent review. Keep the production packaging gate closed.
"""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

# Support both `python scripts/audit_...py` and CI's `python -m unittest
# scripts/test_...py` without duplicating the production ABI policy.
try:
    from scripts.verify_nurabrowser_runtime import verify_dynamic_abi
except ModuleNotFoundError:
    from verify_nurabrowser_runtime import verify_dynamic_abi


SOURCE_SHA = re.compile(r"[0-9a-f]{40}\Z")
NEEDED = re.compile(r"Shared library: \[([^\]]+)\]")
REQUIRED = (
    "browser/nurabrowser-core",
    "browser/libexec/WPEWebProcess",
    "browser/libexec/WPENetworkProcess",
    "bin/bwrap",
    "bin/xdg-dbus-proxy",
    "lib/ld-musl-armhf.so.1",
    "modules/gio/giomodule.cache",
)


def digest(path):
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def readelf(path, option):
    return subprocess.run(["readelf", option, str(path)], check=True,
                          capture_output=True, text=True, timeout=30).stdout


def marker_source(root):
    marker = root / "CANDIDATE-ONLY.txt"
    if not marker.is_file() or marker.is_symlink():
        raise ValueError("missing candidate provenance marker")
    fields = {}
    for line in marker.read_text().splitlines():
        if "=" not in line:
            raise ValueError("malformed provenance marker")
        key, value = line.split("=", 1)
        if key in fields:
            raise ValueError("duplicate provenance key")
        fields[key] = value
    source = fields.get("source_sha", "")
    if not SOURCE_SHA.fullmatch(source):
        raise ValueError("invalid candidate source SHA")
    if fields.get("target") != "armv7-unknown-linux-musleabihf":
        raise ValueError("candidate target is not ARMv7/musl")
    if fields.get("runtime_qualification") != "UNQUALIFIED":
        raise ValueError("unexpected candidate qualification claim")
    if fields.get("device_deployment") != "FORBIDDEN":
        raise ValueError("candidate must forbid ordinary device deployment")
    return source


def source_tree(repo, source, release):
    """Compare exact C engine trees; never relabel the build's original SHA."""
    def object_id(revision):
        return subprocess.run(
            ["git", "rev-parse", f"{revision}:components/nura-browser-engine"],
            cwd=repo, capture_output=True, text=True, check=True, timeout=20,
        ).stdout.strip()

    return {"build_tree": object_id(source),
            "release_tree": object_id(release)}


def attribute_findings(attrs):
    failures = []
    if "Tag_FP_arch: VFPv3-D16" not in attrs or "Tag_ABI_VFP_args: VFP registers" not in attrs:
        failures.append("ISA_NOT_VFPV3_D16")
    if re.search(r"NEON|Advanced_SIMD", attrs, re.I):
        failures.append("ISA_NEON_ATTRIBUTE")
    return failures


def inspect_elf(path):
    findings = []
    header = readelf(path, "-h")
    if not ("ELF32" in header and re.search(r"Machine:\s+ARM\b", header)
            and "hard-float ABI" in header):
        findings.append("ELF_NOT_ARMV7_HARDFLOAT")
    attrs = readelf(path, "-A")
    findings.extend(attribute_findings(attrs))
    program = readelf(path, "-l")
    if "INTERP" in program and "/lib/ld-musl-armhf.so.1" not in program:
        findings.append("WRONG_ELF_INTERPRETER")
    dynamic = readelf(path, "-d")
    try:
        verify_dynamic_abi(dynamic, "musl")
    except ValueError:
        findings.append("GLIBC_NEEDED_IN_MUSL")
    return findings, NEEDED.findall(dynamic)


def audit(root, repo, release_sha):
    if root.is_symlink():
        raise ValueError("candidate root must not be a symlink")
    root = root.resolve(strict=True)
    issues = []
    if not root.is_dir():
        raise ValueError("candidate must be a directory")
    if not SOURCE_SHA.fullmatch(release_sha):
        raise ValueError("release SHA must be a full Git commit")
    source = marker_source(root)
    trees = source_tree(repo, source, release_sha)
    if trees["build_tree"] != trees["release_tree"]:
        issues.append({"path": "components/nura-browser-engine", "code": "SOURCE_TREE_MISMATCH"})

    entries = []
    files = {}
    for path in sorted(root.rglob("*")):
        name = path.relative_to(root).as_posix()
        if path.is_symlink():
            issues.append({"path": name, "code": "SYMLINK_NOT_ALLOWED"})
        elif path.is_file():
            files[name] = path
            entries.append({"path": name, "sha256": digest(path)})
    for name in REQUIRED:
        if name not in files:
            issues.append({"path": name, "code": "REQUIRED_FILE_MISSING"})
    if not any(n.startswith("resources/") for n in files):
        issues.append({"path": "resources/", "code": "RUNTIME_RESOURCES_MISSING"})
    if not any(n.startswith("modules/gio/") and n.endswith(".so") for n in files):
        issues.append({"path": "modules/gio/", "code": "DLOPEN_MODULES_MISSING"})

    elf_count = 0
    # A preflight requires every DT_NEEDED library to be contained in the
    # private closure. A host or device system-library fallback is not proof.
    for name, path in files.items():
        with path.open("rb") as stream:
            if stream.read(4) != b"\x7fELF":
                continue
        elf_count += 1
        try:
            codes, needed = inspect_elf(path)
        except (OSError, subprocess.SubprocessError) as exc:
            issues.append({"path": name, "code": "ELF_INSPECTION_FAILED",
                           "detail": type(exc).__name__})
            continue
        issues.extend({"path": name, "code": code} for code in codes)
        for dep in needed:
            if f"lib/{dep}" not in files and f"browser/libexec/{dep}" not in files:
                issues.append({"path": name, "code": "DT_NEEDED_NOT_BUNDLED", "library": dep})

    return {
        "schema": "nuraloumi-wpe-candidate-preflight-v1",
        "qualification": "UNQUALIFIED",
        "build_source_sha": source,
        "release_source_sha": release_sha,
        "engine_git_trees": trees,
        "files": entries,
        "elf_count": elf_count,
        "issues": sorted(issues, key=lambda issue: (issue["path"], issue["code"])),
        "unproven": ["full_decoded_instruction_closure", "transitive_dlopen_modules",
                     "sandbox_behavior", "WebProcess_lifecycle", "device_pixels_touch",
                     "independent_review"],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("--release-sha", required=True)
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output", type=Path, help="write an audit report outside the candidate tree")
    args = parser.parse_args()
    try:
        report = audit(args.candidate, args.repo, args.release_sha)
    except (ValueError, OSError, subprocess.SubprocessError) as exc:
        parser.exit(2, f"WPE_CANDIDATE_AUDIT=ERROR: {exc}\n")
    rendered = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.output:
        if args.output.resolve().is_relative_to(args.candidate.resolve()):
            parser.exit(2, "audit report must stay outside candidate tree\n")
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered)
    print(f"WPE_CANDIDATE_AUDIT={'BLOCKED' if report['issues'] else 'STATIC_PREFLIGHT_PASS'} "
          f"files={len(report['files'])} ELF={report['elf_count']} issues={len(report['issues'])} "
          "runtime_qualification=UNQUALIFIED")
    for issue in report["issues"]:
        print(f"  {issue['code']}: {issue['path']}" +
              (f" -> {issue['library']}" if "library" in issue else ""))
    if report["issues"]:
        parser.exit(1)


if __name__ == "__main__":
    main()
