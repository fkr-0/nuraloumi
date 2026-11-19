"""Native core sandbox startup regression: only local, bounded invocations.

Set NURABROWSER_TEST_CORE to an already built native Meson executable.
No GUI, network, private-root, device, or compositor operations.
"""
import os
import pathlib
import subprocess
import unittest


class SandboxRuntimeStartup(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        value = os.environ.get("NURABROWSER_TEST_CORE")
        if not value:
            raise unittest.SkipTest("NURABROWSER_TEST_CORE not supplied")
        cls.core = pathlib.Path(value)
        if not cls.core.is_file():
            raise AssertionError(f"missing native core: {cls.core}")

    def invoke(self, overrides):
        env = os.environ.copy()
        env.pop("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS", None)
        env.pop("WEBKIT_USE_SANDBOX", None)
        env.update(overrides)
        return subprocess.run(
            [str(self.core), "--platform=headless", "--no-control",
             "--url=about:blank", "--exit-after-ms=1000"],
            env=env, capture_output=True, text=True, timeout=8, check=False,
        )

    def test_explicit_sandbox_disable_rejected(self):
        for value in ("0", "false", ""):
            with self.subTest(value=value):
                result = self.invoke({"WEBKIT_USE_SANDBOX": value})
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("WEBKIT_USE_SANDBOX must remain enabled", result.stderr)

    def test_dangerous_override_rejected_even_if_empty(self):
        result = self.invoke({"WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS": ""})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("refusing to run with WebKit sandbox disabled", result.stderr)

    def test_missing_helpers_rejected_before_webkit(self):
        result = self.invoke({"PATH": "/nonexistent"})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("sandbox helpers unavailable", result.stderr)


if __name__ == "__main__":
    unittest.main()
