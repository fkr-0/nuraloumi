"""Fail-closed contracts for the NuraLoumi-owned ARMHF browser CI job."""
from pathlib import Path
import os
import subprocess
import unittest

ROOT = Path(__file__).resolve().parent.parent
WORKFLOW = ROOT / ".github/workflows/nurabrowser-engine-armhf.yml"
BUILDER = ROOT / "scripts/ci-build-nurabrowser-engine.sh"


class NativeBrowserEngineCi(unittest.TestCase):
    def test_workflow_has_pinned_input_integrity_and_ci_only_output(self):
        text = WORKFLOW.read_text()
        self.assertIn("sha256sum -c", text)
        self.assertIn("Dockerfile.ci-armhf", text)
        self.assertIn("nurabrowser-engine-armhf.tar", text)
        self.assertIn("CI_SOURCE_SHA", text)
        self.assertIn("--network none", text)
        self.assertIn("if-no-files-found: error", text)
        self.assertNotIn("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS", text)
        self.assertNotIn("ssh ", text)
        self.assertNotIn("scp ", text)

    def test_ci_stage_is_internal_and_unqualified(self):
        text = BUILDER.read_text()
        self.assertIn("DESTDIR=/out/stage meson install", text)
        self.assertIn("libexec/nuraloumi/browser/nurabrowser-core", text)
        self.assertIn("dependency_instruction_closure=unqualified", text)
        self.assertIn("device_deployment=forbidden", text)
        self.assertNotIn("/usr/bin/,browse", text)
        self.assertNotIn("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS", text)

    def test_invalid_commit_fails_before_any_build_or_write(self):
        for value in ("", "abc", "Z" * 40, "0" * 39, "0" * 41):
            with self.subTest(source_sha=value):
                result = subprocess.run(
                    ["sh", str(BUILDER)],
                    cwd=ROOT,
                    env={**os.environ, "CI_SOURCE_SHA": value},
                    capture_output=True, text=True, timeout=5,
                )
                self.assertEqual(result.returncode, 2)


if __name__ == "__main__":
    unittest.main()
