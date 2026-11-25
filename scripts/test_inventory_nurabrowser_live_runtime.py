"""Tests for the read-only WPE live runtime closure inventory."""

from pathlib import Path
import tempfile
import unittest

try:
    from scripts import inventory_nurabrowser_live_runtime as audit
except ModuleNotFoundError:
    import inventory_nurabrowser_live_runtime as audit


class LiveClosureTests(unittest.TestCase):
    def test_parse_dynamic(self):
        text = ("0x00000001 (NEEDED) Shared library: [libicuuc.so.76]\n"
                "0x0000000e (SONAME) Library soname: [libsample.so.1]\n"
                "0x0000001d (RUNPATH) Library runpath: [$ORIGIN/lib]\n")
        self.assertEqual(audit.parse_dynamic(text),
                         {"needed": ["libicuuc.so.76"],
                          "soname": "libsample.so.1",
                          "runpath": ["$ORIGIN/lib"]})

    def test_marker_denies_promotion(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = "7e18074746b395f8245c312736063d0bd38324e5"
            marker = root / "CANDIDATE-ONLY.txt"
            marker.write_text(
                f"source_sha={source}\n"
                "target=armv7-unknown-linux-musleabihf\n"
                "upstream_wpewebkit=2.54.2\n"
                "runtime_qualification=UNQUALIFIED\n"
                "device_deployment=FORBIDDEN\n"
            )
            self.assertEqual(audit.marker_source(root), source)
            marker.write_text(marker.read_text().replace("UNQUALIFIED", "PASS"))
            with self.assertRaisesRegex(ValueError, "promotion flags"):
                audit.marker_source(root)

    def test_soname_alias_not_silently_satisfied(self):
        entries = {
            "browser/nurabrowser-core": {"needed": ["libEGL.so.1"],
                                         "soname": None, "issues": []},
            "mesa-d16-gnu-tls-candidate-20261124/libEGL.so.1.0.0":
                {"needed": [], "soname": "libEGL.so.1", "issues": []},
        }
        reached, edges, issues = audit.resolve_graph(
            entries, ["browser/nurabrowser-core"])
        self.assertEqual(len(reached), 2)
        self.assertEqual(len(edges), 1)
        self.assertEqual(issues[0]["code"], "SONAME_ALIAS_NOT_MATERIALIZED")

    def test_missing_private_needed_is_blocker(self):
        entries = {"browser/nurabrowser-core": {
            "needed": ["libhostonly.so.7"], "soname": None, "issues": []}}
        _, _, issues = audit.resolve_graph(
            entries, ["browser/nurabrowser-core"])
        self.assertEqual(issues[0]["code"], "DT_NEEDED_NOT_PRIVATELY_RESOLVED")

    def test_duplicate_basenames_are_not_chosen_by_filesystem_order(self):
        entries = {
            "browser/nurabrowser-core": {"needed": ["libsame.so.1"],
                                         "soname": None, "issues": []},
            "lib/libsame.so.1": {"needed": [], "soname": None, "issues": []},
            "target-wayland/libsame.so.1": {"needed": [], "soname": None, "issues": []},
        }
        _, edges, issues = audit.resolve_graph(entries, ["browser/nurabrowser-core"])
        self.assertFalse(edges)
        self.assertEqual(issues[0]["code"], "AMBIGUOUS_PRIVATE_NAME")

    def test_scan_is_scoped_and_fails_closed_on_symlink(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "lib").mkdir()
            (root / "results-live").mkdir()
            (root / "lib/libreal.so").write_bytes(b"data")
            (root / "lib/libalias.so").symlink_to("libreal.so")
            (root / "results-live/sensitive.log").write_text("not a runtime file")
            files, symlinks = audit.enumerate_files(root)
            self.assertEqual(list(files), ["lib/libreal.so"])
            self.assertEqual(symlinks, ["lib/libalias.so"])


if __name__ == "__main__":
    unittest.main()
