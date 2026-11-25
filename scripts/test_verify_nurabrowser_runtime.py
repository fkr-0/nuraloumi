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
    "libexec/WPENetworkProcess": "networkprocess",
    "bin/bwrap": "bwrap",
    "bin/xdg-dbus-proxy": "xdg-dbus-proxy",
    "lib/ld-musl-armhf.so.1": "loader",
    "lib/libWPEWebKit.so": "library",
    "lib/modules/render.so": "module",
    "share/webkit/icudtl.dat": "resource",
}


class DecodedISAGate(unittest.TestCase):
    def test_d15_and_normal_vfp_instruction_are_allowed(self):
        gate.verify_decoded_isa("  0008: vldr d15, [pc, #4]\n"
                                "  000c: vmov.f64 d0, d15\n"
                                "00000000 <symbol_d16>:\n")

    def test_upper_vfp_registers_rejected_even_with_d16_attributes(self):
        for operand in ("d16", "d17", "d23", "d30", "d31"):
            with self.subTest(operand=operand):
                with self.assertRaisesRegex(ValueError, "upper VFP"):
                    gate.verify_decoded_isa(f"  1950c0: vldr {operand}, [pc, #136]\n")

    def test_neon_q_registers_and_decoded_opcodes_rejected(self):
        for line in ("  0004: vld1.32 {d0, d1}, [r0]",
                     "  0004: vst1.32 {d0}, [r0]",
                     "  0004: vrev64.8 d0, d1",
                     "  0004: vmov q0, q1",
                     "  0004: sdiv r0, r1, r2",
                     "  0004: udiv r0, r1, r2"):
            with self.subTest(line=line):
                with self.assertRaisesRegex(ValueError, "forbidden|NEON"):
                    gate.verify_decoded_isa(line)


class DynamicABIGate(unittest.TestCase):
    def test_musl_rejects_glibc_needed_even_without_interpreter(self):
        for soname in ("libc.so.6", "libpthread.so.0", "libdl.so.2",
                       "librt.so.1", "libm.so.6", "libresolv.so.2",
                       "ld-linux-armhf.so.3"):
            with self.subTest(soname=soname):
                with self.assertRaisesRegex(ValueError, "glibc dependency"):
                    gate.verify_dynamic_abi(
                        f" 0x00000001 (NEEDED) Shared library: [{soname}]", "musl")

    def test_glibc_rejects_musl_and_musl_accepts_its_own_libc(self):
        dynamic = " 0x00000001 (NEEDED) Shared library: [libc.musl-armv7.so.1]"
        gate.verify_dynamic_abi(dynamic, "musl")
        with self.assertRaisesRegex(ValueError, "musl dependency"):
            gate.verify_dynamic_abi(dynamic, "glibc")

    def test_regular_wpe_shared_libraries_are_not_blocked(self):
        gate.verify_dynamic_abi(
            " 0x00000001 (NEEDED) Shared library: [libWPEPlatform-2.0.so.2]\n"
            " 0x00000001 (NEEDED) Shared library: [libc.musl-armv7.so.1]",
            "musl")


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

    def test_musl_package_consumes_only_exact_qualified_same_commit_producer(self):
        repository = Path(__file__).resolve().parents[1]
        workflow = (repository / ".github/workflows/sl101-musl-armv7.yml").read_text()
        self.assertIn("browser_producer_run_id", workflow)
        self.assertIn("browser_runtime_manifest_sha256", workflow)
        self.assertIn("nurabrowser-wpe-musl-qualified-${{ github.sha }}", workflow)
        self.assertIn('run.get("path") == ".github/workflows/nurabrowser-wpe-musl-producer.yml"', workflow)
        self.assertIn('run.get("head_sha") == expected_sha', workflow)
        self.assertIn("verify_nurabrowser_runtime.py target/browser-producer-download", workflow)

    def test_complete_contract_accepts_offline_fixture(self):
        self.assertEqual(len(self.verify()["files"]), 9)

    def test_non_elf_resources_are_hashed_without_elf_inspection(self):
        inspected = []
        gate.verify(self.root, SHA, TARGET, REV,
                    inspector=lambda path, abi: inspected.append(path.relative_to(self.root).as_posix()))
        self.assertIn("lib/modules/render.so", inspected)
        self.assertNotIn("share/webkit/icudtl.dat", inspected)

    def test_executable_resource_is_rejected(self):
        resource = self.root / "share/webkit/icudtl.dat"
        resource.chmod(0o755)
        with self.assertRaisesRegex(ValueError, "resource must not be executable"):
            self.verify()

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
        for name in ("libexec/WPEWebProcess", "libexec/WPENetworkProcess",
                     "bin/bwrap", "bin/xdg-dbus-proxy", "lib/modules/render.so"):
            with self.subTest(name=name):
                file = self.root / name
                data = file.read_bytes()
                file.unlink()
                with self.assertRaises(ValueError):
                    self.verify()
                file.write_bytes(data)

    def test_wpe_process_roles_and_shared_libexec_dir(self):
        entry = next(e for e in self.document["files"] if e["role"] == "networkprocess")
        entry["role"] = "module"
        self.save()
        with self.assertRaisesRegex(ValueError, "missing engine, WPE processes"):
            self.verify()
        entry["role"] = "networkprocess"
        entry["path"] = "bin/WPENetworkProcess"
        (self.root / "libexec/WPENetworkProcess").rename(self.root / "bin/WPENetworkProcess")
        self.save()
        with self.assertRaisesRegex(ValueError, "share the compiled libexec"):
            self.verify()

    def test_optional_gpu_process_must_be_in_same_libexec_dir(self):
        gpu = self.root / "libexec/WPEGPUProcess"
        gpu.write_bytes(b"optional synthetic gpu")
        gpu.chmod(0o755)
        entry = {"role": "gpuprocess", "path": "libexec/WPEGPUProcess",
                 "sha256": hashlib.sha256(gpu.read_bytes()).hexdigest()}
        self.document["files"].append(entry)
        self.save()
        self.verify()
        (self.root / "bin/WPEGPUProcess").write_bytes(gpu.read_bytes())
        (self.root / "bin/WPEGPUProcess").chmod(0o755)
        gpu.unlink()
        entry["path"] = "bin/WPEGPUProcess"
        self.save()
        with self.assertRaisesRegex(ValueError, "share the compiled libexec"):
            self.verify()

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
