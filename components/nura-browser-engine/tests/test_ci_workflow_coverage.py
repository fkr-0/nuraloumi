"""Ensure main CI executes the complete browser Python regression inventory."""
from pathlib import Path
import unittest

REPO = Path(__file__).resolve().parents[3]
WORKFLOW = REPO / ".github/workflows/ci.yml"
TESTS = Path(__file__).resolve().parent


class BrowserCiCoverageTests(unittest.TestCase):
    def test_full_browser_python_discovery_is_registered(self):
        workflow = WORKFLOW.read_text(encoding="utf-8")
        command = (
            "python3 -m unittest discover -s "
            "components/nura-browser-engine/tests -p 'test_*.py' -v"
        )
        self.assertEqual(workflow.count(command), 1)
        self.assertIn("      - name: Browser/video GPU isolation", workflow)
        self.assertNotRegex(workflow, r"(?m)^\s*if:\s*false\s*$")

    def test_private_engine_meson_install_contract(self):
        meson = (REPO / 'components/nura-browser-engine/meson.build').read_text(encoding='utf-8')
        self.assertEqual(meson.count("executable('nurabrowser-core', browser_sources,"), 1)
        self.assertRegex(meson, r"(?s)executable\('nurabrowser-core', browser_sources,.*?install:\s*true,\s*install_dir:\s*get_option\('libexecdir'\)\s*/\s*'nuraloumi'\s*/\s*'browser'\)")
        self.assertNotRegex(meson, r"(?m)^\s*(?:executable|install_data|install_subdir|install_symlink)\s*\(\s*['\"](?:sl101-browse|,browse)['\"]")
        self.assertNotIn('rename:', meson)

    def test_ci_discovery_includes_every_browser_test_module(self):
        modules = sorted(path.name for path in TESTS.glob("test_*.py"))
        self.assertIn("test_offline_qualification.py", modules)
        self.assertIn("test_migration_contract.py", modules)
        self.assertIn("test_runtime_closure.py", modules)
        self.assertIn("test_ci_workflow_coverage.py", modules)
        self.assertGreaterEqual(len(modules), 8)
        workflow = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("-p 'test_*.py'", workflow)
        self.assertNotIn("unittest discover -s components/nura-browser-engine/tests -p 'test_migration_contract.py'", workflow)


if __name__ == "__main__":
    unittest.main()
