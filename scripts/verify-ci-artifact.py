#!/usr/bin/env python3
"""Download and verify an exact GitHub Actions artifact. NEVER deploys."""

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import tarfile

REPO = "fkr-0/nuraloumi"
SHA = re.compile(r"[0-9a-f]{40}\Z")
HEXDIGEST = re.compile(r"[0-9a-f]{64}\Z")
LANES = {
    "alpine": (
        "SL101 Alpine armv7 musl package",
        "nuraloumi-alpine322-armv7-musleabihf-",
        "armv7-unknown-linux-musleabihf",
    ),
    "debian": (
        "SL101 Debian armhf glibc package",
        "nuraloumi-debian13-armv7-gnueabihf-",
        "armv7-unknown-linux-gnueabihf",
    ),
}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def run_json(*args: str) -> dict:
    output = subprocess.run(
        ["gh", *args], check=True, capture_output=True, text=True, timeout=120
    )
    return json.loads(output.stdout)


def validate_run(run: dict, lane: str, sha: str) -> None:
    name, _, _ = LANES[lane]
    require(
        run.get("status") == "completed"
        and run.get("conclusion") == "success"
        and run.get("headSha") == sha
        and run.get("headBranch") == "main"
        and run.get("event") in ("push", "workflow_dispatch")
        and run.get("workflowName") == name,
        "GitHub Actions run is not a successful main-branch build for the exact SHA and ABI",
    )


def checksum_file(root: Path, listing: Path) -> set[str]:
    require(listing.is_file(), f"missing checksum file: {listing}")
    checked: set[str] = set()
    for line in listing.read_text().splitlines():
        digest, sep, name = line.partition("  ")
        require(bool(sep) and bool(HEXDIGEST.fullmatch(digest)), "bad SHA256SUMS line")
        p = PurePosixPath(name)
        require(
            name and not p.is_absolute() and ".." not in p.parts and not name.startswith("./"),
            "unsafe checksum filename",
        )
        require(name not in checked, "duplicate checksum filename")
        path = root.joinpath(*p.parts)
        require(path.is_file() and not path.is_symlink(), f"missing regular file: {name}")
        h = hashlib.sha256()
        with path.open("rb") as source:
            while chunk := source.read(1024 * 1024):
                h.update(chunk)
        require(h.hexdigest() == digest, f"SHA256 mismatch: {name}")
        checked.add(name)
    require(bool(checked), "empty checksums")
    return checked


def verify_downloaded(folder: Path, lane: str, sha: str, expected_version: str | None = None) -> dict:
    require(lane in LANES and bool(SHA.fullmatch(sha)), "invalid lane or SHA")
    _, _, target = LANES[lane]
    require(checksum_file(folder, folder / "SHA256SUMS.txt") == {"nuraloumi-ci.tar"},
            "top-level checksums must pin the sole release tarball")
    manifest = (folder / "nuraloumi-ci/MANIFEST.txt").read_bytes()
    fields = {}
    for line in manifest.decode("utf-8").splitlines():
        if line == "files:":
            break
        if "=" in line:
            k, v = line.split("=", 1)
            require(k not in fields, f"duplicate manifest key {k}")
            fields[k] = v
    for key, expected in (
        ("schema", "nuraloumi-package-manifest-v1"),
        ("git_commit", sha),
        ("git_dirty", "false"),
        ("target", target),
    ):
        require(fields.get(key) == expected, f"manifest {key} mismatch")
    binaries = fields.get("binaries", "").split(",")
    require(len(binaries) >= 4 and len(set(binaries)) == len(binaries)
            and all(re.fullmatch(r"nuraloumi-[a-z0-9-]+", name) for name in binaries)
            and {"nuraloumi-menu", "nuraloumi-panel", "nuraloumi-probe",
                 "nuraloumi-thumbnail-helper"}.issubset(binaries), "incomplete binaries list")
    require(
        f"git_sha={sha}" in (folder / "BUILD-INFO.txt").read_text().splitlines(),
        "BUILD-INFO Git SHA mismatch",
    )
    audit = (folder / "ELF-AUDIT.txt").read_text()
    passes = re.findall(r"^PASS (nuraloumi-[a-z0-9-]+) ELF32 ARM VFPv3-D16 hardfloat (musl|glibc) no-NEON-attributes$", audit, re.M)
    expected_abi = "musl" if lane == "alpine" else "glibc"
    require(len(passes) == len(binaries) and {name for name, abi in passes if abi == expected_abi} == set(binaries),
            "ELF audit incomplete or ABI mismatch")
    # Ensure the separately uploaded manifest is the one *inside* the
    # SHA256-verified tarball, not an unrelated copy.
    with tarfile.open(folder / "nuraloumi-ci.tar") as archive:
        files = {}
        for member in archive.getmembers():
            name = member.name.removeprefix("./")
            path = PurePosixPath(name)
            require(not path.is_absolute() and ".." not in path.parts
                    and not member.issym() and not member.islnk()
                    and (member.isfile() or member.isdir()), "unsafe tar member")
            if member.isfile():
                require(name not in files, "duplicate tar member")
                source = archive.extractfile(member)
                require(source is not None, "tar member unreadable")
                files[name] = source.read()
        require(files.get("MANIFEST.txt") == manifest, "tar manifest mismatch")
        lines = manifest.decode("utf-8").splitlines()
        require(lines.count("files:") == 1, "manifest files section missing or repeated")
        records = {}
        for line in lines[lines.index("files:") + 1:]:
            digest, separator, name = line.strip().partition("  ")
            path = PurePosixPath(name)
            require(separator and HEXDIGEST.fullmatch(digest) and name not in records
                    and name and not path.is_absolute() and ".." not in path.parts,
                    "invalid manifest file record")
            records[name] = digest
        require(set(files) == set(records) | {"MANIFEST.txt"}, "partial or extra tar bundle")
        for name, digest in records.items():
            require(hashlib.sha256(files[name]).hexdigest() == digest,
                    f"tar manifest SHA256 mismatch: {name}")
        for name in binaries:
            data = files.get("bin/" + name, b"")
            require(len(data) >= 52 and data[:5] == b"\x7fELF\x01"
                    and data[5] == 1 and data[18:20] == b"\x28\x00"
                    and int.from_bytes(data[36:40], "little") & 0x400,
                    f"wrong ELF32 ARM hardfloat ABI or missing binary: {name}")
    version = fields.get("version", "")
    require(bool(re.fullmatch(r"[0-9]+(?:\.[0-9]+){2}(?:-(?:dev|rc)\.[0-9]+)?", version)), "bad version")
    if expected_version is not None:
        require(version == expected_version, "release version mismatch")
    if lane == "alpine":
        base = folder / "native-apk"
        pkg_version = version.replace("-dev.", "_pre").replace("-rc.", "_rc")
        native_name = f"nuraloumi-{pkg_version}-r0.apk"
        packages = list(base.rglob(native_name))
        require(len(packages) == 1, "expected exactly one APK")
        checked = checksum_file(base, base / "NURALOUMI-APK-SHA256SUMS.txt")
        require(str(packages[0].relative_to(base)) in checked, "APK not in checksums")
        require((base / "CI-EPHEMERAL-KEY.rsa.pub").is_file(), "missing APK public key")
        policy = (base / "CI-APK-SIGNING-POLICY.txt").read_text()
        require(
            "SIGNING_MODE=ephemeral-ci-development-only" in policy.splitlines(),
            "missing or invalid development-signing policy",
        )
        require(f"GIT_SHA={sha}" in policy.splitlines(), "APK signing provenance SHA mismatch")
        fingerprint = hashlib.sha256(
            (base / "CI-EPHEMERAL-KEY.rsa.pub").read_bytes()
        ).hexdigest()
        require(
            f"PUBLIC_KEY_SHA256={fingerprint}" in policy.splitlines(),
            "APK signing public key fingerprint mismatch",
        )
    else:
        base = folder / "native-deb"
        pkg_version = version.replace("-dev.", "~dev.").replace("-rc.", "~rc.")
        native_name = f"nuraloumi_{pkg_version}_armhf.deb"
        packages = list(base.glob(native_name))
        require(len(packages) == 1, "expected exactly one DEB")
        checked = checksum_file(base, base / f"{native_name}.sha256")
        require(native_name in checked, "DEB not in checksums")
    return {"sha": sha, "target": target, "version": version, "package": str(packages[0])}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-id", type=int, required=True)
    parser.add_argument("--sha", required=True, help="exact 40-digit reviewed Git SHA")
    parser.add_argument("--lane", choices=sorted(LANES), required=True)
    parser.add_argument("--expected-version", required=True,
                        help="exact release version reviewed alongside --sha")
    parser.add_argument("--out", type=Path, required=True, help="empty local download directory")
    args = parser.parse_args()
    require(args.run_id > 0 and bool(SHA.fullmatch(args.sha)), "invalid run ID or SHA")
    run = run_json(
        "run", "view", str(args.run_id), "--repo", REPO, "--json",
        "conclusion,status,headSha,headBranch,event,workflowName",
    )
    validate_run(run, args.lane, args.sha)
    artifacts = run_json(
        "api", f"repos/{REPO}/actions/runs/{args.run_id}/artifacts?per_page=100",
    ).get("artifacts", [])
    prefix = LANES[args.lane][1]
    matches = [
        a["name"] for a in artifacts
        if not a.get("expired") and a.get("name", "").startswith(prefix)
        and a["name"].endswith("-" + args.sha)
    ]
    require(len(matches) == 1, "expected one live artifact named for exact SHA")
    args.out.mkdir(parents=True, exist_ok=True)
    require(not any(args.out.iterdir()), "download destination must be empty")
    subprocess.run(
        ["gh", "run", "download", str(args.run_id), "--repo", REPO,
         "--name", matches[0], "--dir", str(args.out)],
        check=True, timeout=600,
    )
    result = verify_downloaded(args.out, args.lane, args.sha, args.expected_version)
    print(f"VERIFIED_CI_ARTIFACT=PASS {result} DEVICE_QUALIFICATION=PENDING")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ValueError, FileNotFoundError, OSError, subprocess.SubprocessError, tarfile.TarError) as error:
        print(f"VERIFIED_CI_ARTIFACT=FAIL reason={error}", file=sys.stderr)
        raise SystemExit(2) from error
