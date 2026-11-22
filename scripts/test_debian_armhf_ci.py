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

    def test_glibc_loader_needed_is_mapped_to_libc6(self):
        # Some armhf glibc-linked binaries list the interpreter in DT_NEEDED.
        self.assertIn("libc.so.6|ld-linux-armhf.so.3|libm.so.6", self.ci)
        package_script = (ROOT / "scripts/build-ci-debian-deb.sh").read_text()
        self.assertIn("Depends: libc6 (>= 2.41)", package_script)

    def test_independent_opt_in_glibc_gpu_artifact(self):
        for part in (
            'gpu_producer_run_id:',
            'gpu_producer_revision:',
            'gpu_archive_sha256:',
            'sl101-gpu-video-bundle-glibc',
            'SL101_GPU_PRODUCER_REPOSITORY',
            'SL101_GPU_PRODUCER_READ_TOKEN',
            'scripts/extract_sl101_video_bundle.py',
            'scripts/verify_sl101_video_bundle.py target/verified-gpu-video',
            '--target "$TARGET"',
            'NURALOUMI_VIDEO_BUNDLE_SOURCE_VERIFIED: \'1\'',
            'target/gha-artifact/nuraloumi-gpu-glibc-ci.tar',
            'GPU-GLIBC-SHA256SUMS.txt',
            'nuraloumi-gpu-source-verified-glibc-armhf-',
        ):
            self.assertIn(part, self.ci)
        self.assertIn('test -z "$PRODUCER_REV"', self.ci)
        self.assertIn('test -z "$PRODUCER_SHA"', self.ci)
        self.assertIn('sha256sum --check --status', self.ci)
        # The native Debian DEB is software-only, no private musl blobs.
        self.assertLess(self.ci.index('Assemble a Debian 13 armhf .deb'), self.ci.index('Retrieve independently built glibc GPU'))

    def test_glibc_package_consumes_exact_same_commit_qualified_browser_runtime(self):
        for part in (
            'browser_producer_run_id:',
            'browser_runtime_manifest_sha256:',
            'nurabrowser-armhf-qualified-${{ github.sha }}',
            'run.get("path") == ".github/workflows/nurabrowser-engine-armhf.yml"',
            'run.get("head_sha") == expected_sha',
            'verify_nurabrowser_runtime.py target/browser-producer-download',
            'NURALOUMI_BROWSER_RUNTIME_DIR: ${{ github.workspace }}/target/browser-producer-download',
            'NURALOUMI_BROWSER_PRODUCER_REVISION: ${{ github.sha }}',
        ):
            self.assertIn(part, self.ci)
        self.assertIn('if: ${{ github.event_name == \'workflow_dispatch\' }}', self.ci)
        self.assertIn('if: ${{ github.event_name != \'workflow_dispatch\' }}', self.ci)

    def test_existing_glibc_engine_artifact_is_explicitly_not_a_runtime_qualification(self):
        producer = (ROOT / '.github/workflows/nurabrowser-engine-armhf.yml').read_text()
        builder = (ROOT / 'scripts/ci-build-nurabrowser-engine.sh').read_text()
        self.assertIn('nurabrowser-engine-armhf.tar', producer)
        self.assertIn('web_process_sandbox=unqualified', builder)
        self.assertIn('dependency_instruction_closure=unqualified', builder)
        self.assertNotIn('nurabrowser-armhf-qualified-', producer)

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
