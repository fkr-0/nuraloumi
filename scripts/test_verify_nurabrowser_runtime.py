"""Offline producer-contract fixtures, not real ARM or SL101 qualification."""

import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("verify_nurabrowser_runtime.py")
spec = importlib.util.spec_from_file_location("browser_runtime_gate", SCRIPT)
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)
SHA = "a" * 40
REV = "b" * 40
TARGET = "armv7-unknown-linux-musleabihf"
ROLES = {
    "nurabrowser-core": "engine",
    "libexec/WPEWebProcess": "webprocess",
    "bin/bwrap": "bwrap",
    "bin/xdg-dbus-proxy": "xdg-dbus-proxy",
    "lib/ld-musl-armhf.so.1": "loader",
    "lib/libWPEWebKit.so": "library",
    "lib/modules/render.so": "module",
}


class BrowserRuntimeContract(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        entries = []
        for path, role in ROLES.items():
            file = self.root / path
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_bytes(b"offline fixture " + path.encode())
            if role in gate.EXECUTABLE_ROLES:
                file.chmod(0o755)
            entries.append({"path": path, "role": role,
                            "sha256": hashlib.sha256(file.read_bytes()).hexdigest()})
        self.document = {"schema": "nuraloumi-browser-runtime-v1", "source_sha": SHA,
                         "producer_revision": REV, "target": TARGET, "abi": "musl",
                         "qualification": {k: "PASS" for k in
                                           ("sandbox", "instruction_closure", "dlopen_closure", "webprocess", "review")},
                         "files": entries}
        self.save()

    def save(self):
        (self.root / "runtime.json").write_text(json.dumps(self.document))

    def verify(self):
        return gate.verify(self.root, SHA, TARGET, REV, inspector=lambda path, abi: None)

    def test_all_official_ci_lanes_run_runtime_contract_tests(self):
        repository = Path(__file__).resolve().parents[1]
        for workflow in ("ci.yml", "sl101-musl-armv7.yml", "sl101-debian-armhf.yml"):
            with self.subTest(workflow=workflow):
                text = (repository / ".github/workflows" / workflow).read_text()
                self.assertIn("scripts/test_verify_nurabrowser_runtime.py", text)
                if workflow != "ci.yml":
                    self.assertIn("components/nura-browser-engine/**", text)
                    self.assertIn("scripts/verify_nurabrowser_runtime.py", text)

    def test_complete_contract_accepts_offline_fixture(self):
        self.assertEqual(len(self.verify()["files"]), 7)

    def test_executable_processes_and_loader_required(self):
        for name, role in ROLES.items():
            if role not in gate.EXECUTABLE_ROLES:
                continue
            with self.subTest(name=name):
                file = self.root / name
                file.chmod(0o644)
                with self.assertRaisesRegex(ValueError, "not executable"):
                    self.verify()
                file.chmod(0o755)

    def test_absent_engine_rejects(self):
        (self.root / "nurabrowser-core").unlink()
        with self.assertRaisesRegex(ValueError, "missing regular"):
            self.verify()

    def test_missing_webprocess_or_helper_rejects(self):
        for name in ("libexec/WPEWebProcess", "bin/bwrap", "bin/xdg-dbus-proxy", "lib/modules/render.so"):
            with self.subTest(name=name):
                file = self.root / name
                data = file.read_bytes()
                file.unlink()
                with self.assertRaises(ValueError):
                    self.verify()
                file.write_bytes(data)

    def test_digest_tamper_rejects(self):
        (self.root / "lib/libWPEWebKit.so").write_bytes(b"tampered")
        with self.assertRaisesRegex(ValueError, "SHA256 mismatch"):
            self.verify()

    def test_wrong_source_or_stale_producer_rejects(self):
        for key in ("source_sha", "producer_revision"):
            with self.subTest(key=key):
                self.document[key] = "c" * 40
                self.save()
                with self.assertRaisesRegex(ValueError, "wrong browser source"):
                    self.verify()
                self.document[key] = SHA if key == "source_sha" else REV
                self.save()

    def test_wrong_abi_rejects(self):
        self.document["abi"] = "glibc"
        self.save()
        with self.assertRaisesRegex(ValueError, "wrong browser source"):
            self.verify()

    def test_unqualified_and_partial_reject(self):
        self.document["qualification"]["sandbox"] = "unqualified"
        self.save()
        with self.assertRaisesRegex(ValueError, "qualifications"):
            self.verify()
        self.document["qualification"]["sandbox"] = "PASS"
        self.document["files"].pop()
        self.save()
        with self.assertRaises(ValueError):
            self.verify()

    def test_symlink_and_traversal_reject(self):
        self.document["files"][0]["path"] = "../escape"
        self.save()
        with self.assertRaisesRegex(ValueError, "unsafe"):
            self.verify()

    def test_forbidden_arm_instruction_gate_not_bypassed(self):
        def reject_forbidden(path, abi):
            raise ValueError("forbidden ARM instruction")
        with self.assertRaisesRegex(ValueError, "forbidden ARM instruction"):
            gate.verify(self.root, SHA, TARGET, REV, inspector=reject_forbidden)


if __name__ == "__main__":
    unittest.main()
