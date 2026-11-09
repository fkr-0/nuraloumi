"""Static regression contract for the CI-only, signed Alpine armv7 musl job."""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github/workflows/sl101-musl-armv7.yml"
SCRIPT = ROOT / "scripts/prepare-ci-musl-sysroot.sh"
DOC = ROOT / "docs/qualification/MUSL-GITHUB-CI.md"
HOST_CI = ROOT / ".github/workflows/ci.yml"


class MuslCiContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.ci = WORKFLOW.read_text()
        cls.prepare = SCRIPT.read_text()
        cls.doc = DOC.read_text()

    def test_only_armv7_musl_target_on_main_push(self):
        for part in (
            "  push:\n    branches:\n      - main",
            "armv7-unknown-linux-musleabihf",
            "docker/setup-qemu-action@v3",
            "platforms: arm",
            "rust-src",
            "clang lld llvm pkg-config binutils file python3",
            "sh scripts/build-sl101-armv7.sh",
        ):
            self.assertIn(part, self.ci)
        self.assertNotIn("armv7-unknown-linux-gnueabihf", self.ci)

    def test_host_ci_installs_xkbcommon_needed_by_tests(self):
        self.assertIn("libxkbcommon-dev", HOST_CI.read_text())

    def test_sysroot_is_signed_native_alpine_and_safe(self):
        for part in (
            "linux/arm/v7",
            "apk --root /out/sysroot",
            "--keys-dir /etc/apk/keys",
            "--no-scripts",
            "musl musl-dev gcc libgcc",
            "cairo-dev wayland-dev libxkbcommon-dev",
            "freetype-dev harfbuzz-dev fribidi-dev",
            "refuse-overwrite-tablet-sysroot",
            "pkg-config --exists cairo freetype2 harfbuzz fribidi wayland-client xkbcommon",
        ):
            self.assertIn(part, self.prepare)
        for forbidden in ("--allow-untrusted", "ssh ", "scp ", "192.168."):
            if forbidden == "--allow-untrusted":
                # A prohibition in a comment is allowed; never as an apk flag.
                self.assertNotIn("add --allow-untrusted", self.prepare)
            else:
                self.assertNotIn(forbidden, self.ci)
        self.assertIn("no-SIGILL", self.doc)

    def test_complete_artifact_and_elf_audit(self):
        for part in (
            "cargo xtask package --target",
            "cargo xtask package-check --dir",
            "git_dirty=false",
            'export GIT_DIR="$PWD/.git"',
            'export GIT_WORK_TREE="$PWD"',
            "scripts/package-binaries.py",
            "nuraloumi-ci.tar",
            "MANIFEST.txt",
            "BUILD-INFO.txt",
            "ELF-AUDIT.txt",
            "SHA256SUMS.txt",
            "libc.musl-armv7.so.1",
            "ld-musl-armhf.so.1",
            "Tag_FP_arch: VFPv3-D16",
            "upload-artifact@v4",
            "retention-days: 14",
        ):
            self.assertIn(part, self.ci)


if __name__ == "__main__":
    unittest.main()
