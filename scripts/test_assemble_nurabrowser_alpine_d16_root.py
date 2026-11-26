import hashlib
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

from scripts.assemble_nurabrowser_alpine_d16_root import (
    WPE_FROM_PRODUCER, assemble,
)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


class AlpineRootD16AssemblyTests(unittest.TestCase):
    def fixture(self, base):
        root = base / "root"
        root.mkdir()
        for name in WPE_FROM_PRODUCER:
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(("WPE:" + name).encode())
        library_dir = root / "usr/lib"
        names = ("libcrypto.so.3", "libssl.so.3", "libnettle.so.8",
                 "libhogweed.so.6", "libgcrypt.so.20")
        for name in names:
            versioned = library_dir / (name + ".old")
            versioned.write_bytes(b"old")
            (library_dir / name).symlink_to(versioned.name)
        producer = base / "producer"
        producer.mkdir()
        tarball = producer / "nurabrowser-wpe-musl-candidate.tar"
        with tarfile.open(tarball, "w") as archive:
            for name in WPE_FROM_PRODUCER:
                payload = (root / name).read_bytes()
                info = tarfile.TarInfo("stage/" + name)
                info.size = len(payload)
                archive.addfile(info, io.BytesIO(payload))
        (producer / "SHA256SUMS.txt").write_text(
            f"{digest(tarball)}  /out/{tarball.name}\n")
        (producer / "CANDIDATE-ONLY.txt").write_text(
            "source_sha=" + "a" * 40 + "\n"
            "upstream_wpewebkit=2.54.2\n"
            "target=armv7-unknown-linux-musleabihf\n"
            "runtime_qualification=UNQUALIFIED\n"
            "device_deployment=FORBIDDEN\n")
        overlay = base / "overlay"
        replacement_dir = overlay / "replacements"
        (replacement_dir / "ossl-modules").mkdir(parents=True)
        for name in (*names, "ossl-modules/legacy.so"):
            candidate = replacement_dir / name
            candidate.write_bytes(("D16:" + name).encode())
        (overlay / "REPLACEMENTS-SHA256SUMS.txt").write_text(
            "".join(f"{digest(path)}  ./{path.relative_to(replacement_dir)}\n"
                    for path in sorted(replacement_dir.rglob("*")) if path.is_file()))
        source_manifest = overlay / "SOURCE-MANIFEST.json"
        source_manifest.write_text(json.dumps({"source": "fixture"}))
        (overlay / "CANDIDATE-ONLY.txt").write_text(
            "normal_deployment=FORBIDDEN\n"
            "openssl_alpine_release=3.5.9-r2\n"
            f"source_manifest_sha256={digest(source_manifest)}\n")
        return root, producer, overlay

    def test_fresh_root_preserves_aliases_and_original_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            root, producer, overlay = self.fixture(base)
            output = base / "new-root"
            evidence = assemble(root, producer, overlay, output)
            self.assertEqual(evidence["qualification"], "UNQUALIFIED")
            self.assertEqual(evidence["device_deployment"], "FORBIDDEN")
            self.assertFalse((output / "runtime.json").exists())
            self.assertEqual((root / "usr/lib/libnettle.so.8").read_bytes(), b"old")
            self.assertTrue((output / "usr/lib/libnettle.so.8").is_symlink())
            self.assertEqual((output / "usr/lib/libnettle.so.8").read_bytes(),
                             b"D16:libnettle.so.8")
            self.assertTrue((output / "candidate/PROVENANCE.json").is_file())
            self.assertIn("libnettle.so.8", (
                output / "candidate/ROOTFS-SYMLINKS.json").read_text())
            with self.assertRaisesRegex(ValueError, "fresh"):
                assemble(root, producer, overlay, output)

    def test_source_mismatch_fails_before_any_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            root, producer, overlay = self.fixture(base)
            (root / WPE_FROM_PRODUCER[0]).write_bytes(b"wrong source")
            output = base / "must-not-exist"
            with self.assertRaisesRegex(ValueError, "mismatches WPE producer"):
                assemble(root, producer, overlay, output)
            self.assertFalse(output.exists())

    def graphics_fixture(self, root, base):
        from scripts.assemble_nurabrowser_alpine_d16_root import (
            GRAPHICS_ALIASES, GRAPHICS_FROM_NURA,
        )
        import scripts.assemble_nurabrowser_alpine_d16_root as module

        library_dir = root / "usr/lib"
        for name, soname in GRAPHICS_ALIASES.items():
            target = library_dir / name
            target.write_bytes(b"old mesa")
            (library_dir / soname).symlink_to(target.name)
        (library_dir / "libgallium-25.1.9.so").write_bytes(b"old gallium")
        (library_dir / "libwayland-client.so.0.23.1").write_bytes(b"old wayland")
        (library_dir / "libwayland-client.so.0").symlink_to(
            "libwayland-client.so.0.23.1")
        (library_dir / "gbm").mkdir()
        (library_dir / "gbm/dri_gbm.so").write_bytes(b"old plugin")
        (library_dir / "dri").mkdir()
        (library_dir / "dri/libdril_dri.so").write_bytes(b"old dri")
        (library_dir / "gallium-pipe").mkdir()
        (library_dir / "gallium-pipe/pipe_kmsro.so").write_bytes(
            b"\x7fELF old neon plugin")
        graphics = base / "graphics"
        graphics.mkdir()
        pinned = {}
        for name in GRAPHICS_FROM_NURA:
            candidate = graphics / name
            candidate.write_bytes(b"\x7fELF D16:" + name.encode())
            pinned[name] = digest(candidate)
        return graphics, patch.object(module, "GRAPHICS_FROM_NURA", pinned)

    def test_pinned_mesa26_replaces_old_mesa_only_in_fresh_root(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            root, producer, overlay = self.fixture(base)
            graphics, pins = self.graphics_fixture(root, base)
            output = base / "mesa-root"

            class D16Readelf:
                stdout = ("Tag_CPU_arch: v7\\n"
                          "Tag_FP_arch: VFPv3-D16\\n"
                          "Tag_ABI_VFP_args: VFP registers\\n")

            with pins, patch(
                "scripts.assemble_nurabrowser_alpine_d16_root.subprocess.run",
                return_value=D16Readelf(),
            ):
                result = assemble(root, producer, overlay, output,
                                  graphics=graphics)
            self.assertEqual(len(result["graphics_replacements"]), 8)
            self.assertEqual(result["blocking_neon_elf_attributes"], [])
            self.assertFalse((output / "usr/lib/libgallium-25.1.9.so").exists())
            self.assertFalse((output / "usr/lib/gallium-pipe").exists())
            self.assertFalse((output / "usr/lib/gbm/dri_gbm.so").exists())
            self.assertFalse((output / "usr/lib/libwayland-client.so.0.23.1").exists())
            self.assertEqual(
                (output / "usr/lib/libwayland-client.so.0").readlink().name,
                "libwayland-client.so.0.26.0")
            self.assertTrue((output / "usr/lib/libEGL.so.1").is_symlink())
            self.assertEqual(
                digest(output / "usr/lib/libEGL.so.1"),
                digest(graphics / "libEGL.so.1.0.0"))
            self.assertEqual(
                (root / "usr/lib/libEGL.so.1.0.0").read_bytes(), b"old mesa")
            self.assertEqual(
                json.loads((output / "candidate/PROVENANCE.json").read_text())
                ["graphics_lineage"],
                "nura-private-mesa-26.2.4-d16-llvm23-20261126")

    def test_graphics_tamper_fails_before_output_created(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            root, producer, overlay = self.fixture(base)
            graphics, pins = self.graphics_fixture(root, base)
            (graphics / "libEGL.so.1.0.0").write_bytes(b"altered")
            output = base / "must-not-exist"
            with pins, self.assertRaisesRegex(ValueError,
                                              "graphics SHA256 mismatch"):
                assemble(root, producer, overlay, output, graphics=graphics)
            self.assertFalse(output.exists())

    def test_graphics_neon_attribute_fails_before_output_created(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            root, producer, overlay = self.fixture(base)
            graphics, pins = self.graphics_fixture(root, base)
            output = base / "must-not-exist"

            class NeonReadelf:
                stdout = ("Tag_FP_arch: VFPv3-D16\\n"
                          "Tag_ABI_VFP_args: VFP registers\\n"
                          "Tag_Advanced_SIMD_arch: NEONv1\\n")

            with pins, patch(
                "scripts.assemble_nurabrowser_alpine_d16_root.subprocess.run",
                return_value=NeonReadelf(),
            ), self.assertRaisesRegex(ValueError, "not proved D16"):
                assemble(root, producer, overlay, output, graphics=graphics)
            self.assertFalse(output.exists())

    def test_remaining_neon_elf_is_recorded_as_release_blocker(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            root, producer, overlay = self.fixture(base)
            (root / "usr/lib/libunqualified.so").write_bytes(b"\x7fELF" + b"test")
            output = base / "new-root"

            class ReadelfResult:
                stdout = "Tag_Advanced_SIMD_arch: NEONv1\n"

            with patch("scripts.assemble_nurabrowser_alpine_d16_root.subprocess.run",
                       return_value=ReadelfResult()) as reader:
                evidence = assemble(root, producer, overlay, output)
            self.assertEqual(evidence["blocking_neon_elf_attributes"],
                             ["usr/lib/libunqualified.so"])
            self.assertEqual(reader.call_count, 1)
            self.assertEqual(evidence["qualification"], "UNQUALIFIED")


if __name__ == "__main__":
    unittest.main()
