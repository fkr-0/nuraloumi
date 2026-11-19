#!/usr/bin/env python3
"""Host-only fail-closed contract tests for the isolated NuraBrowser E2E harness."""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
import importlib.util


SCRIPT = Path(__file__).with_name("qualify-browser-nested-wayland.py")


def load_harness():
    spec = importlib.util.spec_from_file_location("nura_nested_harness", SCRIPT)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class NestedBrowserHarnessTests(unittest.TestCase):
    def invoke(self, *args: str, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(SCRIPT), *args],
            env=env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=8,
            check=False,
        )

    def test_missing_tools_report_skip_77_without_touching_live_session(self) -> None:
        with tempfile.TemporaryDirectory(prefix="nura-e2e-tests-") as base:
            env = os.environ.copy()
            env.update({"PATH": "", "WAYLAND_DISPLAY": "wayland-live-untrusted",
                        "SWAYSOCK": "/nonexistent/live-sway.sock"})
            result = self.invoke("--probe", "--output-dir", str(Path(base) / "evidence"), env=env)
            self.assertEqual(result.returncode, 77, result.stderr)
            evidence = json.loads(result.stdout)
            self.assertEqual(evidence["status"], "SKIP")
            self.assertTrue(evidence["missing"])
            self.assertFalse((Path(base) / "evidence").exists())

    def test_preflight_never_runs_fake_compositor_when_browser_missing(self) -> None:
        with tempfile.TemporaryDirectory(prefix="nura-e2e-tests-") as base:
            bindir = Path(base) / "bin"
            bindir.mkdir()
            marker = Path(base) / "unexpected-run"
            for command in ("sway", "grim", "wtype"):
                executable = bindir / command
                executable.write_text(f"#!/bin/sh\nprintf touched > {marker}\nexit 1\n", encoding="utf-8")
                executable.chmod(0o755)
            env = os.environ.copy()
            env["PATH"] = str(bindir)
            result = self.invoke("--probe", "--browser-bin", str(Path(base) / "missing-browser"),
                                 "--engine-bin", str(Path(base) / "missing-engine"), env=env)
            self.assertEqual(result.returncode, 77, result.stderr)
            self.assertFalse(marker.exists())
            evidence = json.loads(result.stdout)
            self.assertIn("browser-bin", evidence["missing"])
            self.assertIn("engine-bin", evidence["missing"])

    def test_no_implicit_live_session_fallback(self) -> None:
        env = os.environ.copy()
        env.update({"PATH": "", "DISPLAY": ":0", "WAYLAND_DISPLAY": "wayland-0",
                    "SWAYSOCK": "/run/user/1234/sway-live.sock"})
        result = self.invoke("--probe", env=env)
        self.assertEqual(result.returncode, 77)
        evidence = json.loads(result.stdout)
        self.assertNotIn("wayland-0", json.dumps(evidence))
        self.assertNotIn("sway-live.sock", json.dumps(evidence))

    def test_green_pixel_oracle_requires_large_real_colored_region(self) -> None:
        from PIL import Image, ImageDraw

        harness = load_harness()
        with tempfile.TemporaryDirectory(prefix="nura-oracle-test-") as base:
            screenshot = Path(base) / "shot.png"

            def fake_grim(_argv, **_kwargs):
                image = Image.new("RGB", (120, 120), "black")
                ImageDraw.Draw(image).rectangle((12, 21, 90, 70), fill=(4, 245, 29))
                image.save(screenshot)
                return subprocess.CompletedProcess(["grim"], 0)

            with mock.patch.object(harness.subprocess, "run", side_effect=fake_grim):
                evidence = harness.capture("grim", {}, screenshot)
            self.assertTrue(evidence["page_pixels_verified"])
            self.assertGreaterEqual(evidence["oracle_green_samples"], 100)
            self.assertEqual(evidence["oracle_bounds_px"], [12, 21, 90, 69])

    def test_dark_frame_does_not_count_as_page_pixels(self) -> None:
        from PIL import Image

        harness = load_harness()
        with tempfile.TemporaryDirectory(prefix="nura-oracle-test-") as base:
            screenshot = Path(base) / "shot.png"

            def fake_grim(_argv, **_kwargs):
                Image.new("RGB", (120, 120), "black").save(screenshot)
                return subprocess.CompletedProcess(["grim"], 0)

            with mock.patch.object(harness.subprocess, "run", side_effect=fake_grim):
                evidence = harness.capture("grim", {}, screenshot)
            self.assertFalse(evidence["page_pixels_verified"])


if __name__ == "__main__":
    unittest.main()
