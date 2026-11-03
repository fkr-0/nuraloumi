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


if __name__ == "__main__":
    unittest.main()
