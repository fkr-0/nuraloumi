#!/usr/bin/env python3
"""Read-only, chroot-isolated ARMv7/musl loader preflight for a WPE candidate.

This checks the three native WPE executables against an explicit Alpine root,
never the host's shared libraries. It does not qualify dlopen, Seccomp, ISA
decoding, sandbox behavior, Wayland pixels or a production runtime.json.
"""

import argparse
import hashlib
import json
import platform
from pathlib import Path
import re
import subprocess


PROCESSES = (
    "usr/libexec/nuraloumi/browser/nurabrowser-core",
    "usr/libexec/nuraloumi/browser/libexec/WPEWebProcess",
    "usr/libexec/nuraloumi/browser/libexec/WPENetworkProcess",
)
CRYPTO = ("libcrypto.so.3", "libssl.so.3", "libnettle.so.8",
          "libhogweed.so.6", "libgcrypt.so.20")
REQUIRED = ("lib/ld-musl-armhf.so.1", "usr/bin/bwrap",
            "usr/bin/xdg-dbus-proxy", "usr/lib/libicui18n.so.76",
            "usr/lib/libicuuc.so.76", "usr/lib/libicudata.so.76",
            *(f"usr/lib/{name}" for name in CRYPTO), *PROCESSES)
LOADER_LINE = re.compile(
    r"^\s*(?:(?P<needed>\S+)\s+=>\s+)?(?P<path>/\S+)\s+\(0x[0-9a-fA-F]+\)\s*$"
)
HEX_SHA = re.compile(r"[0-9a-f]{64}\Z")
HEX_COMMIT = re.compile(r"[0-9a-f]{40}\Z")


class ProbeError(ValueError):
    pass


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def producer_identity(directory):
    marker = directory / "CANDIDATE-ONLY.txt"
    tarball = directory / "nurabrowser-wpe-musl-candidate.tar"
    sums = directory / "SHA256SUMS.txt"
    if any(not path.is_file() or path.is_symlink()
           for path in (marker, tarball, sums)):
        raise ProbeError("missing regular producer marker, tar, or checksum file")
    fields = {}
    for line in marker.read_text().splitlines():
        if line.count("=") != 1:
            raise ProbeError("invalid producer marker")
        key, value = line.split("=", 1)
        if key in fields:
            raise ProbeError("duplicate producer marker key")
        fields[key] = value
    if (not HEX_COMMIT.fullmatch(fields.get("source_sha", ""))
            or fields.get("upstream_wpewebkit") != "2.54.2"
            or fields.get("target") != "armv7-unknown-linux-musleabihf"
            or fields.get("runtime_qualification") != "UNQUALIFIED"
            or fields.get("device_deployment") != "FORBIDDEN"):
        raise ProbeError("unexpected producer provenance or promotion flags")
    recorded = []
    for line in sums.read_text().splitlines():
        parts = line.split(None, 1)
        if (len(parts) == 2 and HEX_SHA.fullmatch(parts[0])
                and Path(parts[1].removeprefix("*")).name == tarball.name):
            recorded.append(parts[0])
    if len(recorded) != 1 or recorded[0] != sha256(tarball):
        raise ProbeError("producer tar checksum mismatch")
    return fields["source_sha"], recorded[0]


def private_file(root, absolute):
    if not absolute.startswith("/") or ".." in Path(absolute).parts:
        raise ProbeError(f"unsafe loader path: {absolute}")
    candidate = root / absolute.lstrip("/")
    if not candidate.is_file() or not candidate.resolve().is_relative_to(root):
        raise ProbeError(f"loader resolved outside private root or missing: {absolute}")
    return candidate


def parse_loader_output(output, root):
    paths = []
    for line in output.splitlines():
        match = LOADER_LINE.fullmatch(line)
        if match is None:
            raise ProbeError(f"unrecognized musl loader output: {line[:160]}")
        path = match.group("path")
        private_file(root, path)
        paths.append(path)
    if len(paths) < 3 or "/lib/ld-musl-armhf.so.1" not in paths:
        raise ProbeError("incomplete musl loader listing")
    return paths


def crypto_issues(name, attributes):
    issues = []
    if "Tag_FP_arch: VFPv3-D16" not in attributes:
        issues.append(f"{name}: ISA_NOT_VFPV3_D16")
    if re.search(r"NEON|Advanced_SIMD", attributes, re.IGNORECASE):
        issues.append(f"{name}: ISA_NEON_ATTRIBUTE")
    if "Tag_ABI_VFP_args: VFP registers" not in attributes:
        issues.append(f"{name}: ABI_NOT_HARDFLOAT")
    return issues


def probe(root, producer, image):
    if root.is_symlink() or producer.is_symlink() or not root.is_dir():
        raise ProbeError("root/provenance directory missing or a symlink")
    root = root.resolve(strict=True)
    source_sha, tar_sha256 = producer_identity(producer)
    for relative in REQUIRED:
        private_file(root, "/" + relative)
    if any((root / rel).exists() for rel in (
            "runtime.json", "usr/libexec/nuraloumi/browser/runtime.json")):
        raise ProbeError("unqualified Alpine root must not claim runtime.json")
    docker_info = subprocess.run(
        ["docker", "image", "inspect", "--format", "{{.Id}}", image],
        check=True, capture_output=True, text=True, timeout=15,
    )
    image_id = docker_info.stdout.strip()
    if not image_id.startswith("sha256:"):
        raise ProbeError("Docker image digest unavailable")
    processes = {}
    for executable in PROCESSES:
        command = [
            "docker", "run", "--rm", "--pull=never",
            "--platform", "linux/arm/v7", "--network", "none",
            "--read-only", "--cap-drop", "ALL", "--cap-add", "SYS_CHROOT",
            "--security-opt", "no-new-privileges", "--pids-limit", "80",
            "--memory", "768m", "--cpus", "0.5",
            "--mount", f"type=bind,src={root},dst=/private-root,readonly",
            image, "chroot", "/private-root", "/lib/ld-musl-armhf.so.1",
            "--list", "/" + executable,
        ]
        result = subprocess.run(
            command, check=True, capture_output=True, text=True, timeout=45,
        )
        processes[executable] = {
            "loaded_paths": parse_loader_output(result.stdout, root),
            "loader_stdout_sha256": hashlib.sha256(
                result.stdout.encode()).hexdigest(),
        }
    issues = []
    for name in CRYPTO:
        result = subprocess.run(
            ["readelf", "-A", str(private_file(root, "/usr/lib/" + name))],
            check=True, capture_output=True, text=True, timeout=15,
        )
        issues.extend(crypto_issues(name, result.stdout))
    return {
        "schema": "nuraloumi-alpine-private-loader-preflight-v1",
        "qualification": "UNQUALIFIED",
        "status": "BLOCKED" if issues else "LOADER_PREFLIGHT_ONLY",
        "source_sha": source_sha,
        "producer_tar_sha256": tar_sha256,
        "host_arch": platform.machine(),
        "docker_image_id": image_id,
        "processes": processes,
        "crypto_attribute_issues": sorted(issues),
        "unproven": [
            "native_execution_on_sl101", "mixed_arm_thumb_decoded_instruction_isa",
            "transitive_dlopen_and_plugins", "seccomp_and_networkprocess_isolation",
            "real_wayland_pixels_navigation_touch", "independent_release_review",
        ],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path, help="extracted standalone Alpine root")
    parser.add_argument("producer", type=Path, help="matching producer archive directory")
    parser.add_argument("--image", default="alpine:3.22")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        if args.output and (args.output.resolve().is_relative_to(args.root.resolve())
                            or args.output.resolve().is_relative_to(args.producer.resolve())):
            raise ProbeError("evidence output must be outside the inputs")
        result = probe(args.root, args.producer, args.image)
    except (ProbeError, OSError, subprocess.SubprocessError) as exc:
        parser.exit(2, f"WPE_LOADER_PREFLIGHT=ERROR: {exc}\n")
    if args.output:
        args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(f"WPE_LOADER_PREFLIGHT={result['status']} "
          f"processes={len(result['processes'])} "
          f"crypto_issues={len(result['crypto_attribute_issues'])} "
          "qualification=UNQUALIFIED")
    for issue in result["crypto_attribute_issues"]:
        print(f"  {issue}")
    if result["crypto_attribute_issues"]:
        parser.exit(1)


if __name__ == "__main__":
    main()
