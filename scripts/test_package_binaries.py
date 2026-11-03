"""Fixture validation for Cargo metadata binary enumeration."""
import importlib.util
import pathlib
import unittest

path = pathlib.Path(__file__).with_name("package-binaries.py")
spec = importlib.util.spec_from_file_location("package_binaries", path)
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)


class BinaryTests(unittest.TestCase):
    def test_new_surface_auto_discovered_xtask_excluded(self):
        value = {"packages": [
            {"name": "nuraloumi-shell", "targets": [
                {"name": "nuraloumi-panel", "kind": ["bin"]},
                {"name": "nuraloumi-menu", "kind": ["bin"]},
                {"name": "nuraloumi-feeds", "kind": ["bin"]}]},
            {"name": "nuraloumi-providers", "targets": [
                {"name": "nuraloumi-probe", "kind": ["bin"]}]},
            {"name": "nuraloumi-wayland", "targets": [
                {"name": "nuraloumi-thumbnail-helper", "kind": ["bin"]}]},
            {"name": "nuraloumi-xtask", "targets": [
                {"name": "nuraloumi-xtask", "kind": ["bin"]}]}]}
        self.assertIn("nuraloumi-feeds", mod.targets(value))
        self.assertNotIn("nuraloumi-xtask", mod.targets(value))

    def test_missing_baseline_and_unsafe_names_fail_closed(self):
        with self.assertRaises(ValueError):
            mod.targets({"packages": []})
        with self.assertRaises(ValueError):
            mod.targets({"packages": [{"name": "nuraloumi-shell", "targets": [
                {"name": "nuraloumi-../escape", "kind": ["bin"]}]}]})

    def test_unprefixed_and_duplicate_screenshot_binaries_fail_closed(self):
        baseline = [
            {"name": "nuraloumi-panel", "kind": ["bin"]},
            {"name": "nuraloumi-menu", "kind": ["bin"]},
        ]
        packages = [
            {"name": "nuraloumi-shell", "targets": baseline.copy()},
            {"name": "nuraloumi-providers", "targets": [
                {"name": "nuraloumi-probe", "kind": ["bin"]}]},
            {"name": "nuraloumi-wayland", "targets": [
                {"name": "nuraloumi-thumbnail-helper", "kind": ["bin"]}]},
        ]
        packages[0]["targets"].append({"name": "screenshot", "kind": ["bin"]})
        with self.assertRaisesRegex(ValueError, "invalid binary name"):
            mod.targets({"packages": packages})
        packages[0]["targets"][-1]["name"] = "nuraloumi-screenshot"
        self.assertIn("nuraloumi-screenshot", mod.targets({"packages": packages}))
        packages[0]["targets"].append(
            {"name": "nuraloumi-screenshot", "kind": ["bin"]})
        with self.assertRaisesRegex(ValueError, "duplicate binary name"):
            mod.targets({"packages": packages})

    def test_current_shell_manifest_explicitly_registers_all_sources(self):
        # autobins=False prevents Cargo from silently shipping an unexpected
        # filename. Compare tracked sources only: another lane can have an
        # uncommitted in-progress surface which must not be packaged yet.
        import subprocess
        import tomllib

        root = path.parent.parent
        crate = root / "crates" / "nuraloumi-shell"
        manifest = tomllib.loads((crate / "Cargo.toml").read_text())
        self.assertIs(manifest["package"]["autobins"], False)
        declared = manifest["bin"]
        tracked = subprocess.run(
            ["git", "ls-files", "--", "crates/nuraloumi-shell/src/bin/*.rs"],
            cwd=root, check=True, text=True, capture_output=True, timeout=15)
        source_paths = sorted(
            "src/bin/" + pathlib.Path(name).name
            for name in tracked.stdout.splitlines())
        declared_paths = sorted(item["path"] for item in declared)
        self.assertEqual(source_paths, declared_paths)
        self.assertEqual(len({item["name"] for item in declared}), len(declared))
        self.assertIn(
            {"name": "nuraloumi-screenshot", "path": "src/bin/screenshot.rs"},
            declared)

    def test_cargo_metadata_and_package_enumerator_agree_on_screenshot(self):
        import json
        import subprocess

        root = path.parent.parent
        process = subprocess.run(
            ["cargo", "metadata", "--locked", "--offline", "--no-deps",
             "--format-version", "1"],
            cwd=root, check=True, text=True, capture_output=True, timeout=30)
        metadata = json.loads(process.stdout)
        shell = next(p for p in metadata["packages"]
                     if p["name"] == "nuraloumi-shell")
        screenshot = [t for t in shell["targets"]
                      if t["src_path"].endswith("/src/bin/screenshot.rs")
                      and "bin" in t["kind"]]
        self.assertEqual([t["name"] for t in screenshot], ["nuraloumi-screenshot"])
        self.assertIn("nuraloumi-screenshot", mod.targets(metadata))
        self.assertNotIn("screenshot", mod.targets(metadata))


if __name__ == "__main__":
    unittest.main()
