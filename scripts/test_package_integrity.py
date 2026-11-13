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
            "nuraloumi-shell": ["nuraloumi-menu", "nuraloumi-panel", "nuraloumi-feeds"],
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
                     "share/applications/nuraloumi-player.desktop",
                     "share/nuraloumi/qualification/README.md"]:
            self.payloads[name] = b"fixture"
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
            "binaries=" + ",".join(self.names), "files:",
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

    def test_tampered_hash_rejected(self):
        (self.stage / "bin" / self.names[0]).write_bytes(b"\x7fELFtampered")
        check = self.verify()
        self.assertNotEqual(check.returncode, 0)
        self.assertIn("SHA256 mismatch", check.stderr)

    def test_unlisted_file_rejected(self):
        (self.stage / "unlisted.txt").write_text("oops")
        self.assertNotEqual(self.verify().returncode, 0)

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
