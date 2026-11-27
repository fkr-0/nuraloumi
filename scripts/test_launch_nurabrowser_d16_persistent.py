"""Host-only checks for the persistent, no-network D16 crypto builder launcher."""
from pathlib import Path
import subprocess
import unittest

SCRIPT = Path(__file__).with_name("launch_nurabrowser_d16_persistent.sh")


class PersistentCryptoLauncherContract(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.text = SCRIPT.read_text()

    def test_shell_syntax(self):
        self.assertEqual(
            subprocess.run(["sh", "-n", str(SCRIPT)], capture_output=True).returncode,
            0,
        )

    def test_persistent_and_fail_closed(self):
        for required in (
            "local-state/build/nurabrowser-d16",
            'sources="$state/sources"',
            'overlay="$state/overlay"',
            'SOURCE-MANIFEST.json',
            'existing build work detected',
            "less than 8 GiB free",
            'docker container inspect "$container"',
        ):
            with self.subTest(required=required):
                self.assertIn(required, self.text)

    def test_no_network_or_host_install(self):
        for required in (
            "--network none", "--read-only", "--cap-drop ALL",
            "--security-opt no-new-privileges", "--pull never",
            "--platform linux/arm/v7", "--tmpfs /tmp:rw,size=256m",
        ):
            with self.subTest(required=required):
                self.assertIn(required, self.text)
        self.assertNotIn("--privileged", self.text)
        self.assertNotIn("--rm", self.text)
        self.assertNotIn("ssh ", self.text)
        self.assertNotIn("sudo ", self.text)

    def test_uses_all_twenty_cores_with_memory_limit(self):
        self.assertIn('--cpus 20', self.text)
        self.assertIn('--memory 16g', self.text)
        self.assertIn('--env NURA_BUILD_JOBS=20', self.text)

    def test_only_source_and_output_binds(self):
        self.assertIn('src=$scripts,dst=/work/scripts,readonly', self.text)
        self.assertIn('src=$sources,dst=/inputs,readonly', self.text)
        self.assertIn('src=$overlay,dst=/out', self.text)
        self.assertNotIn('src=$repo,dst=/work', self.text)


if __name__ == "__main__":
    unittest.main()
