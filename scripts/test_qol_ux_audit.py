"""Deterministic, mutation-driven regression tests for the geometry audit."""
import copy
import json
import subprocess
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from qol_ux_audit import audit, inspect  # noqa: E402


class AuditTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.doc = json.loads((ROOT / "tests/fixtures/qol/scenes.json").read_text())

    def test_fixture_expected_failures_match(self):
        self.assertTrue(inspect(self.doc)["passed"])

    def test_target_geometries_and_fractional_scale(self):
        names = {s["name"] for s in self.doc["scenes"]}
        for name in ("baseline-1280x800", "after-1280x800", "after-854x480",
                     "after-800x480", "after-fractional-854x480"):
            self.assertIn(name, names)
        for scene in self.doc["scenes"]:
            self.assertEqual(audit(scene), sorted(scene["expected_failures"]))

    def test_scroll_equal_and_one_row_short(self):
        scene = copy.deepcopy(self.doc["scenes"][1])
        for extent in (200, 248):
            scene["scroll"].update(content=extent, viewport=248, visible=False)
            self.assertNotIn("scroll-visibility", audit(scene))
        scene["scroll"].update(content=249, viewport=248, visible=False)
        self.assertIn("scroll-visibility", audit(scene))

    def test_overlap_and_minimum_hit_target(self):
        scene = copy.deepcopy(self.doc["scenes"][1])
        scene["controls"][0]["hit"][2] = 47.99
        self.assertIn("tap-target:mute", audit(scene))
        scene["controls"][0]["hit"] = scene["controls"][1]["hit"][:]
        self.assertIn("hit-overlap:mute:minus", audit(scene))

    def test_invalid_geometry_fails_closed(self):
        scene = copy.deepcopy(self.doc["scenes"][1])
        scene["controls"][0]["hit"][0] = float("nan")
        with self.assertRaises(ValueError):
            audit(scene)

    def test_cli(self):
        p = subprocess.run([sys.executable, str(ROOT / "scripts/qol_ux_audit.py"),
                            str(ROOT / "tests/fixtures/qol/scenes.json"), "--json"],
                           capture_output=True, text=True, check=False)
        self.assertEqual(p.returncode, 0, p.stderr)
        self.assertTrue(json.loads(p.stdout)["passed"])


if __name__ == "__main__":
    unittest.main()
