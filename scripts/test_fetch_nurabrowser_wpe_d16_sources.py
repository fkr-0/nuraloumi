import hashlib
import io
import tempfile
import unittest
from pathlib import Path

from scripts.fetch_nurabrowser_wpe_d16_sources import (
    APORTS_REVISION, SOURCES, fetch_sources,
)


class PinnedD16SourcesTests(unittest.TestCase):
    def test_sources_pin_r2_and_each_unique_file(self):
        names = [entry[0] for entry in SOURCES]
        self.assertEqual(len(names), len(set(names)))
        self.assertEqual(len(names), 12)
        self.assertRegex(APORTS_REVISION, r"^[0-9a-f]{40}$")
        self.assertIn("CVE-2026-107493.patch", names)
        self.assertIn("CVE-2026-124709.patch", names)

    def test_source_digest_manifest_matches_the_real_builder(self):
        builder = Path(__file__).with_name(
            "rebuild_nurabrowser_wpe_d16_crypto.sh").read_text()
        for name, _, digest in SOURCES:
            with self.subTest(name=name):
                self.assertIn(f"check_source {name} {digest}", builder)

    def test_fresh_output_only_and_mismatch_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "sources"
            expected = hashlib.sha512(b"source").hexdigest()

            def open_fixture(request, timeout):
                self.assertEqual(timeout, 90)
                return io.BytesIO(b"source")

            result = fetch_sources(output,
                                   (("sample.tar.gz", "https://example.org/source", expected),),
                                   open_fixture)
            self.assertEqual(result["sources"][0]["bytes"], 6)
            self.assertTrue((output / "SOURCE-MANIFEST.json").is_file())
            with self.assertRaisesRegex(ValueError, "fresh"):
                fetch_sources(output, (), open_fixture)
            bad_output = Path(directory) / "bad"
            with self.assertRaisesRegex(ValueError, "SHA-512 mismatch"):
                fetch_sources(bad_output,
                              (("sample.tar.gz", "https://example.org/source", "0" * 128),),
                              open_fixture)
            self.assertFalse((bad_output / "sample.tar.gz").exists())


if __name__ == "__main__":
    unittest.main()
