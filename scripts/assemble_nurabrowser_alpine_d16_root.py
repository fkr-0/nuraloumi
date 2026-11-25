#!/usr/bin/env python3
"""Build a fresh, unqualified Alpine WPE root with source-pinned D16 crypto.

Never modify the original Alpine root, a live browser, or an existing output.
Original WPE process bytes are verified against the exact producer archive.
This is a separate static candidate, NEVER a runtime.json or release gate.
"""

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import tarfile

if __package__:
    from scripts.assemble_nurabrowser_wpe_d16_candidate import verified_replacements
    from scripts.probe_nurabrowser_alpine_loader import (
        PROCESSES, private_file, producer_identity, sha256,
    )
else:
    from assemble_nurabrowser_wpe_d16_candidate import verified_replacements
    from probe_nurabrowser_alpine_loader import (
        PROCESSES, private_file, producer_identity, sha256,
    )


WPE_FROM_PRODUCER = PROCESSES + (
    "usr/lib/libWPEWebKit-2.0.so.1.11.5",
)


def check_producer_bytes(producer, root):
    archive_path = producer / "nurabrowser-wpe-musl-candidate.tar"
    hashes = {}
    with tarfile.open(archive_path, "r:") as archive:
        for rel in WPE_FROM_PRODUCER:
            try:
                member = archive.getmember("stage/" + rel)
            except KeyError as exc:
                raise ValueError(f"producer lacks WPE executable: {rel}") from exc
            if not member.isfile():
                raise ValueError(f"producer WPE object is not regular: {rel}")
            stream = archive.extractfile(member)
            if stream is None:
                raise ValueError(f"producer WPE object unreadable: {rel}")
            digest = hashlib.sha256()
            with stream:
                for block in iter(lambda: stream.read(1024 * 1024), b""):
                    digest.update(block)
            original_hash = sha256(private_file(root, "/" + rel))
            if digest.hexdigest() != original_hash:
                raise ValueError(f"Alpine root mismatches WPE producer: {rel}")
            hashes[rel] = original_hash
    return hashes


def copy_into_aliased_library(output, soname, source):
    library_dir = output / "usr/lib"
    alias = library_dir / soname
    if not alias.is_file():
        raise ValueError(f"missing Alpine SONAME alias: {soname}")
    target = alias.resolve()
    if not target.is_relative_to(library_dir.resolve()) or not target.is_file():
        raise ValueError(f"SONAME target escapes private directory: {soname}")
    # Preserve existing SONAME -> versioned-file symlinks, replacing their
    # *target*, so neither path can select an old incompatible crypto object.
    shutil.copyfile(source, target)
    if sha256(alias) != sha256(source):
        raise ValueError(f"installed replacement hash mismatch: {soname}")
    return target.relative_to(output).as_posix()


def assemble(root, producer, overlay, output):
    inputs = (root, producer, overlay)
    if (output.exists() or output.is_symlink()
            or any(not item.is_dir() or item.is_symlink() for item in inputs)):
        raise ValueError("all inputs must exist and output must be fresh")
    output_absolute = output.absolute()
    if any(output_absolute.is_relative_to(item.resolve()) for item in inputs):
        raise ValueError("output must not be inside an input tree")
    root, producer, overlay = (item.resolve(strict=True) for item in inputs)
    output = output_absolute
    source_sha, producer_hash = producer_identity(producer)
    if (root / "runtime.json").exists() or (
            root / "usr/libexec/nuraloumi/browser/runtime.json").exists():
        raise ValueError("Alpine candidate cannot contain runtime.json")
    wpe_hashes = check_producer_bytes(producer, root)
    replacements = verified_replacements(overlay)
    if "ossl-modules/legacy.so" not in replacements:
        raise ValueError("OpenSSL legacy module missing from D16 overlay")
    marker = (overlay / "CANDIDATE-ONLY.txt").read_text()
    source_manifest = overlay / "SOURCE-MANIFEST.json"
    if ("openssl_alpine_release=3.5.9-r2" not in marker
            or not source_manifest.is_file()
            or f"source_manifest_sha256={sha256(source_manifest)}" not in marker):
        raise ValueError("D16 r2 source provenance missing or mismatched")

    shutil.copytree(root, output, symlinks=True)
    installed = {}
    for soname in ("libcrypto.so.3", "libssl.so.3", "libnettle.so.8",
                   "libhogweed.so.6", "libgcrypt.so.20"):
        installed[soname] = {
            "sha256": sha256(replacements[soname]),
            "target": copy_into_aliased_library(output, soname, replacements[soname]),
        }
    module = output / "usr/lib/ossl-modules/legacy.so"
    module.parent.mkdir(parents=True, exist_ok=True)
    if module.is_symlink():
        raise ValueError("OpenSSL legacy module destination is a symlink")
    shutil.copyfile(replacements["ossl-modules/legacy.so"], module)
    installed["ossl-modules/legacy.so"] = {
        "sha256": sha256(module),
        "target": module.relative_to(output).as_posix(),
    }

    candidate_dir = output / "candidate"
    candidate_dir.mkdir(exist_ok=True)
    evidence = {
        "schema": "nuraloumi-alpine-wpe-d16-assembly-v1",
        "qualification": "UNQUALIFIED",
        "device_deployment": "FORBIDDEN",
        "producer_source_sha": source_sha,
        "producer_tar_sha256": producer_hash,
        "crypto_source_manifest_sha256": sha256(source_manifest),
        "crypto_replacements": installed,
        "producer_wpe_sha256": wpe_hashes,
        "unproven": [
            "complete_isa_dlopen_gpu_llvm", "networkprocess_seccomp_negative_controls",
            "native_wayland_pixels_touch_rollback_review",
        ],
    }
    (candidate_dir / "PROVENANCE.json").write_text(
        json.dumps(evidence, indent=2, sort_keys=True) + "\n")
    (candidate_dir / "CANDIDATE-ONLY.txt").write_text(
        "runtime_qualification=UNQUALIFIED\n"
        "device_deployment=FORBIDDEN\n"
        f"source_sha={source_sha}\n"
        f"producer_sha256={producer_hash}\n"
        f"crypto_source_manifest_sha256={sha256(source_manifest)}\n"
    )
    symlinks = [
        {"path": path.relative_to(output).as_posix(),
         "target": str(path.readlink())}
        for path in sorted(output.rglob("*")) if path.is_symlink()
    ]
    (candidate_dir / "ROOTFS-SYMLINKS.json").write_text(
        json.dumps(symlinks, indent=2, sort_keys=True) + "\n")
    files = sorted(path for path in output.rglob("*") if path.is_file()
                   and not path.is_symlink()
                   and path != candidate_dir / "ROOTFS-SHA256SUMS.txt")
    (candidate_dir / "ROOTFS-SHA256SUMS.txt").write_text(
        "".join(f"{sha256(path)}  ./{path.relative_to(output).as_posix()}\n"
                for path in files))
    return evidence


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("alpine_root", type=Path)
    parser.add_argument("producer_directory", type=Path)
    parser.add_argument("d16_overlay_directory", type=Path)
    parser.add_argument("fresh_output", type=Path)
    args = parser.parse_args()
    try:
        evidence = assemble(args.alpine_root, args.producer_directory,
                            args.d16_overlay_directory, args.fresh_output)
    except (OSError, ValueError, tarfile.TarError) as exc:
        parser.exit(1, f"D16_ALPINE_ASSEMBLY=FAIL: {exc}\n")
    print("D16_ALPINE_ASSEMBLY=CANDIDATE_ONLY "
          f"source_sha={evidence['producer_source_sha']} "
          f"replacements={len(evidence['crypto_replacements'])}")


if __name__ == "__main__":
    main()
