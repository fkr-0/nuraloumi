"""Fast host-only tests; never report a mock ELF as device qualification."""

import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).with_name("audit_nurabrowser_wpe_candidate.py")
spec = importlib.util.spec_from_file_location("wpe_preflight", SCRIPT)
preflight = importlib.util.module_from_spec(spec)
spec.loader.exec_module(preflight)


class CandidatePreflightTests(unittest.TestCase):
    def test_d16_attributes_only(self):
        good = "Tag_FP_arch: VFPv3-D16\nTag_ABI_VFP_args: VFP registers\n"
        self.assertEqual(preflight.attribute_findings(good), [])
        self.assertIn("ISA_NOT_VFPV3_D16", preflight.attribute_findings(
            good.replace("VFPv3-D16", "VFPv3")))
        self.assertIn("ISA_NEON_ATTRIBUTE", preflight.attribute_findings(
            good + "Tag_Advanced_SIMD_arch: NEONv1\n"))

    def test_candidate_marker_pins_original_sha_and_unqualified_state(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            marker = root / "CANDIDATE-ONLY.txt"
            marker.write_text("source_sha=" + "a" * 40 +
                              "\ntarget=armv7-unknown-linux-musleabihf\n"
                              "runtime_qualification=UNQUALIFIED\n"
                              "device_deployment=FORBIDDEN\n")
            self.assertEqual(preflight.marker_source(root), "a" * 40)
            marker.write_text(marker.read_text().replace("UNQUALIFIED", "PASS"))
            with self.assertRaisesRegex(ValueError, "qualification"):
                preflight.marker_source(root)

    def test_no_pass_is_inferred_from_complete_synthetic_inventory(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "CANDIDATE-ONLY.txt").write_text(
                "source_sha=" + "a" * 40 +
                "\ntarget=armv7-unknown-linux-musleabihf\n"
                "runtime_qualification=UNQUALIFIED\n"
                "device_deployment=FORBIDDEN\n")
            for name in (*preflight.REQUIRED, "resources/icu/data.dat", "modules/gio/mock.so"):
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"synthetic resource")
            with patch.object(preflight, "source_tree", return_value={"build_tree": "x", "release_tree": "x"}):
                report = preflight.audit(root, root, "b" * 40)
            self.assertEqual(report["qualification"], "UNQUALIFIED")
            self.assertIn("full_decoded_instruction_closure", report["unproven"])
            self.assertEqual(report["elf_count"], 0)

    def test_missing_closure_components_fails_closed(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "CANDIDATE-ONLY.txt").write_text(
                "source_sha=" + "a" * 40 +
                "\ntarget=armv7-unknown-linux-musleabihf\n"
                "runtime_qualification=UNQUALIFIED\n"
                "device_deployment=FORBIDDEN\n")
            with patch.object(preflight, "source_tree", return_value={"build_tree": "x", "release_tree": "y"}):
                report = preflight.audit(root, root, "b" * 40)
            self.assertTrue(any(x["code"] == "SOURCE_TREE_MISMATCH" for x in report["issues"]))
            self.assertTrue(any(x["code"] == "REQUIRED_FILE_MISSING" for x in report["issues"]))

    def test_missing_needed_library_fails_closed(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "CANDIDATE-ONLY.txt").write_text(
                "source_sha=" + "a" * 40 +
                "\ntarget=armv7-unknown-linux-musleabihf\n"
                "runtime_qualification=UNQUALIFIED\n"
                "device_deployment=FORBIDDEN\n")
            executable = root / "browser/nurabrowser-core"
            executable.parent.mkdir(parents=True)
            executable.write_bytes(b"\x7fELFfake header")
            with patch.object(preflight, "source_tree", return_value={"build_tree": "x", "release_tree": "x"}), \
                 patch.object(preflight, "inspect_elf", return_value=([], ["libmissing.so.1"])):
                report = preflight.audit(root, root, "b" * 40)
            self.assertTrue(any(x["code"] == "DT_NEEDED_NOT_BUNDLED" and
                                x.get("library") == "libmissing.so.1" for x in report["issues"]))


if __name__ == "__main__":
    unittest.main()
