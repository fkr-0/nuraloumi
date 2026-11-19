"""Pure file-based checks for the composite SL101 release verifier."""
import importlib.util
import os
from pathlib import Path
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location(
    'sl101_composite', Path(__file__).with_name('sl101_userspace_composite_release.py')
)
assert SPEC and SPEC.loader
release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release)


class ManifestTests(unittest.TestCase):
    def test_manifest_accepts_exact_pins(self):
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch)
            (path / 'MANIFEST.txt').write_text(
                'schema=nuraloumi-package-manifest-v1\n'
                'version=0.0.8-rc.1\n'
                'target=armv7-unknown-linux-musleabihf\n'
                'git_commit=' + release.NEW_COMMIT + '\n'
                'git_dirty=false\nfiles:\n'
                '  ' + 'a' * 64 + '  bin/test\n'
            )
            header, files = release.manifest(path, '0.0.8-rc.1', release.NEW_COMMIT)
            self.assertEqual(header['version'], '0.0.8-rc.1')
            self.assertEqual(files['bin/test'], 'a' * 64)

    def test_manifest_blocks_path_escape(self):
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch)
            (path / 'MANIFEST.txt').write_text(
                'schema=nuraloumi-package-manifest-v1\n'
                'version=0.0.8-rc.1\n'
                'target=armv7-unknown-linux-musleabihf\n'
                'git_commit=' + release.NEW_COMMIT + '\n'
                'git_dirty=false\nfiles:\n'
                '  ' + 'a' * 64 + '  ../outside\n'
            )
            with self.assertRaisesRegex(RuntimeError, 'manifest-path'):
                release.manifest(path, '0.0.8-rc.1', release.NEW_COMMIT)

    def test_inventory_blocks_unlisted_overlay(self):
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch)
            (path / 'MANIFEST.txt').write_text('placeholder')
            (path / 'unexpected').write_bytes(b'not in CI')
            with self.assertRaisesRegex(RuntimeError, 'tree-sha-unexpected'):
                release.verify_tree(path, {}, False)

    def test_overlay_pins_have_expected_libs(self):
        self.assertEqual(len(release.VIDEO_PINS), 3)
        self.assertTrue(all(path.startswith(release.VIDEO_PREFIX)
                            for path in release.VIDEO_PINS))

    @unittest.skipUnless(os.geteuid() == 0, 'ownership check needs root')
    def test_publish_makes_private_directories_traversable(self):
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch) / 'release'
            (path / 'lib' / 'nuraloumi').mkdir(parents=True)
            path.chmod(0o700)
            (path / 'lib' / 'nuraloumi').chmod(0o700)
            release.make_release_traversable(path)
            self.assertEqual(path.stat().st_mode & 0o777, 0o755)
            self.assertEqual((path / 'lib' / 'nuraloumi').stat().st_mode & 0o777, 0o755)


if __name__ == '__main__':
    unittest.main()
