"""Negative contract tests: a missing or untrusted bundle must never qualify."""
import io
import json
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from verify_sl101_video_bundle import verify
from extract_sl101_video_bundle import extract

REV = 'a' * 40
DIGEST = 'b' * 64


class BundleConsumerTests(unittest.TestCase):
    def test_missing_bundle_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaises(ValueError):
                verify(Path(tmp) / 'missing', REV, DIGEST)

    def test_unpinned_source_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaisesRegex(ValueError, 'unpinned'):
                verify(Path(tmp), 'main', DIGEST)

    def test_source_revision_mismatch_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            Path(tmp, 'provenance.json').write_text(json.dumps({'schema':'sl101-gpu-video-bundle-v1', 'git_revision':'c' * 40}))
            with self.assertRaisesRegex(ValueError, 'provenance mismatch'):
                verify(Path(tmp), REV, DIGEST)

    def test_missing_plugin_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            meta = {'schema':'sl101-gpu-video-bundle-v1', 'project':'sl101_linux', 'git_revision':REV, 'target':'armv7-unknown-linux-musleabihf', 'abi':'armv7-musleabihf-vfpv3-d16-no-neon-no-idiv', 'files':{}}
            Path(tmp, 'provenance.json').write_text(json.dumps(meta))
            with self.assertRaisesRegex(ValueError, 'missing engine'):
                verify(Path(tmp), REV, DIGEST)

    def test_archive_path_traversal_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            archive = Path(tmp) / 'bundle.tar.gz'
            with tarfile.open(archive, 'w:gz') as tar:
                info = tarfile.TarInfo('../escape')
                info.size = 1
                tar.addfile(info, io.BytesIO(b'x'))
            dest = Path(tmp) / 'output'
            dest.mkdir()
            with self.assertRaisesRegex(ValueError, 'unsafe archive'):
                extract(archive, dest)
            self.assertFalse((Path(tmp) / 'escape').exists())

    def test_archive_symlink_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            archive = Path(tmp) / 'bundle.tar.gz'
            with tarfile.open(archive, 'w:gz') as tar:
                info = tarfile.TarInfo('lib/libEGL.so.1')
                info.type = tarfile.SYMTYPE
                info.linkname = '/tmp/escape'
                tar.addfile(info)
            dest = Path(tmp) / 'output'
            dest.mkdir()
            with self.assertRaisesRegex(ValueError, 'unsupported archive'):
                extract(archive, dest)


if __name__ == '__main__':
    unittest.main()
