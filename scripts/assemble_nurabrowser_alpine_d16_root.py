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
import re
import shutil
import subprocess
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
# Private Mesa 26.2.4 software-rendering closure, independently SHA-256
# verified from Nura 192.168.23.106 (2026-11-26). These are binary
# provenance pins, NOT a reproducible source build or deployment approval.
GRAPHICS_FROM_NURA = {
    "libEGL.so.1.0.0": "5e8870bc32cae165dc1a619be92d2a654d8dde15040e254613d2706dc7a0191b",
    "libGLESv2.so.2.0.0": "8203bd3ed741e1cab850b5b6e2f1184be177704b5046e545684499d4b6a895ab",
    "libgbm.so.1.0.0": "b55be24bb59d6c9f820e7d1595df14b5fc5cf54f90705e5259514365c8bb664b",
    "libgallium-26.2.4.so": "09e03bea18be4120a6d22dde9887e4a452dcd488df6933c5cf46a4c15ec8e87b",
    "libLLVM.so.23.1": "99e1add0a47799197eef0d5a70c714310d2301687d69670525f51c19976ac255",
    "libdril_dri.so": "9dd10daf27360d5cb7928da5d5191250e00150a2c998897fc4c2d0111ce64820",
    "libGLESv1_CM.so.1.1.0": "5819317c16229b2082c153e52f60835b2d793caccdfe640d8e60248a3996fb2c",
    "libwayland-client.so.0.26.0": "4b8fa24d5b31b3be7444f3683947f71f8d68823ea509184dd5e4958429f6c3a3",
}
GRAPHICS_ALIASES = {
    "libEGL.so.1.0.0": "libEGL.so.1",
    "libGLESv2.so.2.0.0": "libGLESv2.so.2",
    "libgbm.so.1.0.0": "libgbm.so.1",
    "libGLESv1_CM.so.1.1.0": "libGLESv1_CM.so.1",
}
GRAPHICS_DIRECT = {
    "libgallium-26.2.4.so": "usr/lib/libgallium-26.2.4.so",
    "libLLVM.so.23.1": "usr/lib/libLLVM.so.23.1",
    "libdril_dri.so": "usr/lib/dri/libdril_dri.so",
}
NEON_ATTRIBUTE = re.compile(r"NEON|Advanced_SIMD", re.IGNORECASE)


def find_neon_elf_objects(root):
    """Conservatively record any remaining NEON-tagged ELF in a private root.

    Stripped ELF may lack complete attributes; absence is NOT an ISA PASS.
    Symlink aliases are omitted because their regular target is scanned.
    """
    findings = []
    for path in sorted(root.rglob("*")):
        if path.is_symlink() or not path.is_file():
            continue
        with path.open("rb") as stream:
            if stream.read(4) != b"\x7fELF":
                continue
        result = subprocess.run(
            ["readelf", "-A", str(path)], check=True, capture_output=True,
            text=True, timeout=20,
        )
        if NEON_ATTRIBUTE.search(result.stdout):
            findings.append(path.relative_to(root).as_posix())
    return findings


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


def verified_graphics(graphics):
    """Reject altered, incomplete, non-D16 or symlinked private graphics."""
    if not graphics.is_dir() or graphics.is_symlink():
        raise ValueError("graphics overlay must be a real directory")
    verified = {}
    for filename, digest in GRAPHICS_FROM_NURA.items():
        candidate = graphics / filename
        if not candidate.is_file() or candidate.is_symlink():
            raise ValueError(f"missing regular D16 graphics file: {filename}")
        if sha256(candidate) != digest:
            raise ValueError(f"D16 graphics SHA256 mismatch: {filename}")
        with candidate.open("rb") as stream:
            if stream.read(4) != b"\x7fELF":
                raise ValueError(f"D16 graphics file is not ELF: {filename}")
        attributes = subprocess.run(
            ["readelf", "-A", str(candidate)], check=True, capture_output=True,
            text=True, timeout=20,
        ).stdout
        if (NEON_ATTRIBUTE.search(attributes)
                or "Tag_FP_arch: VFPv3-D16" not in attributes
                or "Tag_ABI_VFP_args: VFP registers" not in attributes):
            raise ValueError(f"graphics ELF not proved D16 hardfloat: {filename}")
        verified[filename] = candidate
    return verified


def install_graphics_in_fresh_root(output, verified):
    """Replace mutually coupled Mesa objects only in the just-copied root."""
    installed = {}
    for name, soname in GRAPHICS_ALIASES.items():
        target = copy_into_aliased_library(output, soname, verified[name])
        installed[name] = {"sha256": sha256(verified[name]), "target": target}
    # Mesa 26 EGL uses Wayland symbols absent from Alpine 3.22's 0.23.1
    # client (wl_fixes_interface and wl_display_dispatch_queue_timeout).
    # Install the pinned Nura 0.26.0 object under its truthful filename.
    library_dir = output / "usr/lib"
    wayland_alias = library_dir / "libwayland-client.so.0"
    if not wayland_alias.is_symlink():
        raise ValueError("missing original Wayland SONAME alias")
    old_wayland = wayland_alias.resolve()
    if (not old_wayland.is_relative_to(library_dir.resolve())
            or old_wayland.name != "libwayland-client.so.0.23.1"
            or not old_wayland.is_file()):
        raise ValueError("unexpected old Wayland client version")
    new_wayland = library_dir / "libwayland-client.so.0.26.0"
    if new_wayland.exists() or new_wayland.is_symlink():
        raise ValueError("unexpected new Wayland client destination")
    shutil.copyfile(verified["libwayland-client.so.0.26.0"], new_wayland)
    wayland_alias.unlink()
    wayland_alias.symlink_to(new_wayland.name)
    old_wayland.unlink()
    installed["libwayland-client.so.0.26.0"] = {
        "sha256": sha256(new_wayland),
        "target": new_wayland.relative_to(output).as_posix(),
    }
    for name, relative in GRAPHICS_DIRECT.items():
        target = output / relative
        if target.is_symlink() or not target.parent.is_dir():
            raise ValueError(f"unsafe graphics destination: {relative}")
        if name != "libdril_dri.so" and target.exists():
            raise ValueError(f"unexpected prior graphics version: {relative}")
        if name == "libdril_dri.so" and not target.is_file():
            raise ValueError(f"expected DRI loader absent: {relative}")
        shutil.copyfile(verified[name], target)
        installed[name] = {"sha256": sha256(target), "target": relative}
    # Mesa 25.1.9 plugins cannot be mixed with the Mesa 26.2.4 Gallium ABI.
    for old_relative in ("usr/lib/libgallium-25.1.9.so",
                         "usr/lib/gbm/dri_gbm.so"):
        old = output / old_relative
        if old.is_symlink() or not old.is_file():
            raise ValueError(f"unexpected legacy Mesa object: {old_relative}")
        old.unlink()
    old_pipes = output / "usr/lib/gallium-pipe"
    if old_pipes.is_symlink() or not old_pipes.is_dir():
        raise ValueError("expected old gallium-pipe directory absent")
    shutil.rmtree(old_pipes)
    # Reject other DT_NEEDED or dlopen strings accidentally retaining Mesa 25.
    legacy_refs = []
    for path in output.rglob("*"):
        if not path.is_file() or path.is_symlink():
            continue
        with path.open("rb") as stream:
            if stream.read(4) == b"\x7fELF" and b"libgallium-25.1.9.so" in (
                    stream.read()):
                legacy_refs.append(path.relative_to(output).as_posix())
    if legacy_refs:
        raise ValueError(f"old Gallium references remain: {legacy_refs}")
    return installed


def assemble(root, producer, overlay, output, graphics=None):
    inputs = (root, producer, overlay) + ((graphics,) if graphics else ())
    if (output.exists() or output.is_symlink()
            or any(not item.is_dir() or item.is_symlink() for item in inputs)):
        raise ValueError("all inputs must exist and output must be fresh")
    output_absolute = output.absolute()
    if any(output_absolute.is_relative_to(item.resolve()) for item in inputs):
        raise ValueError("output must not be inside an input tree")
    root, producer, overlay = (item.resolve(strict=True) for item in inputs[:3])
    output = output_absolute
    source_sha, producer_hash = producer_identity(producer)
    if (root / "runtime.json").exists() or (
            root / "usr/libexec/nuraloumi/browser/runtime.json").exists():
        raise ValueError("Alpine candidate cannot contain runtime.json")
    wpe_hashes = check_producer_bytes(producer, root)
    replacements = verified_replacements(overlay)
    verified_mesa = verified_graphics(graphics.resolve()) if graphics else None
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

    installed_graphics = (install_graphics_in_fresh_root(output, verified_mesa)
                          if verified_mesa is not None else {})

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
        "graphics_replacements": installed_graphics,
        "graphics_lineage": ("nura-private-mesa-26.2.4-d16-llvm23-20261126"
                             if installed_graphics else None),
        "producer_wpe_sha256": wpe_hashes,
        "blocking_neon_elf_attributes": find_neon_elf_objects(output),
        "unproven": [
            "complete_isa_dlopen_gpu_llvm", "networkprocess_seccomp_negative_controls",
            "mesa26_llvm23_source_rebuild_and_dlopen_negative_controls",
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
    parser.add_argument("--graphics-overlay", type=Path,
                        help="optional eight-file, hash-pinned Nura Mesa26/LLVM23/Wayland D16 closure")
    args = parser.parse_args()
    try:
        evidence = assemble(args.alpine_root, args.producer_directory,
                            args.d16_overlay_directory, args.fresh_output,
                            graphics=args.graphics_overlay)
    except (OSError, ValueError, tarfile.TarError,
            subprocess.SubprocessError) as exc:
        parser.exit(1, f"D16_ALPINE_ASSEMBLY=FAIL: {exc}\n")
    print("D16_ALPINE_ASSEMBLY=CANDIDATE_ONLY "
          f"source_sha={evidence['producer_source_sha']} "
          f"replacements={len(evidence['crypto_replacements'])} "
          f"graphics={len(evidence['graphics_replacements'])}")


if __name__ == "__main__":
    main()
