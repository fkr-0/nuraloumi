"""Fail-closed contract for the long-running ARMv7/musl WPE candidate job."""

from pathlib import Path
import hashlib
import io
import shutil
import subprocess
import tarfile
import tempfile
import textwrap
import unittest

ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github/workflows/nurabrowser-wpe-musl-producer.yml"
BUILDER = ROOT / "scripts/ci-build-nurabrowser-wpe-musl.sh"
CHECKPOINT = ROOT / "scripts/ci-wpe-checkpoint.sh"
FAKE_SHA = "a" * 40


class WpeMuslProducerCiTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.workflow = WORKFLOW.read_text(encoding="utf-8")
        cls.builder = BUILDER.read_text(encoding="utf-8")

    def test_build_is_bounded_and_preserves_failure_status(self):
        for part in (
            "timeout-minutes: 360",
            "timeout --signal=TERM --kill-after=120s 270m",
            "statuses=(",
            "PIPESTATUS[@]",
            "builder_rc=",
            "log_rc=",
            "Docker-CID.txt",
            "docker rm --force",
            "BUILD-LOG.txt",
            "BUILD-STATUS.txt",
            "qualification=UNQUALIFIED",
            "if test \"$builder_rc\" -ne 0 || test \"$log_rc\" -ne 0",
        ):
            self.assertIn(part, self.workflow)

    def test_arm64_kernel_must_not_be_mistaken_for_armv7_userspace(self):
        # The native AArch32 container inherits the host kernel's aarch64 uname.
        # WPE 2.54.2 WebKitCommon explicitly supports FORCE_32BIT on Arm64.
        for part in (
            "-DFORCE_32BIT=ON -DUSE_MIMALLOC=ON -DUSE_SYSTEM_MALLOC=OFF",
            "CMAKE_SYSTEM_PROCESSOR:INTERNAL=armv7l",
            "USE_MIMALLOC:BOOL=ON",
            "USE_SYSTEM_MALLOC:BOOL=OFF",
            "WPE CMake did not select the ARMv7 target processor",
            "WPE CMake did not select the ARMv7 mimalloc backend",
            "cmake --build /out/build/webkit --target WPEPlatformHeadless --parallel 2",
        ):
            self.assertIn(part, self.builder)
        # Reject the old configuration before spending time on Ninja.
        self.assertLess(
            self.builder.index("CMAKE_SYSTEM_PROCESSOR:INTERNAL=armv7l"),
            self.builder.index("cmake --build /out/build/webkit"),
        )
        self.assertLess(
            self.builder.index("--target WPEPlatformHeadless --parallel 2"),
            self.builder.index("cmake --build /out/build/webkit --parallel 2"),
        )

    def test_timeout_interval_is_accepted_by_coreutils(self):
        result = subprocess.run(
            ["timeout", "--signal=TERM", "--kill-after=120s", "270m", "true"],
            capture_output=True, text=True, timeout=5,
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_real_arm64_host_executes_armv7_directly(self):
        job = self.workflow.split("  native-armv7-musl-wpe-candidate:", 1)[1]
        self.assertIn("runs-on: ubuntu-24.04-arm", job)
        self.assertIn("bash scripts/ci-probe-armv7-native-compat.sh", job)
        self.assertIn("docker run --rm --cidfile", job)
        self.assertIn("--platform linux/arm/v7", job)
        self.assertNotIn("setup-qemu-action", job)
        self.assertNotIn("qemu-arm-static", job)

    def test_opt_in_media_candidate_is_fresh_and_never_the_default(self):
        # A source-only CI_WPE_MEDIA gate is inert unless the hosted producer
        # forwards it. Normal pull requests retain the previous no-video build.
        self.assertIn("      media_enabled:", self.workflow)
        self.assertIn("        default: false", self.workflow.split(
            "      media_enabled:", 1)[1].split("      resume_run_id:", 1)[0])
        self.assertIn(
            "-e CI_WPE_MEDIA=\"${{ github.event_name == 'workflow_dispatch' && inputs.media_enabled && '1' || '0' }}\"",
            self.workflow,
        )
        self.assertIn("MEDIA_ENABLED:", self.workflow)
        self.assertIn("Media-enabled WPE requires a fresh build", self.workflow)
        self.assertIn("media builds require a fresh isolated /out", self.builder)
        self.assertIn("media_video=ON", self.builder)
        self.assertIn("media_gstreamer=ON", self.builder)
        self.assertIn("media_video=OFF", self.builder)
        self.assertIn(
            "python3 -m unittest scripts/test_verify_nurabrowser_media_candidate.py -v",
            self.workflow,
        )

    def test_build_shell_syntax_parses_without_docker(self):
        part = self.workflow.split(
            "      - name: Build native Alpine 3.22 ARMv7 candidate", 1
        )[1].split("      - name: Retain WPE musl Ninja diagnostics", 1)[0]
        run = textwrap.dedent(part.split("        run: |\n", 1)[1])
        checked = subprocess.run(
            ["bash", "-n"], input=run, capture_output=True,
            text=True, timeout=5,
        )
        self.assertEqual(checked.returncode, 0, checked.stderr)

    def test_diagnostics_survive_failure_but_candidate_is_success_only(self):
        diagnostic = self.workflow.split(
            "      - name: Retain WPE musl Ninja diagnostics", 1
        )[1].split("      - name: Upload unqualified build evidence", 1)[0]
        self.assertIn("if: " + chr(36) + "{{ always() }}", diagnostic)
        self.assertIn("BUILD-LOG.txt", diagnostic)
        self.assertIn("BUILD-STATUS.txt", diagnostic)
        candidate = self.workflow.split(
            "      - name: Upload unqualified build evidence", 1
        )[1]
        self.assertIn("if: " + chr(36) + "{{ success() }}", candidate)
        self.assertIn("runtime_qualification=UNQUALIFIED", self.workflow)

    def test_never_pretends_candidate_is_deployable(self):
        for part in (
            "runtime_qualification=UNQUALIFIED",
            "device_deployment=FORBIDDEN",
            "missing=transitive_elf_dlopen_sandbox_real_pixels",
        ):
            self.assertIn(part, self.builder)
        self.assertNotIn("runtime_qualification=PASS", self.builder)

    def test_resume_requires_exact_source_and_unqualified_state(self):
        for part in (
            "inputs.resume_run_id",
            "inputs.resume_archive_sha256",
            'run.get("path") == ".github/workflows/nurabrowser-wpe-musl-producer.yml"',
            'run.get("head_sha") == os.environ["GITHUB_SHA"]',
            'run.get("conclusion") == "failure"',
            "run-id: ${{ inputs.resume_run_id }}",
            "target/wpe-resume-download/build-state.tar.zst",
            "bash scripts/ci-wpe-checkpoint.sh restore",
            "compression-level: 0",
        ):
            self.assertIn(part, self.workflow)
        for part in (
            "source_sha=$CI_SOURCE_SHA",
            "builder_sha256=$builder_sha256",
            "packages_sha256=$packages_sha256",
            "WPE_RESTORED_NINJA=VERIFIED",
        ):
            self.assertIn(part, self.builder)

    def test_checkpoint_shell_syntax(self):
        for path in (BUILDER, CHECKPOINT):
            run = subprocess.run(
                ["sh" if path == BUILDER else "bash", "-n", str(path)],
                capture_output=True, text=True, timeout=5,
            )
            self.assertEqual(run.returncode, 0, run.stderr)

    @unittest.skipUnless(shutil.which("zstd"), "zstd required for archive smoke")
    def test_checkpoint_round_trip_and_rejects_wrong_digest(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "build-output"
            restored = Path(tmp) / "restored"
            archive = Path(tmp) / "build-state.tar.zst"
            (root / "src/wpewebkit-2.54.2").mkdir(parents=True)
            (root / "build/webkit").mkdir(parents=True)
            (root / "src/wpewebkit-2.54.2/CMakeLists.txt").write_text("project(WPE)\n")
            (root / "build/webkit/build.ninja").write_text("rule cc\n")
            (root / "BUILD-PROVENANCE.txt").write_text(
                f"source_sha={FAKE_SHA}\nbuilder_sha256={'b' * 64}\n"
                f"packages_sha256={'c' * 64}\nqualification=UNQUALIFIED\n"
            )
            result = subprocess.run(
                ["bash", str(CHECKPOINT), "create", str(root), str(archive)],
                capture_output=True, text=True, timeout=15,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            rejected = subprocess.run(
                ["bash", str(CHECKPOINT), "restore", str(archive),
                 str(restored), "0" * 64, FAKE_SHA],
                capture_output=True, text=True, timeout=15,
            )
            self.assertNotEqual(rejected.returncode, 0)
            self.assertFalse(restored.exists())
            result = subprocess.run(
                ["bash", str(CHECKPOINT), "restore", str(archive),
                 str(restored), digest, FAKE_SHA],
                capture_output=True, text=True, timeout=15,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                (restored / "build/webkit/build.ninja").read_bytes(),
                (root / "build/webkit/build.ninja").read_bytes(),
            )
            repeat = subprocess.run(
                ["bash", str(CHECKPOINT), "restore", str(archive),
                 str(restored), digest, FAKE_SHA],
                capture_output=True, text=True, timeout=15,
            )
            self.assertNotEqual(repeat.returncode, 0)

    @unittest.skipUnless(shutil.which("zstd"), "zstd required for archive smoke")
    def test_checkpoint_rejects_traversal_member(self):
        with tempfile.TemporaryDirectory() as tmp:
            raw = Path(tmp) / "in.tar"
            archive = Path(tmp) / "in.tar.zst"
            with tarfile.open(raw, "w") as tar:
                info = tarfile.TarInfo("src/../../escape")
                payload = b"malicious"
                info.size = len(payload)
                tar.addfile(info, io.BytesIO(payload))
            subprocess.run(
                ["zstd", "-q", "-1", "-o", str(archive), str(raw)],
                check=True, capture_output=True, timeout=15,
            )
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            destination = Path(tmp) / "dest"
            result = subprocess.run(
                ["bash", str(CHECKPOINT), "restore", str(archive),
                 str(destination), digest, FAKE_SHA],
                capture_output=True, text=True, timeout=15,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse((Path(tmp) / "escape").exists())


if __name__ == "__main__":
    unittest.main()
