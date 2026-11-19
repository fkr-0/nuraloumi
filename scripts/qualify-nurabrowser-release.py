#!/usr/bin/env python3
"""Read-only, fail-closed NuraBrowser Gate 5 installable archive preflight."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tarfile

PUBLIC = "usr/bin/nuraloumi-browser"
ENGINE = "usr/libexec/nuraloumi/browser/nurabrowser-core"
DESKTOP = "usr/share/applications/nuraloumi-browser.desktop"


def require(ok, reason):
    if not ok:
        raise ValueError(reason)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def check_archive(archive_path, approved_digest):
    require(re.fullmatch("[a-f0-9]{64}", approved_digest), "invalid approved digest")
    require(archive_path.is_file() and not archive_path.is_symlink(), "archive missing")
    require(digest(archive_path.read_bytes()) == approved_digest, "archive digest mismatch")
    files = {}
    with tarfile.open(archive_path, "r:*") as archive:
        for member in archive:
            name = member.name.removeprefix("./")
            require(name not in files, "duplicate archive path")
            require(not name.startswith("/") and ".." not in Path(name).parts, "unsafe archive path")
            require(member.uid == 0 and member.gid == 0, "unsafe archive ownership")
            require(member.isfile() or member.isdir(), "archive link or special file")
            require(member.size <= 100 * 1024 * 1024, "oversized archive member")
            require(not member.mode & 0o6000, "setuid/setgid archive member")
            if member.isfile():
                stream = archive.extractfile(member)
                require(stream is not None, "unreadable archive member")
                data = stream.read()
                require(len(data) == member.size, "short archive member")
                files[name] = data
            else:
                files[name] = None
    return files


def check_contract(files, manifest, source_sha):
    require(re.fullmatch("[a-f0-9]{40}", source_sha), "invalid source SHA")
    require(manifest.get("source_sha") == source_sha, "source provenance mismatch")
    require(manifest.get("schema") == "nuraloumi.browser.release.v1", "manifest schema mismatch")
    require(manifest.get("gate2") == "PASS", "Gate 2 closure PENDING")
    require(manifest.get("target") in ("armv7-alpine-linux-musleabihf",
                                      "armv7-debian-linux-gnueabihf"), "unknown target ABI")
    for path in (PUBLIC, ENGINE, DESKTOP):
        require(isinstance(files.get(path), bytes), "missing installable " + path)
    require("usr/bin/,browse" not in files and
            "usr/libexec/nura-browser/nurabrowser-core" not in files,
            "historical mixed-source browser included")
    actual = {name: digest(data) for name, data in files.items() if isinstance(data, bytes)}
    require(manifest.get("files") == actual, "inventory or file digests mismatch")
    desktop = files[DESKTOP].decode("utf-8")
    require("[Desktop Entry]" in desktop and "Type=Application" in desktop and
            re.search(r"^Exec=nuraloumi-browser(?:\s|$)", desktop, re.M),
            "desktop launch contract mismatch")
    require(files[PUBLIC].startswith(b"\x7fELF") and
            files[ENGINE].startswith(b"\x7fELF"), "native ELF missing")
    require(manifest.get("launch") == "nuraloumi-browser" and
            manifest.get("rollback") == "retain-previous-atomically",
            "launch/rollback contract missing")
    return actual


def check_elf(path, expected_loader, forbidden_loader):
    outputs = []
    for flag in ("-h", "-l", "-A"):
        outputs.append(subprocess.run(["readelf", flag, str(path)], check=True,
                                      capture_output=True, text=True, timeout=15).stdout)
    header, program, attrs = outputs
    require(re.search(r"Machine:\s+ARM", header) and "hard-float ABI" in header,
            "not ARMHF ELF: " + path.name)
    require("Tag_CPU_arch: v7" in attrs and "Tag_FP_arch: VFPv3-D16" in attrs and
            "Tag_Advanced_SIMD_arch" not in attrs, "NEON/ABI attributes: " + path.name)
    require(expected_loader in program and forbidden_loader not in program,
            "glibc/musl crossmix: " + path.name)


def qualify(archive, approved_digest, manifest, source_sha, extracted_root):
    files = check_archive(archive, approved_digest)
    hashes = check_contract(files, manifest, source_sha)
    require(extracted_root is not None, "extracted ARMHF ELF audit required")
    root = extracted_root.resolve(strict=True)
    alpine = "alpine" in manifest["target"]
    for name in (PUBLIC, ENGINE):
        path = root / name
        require(path.is_file() and not path.is_symlink() and
                path.resolve().is_relative_to(root), "unsafe extracted ELF path")
        require(digest(path.read_bytes()) == hashes[name], "extracted ELF hash mismatch")
        check_elf(path, "ld-musl-armhf.so.1" if alpine else "ld-linux-armhf.so.3",
                  "ld-linux-armhf.so.3" if alpine else "ld-musl-armhf.so.1")
    return {"status": "STATIC-PACKAGE-PASS-DEVICE-PENDING", "source_sha": source_sha,
            "archive_sha256": approved_digest}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", required=True, type=Path)
    parser.add_argument("--archive-sha256", required=True)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--extracted-root", type=Path)
    args = parser.parse_args()
    try:
        result = qualify(args.archive, args.archive_sha256,
                         json.loads(args.manifest.read_text()), args.source_sha,
                         args.extracted_root)
    except (ValueError, OSError, subprocess.SubprocessError, tarfile.TarError) as exc:
        print("PENDING/FAIL:", exc)
        return 1
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
