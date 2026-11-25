"""Fail-closed GitHub Arm64/AArch32 capability gate (never a runtime claim)."""

from pathlib import Path
import platform
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github/workflows/nurabrowser-wpe-musl-producer.yml"
PROBE = ROOT / "scripts/ci-probe-armv7-native-compat.sh"


class ArmRunnerProbeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.workflow = WORKFLOW.read_text(encoding="utf-8")
        cls.probe = PROBE.read_text(encoding="utf-8")

    def test_probe_uses_real_host_arm64_without_qemu(self):
        job = self.workflow.split("  hosted-arm64-native-armv7-probe:", 1)[1].split(
            "  native-armv7-musl-wpe-candidate:", 1
        )[0]
        for snippet in (
            "runs-on: ubuntu-24.04-arm",
            "timeout-minutes: 15",
            "inputs.probe_only",
            "bash scripts/ci-probe-armv7-native-compat.sh",
            "if: ${{ always() }}",
            "HOST-PROCESS-ELF.txt",
        ):
            self.assertIn(snippet, job)
        self.assertNotIn("setup-qemu-action", job)
        self.assertNotIn("ci-build-nurabrowser-wpe-musl.sh", job)
        self.assertIn(
            "if: ${{ !(github.event_name == 'workflow_dispatch' && inputs.probe_only) }}",
            self.workflow,
        )

    def test_probe_does_not_mistake_qemu_for_native_armv7(self):
        for snippet in (
            'test "$(uname -m)" = aarch64',
            'docker pull --platform linux/arm/v7 alpine:3.22',
            'docker inspect --format',
            'sudo readelf -h "/proc/$pid/exe"',
            "Class:[[:space:]]+ELF32",
            "Machine:[[:space:]]+ARM",
            'test "$(apk --print-arch)" = armv7',
            "-mcpu=cortex-a9",
            "-mfpu=vfpv3-d16",
            "-mfloat-abi=hard",
            "-mno-unaligned-access",
            "Advanced_SIMD|NEON",
            "native_armv7=PASS",
            "qualification=UNQUALIFIED",
        ):
            self.assertIn(snippet, self.probe)
        self.assertNotIn("qemu-arm-static", self.probe)

    def test_probe_shell_syntax(self):
        result = subprocess.run(
            ["bash", "-n", str(PROBE)],
            text=True, capture_output=True, timeout=5,
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    @unittest.skipIf(platform.machine() == "aarch64", "negative host case requires non-Arm64")
    def test_non_arm64_host_fails_before_docker(self):
        with tempfile.TemporaryDirectory() as tmp:
            result = subprocess.run(
                ["bash", str(PROBE), tmp],
                text=True, capture_output=True, timeout=5,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("native_armv7=UNKNOWN", (Path(tmp) / "RESULT.txt").read_text())
            self.assertNotIn("native_armv7=PASS", (Path(tmp) / "RESULT.txt").read_text())


if __name__ == "__main__":
    unittest.main()
