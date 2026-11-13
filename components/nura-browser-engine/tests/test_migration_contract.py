"""Contract tests for the local NuraLoumi-owned WPE backend migration.

Historical SL101 qualification scripts remain for reference, but are not
accepted as ARMHF or device evidence for this new repository ownership.
"""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
SHELL = ROOT.parent.parent / "crates" / "nuraloumi-shell"


class BrowserMigrationContract(unittest.TestCase):
    def test_wpe_engine_is_internal_and_has_no_gtk(self):
        meson = (ROOT / "meson.build").read_text()
        self.assertIn("wpe-webkit-2.0", meson)
        self.assertIn("wpe-platform-2.0", meson)
        self.assertIn("src/bookmarks/bookmarks.c", meson)
        self.assertIn("libexecdir') / 'nuraloumi' / 'browser'", meson)
        self.assertNotIn("install_data('scripts/launch.sh'", meson)
        self.assertNotIn("dependency('gtk", meson)
        self.assertNotIn("gtk-layer-shell", meson)

    def test_app_uses_rust_nuraloumi_cairo_wayland(self):
        cargo = (SHELL / "Cargo.toml").read_text()
        app = (SHELL / "src/bin/browser.rs").read_text()
        self.assertIn('name = "nuraloumi-browser"', cargo)
        self.assertIn("WaylandBackend", app)
        self.assertIn("CairoRenderer", app)
        self.assertIn("BrowserUi", app)
        self.assertNotIn("gtk::", app)

    def test_native_tab_state_and_persistent_bookmarks(self):
        navigation = (ROOT / "src/navigation/navigation.c").read_text()
        state = (ROOT / "src/bookmarks/bookmarks.c").read_text()
        self.assertIn("g_object_new(WEBKIT_TYPE_WEB_VIEW", navigation)
        for name in ("tab-new", "tab-select", "tab-close", "tabs", "bookmark-toggle", "bookmarks"):
            self.assertIn(f'"{name}"', navigation)
        self.assertIn("O_NOFOLLOW", state)
        self.assertIn("renameat", state)
        self.assertIn("fsync", state)


if __name__ == "__main__":
    unittest.main()
