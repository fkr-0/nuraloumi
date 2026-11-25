"""Synthetic fail-closed one-tab observer tests; no native WPE run is implied."""
import copy
import importlib.util
import os
from pathlib import Path
import tempfile
import unittest


PATH = Path(__file__).with_name("nurabrowser_one_tab_soak.py")
spec = importlib.util.spec_from_file_location("one_tab_soak", PATH)
soak = importlib.util.module_from_spec(spec)
spec.loader.exec_module(soak)

PIDS = {"ui": 101, "web": 102, "network": 103}
URI = "https://example.org/soak"
THERMAL = Path("/fake/thermal_zone0/temp")


def good_records():
    return {
        pid: {"pid": pid, "comm": soak.ROLES[role][:15], "state": "S",
              "start_ticks": pid * 100, "cpu_ticks": 50, "rss_kib": 5000}
        for role, pid in PIDS.items()
    }


class FakeBrowser:
    def __init__(self):
        self.responses = {
            "uri": {"uri": URI, "is_loading": False},
            "tabs": {"tabs": [{"uri": URI}], "active": 0},
        }

    def __call__(self, command):
        return copy.deepcopy(self.responses[command])


class SoakTests(unittest.TestCase):
    def setUp(self):
        self.records = good_records()
        self.browser = FakeBrowser()
        self.temp = 45000
        self.observer = soak.OneTabObserver(
            PIDS, URI, 20000, 75, [THERMAL],
            lambda pid: copy.deepcopy(self.records[pid]), self.browser,
            lambda path: self.temp,
        )

    def test_config_requires_exact_three_pids_and_limits(self):
        good = dict(pids=PIDS, duration=1800, interval=10,
                    max_rss_kib=20000, max_temp_c=75,
                    expected_uri=URI, thermal_paths=[THERMAL])
        soak.validate_config(**good)
        invalid = [
            ("pids", {"ui": 101, "web": 102}),
            ("pids", {"ui": 101, "web": 101, "network": 103}),
            ("pids", {"ui": 0, "web": 102, "network": 103}),
            ("duration", 0), ("duration", 1800.1),
            ("interval", 0.01), ("interval", 31),
            ("max_rss_kib", 0), ("max_temp_c", float("nan")),
            ("max_temp_c", 126), ("expected_uri", "file:///tmp/poc"),
            ("expected_uri", ""), ("thermal_paths", []),
            ("thermal_paths", [THERMAL, THERMAL]),
        ]
        for key, value in invalid:
            with self.subTest(key=key, value=value):
                with self.assertRaises(ValueError):
                    soak.validate_config(**(good | {key: value}))

    def test_read_only_own_process_and_fixture_thermal(self):
        row = soak.read_process(os.getpid())
        self.assertEqual(row["pid"], os.getpid())
        self.assertGreater(row["rss_kib"], 0)
        self.assertGreater(row["start_ticks"], 0)
        self.assertTrue(row["comm"])
        self.assertIn(row["state"], ("R", "S"))
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "temp"
            path.write_text("42000\n", encoding="ascii")
            self.assertEqual(soak.read_thermal_millic(path), 42000)
            path.write_text("9999999\n", encoding="ascii")
            with self.assertRaises(ValueError):
                soak.read_thermal_millic(path)

    def test_one_tab_sample_keeps_process_and_temperature_evidence(self):
        result = self.observer.sample()
        self.assertEqual(result["aggregate_rss_kib"], 15000)
        self.assertEqual(result["tab_count"], 1)
        self.assertEqual(result["processes"]["network"]["comm"], "WPENetworkProce")
        self.assertEqual(result["processes"]["ui"]["comm"], "nurabrowser-cor")
        self.assertEqual(result["page_uri"], URI)
        self.assertEqual(result["thermal_millic"], {str(THERMAL): 45000})
        self.assertEqual(set(result["processes"]), set(PIDS))

    def test_fails_on_identity_reuse_zombie_or_absent_process(self):
        for role, field, value in [
            ("web", "comm", "unexpected"),
            ("web", "pid", 999),
            ("web", "state", "Z"),
        ]:
            with self.subTest(field=field):
                observer = self.fresh_observer()
                self.records[PIDS[role]][field] = value
                with self.assertRaises(RuntimeError):
                    observer.sample()
                self.records = good_records()
        self.observer.sample()
        self.records[PIDS["web"]]["start_ticks"] += 1
        with self.assertRaisesRegex(RuntimeError, "reused"):
            self.observer.sample()
        self.records = good_records()
        del self.records[PIDS["network"]]
        with self.assertRaises(KeyError):
            self.fresh_observer().sample()

    def fresh_observer(self):
        return soak.OneTabObserver(
            PIDS, URI, 20000, 75, [THERMAL],
            lambda pid: copy.deepcopy(self.records[pid]),
            self.browser, lambda path: self.temp,
        )

    def test_wrong_tab_uri_loading_or_ambiguous_tabs_fails(self):
        invalid = [
            ("uri", {"uri": "https://other.example/", "is_loading": False}),
            ("uri", {"uri": URI, "is_loading": True}),
            ("uri", {"uri": URI}),
            ("tabs", {"tabs": [{"uri": URI}, {"uri": URI}], "active": 0}),
            ("tabs", {"tabs": [{"uri": URI}], "active": 1}),
            ("tabs", {"tabs": [URI], "active": 0}),
            ("tabs", {"tabs": [{"uri": "https://other.example/"}], "active": 0}),
            ("tabs", {"tabs": [], "active": 0}),
        ]
        for command, response in invalid:
            with self.subTest(command=command, response=response):
                self.browser = FakeBrowser()
                self.browser.responses[command] = response
                with self.assertRaises(RuntimeError):
                    self.fresh_observer().sample()

    def test_resource_abort_thresholds(self):
        self.records[PIDS["web"]]["rss_kib"] = 12000
        with self.assertRaisesRegex(RuntimeError, "RSS"):
            self.observer.sample()
        self.records = good_records()
        self.temp = 76000
        with self.assertRaisesRegex(RuntimeError, "Thermal"):
            self.fresh_observer().sample()

    def test_short_probe_does_not_claim_30_minute_liveness(self):
        now = [0.0]
        samples = []
        summary = soak.run_soak(
            self.observer, 1.0, 0.5, samples.append,
            clock=lambda: now[0], sleeper=lambda seconds: now.__setitem__(0, now[0] + seconds),
        )
        self.assertEqual(len(samples), 3)
        self.assertEqual(summary["result"], "SHORT_PROBE_ONLY")
        self.assertEqual(summary["release_qualification"], "NOT_ESTABLISHED")

    def test_synthetic_full_interval_is_not_release_qualification(self):
        now = [0.0]
        samples = []
        summary = soak.run_soak(
            self.observer, 1800, 10, samples.append,
            clock=lambda: now[0], sleeper=lambda seconds: now.__setitem__(0, now[0] + seconds),
        )
        self.assertEqual(len(samples), 181)
        self.assertEqual(samples[-1]["elapsed_seconds"], 1800)
        self.assertEqual(summary["result"], "LIVENESS_PASS")
        self.assertEqual(summary["release_qualification"], "NOT_ESTABLISHED")


if __name__ == "__main__":
    unittest.main()
