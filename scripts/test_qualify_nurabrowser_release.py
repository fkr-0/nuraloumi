"""Hostile archive and provenance fixtures; never invoke a device."""
import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "gate", Path(__file__).with_name("qualify-nurabrowser-release.py"))
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)
SHA = "a" * 40


class Gate5Tests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.path = Path(temp.name) / "bundle.tar"
        self.files = {
            gate.PUBLIC: b"\x7fELFpublic",
            gate.ENGINE: b"\x7fELFengine",
            gate.DESKTOP: b"[Desktop Entry]\nType=Application\nExec=nuraloumi-browser %u\n",
        }
        self.manifest = {
            "schema": "nuraloumi.browser.release.v1",
            "source_sha": SHA,
            "target": "armv7-alpine-linux-musleabihf",
            "gate2": "PASS",
            "files": {name: gate.digest(data) for name, data in self.files.items()},
            "launch": "nuraloumi-browser",
            "rollback": "retain-previous-atomically",
        }

    def bundle(self, extra=None):
        with tarfile.open(self.path, "w") as archive:
            for name, data in (self.files | (extra or {})).items():
                member = tarfile.TarInfo(name)
                member.size = len(data)
                archive.addfile(member, io.BytesIO(data))
        return gate.check_archive(self.path, gate.digest(self.path.read_bytes()))

    def test_valid_contract(self):
        self.assertEqual(gate.check_contract(self.bundle(), self.manifest, SHA),
                         self.manifest["files"])

    def test_gate2_pending(self):
        self.manifest["gate2"] = "PENDING"
        with self.assertRaisesRegex(ValueError, "Gate 2"):
            gate.check_contract(self.bundle(), self.manifest, SHA)

    def test_wrong_source(self):
        with self.assertRaisesRegex(ValueError, "provenance"):
            gate.check_contract(self.bundle(), self.manifest, "b" * 40)

    def test_historical_alias(self):
        with self.assertRaisesRegex(ValueError, "historical"):
            gate.check_contract(self.bundle({"usr/bin/,browse": b"old"}), self.manifest, SHA)

    def test_modified_binary(self):
        self.files[gate.ENGINE] = b"bad"
        with self.assertRaisesRegex(ValueError, "digests"):
            gate.check_contract(self.bundle(), self.manifest, SHA)

    def test_path_escape(self):
        with self.assertRaisesRegex(ValueError, "unsafe archive"):
            self.bundle({"../escape": b"x"})

    def test_bad_desktop(self):
        self.files[gate.DESKTOP] = b"[Desktop Entry]\nType=Application\nExec=,browse\n"
        self.manifest["files"] = {name: gate.digest(data) for name, data in self.files.items()}
        with self.assertRaisesRegex(ValueError, "desktop launch"):
            gate.check_contract(self.bundle(), self.manifest, SHA)

    def test_bad_approved_digest(self):
        self.bundle()
        with self.assertRaisesRegex(ValueError, "digest mismatch"):
            gate.check_archive(self.path, "0" * 64)

    def test_missing_rollback(self):
        self.manifest.pop("rollback")
        with self.assertRaisesRegex(ValueError, "rollback"):
            gate.check_contract(self.bundle(), self.manifest, SHA)


if __name__ == "__main__":
    unittest.main()
