import hashlib
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest

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


if __name__ == "__main__":
    unittest.main()
