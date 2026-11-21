"""Offline GPU-release tar regressions: full producer modules survive packaging."""

import hashlib
import io
import json
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from verify_packaged_sl101_video import PREFIX, verify
from verify_sl101_video_bundle import ABIS, REQUIRED

SOURCE = 'a' * 40
PRODUCER = 'b' * 40
VERSION = '0.0.8-rc.1'
TARGET = 'armv7-unknown-linux-musleabihf'
EXTRA_PLUGIN = 'gst/libgstvideoconvert.so'


class PackagedVideoTests(unittest.TestCase):
    def build(self, root, omit=(), substitute=None, inject=None):
        modules = {name: b'producer-elf:' + name.encode()
                   for name in (*REQUIRED, EXTRA_PLUGIN)}
        provenance = {
            'schema': 'sl101-gpu-video-bundle-v1', 'project': 'sl101_linux',
            'git_revision': PRODUCER, 'target': TARGET, 'abi': ABIS[TARGET],
            'files': {name: hashlib.sha256(data).hexdigest()
                      for name, data in modules.items()},
        }
        files = {PREFIX + name: data for name, data in modules.items()
                 if name not in omit}
        files[PREFIX + 'provenance.json'] = json.dumps(provenance).encode()
        arm_elf = b'\x7fELF\x01' + b'\x00' * 13 + b'\x28\x00' + b'fixture'
        files['bin/nuraloumi-player'] = arm_elf
        files['bin/nuraloumi-playerd'] = arm_elf
        if substitute:
            name, content = substitute
            files[PREFIX + name] = content
        if inject:
            name, content = inject
            files[PREFIX + name] = content
        manifest = [
            'schema=nuraloumi-package-manifest-v1',
            'version=' + VERSION, 'target=' + TARGET,
            'git_commit=' + SOURCE, 'git_dirty=false',
            'binaries=nuraloumi-player,nuraloumi-playerd',
            'files:',
        ]
        manifest.extend('  ' + hashlib.sha256(data).hexdigest() + '  ' + name
                        for name, data in sorted(files.items()))
        files['MANIFEST.txt'] = ('\n'.join(manifest) + '\n').encode()
        archive = Path(root) / 'gpu.tar'
        with tarfile.open(archive, 'w:') as tar:
            for name, data in sorted(files.items()):
                item = tarfile.TarInfo(name)
                item.size = len(data)
                tar.addfile(item, io.BytesIO(data))
        return archive

    def test_all_producer_plugins_survive(self):
        with tempfile.TemporaryDirectory() as temp:
            result = verify(self.build(temp), SOURCE, VERSION, PRODUCER)
            self.assertEqual(result['gpu_modules'], len(REQUIRED) + 1)

    def test_dropped_transitive_plugin_fails_even_with_rehashed_manifest(self):
        with tempfile.TemporaryDirectory() as temp:
            path = self.build(temp, omit={EXTRA_PLUGIN})
            with self.assertRaisesRegex(ValueError, 'dropped or injected'):
                verify(path, SOURCE, VERSION, PRODUCER)

    def test_unlisted_plugin_not_allowed_even_with_rehashed_manifest(self):
        with tempfile.TemporaryDirectory() as temp:
            path = self.build(temp, inject=('gst/unlisted.so', b'unlisted'))
            with self.assertRaisesRegex(ValueError, 'dropped or injected'):
                verify(path, SOURCE, VERSION, PRODUCER)

    def test_tampered_module_not_allowed_even_with_rehashed_manifest(self):
        with tempfile.TemporaryDirectory() as temp:
            name = 'gst/libgstv4l2codecs.so'
            path = self.build(temp, substitute=(name, b'tampered'))
            with self.assertRaisesRegex(ValueError, 'GPU producer SHA mismatch'):
                verify(path, SOURCE, VERSION, PRODUCER)

    def test_exact_source_version_and_producer_revision_required(self):
        with tempfile.TemporaryDirectory() as temp:
            path = self.build(temp)
            for source, version, producer, message in (
                ('c' * 40, VERSION, PRODUCER, 'git_commit'),
                (SOURCE, '0.0.8-rc.2', PRODUCER, 'version'),
                (SOURCE, VERSION, 'd' * 40, 'git_revision'),
            ):
                with self.subTest(message=message), self.assertRaisesRegex(ValueError, message):
                    verify(path, source, version, producer)

    def test_symlink_member_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            archive = self.build(temp)
            with tarfile.open(archive, 'a:') as tar:
                item = tarfile.TarInfo('symlink')
                item.type = tarfile.SYMTYPE
                item.linkname = '../outside'
                tar.addfile(item)
            with self.assertRaisesRegex(ValueError, 'unsafe GPU tar member'):
                verify(archive, SOURCE, VERSION, PRODUCER)

    def test_duplicate_member_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            archive = self.build(temp)
            with tarfile.open(archive, 'a:') as tar:
                data = b'duplicate'
                item = tarfile.TarInfo('bin/nuraloumi-player')
                item.size = len(data)
                tar.addfile(item, io.BytesIO(data))
            with self.assertRaisesRegex(ValueError, 'duplicate GPU tar member'):
                verify(archive, SOURCE, VERSION, PRODUCER)


if __name__ == '__main__':
    unittest.main()
