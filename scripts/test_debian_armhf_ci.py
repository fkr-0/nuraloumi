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

    def test_git_installed_before_checkout_to_preserve_provenance(self):
        self.assertLess(
            self.ci.index("- name: Install Git before checkout"),
            self.ci.index("- uses: actions/checkout@v4"),
        )
        self.assertIn("git ca-certificates", self.ci)

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
            "cargo -Z build-std=std,panic_abort build --locked --release --workspace --bins",
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

    def test_armv7_std_is_built_with_d16_feature_flags(self):
        self.assertIn("toolchain: nightly-2026-11-12", self.ci)
        self.assertIn("components: rust-src", self.ci)
        self.assertIn("-Z build-std=std,panic_abort", self.ci)
        self.assertIn("-C target-feature=-neon,-d32,-hwdiv,-hwdiv-arm", self.ci)

    def test_mapping_symbols_survive_release_strip_for_strict_opcode_audit(self):
        # Without ARM mapping symbols objdump may interpret inline data as
        # executable VFP D16-D31 instructions. Do not loosen the opcode guard.
        self.assertIn("CARGO_PROFILE_RELEASE_STRIP: debuginfo", self.ci)
        self.assertIn("rustup component list --installed | grep -Fx 'rust-src'", self.ci)
        inspector = (ROOT / "scripts/inspect-armv7-elf.sh").read_text()
        self.assertIn("upper-VFP-register-d16-d31-detected", inspector)
        self.assertIn("possible-NEON-opcode-or-q-register", inspector)

    def test_disassembly_gate_is_enforced_for_sized_neon_and_upper_d(self):
        import re

        self.assertIn('sh scripts/inspect-armv7-elf.sh "$bin" | tee -a "$audit"', self.ci)
        line = next(line for line in self.ci.splitlines()
                    if 'llvm-objdump -d "$bin" | grep -Ei' in line)
        pattern = line.split("grep -Ei '", 1)[1].split("'", 1)[0]
        pattern = pattern.replace("[[:space:]]", r"\s")
        for instruction in ("vld1.32 {d0}, [r0]", "vst2.8 {d0, d1}, [r0]",
                            "vdup.8 d0, r0", "vqadd.s16 d0, d0, d1"):
            self.assertIsNotNone(re.search(pattern, "  " + instruction + " ", re.I),
                                 instruction)
        self.assertIsNone(re.search(pattern, "  vldr d15, [pc, #4] ", re.I))
        inspector = (ROOT / "scripts/inspect-armv7-elf.sh").read_text()
        self.assertIn("UPPER_VFP_RE=", inspector)
        self.assertIn("(sdiv|udiv)", inspector)
        self.assertIn("possible-NEON-opcode-or-q-register", inspector)

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
