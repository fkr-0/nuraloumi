#!/usr/bin/env python3
"""Fail-closed source guards for the WPE 2.54.2 EGL display fix."""

import tempfile
import unittest
from pathlib import Path

from patch_wpe_egl_bootstrap import NEW, OLD, SOURCE, apply


class EGLBootstrapTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / SOURCE
        self.source.parent.mkdir(parents=True)

    def test_original_crashing_source_is_repaired_and_idempotent(self):
        self.source.write_text("start\n" + OLD + "end\n")
        self.assertTrue(apply(self.root))
        self.assertIn(NEW, self.source.read_text())
        self.assertNotIn("CRASH();", self.source.read_text())
        self.assertFalse(apply(self.root))

    def test_wrong_platform_source_is_rejected_without_mutation(self):
        self.source.write_text("eglGetPlatformDisplayEXT(EGL_PLATFORM_WAYLAND_EXT, 0, nullptr);\n")
        before = self.source.read_bytes()
        with self.assertRaisesRegex(ValueError, "unexpected WPE"):
            apply(self.root)
        self.assertEqual(self.source.read_bytes(), before)

    def test_ambiguous_source_is_rejected(self):
        self.source.write_text(OLD + OLD)
        with self.assertRaisesRegex(ValueError, "unexpected WPE"):
            apply(self.root)


if __name__ == "__main__":
    unittest.main()
