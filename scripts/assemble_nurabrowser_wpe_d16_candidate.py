#!/usr/bin/env python3
"""Build a fresh, unqualified WPE/musl closure from D16 crypto replacements.

Never modify the original canary, rewrite its source SHA, generate runtime.json
or touch the live tablet. A separate full ISA, dlopen, sandbox and pixel review
is mandatory before this can become a release artifact.
"""

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil


PAIRS = ("libcrypto.so.3", "libssl.so.3", "libnettle.so.8",
         "libhogweed.so.6", "libgcrypt.so.20")
SOURCE_SHA = re.compile(r"[0-9a-f]{40}\Z")


def digest(path):
    h = hashlib.sha256()
    with path.open("rb") as src:
        for chunk in iter(lambda: src.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def verified_replacements(overlay):
    marker = overlay / "CANDIDATE-ONLY.txt"
    if not marker.is_file() or "normal_deployment=FORBIDDEN" not in marker.read_text():
        raise ValueError("missing fail-closed crypto-overlay provenance")
    sums = overlay / "REPLACEMENTS-SHA256SUMS.txt"
    if not sums.is_file() or sums.is_symlink():
        raise ValueError("missing replacement checksum inventory")
    entries = {}
    for line in sums.read_text().splitlines():
        parts = line.split(None, 1)
        if len(parts) != 2 or len(parts[0]) != 64:
            raise ValueError("invalid replacement checksum line")
        rel = parts[1].removeprefix("*").removeprefix("./")
        candidate = Path(rel)
        if (not rel or candidate.is_absolute() or ".." in candidate.parts
                or candidate.as_posix() != rel or rel in entries):
            raise ValueError("unsafe replacement path")
        file = overlay / "replacements" / candidate
        if (not file.is_file() or file.is_symlink()
                or any(parent.is_symlink() for parent in file.parents
                       if parent != overlay and parent != overlay.parent)):
            raise ValueError(f"replacement missing or symlink: {rel}")
        if digest(file) != parts[0]:
            raise ValueError(f"replacement hash mismatch: {rel}")
        entries[rel] = file
    for soname in PAIRS:
        if soname not in entries:
            raise ValueError(f"missing crypto ABI pair: {soname}")
    return entries


def assemble(original, overlay, output):
    if original.is_symlink() or overlay.is_symlink() or output.exists():
        raise ValueError("source symlink or output already exists")
    if not original.is_dir() or not overlay.is_dir():
        raise ValueError("missing original candidate or replacements")
    marker = original / "CANDIDATE-ONLY.txt"
    if not marker.is_file() or marker.is_symlink():
        raise ValueError("missing original WPE candidate marker")
    lines = marker.read_text().splitlines()
    if "runtime_qualification=UNQUALIFIED" not in lines or "device_deployment=FORBIDDEN" not in lines:
        raise ValueError("original WPE candidate not fail-closed")
    source_lines = [x for x in lines if x.startswith("source_sha=")]
    if (len(source_lines) != 1
            or not SOURCE_SHA.fullmatch(source_lines[0].split("=", 1)[1])):
        raise ValueError("missing original source commit")
    entries = verified_replacements(overlay)

    # All paths are validated before writing. Do not hardlink to the original:
    # future staging must be unable to corrupt the known canary.
    files = []
    for path in sorted(original.rglob("*")):
        if path.is_symlink():
            raise ValueError("original WPE tree contains a symlink")
        if path.is_file():
            rel = path.relative_to(original)
            if rel.as_posix() == "runtime.json":
                raise ValueError("an unqualified candidate cannot contain runtime.json")
            if rel.name not in ("SHA256SUMS", "CANDIDATE-ONLY.txt"):
                files.append((path, rel))
    output.mkdir(parents=True, exist_ok=False)
    for src, rel in files:
        dest = output / rel
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(src, dest)
    for name in PAIRS:
        dest = output / "lib" / name
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(entries[name], dest)
    for rel, src in entries.items():
        if rel.startswith("ossl-modules/"):
            dest = output / "lib" / rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(src, dest)

    (output / "CANDIDATE-ONLY.txt").write_text(
        "\n".join((
            *[line for line in lines if not line.startswith(
                ("runtime_qualification=", "device_deployment=", "missing="))],
            "runtime_qualification=UNQUALIFIED",
            "device_deployment=FORBIDDEN",
            "missing=full_isa_dlopen_sandbox_real_pixels_independent_review",
            "crypto_overlay=source_pinned_vfpv3_d16_candidate",
        )) + "\n"
    )
    provenance = {
        "schema": "nuraloumi-wpe-d16-crypto-overlay-v1",
        "original_source_sha": source_lines[0].split("=", 1)[1],
        "qualification": "UNQUALIFIED",
        "crypto_sha256": {name: digest(entries[name]) for name in PAIRS},
        "openssl_modules_sha256": {
            rel: digest(src) for rel, src in entries.items()
            if rel.startswith("ossl-modules/")
        },
    }
    (output / "CRYPTO-PROVENANCE.json").write_text(
        json.dumps(provenance, indent=2, sort_keys=True) + "\n"
    )
    manifest_files = sorted(p for p in output.rglob("*") if p.is_file())
    (output / "SHA256SUMS").write_text(
        "".join(f"{digest(path)}  ./{path.relative_to(output).as_posix()}\n"
                for path in manifest_files)
    )
    return provenance


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("original", type=Path)
    parser.add_argument("overlay", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    try:
        result = assemble(args.original, args.overlay, args.output)
    except (ValueError, OSError) as exc:
        parser.exit(1, f"WPE_D16_ASSEMBLY=FAIL: {exc}\n")
    print("WPE_D16_ASSEMBLY=CANDIDATE_ONLY "
          f"source_sha={result['original_source_sha']} "
          f"crypto_libraries={len(result['crypto_sha256'])}")


if __name__ == "__main__":
    main()
