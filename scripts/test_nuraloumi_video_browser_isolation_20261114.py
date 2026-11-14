"""GPU player opt-in must never silently affect the independent WPE browser.

These tests are host-only and operate on committed source; they need no
private ARM driver, device access, WPE runtime, network, or display server.
"""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
PLAYER = ROOT / "crates/nuraloumi-shell/src/bin/player.rs"
BROWSER_UI = ROOT / "crates/nuraloumi-shell/src/bin/browser.rs"
WPE_LAUNCH = ROOT / "components/nura-browser-engine/scripts/launch.sh"
WPE_ENGINE = ROOT / "components/nura-browser-engine/src/engine/engine.c"

PAIR = "GRATE_I420_YU_PAIR"
SCHEDULE = "GRATE_I420_YU_FP_SCHEDULE"


class BrowserIsolationTest(unittest.TestCase):
    def test_gpu_specialization_is_child_only_in_player(self):
        player = PLAYER.read_text()
        self.assertIn('daemon.env("GRATE_I420_YU_PAIR", "1")', player)
        self.assertIn('daemon.env("GRATE_I420_YU_FP_SCHEDULE", "1")', player)
        self.assertNotIn("std::env::set_var", player)
        self.assertIn("configure_single_player_daemon(", player)

    def test_browser_is_cairo_and_never_assumes_grate(self):
        browser = BROWSER_UI.read_text()
        self.assertIn("CairoRenderer::default()", browser)
        self.assertIn("WaylandBackend::connect()", browser)
        self.assertIn("NativeBrowser::from_environment()", browser)
        for forbidden in (PAIR, SCHEDULE, "MESA_LOADER_DRIVER_OVERRIDE",
                          "LIBGL_DRIVERS_PATH", "libnuraloumi-video.so",
                          "gst_element_set_state"):
            self.assertNotIn(forbidden, browser)

    def test_browser_launcher_discards_video_compiler_knobs(self):
        launcher = WPE_LAUNCH.read_text()
        clean = "unset GRATE_I420_YU_PAIR GRATE_I420_YU_FP_SCHEDULE GRATE_THREE_TRACE_FRAME"
        self.assertIn(clean, launcher)
        self.assertLess(launcher.index(clean), launcher.rindex('case "$renderer" in'))
        self.assertIn("unset SL101_THREE_STRESS_MODE SL101_PRIVATE_GST_DIR", launcher)
        self.assertIn("export MESA_LOADER_DRIVER_OVERRIDE=swrast", launcher)
        self.assertIn("export MESA_LOADER_DRIVER_OVERRIDE=grate", launcher)
        self.assertIn("private pinned Grate overlay missing", launcher)
        self.assertIn("Grate overlay integrity receipt mismatch", launcher)

    def test_wpe_engine_never_loads_player_private_video_library(self):
        code = WPE_ENGINE.read_text()
        self.assertNotIn("libnuraloumi-video.so", code)
        self.assertNotIn(PAIR, code)
        self.assertNotIn(SCHEDULE, code)
        self.assertIn('g_getenv("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS")', code)
        self.assertIn("refusing to run with WebKit sandbox disabled", code)


if __name__ == "__main__":
    unittest.main()
