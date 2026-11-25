"""Host-only integrity/transaction tests for the modular shell release."""
from __future__ import annotations

import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).with_name("sl101_shell_release.py")
spec = importlib.util.spec_from_file_location("sl101_shell_release", SCRIPT)
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ShellReleaseTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.stage = self.root / "stage"
        (self.stage / "bin").mkdir(parents=True)
        self.names = sorted(release.REQUIRED | {
            "nuraloumi-thumbnail-helper", "nuraloumi-files", "nuraloumi-maps",
            "nuraloumi-calendar", "nuraloumi-clipboard", "nuraloumi-config",
            "nuraloumi-editor", "nuraloumi-feeds", "nuraloumi-music",
            "nuraloumi-notifications", "nuraloumi-probe", "nuraloumi-resources",
            "nuraloumi-run", "nuraloumi-screenshot", "nuraloumi-tasks",
            "nuraloumi-terminal", "nuraloumi-timer", "nuraloumi-toast",
            "nuraloumi-viewer", "nuraloumi-windows", "nuraloumi-browser",
            "nuraloumi-sway-workspace-switcher", "nuraloumi-slider-observer",
        })
        self.elf = b"\x7fELF\x01\x01" + b"\0" * 12 + b"\x28\x00"
        for name in self.names:
            dest = self.stage / "bin" / name
            dest.write_bytes(self.elf)
            dest.chmod(0o755)
        self.write_manifest()

    def write_manifest(self, extra_headers=None):
        headers = {
            "schema": "nuraloumi-package-manifest-v1",
            "version": "0.0.8-rc.2",
            "target": "armv7-unknown-linux-musleabihf",
            "git_commit": "a" * 40,
            "git_dirty": "false",
            "binaries": ",".join(self.names),
            "release_profile": "shell-only",
            "browser_runtime": "deferred",
        }
        if extra_headers:
            headers.update(extra_headers)
        lines = [f"{key}={value}" for key, value in headers.items()]
        lines.append("files:")
        for name in self.names:
            lines.append(f"  {release.digest(self.stage / 'bin' / name)}  bin/{name}")
        (self.stage / "MANIFEST.txt").write_text("\n".join(lines) + "\n")

    def test_staged_shell_verifies(self):
        result = release.verify_stage(self.stage, root_owned=False)
        self.assertEqual(result["release_profile"], "shell-only")
        self.assertGreaterEqual(len(result["binaries"].split(",")), 28)

    def test_tampered_binary_fails(self):
        (self.stage / "bin/nuraloumi-panel").write_bytes(self.elf + b"tamper")
        with self.assertRaisesRegex(RuntimeError, "file-inventory-or-sha"):
            release.verify_stage(self.stage, root_owned=False)

    def test_extra_unlisted_file_fails(self):
        (self.stage / "surprise").write_text("not in manifest")
        with self.assertRaisesRegex(RuntimeError, "file-inventory-or-sha"):
            release.verify_stage(self.stage, root_owned=False)

    def test_ordinary_browser_required_package_not_shell(self):
        self.write_manifest({"release_profile": "full", "browser_runtime": "required"})
        with self.assertRaisesRegex(RuntimeError, "manifest-release_profile"):
            release.verify_stage(self.stage, root_owned=False)

    def test_dirty_or_wrong_abi_refused(self):
        for override, expected in [
            ({"git_dirty": "true"}, "manifest-git_dirty"),
            ({"target": "armv7-unknown-linux-gnueabihf"}, "manifest-target"),
        ]:
            self.write_manifest(override)
            with self.assertRaisesRegex(RuntimeError, expected):
                release.verify_stage(self.stage, root_owned=False)

    def test_inherited_video_pins_verified(self):
        old = self.root / "old"
        (old / release.VIDEO).mkdir(parents=True)
        with patch.object(release, "inventory", return_value={
            **release.VIDEO_PINS, "gst/extra.so": "e" * 64,
        }):
            files = release.video_files(old)
        self.assertEqual(len(files), 4)
        with patch.object(release, "inventory", return_value={
            **release.VIDEO_PINS, "lib/libnuraloumi-video.so": "0" * 64,
        }):
            with self.assertRaisesRegex(RuntimeError, "inherited-video-pin"):
                release.video_files(old)

    def test_atomic_link_and_rollback(self):
        current = self.root / "current"
        a, b = self.root / "a", self.root / "b"
        a.mkdir()
        b.mkdir()
        current.symlink_to(a)
        release.atomic_link(current, b)
        self.assertEqual(current.readlink(), b)
        release.atomic_link(current, a)
        self.assertEqual(current.readlink(), a)

    def test_symlink_in_stage_refused(self):
        (self.stage / "alias").symlink_to("bin/nuraloumi-panel")
        with self.assertRaisesRegex(RuntimeError, "tree-symlink"):
            release.verify_stage(self.stage, root_owned=False)


if __name__ == "__main__":
    unittest.main()
