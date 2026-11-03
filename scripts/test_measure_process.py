"""Pure parser tests plus a read-only snapshot of our own process."""
import importlib.util
import os
import pathlib
import unittest

PATH = pathlib.Path(__file__).with_name("measure-process.py")
spec = importlib.util.spec_from_file_location("measure_process", PATH)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class SamplerTests(unittest.TestCase):
    def test_stat_with_spaces_and_embedded_close_paren(self):
        fields = ["S"] + ["0"] * 21
        fields[11], fields[12], fields[19], fields[21] = "3", "5", "7", "9"
        ticks, start, rss = module.parse_stat("123 (foo ) process) " + " ".join(fields))
        self.assertEqual((ticks, start, rss), (8, 7, 9))

    def test_bounds_fail_closed(self):
        for samples, interval, pids in [
            (0, 1.0, [1]), (61, 1.0, [1]), (2, 0.01, [1]),
            (2, 2.1, [1]), (2, 0.5, [1, 1]), (2, 0.5, [-1]),
            (2, 0.5, list(range(1, 18))),
        ]:
            with self.subTest(samples=samples, interval=interval, pids=pids):
                with self.assertRaises(ValueError):
                    module.bounded(samples, interval, pids)

    def test_snapshots_own_pid(self):
        result = module.snapshot(os.getpid())
        self.assertGreater(result["rss_kib"], 0)
        self.assertGreater(result["start_ticks"], 0)


if __name__ == "__main__":
    unittest.main()
