"""Non-destructive fixture tests for WPE crypto overlay assembly."""

import hashlib
import importlib.util
from pathlib import Path
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("assemble_nurabrowser_wpe_d16_candidate.py")
spec = importlib.util.spec_from_file_location("wpe_crypto_assembly", SCRIPT)
assembly = importlib.util.module_from_spec(spec)
spec.loader.exec_module(assembly)


class CryptoAssemblyTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.original = self.root / "original"
        self.overlay = self.root / "overlay"
        self.output = self.root / "assembled"
        self.original.mkdir()
        (self.original / "CANDIDATE-ONLY.txt").write_text(
            "schema=nurabrowser-wpe-musl-candidate-v1\n"
            + "source_sha=" + "a" * 40 + "\n"
            + "runtime_qualification=UNQUALIFIED\n"
            + "device_deployment=FORBIDDEN\n"
        )
        binary = self.original / "browser/libexec/WPEWebProcess"
        binary.parent.mkdir(parents=True)
        binary.write_bytes(b"old WPE process")
        lib = self.original / "lib/libcrypto.so.3"
        lib.parent.mkdir(parents=True)
        lib.write_bytes(b"old crypto")
        (self.original / "SHA256SUMS").write_text("stale checksums\n")
        (self.overlay / "replacements").mkdir(parents=True)
        lines = []
        for soname in assembly.PAIRS:
            payload = ("new " + soname).encode()
            file = self.overlay / "replacements" / soname
            file.write_bytes(payload)
            lines.append(hashlib.sha256(payload).hexdigest() + "  ./" + soname + "\n")
        (self.overlay / "REPLACEMENTS-SHA256SUMS.txt").write_text("".join(lines))
        (self.overlay / "CANDIDATE-ONLY.txt").write_text(
            "qualification=UNQUALIFIED\nnormal_deployment=FORBIDDEN\n")

    def test_preserves_original_and_keeps_release_gate_closed(self):
        result = assembly.assemble(self.original, self.overlay, self.output)
        self.assertEqual(result["qualification"], "UNQUALIFIED")
        self.assertEqual((self.original / "lib/libcrypto.so.3").read_bytes(), b"old crypto")
        self.assertEqual((self.output / "lib/libcrypto.so.3").read_bytes(), b"new libcrypto.so.3")
        self.assertEqual((self.output / "browser/libexec/WPEWebProcess").read_bytes(), b"old WPE process")
        self.assertNotIn("runtime.json", {p.name for p in self.output.rglob("*")})
        self.assertIn("device_deployment=FORBIDDEN",
                      (self.output / "CANDIDATE-ONLY.txt").read_text())
        self.assertIn("CRYPTO-PROVENANCE.json", (self.output / "SHA256SUMS").read_text())

    def test_rejects_tampered_replacement(self):
        (self.overlay / "replacements/libcrypto.so.3").write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "hash mismatch"):
            assembly.assemble(self.original, self.overlay, self.output)
        self.assertFalse(self.output.exists())

    def test_missing_abi_pair_rejects(self):
        (self.overlay / "replacements/libssl.so.3").unlink()
        with self.assertRaisesRegex(ValueError, "missing or symlink"):
            assembly.assemble(self.original, self.overlay, self.output)
        self.assertFalse(self.output.exists())

    def test_never_replaces_existing_output(self):
        self.output.mkdir()
        with self.assertRaisesRegex(ValueError, "already exists"):
            assembly.assemble(self.original, self.overlay, self.output)

    def test_rejects_noncanonical_inventory_path(self):
        contents = (self.overlay / "REPLACEMENTS-SHA256SUMS.txt").read_text()
        (self.overlay / "REPLACEMENTS-SHA256SUMS.txt").write_text(
            contents.replace("./libssl.so.3", "./x/../libssl.so.3"))
        with self.assertRaisesRegex(ValueError, "unsafe replacement path"):
            assembly.assemble(self.original, self.overlay, self.output)

    def test_rejects_invalid_source_sha(self):
        marker = self.original / "CANDIDATE-ONLY.txt"
        marker.write_text(marker.read_text().replace("a" * 40, "z" * 40))
        with self.assertRaisesRegex(ValueError, "missing original source commit"):
            assembly.assemble(self.original, self.overlay, self.output)


if __name__ == "__main__":
    unittest.main()
