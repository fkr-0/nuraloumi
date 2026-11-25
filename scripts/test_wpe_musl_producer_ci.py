"""Fail-closed contract for the long-running ARMv7/musl WPE candidate job."""

from pathlib import Path
import subprocess
import textwrap
import unittest

ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github/workflows/nurabrowser-wpe-musl-producer.yml"
BUILDER = ROOT / "scripts/ci-build-nurabrowser-wpe-musl.sh"


class WpeMuslProducerCiTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.workflow = WORKFLOW.read_text(encoding="utf-8")
        cls.builder = BUILDER.read_text(encoding="utf-8")

    def test_build_is_bounded_and_preserves_failure_status(self):
        for part in (
            "timeout-minutes: 360",
            "timeout --signal=TERM --kill-after=30s 5h30m",
            "statuses=(",
            "PIPESTATUS[@]",
            "builder_rc=",
            "log_rc=",
            "BUILD-LOG.txt",
            "BUILD-STATUS.txt",
            "qualification=UNQUALIFIED",
            "if test \"$builder_rc\" -ne 0 || test \"$log_rc\" -ne 0",
        ):
            self.assertIn(part, self.workflow)

    def test_build_shell_syntax_parses_without_docker(self):
        part = self.workflow.split(
            "      - name: Build native Alpine 3.22 ARMv7 candidate", 1
        )[1].split("      - name: Retain WPE musl Ninja diagnostics", 1)[0]
        run = textwrap.dedent(part.split("        run: |\n", 1)[1])
        checked = subprocess.run(
            ["bash", "-n"], input=run, capture_output=True,
            text=True, timeout=5,
        )
        self.assertEqual(checked.returncode, 0, checked.stderr)

    def test_diagnostics_survive_failure_but_candidate_is_success_only(self):
        diagnostic = self.workflow.split(
            "      - name: Retain WPE musl Ninja diagnostics", 1
        )[1].split("      - name: Upload unqualified build evidence", 1)[0]
        self.assertIn("if: " + chr(36) + "{{ always() }}", diagnostic)
        self.assertIn("BUILD-LOG.txt", diagnostic)
        self.assertIn("BUILD-STATUS.txt", diagnostic)
        candidate = self.workflow.split(
            "      - name: Upload unqualified build evidence", 1
        )[1]
        self.assertIn("if: " + chr(36) + "{{ success() }}", candidate)
        self.assertIn("runtime_qualification=UNQUALIFIED", self.workflow)

    def test_never_pretends_candidate_is_deployable(self):
        for part in (
            "runtime_qualification=UNQUALIFIED",
            "device_deployment=FORBIDDEN",
            "missing=transitive_elf_dlopen_sandbox_real_pixels",
        ):
            self.assertIn(part, self.builder)
        self.assertNotIn("runtime_qualification=PASS", self.builder)


if __name__ == "__main__":
    unittest.main()
