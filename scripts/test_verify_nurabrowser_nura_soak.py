#!/usr/bin/env python3
"""Synthetic positive and negative controls for the native soak oracle."""
from datetime import datetime, timedelta, timezone
import json
from pathlib import Path
import tempfile
import unittest

from verify_nurabrowser_nura_soak import verify


class VerifyNativeSoakTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        (self.root / "state").mkdir()
        start = datetime(2026, 11, 27, tzinfo=timezone.utc)
        samples = []
        responses = []
        for tick in range(4):
            stamp = (start + timedelta(seconds=10 * tick)).isoformat().replace("+00:00", "Z")
            samples.append(
                f"{stamp} ui=1 web=1 network=1 rss_kib=330000 "
                f"available_kib=600000 cpu_mC=41000 skin_mC=34000 "
                "ui:101:60000 web:102:250000 network:103:20000"
            )
            responses.extend([
                {"ok": True, "result": {"uri": "https://example.org/", "is_loading": False}},
                {"ok": True, "result": {"tabs": [
                    {"uri": "https://example.org/", "loading": False}], "active": 0}},
            ])
        (self.root / "soak-samples.log").write_text("\n".join(samples) + "\n")
        (self.root / "state" / "browser-state.jsonl").write_text(
            "".join(json.dumps(row) + "\n" for row in responses)
        )
        (self.root / "engine.log").write_text("NuraBrowser: WebKit load-finished (failed=no)\n")
        png = b"\x89PNG\r\n\x1a\n" + b"\0\0\0\rIHDR" + (1280).to_bytes(4, "big") + (800).to_bytes(4, "big")
        for name in ("wayland-first.png", "wayland-last.png"):
            (self.root / name).write_bytes(png)

    def inspect(self):
        return verify(self.root, "https://example.org/", minimum_seconds=30,
                      minimum_samples=4, minimum_pairs=4)

    def test_positive(self):
        self.assertEqual(self.inspect()["stability_gate"], "SOAK_PASS_ONLY")
        self.assertEqual(self.inspect()["release_gate"], "BLOCKED")

    def test_short_run_fails(self):
        self.assertEqual(
            verify(self.root, "https://example.org/")["stability_gate"], "BLOCKED"
        )

    def test_web_process_restart_fails(self):
        path = self.root / "soak-samples.log"
        path.write_text(path.read_text().replace("web:102:250000", "web:999:250000", 1))
        self.assertEqual(self.inspect()["stability_gate"], "BLOCKED")

    def test_wrong_uri_fails(self):
        self.assertEqual(
            verify(self.root, "https://www.tagesschau.de/", minimum_seconds=30,
                   minimum_samples=4, minimum_pairs=4)["stability_gate"], "BLOCKED"
        )

    def test_thermal_abort_fails(self):
        path = self.root / "soak-samples.log"
        path.write_text(path.read_text().replace("cpu_mC=41000", "cpu_mC=70000", 1))
        self.assertEqual(self.inspect()["stability_gate"], "BLOCKED")

    def test_missing_pixel_evidence_fails(self):
        (self.root / "wayland-last.png").unlink()
        self.assertEqual(self.inspect()["stability_gate"], "BLOCKED")


if __name__ == "__main__":
    unittest.main()
