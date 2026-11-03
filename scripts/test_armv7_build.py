"""Offline cross-build gate fixtures: never run Cargo or contact an SL101."""
import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent
TARGET = "armv7-unknown-linux-musleabihf"
REQUIRED = [
    "sysroot/lib/ld-musl-armhf.so.1",
    "sysroot/usr/lib/libcairo.so",
    "sysroot/usr/lib/libxkbcommon.so",
    "sysroot/usr/lib/libgcc_s.so",
    "sysroot/usr/lib/libfreetype.so",
    "sysroot/usr/lib/libharfbuzz.so",
    "sysroot/usr/lib/libfribidi.so",
    "sysroot/usr/lib/Scrt1.o",
    "sysroot/usr/lib/crti.o",
    "sysroot/usr/lib/crtn.o",
    "pkgconfig/cairo.pc",
    "pkgconfig/freetype2.pc",
    "pkgconfig/harfbuzz.pc",
    "pkgconfig/fribidi.pc",
]
BINARIES = (
    "nuraloumi-menu", "nuraloumi-panel", "nuraloumi-probe",
    "nuraloumi-thumbnail-helper",
)


class CrossBuildGateTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="nuraloumi-cross-gate-")
        self.addCleanup(self.temp.cleanup)
        self.root = pathlib.Path(self.temp.name)
        scripts = self.root / "scripts"
        scripts.mkdir()
        shutil.copy2(ROOT / "scripts/build-sl101-armv7.sh",
                     scripts / "build-sl101-armv7.sh")
        # This fixture's audit is never evidence of real ARM32 ELF correctness.
        self.executable(scripts / "inspect-armv7-elf.sh",
                        "#!/bin/sh\necho FIXTURE_AUDIT_ONLY\n")
        self.stub = self.root / "fake-bin"
        self.stub.mkdir()
        self.marker = self.root / "cargo-called"
        self.executable(self.stub / "cargo",
                        '#!/bin/sh\nprintf called > "$NURALOUMI_TEST_CARGO_MARKER"\n')
        self.executable(self.stub / "rustup",
                        f"#!/bin/sh\nprintf '%s\\n' {TARGET}\n")
        names = "\n".join(BINARIES)
        self.executable(self.stub / "python3",
                        f"#!/bin/sh\ncat <<'BINARIES'\n{names}\nBINARIES\n")
        for item in REQUIRED:
            path = self.root / "target" / "sl101-cross" / item
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"fixture")
        self.release = self.root / "target" / TARGET / "release"
        self.release.mkdir(parents=True)

    @staticmethod
    def executable(path, content):
        path.write_text(content)
        path.chmod(0o755)

    def run_build(self):
        env = dict(os.environ)
        env["PATH"] = str(self.stub) + os.pathsep + env.get("PATH", "")
        env["NURALOUMI_TEST_CARGO_MARKER"] = str(self.marker)
        return subprocess.run(
            ["sh", str(self.root / "scripts" / "build-sl101-armv7.sh")],
            cwd=self.root, env=env, text=True, capture_output=True, timeout=10,
        )

    def test_missing_xkb_fails_preflight_without_invoking_cargo(self):
        (self.root / "target/sl101-cross/sysroot/usr/lib/libxkbcommon.so").unlink()
        result = self.run_build()
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("reason=sysroot-not-prepared", result.stderr)
        self.assertIn("libxkbcommon.so", result.stderr)
        self.assertFalse(self.marker.exists())

    def test_missing_declared_binary_fails_after_mock_build(self):
        for name in BINARIES[:-1]:
            self.executable(self.release / name, "#!/bin/sh\nexit 0\n")
        result = self.run_build()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("reason=missing-declared-binary", result.stderr)
        self.assertIn(BINARIES[-1], result.stderr)
        self.assertNotIn("SL101_CROSS_BUILD=PASS", result.stdout)

    def test_stale_extra_binary_fails_before_mock_audit(self):
        for name in (*BINARIES, "nuraloumi-old-surface"):
            self.executable(self.release / name, "#!/bin/sh\nexit 0\n")
        result = self.run_build()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("reason=undeclared-release-binary", result.stderr)
        self.assertIn("nuraloumi-old-surface", result.stderr)
        self.assertNotIn("FIXTURE_AUDIT_ONLY", result.stdout)


if __name__ == "__main__":
    unittest.main()
