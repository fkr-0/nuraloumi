import hashlib
import tempfile
import unittest
from pathlib import Path

from scripts.probe_nurabrowser_alpine_loader import (
    ProbeError, crypto_issues, parse_loader_output, private_file,
    producer_identity,
)


class PrivateLoaderPreflightTests(unittest.TestCase):
    def test_producer_requires_exact_checksum_and_unqualified_marker(self):
        with tempfile.TemporaryDirectory() as temporary:
            producer = Path(temporary)
            tarball = producer / "nurabrowser-wpe-musl-candidate.tar"
            tarball.write_bytes(b"fixture candidate")
            checksum = hashlib.sha256(tarball.read_bytes()).hexdigest()
            (producer / "SHA256SUMS.txt").write_text(
                f"{checksum}  /out/{tarball.name}\n")
            marker = producer / "CANDIDATE-ONLY.txt"
            marker.write_text(
                "source_sha=" + "a" * 40 + "\n"
                "upstream_wpewebkit=2.54.2\n"
                "target=armv7-unknown-linux-musleabihf\n"
                "runtime_qualification=UNQUALIFIED\n"
                "device_deployment=FORBIDDEN\n")
            self.assertEqual(producer_identity(producer), ("a" * 40, checksum))
            tarball.write_bytes(b"modified")
            with self.assertRaisesRegex(ProbeError, "checksum mismatch"):
                producer_identity(producer)
            tarball.write_bytes(b"fixture candidate")
            marker.write_text(marker.read_text().replace(
                "runtime_qualification=UNQUALIFIED", "runtime_qualification=PASS"))
            with self.assertRaisesRegex(ProbeError, "promotion flags"):
                producer_identity(producer)

    def test_loader_rejects_host_fallback_and_escape(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "lib").mkdir()
            (root / "lib/ld-musl-armhf.so.1").write_bytes(b"private")
            with self.assertRaisesRegex(ProbeError, "missing"):
                private_file(root, "/usr/lib/libmissing.so")
            with self.assertRaisesRegex(ProbeError, "unsafe"):
                private_file(root, "/../etc/passwd")
            (root / "lib/outside").symlink_to("/etc/passwd")
            with self.assertRaisesRegex(ProbeError, "outside"):
                private_file(root, "/lib/outside")

    def test_loader_list_requires_private_paths_and_interpreter(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "lib").mkdir()
            (root / "usr/lib").mkdir(parents=True)
            for name in ("lib/ld-musl-armhf.so.1", "usr/lib/libWPE.so",
                         "usr/lib/libc.musl-armv7.so.1"):
                (root / name).write_bytes(b"elf")
            listing = (
                "\t/lib/ld-musl-armhf.so.1 (0x400000)\n"
                "\tlibWPE.so => /usr/lib/libWPE.so (0x500000)\n"
                "\tlibc.musl-armv7.so.1 => /usr/lib/libc.musl-armv7.so.1 (0x600000)\n"
            )
            self.assertEqual(len(parse_loader_output(listing, root)), 3)
            with self.assertRaisesRegex(ProbeError, "unrecognized"):
                parse_loader_output(listing + "Error loading shared library\n", root)
            with self.assertRaisesRegex(ProbeError, "missing"):
                parse_loader_output(listing.replace("libWPE.so", "libABSENT.so"), root)
            with self.assertRaisesRegex(ProbeError, "incomplete"):
                parse_loader_output(listing.replace(
                    "\t/lib/ld-musl-armhf.so.1 (0x400000)\n", ""), root)

    def test_crypto_attribute_gate_rejects_neon_and_non_d16(self):
        good = ("Tag_FP_arch: VFPv3-D16\n"
                "Tag_ABI_VFP_args: VFP registers\n")
        self.assertEqual(crypto_issues("libssl.so.3", good), [])
        issues = crypto_issues("libcrypto.so.3", good.replace("VFPv3-D16", "VFPv3")
                               + "Tag_Advanced_SIMD_arch: NEONv1\n")
        self.assertIn("libcrypto.so.3: ISA_NOT_VFPV3_D16", issues)
        self.assertIn("libcrypto.so.3: ISA_NEON_ATTRIBUTE", issues)


if __name__ == "__main__":
    unittest.main()
