"""Fail-closed static contract for native ARMv7 media runtime bundle CI."""

from pathlib import Path
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts/ci-bundle-nurabrowser-wpe-media-musl.sh"
WORKFLOW = ROOT / ".github/workflows/nurabrowser-wpe-musl-producer.yml"


class NativeMediaBundleTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.script = SCRIPT.read_text(encoding="utf-8")
        cls.workflow = WORKFLOW.read_text(encoding="utf-8")

    def test_pin_and_arm_isa_required(self):
        for part in (
            'test "$(apk --print-arch)" = armv7',
            "CI_BUNDLE_SOURCE_SHA", "CI_BUNDLE_TAR_SHA256",
            "sha256sum -c -", "media_build_requested=1",
            "upstream_wpewebkit=2.54.2", "readelf -A",
            "Tag_FP_arch: VFPv3-D16", "Tag_ABI_VFP_args: VFP registers",
        ):
            self.assertIn(part, self.script)

    def test_nine_factories_and_extended_hls_chain(self):
        for name in (
            "h264parse", "aacparse", "avdec_h264", "avdec_aac",
            "qtdemux", "hlsdemux", "souphttpsrc", "uridecodebin",
            "videoconvert", "tsdemux", "tsparse", "typefind",
            "playbin", "playbin3", "decodebin", "parsebin",
            "appsrc", "appsink", "hlsdemux2", "audioconvert",
            "audioresample", "glupload", "glcolorconvert",
            "glimagesink", "waylandsink", "alsasink", "pulsesink",
        ):
            self.assertIn(name, self.script)
        self.assertIn("gst-inspect-1.0", self.script)
        self.assertIn("MISSING-OPTIONAL", self.script)

    def test_unqualified_private_output(self):
        for token in (
            "runtime_qualification=UNQUALIFIED", "device_deployment=FORBIDDEN",
            "RUNTIME-SHA256SUMS.txt", "test ! -e",
        ):
            self.assertIn(token, self.script)
        for forbidden in ("ssh ", "scp ", "runtime_qualification=PASS"):
            self.assertNotIn(forbidden, self.script)

    def test_generated_root_is_unprivileged_packagable(self):
        # Regresses the first CI packaging failure: the native ARMv7 container
        # could inspect the files, but the unprivileged host runner could not.
        for file_path in (
            "$root/etc/shadow", "$root/etc/shadow-",
            "$root/etc/crontabs/root",
            "$root/usr/libexec/dbus-daemon-launch-helper",
            "$root/lib/apk/db/lock",
        ):
            self.assertIn(file_path, self.script)
        self.assertIn('find "$root" -type f', self.script)
        self.assertIn("! -perm /004 -o -perm /6000", self.script)
        self.assertIn("untraversable", self.script)
        self.assertIn('test -z "$unshippable"', self.script)

    def test_workflow_native_reuse_without_rebuild(self):
        for token in (
            "bundle_source_run_id:", "bundle_source_sha:",
            "bundle_tar_sha256:", "native-armv7-musl-wpe-media-bundle:",
            "runs-on: ubuntu-24.04-arm", "actions/download-artifact@v4",
            "ci-bundle-nurabrowser-wpe-media-musl.sh",
            "sha256sum -c -", "runtime-root.tar.zst",
            "MEDIA_RUNTIME_BUNDLE=HOST_PREFLIGHT_PASS_DEVICE_NOT_RUN",
        ):
            self.assertIn(token, self.workflow)
        self.assertNotIn("setup-qemu-action", self.workflow)

    def test_script_syntax_and_missing_pin_fail_closed(self):
        checked = subprocess.run(["sh", "-n", str(SCRIPT)],
                                 capture_output=True, text=True, timeout=5)
        self.assertEqual(checked.returncode, 0, checked.stderr)
        missing = subprocess.run(["sh", str(SCRIPT)],
                                 capture_output=True, text=True, timeout=5)
        self.assertNotEqual(missing.returncode, 0)
        self.assertIn("missing full source SHA", missing.stderr)


if __name__ == "__main__":
    unittest.main()
