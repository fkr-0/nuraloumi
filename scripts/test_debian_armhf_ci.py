"""CI contract tests with no Cargo build, GitHub calls or tablet access."""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github/workflows/sl101-debian-armhf.yml"
DOC = ROOT / "docs/qualification/DEBIAN-ARMHF-CI.md"


class DebianArmhfCiTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.ci = WORKFLOW.read_text()
        cls.doc = DOC.read_text()

    def test_remote_debian_armhf_multiarch(self):
        for part in (
            "runs-on: ubuntu-24.04",
            "container:",
            "image: debian:trixie",
            "dpkg --add-architecture armhf",
            "libcairo2-dev:armhf",
            "libwayland-dev:armhf",
            "libxkbcommon-dev:armhf",
            "make gcc g++",
        ):
            self.assertIn(part, self.ci)

    def test_main_push_generates_glibc_artifact(self):
        self.assertIn("  push:\n    branches:\n      - main", self.ci)

    def test_glibc_not_alpine(self):
        for part in (
            "armv7-unknown-linux-gnueabihf",
            "gcc-arm-linux-gnueabihf",
            "-mfpu=vfpv3-d16",
            "-mfloat-abi=hard",
            "-C target-feature=-neon,-d32,-hwdiv,-hwdiv-arm",
            "ld-linux-armhf.so.3",
            "[libc.so.6]",
        ):
            self.assertIn(part, self.ci)
        self.assertNotIn("musleabihf", self.ci)

    def test_packaging_all_declared_runtime_surfaces(self):
        for part in (
            "cargo build --locked --release --workspace --bins",
            "cargo xtask package --target",
            "cargo xtask package-check --dir",
            "python3 scripts/package-binaries.py",
            "nuraloumi-ci.tar",
            "MANIFEST.txt",
            "SHA256SUMS.txt",
            "ELF-AUDIT.txt",
            "BUILD-INFO.txt",
            "upload-artifact@v4",
            "retention-days: 14",
            'export GIT_DIR="$PWD/.git"',
            'export GIT_WORK_TREE="$PWD"',
        ):
            self.assertIn(part, self.ci)

    def test_no_device_or_service_access(self):
        for forbidden in (
            "ssh ", "scp ", "192.168.", "systemctl ", "reboot", "sudo ",
            "fastboot", "nvflash",
        ):
            self.assertNotIn(forbidden, self.ci)
        self.assertIn("no-SIGILL", self.doc)
        self.assertIn("git SHA", self.doc)


if __name__ == "__main__":
    unittest.main()
