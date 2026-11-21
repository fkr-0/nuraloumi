"""End-to-end staged-package tamper/omission tests without a real device."""
import hashlib
import os
import pathlib
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent


class PackageIntegrityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # Build an isolated miniature Cargo workspace: the real worktree is
        # concurrently edited by other lanes and may have transient bin names.
        cls.temp = tempfile.TemporaryDirectory(prefix="nuraloumi-package-fixture-")
        cls.root = pathlib.Path(cls.temp.name)
        cls.exe = cls.root / "xtask"
        (cls.root / "Cargo.toml").write_text(
            '[workspace]\nresolver = "2"\n'
            'members = ["crates/nuraloumi-shell", "crates/nuraloumi-providers", '
            '"crates/nuraloumi-wayland"]\n'
            '[workspace.package]\nversion = "0.0.3"\nedition = "2021"\n'
        )
        names = {
            "nuraloumi-shell": ["nuraloumi-menu", "nuraloumi-panel", "nuraloumi-feeds",
                                "nuraloumi-notificationd", "nuraloumi-browser"],
            "nuraloumi-providers": ["nuraloumi-probe"],
            "nuraloumi-wayland": ["nuraloumi-thumbnail-helper"],
        }
        for crate, binaries in names.items():
            crate_root = cls.root / "crates" / crate
            (crate_root / "src" / "bin").mkdir(parents=True)
            manifest = [f'[package]\nname = "{crate}"\nversion.workspace = true\n'
                        'edition.workspace = true\n']
            for name in binaries:
                path = f"src/bin/{name}.rs"
                manifest.append(f'[[bin]]\nname = "{name}"\npath = "{path}"\n')
                (crate_root / path).write_text("fn main() {}\n")
            (crate_root / "Cargo.toml").write_text("".join(manifest))
        # Cargo writes only in the temporary fixture workspace.
        subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=cls.root,
                       capture_output=True, timeout=40, check=True)
        (cls.root / "scripts").mkdir()
        (cls.root / "scripts" / "package-binaries.py").write_bytes(
            (ROOT / "scripts/package-binaries.py").read_bytes()
        )
        env = dict(os.environ, CARGO_MANIFEST_DIR=str(cls.root / "crates/nuraloumi-xtask"))
        subprocess.run(
            ["rustc", "--edition=2021", str(ROOT / "crates/nuraloumi-xtask/src/main.rs"),
             "-o", str(cls.exe)],
            cwd=ROOT, env=env, timeout=90, check=True,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        discover = subprocess.run(
            ["python3", str(cls.root / "scripts/package-binaries.py")],
            cwd=cls.root, text=True, capture_output=True, timeout=40, check=True,
        )
        cls.names = discover.stdout.strip().splitlines()

    @classmethod
    def tearDownClass(cls):
        cls.temp.cleanup()

    def setUp(self):
        self.stage = self.root / self.id().split(".")[-1]
        self.stage.mkdir()
        self.payloads = {}
        for name in self.names:
            self.payloads[f"bin/{name}"] = b"\x7fELFtest-fixture-not-executable"
        for name in ["README.md", "LICENSE", "config/nuraloumi.toml.example",
                     "share/nuraloumi/qualification/README.md"]:
            self.payloads[name] = b"fixture"
        for desktop, binary in {
            "nuraloumi-browser.desktop": "nuraloumi-browser",
            "nuraloumi-player.desktop": "nuraloumi-feeds",
            "nuraloumi-files.desktop": "nuraloumi-feeds",
            "nuraloumi-notifications.desktop": "nuraloumi-feeds",
        }.items():
            self.payloads[f"share/applications/{desktop}"] = (
                f"[Desktop Entry]\nType=Application\nExec={binary}\nTryExec={binary}\n"
            ).encode()
        # The miniature fixture does not build a daemon; use a metadata bin
        # with that name so the service link is still checked.
        self.payloads["share/dbus-1/services/org.freedesktop.Notifications.service"] = (
            b"[D-BUS Service]\nName=org.freedesktop.Notifications\n"
            b"Exec=/usr/bin/nuraloumi-notificationd\n"
        )
        for name, content in self.payloads.items():
            path = self.stage / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
            if name.startswith("bin/"):
                path.chmod(0o755)
        self.manifest()

    def manifest(self):
        lines = [
            "schema=nuraloumi-package-manifest-v1", "version=0.0.3",
            "target=x86_64-unknown-linux-gnu", "git_commit=test",
            "git_dirty=true", "build_command=fixture",
            "binaries=" + ",".join(self.names),
            "browser_runtime=not-targeted", "files:",
        ]
        lines += [
            "  " + hashlib.sha256(value).hexdigest() + "  " + name
            for name, value in sorted(self.payloads.items())
        ]
        (self.stage / "MANIFEST.txt").write_text("\n".join(lines) + "\n")

    def verify(self):
        return subprocess.run(
            [str(self.exe), "package-check", "--dir", str(self.stage)],
            cwd=ROOT, text=True, capture_output=True, timeout=45,
        )

    def test_valid_fixture(self):
        check = self.verify()
        self.assertEqual(check.returncode, 0, check.stderr)
        self.assertIn(f"binaries={len(self.names)}", check.stdout)

    def test_host_bundle_cannot_claim_unqualified_browser_runtime(self):
        manifest = self.stage / "MANIFEST.txt"
        manifest.write_text(manifest.read_text().replace(
            "browser_runtime=not-targeted", "browser_runtime=required"))
        self.assertIn("invalid host browser runtime field", self.verify().stderr)

    def test_tampered_hash_rejected(self):
        (self.stage / "bin" / self.names[0]).write_bytes(b"\x7fELFtampered")
        check = self.verify()
        self.assertNotEqual(check.returncode, 0)
        self.assertIn("SHA256 mismatch", check.stderr)

    def test_unlisted_file_rejected(self):
        (self.stage / "unlisted.txt").write_text("oops")
        self.assertNotEqual(self.verify().returncode, 0)

    def test_desktop_exec_not_in_bundle_rejected(self):
        key = "share/applications/nuraloumi-files.desktop"
        self.payloads[key] = (b"[Desktop Entry]\nType=Application\nExec=missing-helper\n"
                              b"TryExec=missing-helper\n")
        (self.stage / key).write_bytes(self.payloads[key])
        self.manifest()
        self.assertIn("references absent binary", self.verify().stderr)

    def test_missing_activation_rejected(self):
        name = "share/dbus-1/services/org.freedesktop.Notifications.service"
        (self.stage / name).unlink()
        self.assertNotEqual(self.verify().returncode, 0)

    def test_activation_missing_section_rejected_even_with_valid_name_exec(self):
        name = "share/dbus-1/services/org.freedesktop.Notifications.service"
        self.payloads[name] = (
            b"Name=org.freedesktop.Notifications\n"
            b"Exec=/usr/bin/nuraloumi-notificationd\n"
        )
        (self.stage / name).write_bytes(self.payloads[name])
        self.manifest()
        self.assertIn("notification activation", self.verify().stderr)

    def test_duplicate_exec_in_desktop_rejected(self):
        name = "share/applications/nuraloumi-files.desktop"
        self.payloads[name] += b"Exec=missing-helper\n"
        (self.stage / name).write_bytes(self.payloads[name])
        self.manifest()
        self.assertIn("duplicate Exec", self.verify().stderr)

    def test_missing_binary_rejected(self):
        (self.stage / "bin" / self.names[0]).unlink()
        self.assertNotEqual(self.verify().returncode, 0)

    def test_non_elf_rejected(self):
        (self.stage / "bin" / self.names[0]).write_bytes(b"not an elf")
        self.assertNotEqual(self.verify().returncode, 0)

    def test_symlink_rejected(self):
        file = self.stage / "README.md"
        file.unlink()
        file.symlink_to(self.stage / "LICENSE")
        self.assertNotEqual(self.verify().returncode, 0)


if __name__ == "__main__":
    unittest.main()
