"""Opt-in, OFFLINE integration test of Debian package assets with real dpkg-deb.

Run explicitly (requires locally cached debian:trixie and working Docker):
  NURALOUMI_DEB_PAYLOAD_SMOKE=1 python3 -m unittest scripts.test_debian_payload_smoke -v

No download, network, remote host, device, install, or runtime session access.
The staged executables are *synthetic scripts*, not ARM ELF binaries: success
proves package byte parity and fail-closed input validation, NOT ARMv7 ABI,
source provenance, D-Bus ownership, target installation or SL101 operation.
"""

from __future__ import annotations

import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
IMAGE = "debian:trixie"
SHA = "a" * 40
VERSION = "0.0.8~dev.1"


@unittest.skipUnless(
    os.environ.get("NURALOUMI_DEB_PAYLOAD_SMOKE") == "1",
    "explicit offline real-dpkg-deb gate not enabled",
)
class DebianPackagePayloadSmoke(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not shutil.which("docker"):
            raise AssertionError("Docker unavailable: native Debian smoke is NOT qualified")
        inspected = subprocess.run(
            ["docker", "image", "inspect", IMAGE],
            capture_output=True,
            text=True,
            timeout=20,
            check=False,
        )
        if inspected.returncode != 0:
            raise AssertionError(
                "locally cached debian:trixie unavailable; refusing network pull"
            )

    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="nuraloumi-real-deb-smoke-")
        self.addCleanup(self.scratch.cleanup)
        root = Path(self.scratch.name)
        self.stage = root / "stage"
        self.output = root / "out"
        self.output.mkdir()
        for folder in (
            "bin",
            "share/applications",
            "share/dbus-1/services",
            "share/nuraloumi/qualification",
            "config",
        ):
            (self.stage / folder).mkdir(parents=True)

        (self.stage / "MANIFEST.txt").write_text(
            "\n".join(
                (
                    "schema=nuraloumi-package-manifest-v1",
                    "target=armv7-unknown-linux-gnueabihf",
                    "git_dirty=false",
                    f"git_commit={SHA}",
                    "version=0.0.8-dev.1",
                    "",
                )
            )
        )
        for name in ("panel", "player", "files", "notificationd"):
            binary = self.stage / "bin" / f"nuraloumi-{name}"
            binary.write_text("#!/bin/sh\nexit 0\n")
            binary.chmod(0o755)
        for name in ("player", "files", "file-manager", "notifications"):
            desktop = self.stage / "share/applications" / f"nuraloumi-{name}.desktop"
            desktop.write_text(
                f"[Desktop Entry]\nType=Application\nName={name}\n"
            )
        self.service = (
            self.stage
            / "share/dbus-1/services/org.freedesktop.Notifications.service"
        )
        self.service.write_text(
            "[D-BUS Service]\n"
            "Name=org.freedesktop.Notifications\n"
            "Exec=/usr/bin/nuraloumi-notificationd\n"
        )
        (self.stage / "config/nuraloumi.toml.example").write_text("# fixture\n")
        (self.stage / "share/nuraloumi/qualification/README.md").write_text(
            "fixture-only\n"
        )
        (self.stage / "LICENSE").write_text("fixture license\n")
        (self.stage / "README.md").write_text("fixture readme\n")

    def docker(self, args):
        # Fail closed on unavailable cached image and disable all networking.
        # Never grant Docker access to the host device or active user session.
        prefix = [
            "docker", "run", "--rm", "--pull=never", "--network=none",
            "--user", f"{os.getuid()}:{os.getgid()}",
            "--volume", f"{ROOT / 'scripts'}:/scripts:ro",
            "--volume", f"{self.stage}:/stage:ro",
            "--volume", f"{self.output}:/out",
            IMAGE,
        ]
        return subprocess.run(
            prefix + args,
            capture_output=True,
            text=True,
            timeout=45,
            check=False,
        )

    def build(self):
        return self.docker(
            ["sh", "/scripts/build-ci-debian-deb.sh", "/stage", "/out", SHA]
        )

    def test_real_package_roundtrip_retains_staged_assets(self):
        built = self.build()
        self.assertEqual(built.returncode, 0, built.stdout + built.stderr)
        filename = f"nuraloumi_{VERSION}_armhf.deb"
        package = self.output / filename
        self.assertTrue(package.is_file())

        metadata = self.docker(
            [
                "sh", "-ec",
                f"test \"$(dpkg-deb --field /out/{filename} Architecture)\" = armhf;"
                f"test \"$(dpkg-deb --field /out/{filename} Version)\" = '{VERSION}'",
            ]
        )
        self.assertEqual(
            metadata.returncode, 0, metadata.stdout + metadata.stderr
        )
        (self.output / "extracted").mkdir()
        extracted = self.docker(
            ["dpkg-deb", "--extract", f"/out/{filename}", "/out/extracted"]
        )
        self.assertEqual(
            extracted.returncode, 0, extracted.stdout + extracted.stderr
        )
        inputs = [
            *sorted((self.stage / "bin").glob("nuraloumi-*")),
            *sorted((self.stage / "share/applications").glob("*.desktop")),
            self.service,
        ]
        self.assertEqual(len(inputs), 9)
        for original in inputs:
            relative = original.relative_to(self.stage)
            installed = self.output / "extracted/usr" / relative
            with self.subTest(asset=str(relative)):
                self.assertTrue(installed.is_file())
                self.assertEqual(original.read_bytes(), installed.read_bytes())

        expected_hash = hashlib.sha256(package.read_bytes()).hexdigest()
        reported_sha = (self.output / f"{filename}.sha256").read_text()
        self.assertEqual(reported_sha.strip(), f"{expected_hash}  {filename}")

    def test_missing_or_unsafe_activation_files_fail_closed(self):
        cases = [
            ("missing service", self.service, "remove"),
            ("symlinked service", self.service, "symlink"),
            (
                "missing launcher",
                self.stage / "share/applications/nuraloumi-notifications.desktop",
                "remove",
            ),
            ("missing executable", self.stage / "bin/nuraloumi-notificationd", "remove"),
            ("invalid service target", self.service, "bad_exec"),
        ]
        for label, path, change in cases:
            with self.subTest(case=label):
                original = path.read_bytes()
                try:
                    if change in ("remove", "symlink"):
                        path.unlink()
                        if change == "symlink":
                            path.symlink_to("nonexistent-service")
                    elif change == "bad_exec":
                        path.write_text(
                            "[D-BUS Service]\nName=org.freedesktop.Notifications\n"
                            "Exec=/usr/bin/unexpected-program\n"
                        )
                    result = self.build()
                    self.assertNotEqual(
                        result.returncode, 0,
                        f"{label}: improperly accepted invalid staging data",
                    )
                finally:
                    path.unlink(missing_ok=True)
                    path.write_bytes(original)


    def test_symlinked_staged_executable_fails_closed(self):
        # -f/-x follow links; a present executable symlink is not safe input.
        # Use an existing in-stage target so this is not a dangling-link test.
        panel = self.stage / "bin/nuraloumi-panel"
        panel.unlink()
        panel.symlink_to("nuraloumi-files")
        result = self.build()
        self.assertNotEqual(
            result.returncode, 0,
            "symlinked executable was accepted into native package",
        )


if __name__ == "__main__":
    unittest.main()
