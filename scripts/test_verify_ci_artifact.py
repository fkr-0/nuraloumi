"""Local-only provenance tests; never contacts GitHub or a device."""
import hashlib
import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location(
    "ci_artifact_verifier", ROOT / "scripts/verify-ci-artifact.py"
)
verifier = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(verifier)

SHA = "a" * 40


def sample(folder: Path, target: str = "debian") -> None:
    triple = verifier.LANES[target][2]
    manifest = (
        "schema=nuraloumi-package-manifest-v1\n"
        "version=0.0.4-dev.2\n"
        f"target={triple}\n"
        f"git_commit={SHA}\n"
        "git_dirty=false\n"
        "binaries=nuraloumi-menu,nuraloumi-panel,nuraloumi-probe,nuraloumi-thumbnail-helper\n"
        "files:\n"
    ).encode()
    (folder / "nuraloumi-ci").mkdir(parents=True)
    (folder / "nuraloumi-ci/MANIFEST.txt").write_bytes(manifest)
    with tarfile.open(folder / "nuraloumi-ci.tar", "w") as archive:
        info = tarfile.TarInfo("./MANIFEST.txt")
        info.size = len(manifest)
        archive.addfile(info, io.BytesIO(manifest))
    digest = hashlib.sha256((folder / "nuraloumi-ci.tar").read_bytes()).hexdigest()
    (folder / "SHA256SUMS.txt").write_text(f"{digest}  nuraloumi-ci.tar\n")
    (folder / "BUILD-INFO.txt").write_text(f"git_sha={SHA}\n")
    (folder / "ELF-AUDIT.txt").write_text("PASS dummy test\n")
    if target == "debian":
        dest = folder / "native-deb"
        dest.mkdir()
        pkg = dest / "nuraloumi_0.0.4~dev.2_armhf.deb"
        pkg.write_bytes(b"dummy test package, not installable")
        digest = hashlib.sha256(pkg.read_bytes()).hexdigest()
        (dest / f"{pkg.name}.sha256").write_text(f"{digest}  {pkg.name}\n")
    else:
        dest = folder / "native-apk"
        (dest / "tmp/armv7").mkdir(parents=True)
        pkg = dest / "tmp/armv7/nuraloumi-0.0.4_pre2-r0.apk"
        pkg.write_bytes(b"dummy test signed package")
        digest = hashlib.sha256(pkg.read_bytes()).hexdigest()
        (dest / "NURALOUMI-APK-SHA256SUMS.txt").write_text(
            f"{digest}  tmp/armv7/{pkg.name}\n"
        )
        key = b"dummy public key\n"
        (dest / "CI-EPHEMERAL-KEY.rsa.pub").write_bytes(key)
        (dest / "CI-APK-SIGNING-POLICY.txt").write_text(
            f"SIGNING_MODE=ephemeral-ci-development-only\n"
            f"GIT_SHA={SHA}\n"
            f"PUBLIC_KEY_SHA256={hashlib.sha256(key).hexdigest()}\n"
        )


class VerifierTests(unittest.TestCase):
    def test_valid_debian_and_alpine_fixtures(self):
        for lane in ("debian", "alpine"):
            with self.subTest(lane=lane), tempfile.TemporaryDirectory() as d:
                path = Path(d)
                sample(path, lane)
                result = verifier.verify_downloaded(path, lane, SHA)
                self.assertEqual(result["sha"], SHA)
                self.assertEqual(result["version"], "0.0.4-dev.2")

    def test_tampered_native_package_or_manifest_fails_closed(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d)
            sample(path)
            (path / "native-deb/nuraloumi_0.0.4~dev.2_armhf.deb").write_bytes(b"tamper")
            with self.assertRaisesRegex(ValueError, "SHA256 mismatch"):
                verifier.verify_downloaded(path, "debian", SHA)
        with tempfile.TemporaryDirectory() as d:
            path = Path(d)
            sample(path)
            (path / "nuraloumi-ci/MANIFEST.txt").write_text("changed")
            with self.assertRaises(ValueError):
                verifier.verify_downloaded(path, "debian", SHA)

    def test_wrong_abi_sha_or_build_status_fails_closed(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d)
            sample(path)
            with self.assertRaisesRegex(ValueError, "manifest target mismatch"):
                verifier.verify_downloaded(path, "alpine", SHA)
            with self.assertRaisesRegex(ValueError, "manifest git_commit mismatch"):
                verifier.verify_downloaded(path, "debian", "b" * 40)
        run = dict(
            status="completed", conclusion="success", headSha=SHA,
            headBranch="main", event="push",
            workflowName="SL101 Debian armhf glibc package",
        )
        verifier.validate_run(run, "debian", SHA)
        for key, invalid in (
            ("conclusion", "failure"),
            ("headBranch", "experimental"),
            ("headSha", "b" * 40),
            ("workflowName", "SL101 Alpine armv7 musl package"),
        ):
            modified = {**run, key: invalid}
            with self.assertRaises(ValueError):
                verifier.validate_run(modified, "debian", SHA)

    def test_checksum_paths_cannot_escape_folder(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d)
            (path / "SUMS").write_text("0" * 64 + "  ../outside\n")
            with self.assertRaisesRegex(ValueError, "unsafe checksum filename"):
                verifier.checksum_file(path, path / "SUMS")


if __name__ == "__main__":
    unittest.main()
