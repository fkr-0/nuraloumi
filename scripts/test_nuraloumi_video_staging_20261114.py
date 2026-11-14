"""Optional pinned ARM/musl VDE library stage for the ONE NuraLoumi player.

No device access or package installation.
"""
import hashlib
from pathlib import Path
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]
BUNDLE = ROOT / "target/nuraloumi-video-armv7"


class VideoBundleStageTests(unittest.TestCase):
    def test_stager_explicit_opt_in_only_and_arm_target(self):
        src = (ROOT / "crates/nuraloumi-xtask/src/package.rs").read_text()
        self.assertIn('env::var_os("NURALOUMI_VIDEO_BUNDLE_DIR")', src)
        self.assertIn('resolved_target != "armv7-unknown-linux-musleabihf"', src)
        self.assertIn('stage.join("lib/nuraloumi/video")', src)
        self.assertIn('copy_tree(&bundle.join("lib")', src)
        self.assertIn('copy_required(', src)
        self.assertIn("native video engine must be ELF32 ARM", src)

    def test_mesa_and_gstreamer_exact_binary_hashes(self):
        sources = (
            ("lib/libgallium-25.0.7.so",
             "84af7822128f8a08a03c460164b00cd92b9058f22fdbe094f2811119d1d13cf8"),
            ("gst/libgstv4l2codecs.so",
             "79ee247faf6397829d908136ad16e04dc936437a1c71b6d49120e470ec452608"),
        )
        for name, expected in sources:
            if not (BUNDLE / name).is_file():
                self.skipTest("Crossbuilt private bundle unavailable")
            actual = hashlib.sha256((BUNDLE / name).read_bytes()).hexdigest()
            self.assertEqual(actual, expected)
        native = BUNDLE / "lib/libnuraloumi-video.so"
        if not native.is_file():
            self.skipTest("Crossbuilt native library unavailable")
        header = native.read_bytes()[:20]
        self.assertEqual(header[:5], b"\x7fELF\x01")
        self.assertEqual(header[18:20], b"\x28\x00")
        result = subprocess.run(
            ["readelf", "-d", str(native)], text=True, check=True,
            capture_output=True)
        self.assertIn("libc.musl-armv7.so.1", result.stdout)
        self.assertNotIn("libc.so.6", result.stdout)
        self.assertNotIn("(RPATH)", result.stdout)
        self.assertNotIn("(RUNPATH)", result.stdout)

    def test_opt_in_three_pass_uses_same_playerd_and_pinned_library(self):
        build = (ROOT / "scripts/build-nuraloumi-player-video.sh").read_text()
        launch = (ROOT / "crates/nuraloumi-shell/src/bin/player.rs").read_text()
        on_device = (ROOT / "scripts/qualify-nuraloumi-player-video-on-device.sh").read_text()
        pinned = "84af7822128f8a08a03c460164b00cd92b9058f22fdbe094f2811119d1d13cf8"
        self.assertIn(pinned, build)
        self.assertIn(pinned, on_device)
        packager = (ROOT / "crates/nuraloumi-xtask/src/package.rs").read_text()
        self.assertIn(pinned, packager)
        self.assertIn("848386bbbd2042c7a881a079368a5368e85a2040d8a3b847abba3d557c19add6", packager)
        self.assertIn("GRATE_I420_YU_PAIR", launch)
        self.assertIn("GRATE_I420_YU_FP_SCHEDULE", launch)
        self.assertIn("GRATE_I420_YU_PAIR=1 GRATE_I420_YU_FP_SCHEDULE=1", on_device)
        self.assertNotIn("nuraloumi-vde-player", launch)

    def test_alpine_package_installs_internal_library_not_second_executable(self):
        apk = (ROOT / "packaging/alpine/APKBUILD.in").read_text()
        self.assertIn('if [ -d "$builddir/lib/nuraloumi/video" ]; then', apk)
        self.assertIn('$pkgdir/usr/lib/nuraloumi/video', apk)
        self.assertIn('libnuraloumi-video.so', apk)
        self.assertNotIn('nuraloumi-vde-player', apk)
        shell = (ROOT / "crates/nuraloumi-shell/src/bin/player.rs").read_text()
        self.assertIn('/usr/lib/nuraloumi/video', shell)
        self.assertIn('SL101_PRIVATE_GST_DIR', shell)
        self.assertIn('ProcCommand::new(sibling)', shell)

    def test_stage_from_project_source_only_has_one_player_pair(self):
        manifest = (ROOT / "crates/nuraloumi-shell/Cargo.toml").read_text()
        self.assertEqual(manifest.count('name = "nuraloumi-player"'), 1)
        self.assertEqual(manifest.count('name = "nuraloumi-playerd"'), 1)
        bundle_files = [p.name for p in (BUNDLE / "lib").glob("nuraloumi*")]
        if bundle_files:
            self.assertEqual(bundle_files, ["libnuraloumi-video.so"])


if __name__ == "__main__":
    unittest.main()
