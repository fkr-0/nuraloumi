"""Deterministic positive/negative controls for the /proc observation gate."""
import tempfile
import unittest
from pathlib import Path

from verify_nurabrowser_sandbox_snapshot import inspect


class SandboxSnapshotTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.pids = {"ui": 101, "web": 102, "network": 103}
        for role, pid in self.pids.items():
            proc = self.root / str(pid)
            (proc / "ns").mkdir(parents=True)
            (proc / "status").write_text("NoNewPrivs:\t1\nSeccomp:\t2\nSeccomp_filters:\t1\n")
            (proc / "mountinfo").write_text("1 0 0:1 / /sandbox rw - tmpfs tmpfs rw\n")
            for name in ("mnt", "net", "pid", "user", "uts", "ipc"):
                (proc / "ns" / name).symlink_to(f"{name}:[{pid}]")

    def test_positive_snapshot_still_blocks_release(self):
        result = inspect(self.root, self.pids)
        self.assertEqual(result["snapshot_gate"], "SNAPSHOT_PASS_ONLY")
        self.assertEqual(result["release_gate"], "BLOCKED")

    def test_missing_network_filter(self):
        path = self.root / "103/status"
        path.write_text(path.read_text().replace("Seccomp:\t2", "Seccomp:\t0"))
        self.assertIn("network: effective", " ".join(inspect(self.root, self.pids)["failures"]))

    def test_shared_mount_namespace(self):
        link = self.root / "103/ns/mnt"
        link.unlink()
        link.symlink_to("mnt:[101]")
        self.assertIn("network: shared mnt", " ".join(inspect(self.root, self.pids)["failures"]))

    def test_broad_runtime_exposure(self):
        with (self.root / "103/mountinfo").open("a") as output:
            output.write("2 0 0:2 / /run/user/10000 rw - tmpfs tmpfs rw\n")
        self.assertIn("entire desktop runtime", " ".join(inspect(self.root, self.pids)["failures"]))

    def test_missing_proc_entry(self):
        (self.root / "103/status").unlink()
        self.assertEqual(inspect(self.root, self.pids)["snapshot_gate"], "BLOCKED")


if __name__ == "__main__":
    unittest.main()
