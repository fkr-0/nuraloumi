"""Userspace battery sensor validity and release checks; never contacts device."""
import importlib.util
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('userspace_release', ROOT / 'scripts/sl101_userspace_release.py')
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)


class BatteryPolicy(unittest.TestCase):
    def test_no_fixed_capacity_threshold(self):
        for level in (0, 5, 28, 39, 40, 90, 100):
            self.assertTrue(mod.battery_reading_valid(level))

    def test_invalid_readings_fail_closed(self):
        for level in (-1, 101, 150):
            self.assertFalse(mod.battery_reading_valid(level))

    def test_release_gate_logs_power_without_minimum(self):
        source = (ROOT / 'scripts/sl101_userspace_release.py').read_text()
        self.assertIn('threshold=none', source)
        self.assertNotIn('battery-reserve', source)

    def test_qualification_does_not_require_ac(self):
        source = (ROOT / 'scripts/qualify-nuraloumi-player-video-on-device.sh').read_text()
        self.assertNotIn('|| blocked ac\n', source)
        self.assertIn('GATE_POWER_PASS', source)


if __name__ == '__main__':
    unittest.main()
