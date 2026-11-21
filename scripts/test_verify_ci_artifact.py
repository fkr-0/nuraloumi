"""Local-only provenance tests; never contacts GitHub or a device."""
import hashlib
import importlib.util
import io
import json
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
BINS = ("nuraloumi-menu", "nuraloumi-panel", "nuraloumi-probe", "nuraloumi-thumbnail-helper")


def elf_fixture() -> bytes:
    data = bytearray(52)
    data[:6] = b"\x7fELF\x01\x01"
    data[18:20] = b"\x28\x00"
    data[36:40] = (0x400).to_bytes(4, "little")
    return bytes(data)


def sample(folder: Path, target: str = "debian", version: str = "0.0.4-dev.2") -> None:
    triple = verifier.LANES[target][2]
    records = {f"bin/{name}": elf_fixture() for name in BINS}
    browser_payload = {
        "nurabrowser-core": "engine", "libexec/WPEWebProcess": "webprocess",
        "bin/bwrap": "bwrap", "bin/xdg-dbus-proxy": "xdg-dbus-proxy",
        "lib/ld-armhf.so": "loader", "lib/libWPE.so": "library",
        "lib/modules/webkit.so": "module",
    }
    prefix = "libexec/nuraloumi/browser/"
    browser_records = []
    for name, role in browser_payload.items():
        content = elf_fixture()
        records[prefix + name] = content
        browser_records.append({"path": name, "role": role,
                                "sha256": hashlib.sha256(content).hexdigest()})
    browser = {"schema": "nuraloumi-browser-runtime-v1", "source_sha": SHA,
               "producer_revision": "b" * 40, "target": triple,
               "abi": "musl" if target == "alpine" else "glibc",
               "qualification": {key: "PASS" for key in
                                 ("sandbox", "instruction_closure", "dlopen_closure", "webprocess", "review")},
               "files": browser_records}
    records[prefix + "runtime.json"] = json.dumps(browser).encode()
    manifest = (
        "schema=nuraloumi-package-manifest-v1\n"
        f"version={version}\n"
        f"target={triple}\n"
        f"git_commit={SHA}\n"
        "git_dirty=false\n"
        f"binaries={','.join(BINS)}\n"
        "browser_runtime=required\n"
        "files:\n"
        + "".join(f"  {hashlib.sha256(data).hexdigest()}  {name}\n" for name, data in records.items())
    ).encode()
    (folder / "nuraloumi-ci").mkdir(parents=True)
    (folder / "nuraloumi-ci/MANIFEST.txt").write_bytes(manifest)
    with tarfile.open(folder / "nuraloumi-ci.tar", "w") as archive:
        info = tarfile.TarInfo("./MANIFEST.txt")
        info.size = len(manifest)
        archive.addfile(info, io.BytesIO(manifest))
        for name, data in records.items():
            info = tarfile.TarInfo("./" + name)
            info.size = len(data)
            if name.startswith(prefix):
                role = browser_payload.get(name.removeprefix(prefix))
                if role in {"engine", "webprocess", "bwrap", "xdg-dbus-proxy", "loader"}:
                    info.mode = 0o755
            archive.addfile(info, io.BytesIO(data))
    digest = hashlib.sha256((folder / "nuraloumi-ci.tar").read_bytes()).hexdigest()
    (folder / "SHA256SUMS.txt").write_text(f"{digest}  nuraloumi-ci.tar\n")
    (folder / "BUILD-INFO.txt").write_text(f"git_sha={SHA}\n")
    abi = "musl" if target == "alpine" else "glibc"
    (folder / "ELF-AUDIT.txt").write_text("".join(
        f"PASS {name} ELF32 ARM VFPv3-D16 hardfloat {abi} no-NEON-attributes\n"
        for name in BINS
    ))
    if target == "debian":
        dest = folder / "native-deb"
        dest.mkdir()
        deb_version = version.replace("-dev.", "~dev.").replace("-rc.", "~rc.")
        pkg = dest / f"nuraloumi_{deb_version}_armhf.deb"
        pkg.write_bytes(b"dummy test package, not installable")
        digest = hashlib.sha256(pkg.read_bytes()).hexdigest()
        (dest / f"{pkg.name}.sha256").write_text(f"{digest}  {pkg.name}\n")
    else:
        dest = folder / "native-apk"
        (dest / "tmp/armv7").mkdir(parents=True)
        apk_version = version.replace("-dev.", "_pre").replace("-rc.", "_rc")
        pkg = dest / f"tmp/armv7/nuraloumi-{apk_version}-r0.apk"
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
    def test_partial_bundle_and_missing_elf_audit_fail(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d)
            sample(path)
            (path / "ELF-AUDIT.txt").write_text("PASS dummy test\n")
            with self.assertRaisesRegex(ValueError, "ELF audit incomplete"):
                verifier.verify_downloaded(path, "debian", SHA)
        with tempfile.TemporaryDirectory() as d:
            path = Path(d)
            sample(path)
            tar = path / "nuraloumi-ci.tar"
            manifest = (path / "nuraloumi-ci/MANIFEST.txt").read_bytes()
            with tarfile.open(tar, "w") as archive:
                info = tarfile.TarInfo("./MANIFEST.txt")
                info.size = len(manifest)
                archive.addfile(info, io.BytesIO(manifest))
            (path / "SHA256SUMS.txt").write_text(
                f"{hashlib.sha256(tar.read_bytes()).hexdigest()}  nuraloumi-ci.tar\n"
            )
            with self.assertRaisesRegex(ValueError, "partial or extra tar bundle"):
                verifier.verify_downloaded(path, "debian", SHA)

    def test_wrong_elf_abi_fails_with_rehashed_bundle(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d)
            sample(path)
            tar = path / "nuraloumi-ci.tar"
            manifest = (path / "nuraloumi-ci/MANIFEST.txt").read_bytes()
            with tarfile.open(tar) as original:
                browser_files = {member.name: original.extractfile(member).read()
                                 for member in original.getmembers()
                                 if member.isfile() and member.name.startswith("./libexec/nuraloumi/browser/")}
            with tarfile.open(tar, "w") as archive:
                info = tarfile.TarInfo("./MANIFEST.txt")
                info.size = len(manifest)
                archive.addfile(info, io.BytesIO(manifest))
                for name in BINS:
                    bad = bytearray(elf_fixture())
                    bad[18:20] = b"\x3e\x00"
                    data = bytes(bad)
                    info = tarfile.TarInfo("./bin/" + name)
                    info.size = len(data)
                    archive.addfile(info, io.BytesIO(data))
                for name, data in browser_files.items():
                    info = tarfile.TarInfo(name)
                    info.size = len(data)
                    archive.addfile(info, io.BytesIO(data))
            (path / "SHA256SUMS.txt").write_text(
                f"{hashlib.sha256(tar.read_bytes()).hexdigest()}  nuraloumi-ci.tar\n"
            )
            with self.assertRaisesRegex(ValueError, "tar manifest SHA256 mismatch"):
                verifier.verify_downloaded(path, "debian", SHA)
    def test_non_executable_browser_engine_tar_fails(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d)
            sample(path)
            tar = path / "nuraloumi-ci.tar"
            with tarfile.open(tar) as archive:
                members = [(member, archive.extractfile(member).read())
                           for member in archive.getmembers() if member.isfile()]
            with tarfile.open(tar, "w") as archive:
                for member, data in members:
                    if member.name == "./libexec/nuraloumi/browser/nurabrowser-core":
                        member.mode = 0o644
                    archive.addfile(member, io.BytesIO(data))
            (path / "SHA256SUMS.txt").write_text(
                f"{hashlib.sha256(tar.read_bytes()).hexdigest()}  nuraloumi-ci.tar\n"
            )
            with self.assertRaisesRegex(ValueError, "not executable in tar"):
                verifier.verify_downloaded(path, "debian", SHA)

    def test_valid_debian_and_alpine_fixtures(self):
        for lane in ("debian", "alpine"):
            with self.subTest(lane=lane), tempfile.TemporaryDirectory() as d:
                path = Path(d)
                sample(path, lane)
                result = verifier.verify_downloaded(path, lane, SHA)
                self.assertEqual(result["sha"], SHA)
                self.assertEqual(result["version"], "0.0.4-dev.2")

    def test_rc1_debian_and_alpine_artifacts_are_verified(self):
        for lane in ("debian", "alpine"):
            with self.subTest(lane=lane), tempfile.TemporaryDirectory() as d:
                path = Path(d)
                sample(path, lane, version="0.0.8-rc.1")
                result = verifier.verify_downloaded(path, lane, SHA, "0.0.8-rc.1")
                self.assertEqual(result["version"], "0.0.8-rc.1")
                with self.assertRaisesRegex(ValueError, "release version mismatch"):
                    verifier.verify_downloaded(path, lane, SHA, "0.0.8")

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
