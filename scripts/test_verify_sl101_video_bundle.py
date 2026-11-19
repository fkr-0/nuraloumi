"""Negative contract tests: a missing or untrusted bundle must never qualify."""
import io
import hashlib
import json
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parent))
from verify_sl101_video_bundle import ABIS, verify
from extract_sl101_video_bundle import extract

REV = 'a' * 40
DIGEST = 'b' * 64


class BundleConsumerTests(unittest.TestCase):
    def _fake_bundle(self, root, target, needed_libc=None, extra_needed=()):
        files = {}
        names = (
            'lib/libnuraloumi-video.so', 'gst/libgstv4l2codecs.so',
            'lib/libgallium-25.0.7.so', 'lib/libEGL.so.1',
            'lib/libGLESv2.so.2', 'lib/libgbm.so.1', 'lib/libdrm.so.2',
            'lib/libdrm_tegra.so.0',
        )
        for name in names:
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            payload = b'\x7fELF\x01' + b'\0' * 13 + b'\x28\0' + name.encode()
            path.write_bytes(payload)
            files[name] = hashlib.sha256(payload).hexdigest()
        meta = {
            'schema': 'sl101-gpu-video-bundle-v1', 'project': 'sl101_linux',
            'git_revision': REV, 'target': target, 'abi': ABIS[target],
            'files': files,
        }
        (root / 'provenance.json').write_text(json.dumps(meta))
        libc = needed_libc or ('libc.so.6' if target.endswith('gnueabihf') else 'libc.musl-armv7.so.1')

        def fake_run(args, **kwargs):
            if args[0] == 'readelf':
                if args[1] == '-h':
                    return SimpleNamespace(stdout='Flags: hard-float ABI\n')
                if args[1] == '-A':
                    return SimpleNamespace(stdout='Tag_ABI_VFP_args: VFP registers\nTag_FP_arch: VFPv3-D16\n')
                if args[1] == '-d':
                    soname = Path(args[2]).name
                    deps = [libc, *extra_needed] if soname == 'libnuraloumi-video.so' else []
                    data = f'(SONAME) Library soname: [{soname}]\n'
                    data += ''.join(f'(NEEDED) Shared library: [{d}]\n' for d in deps)
                    return SimpleNamespace(stdout=data)
            if args[:2] == ('sh', 'scripts/inspect-armv7-elf.sh'):
                return SimpleNamespace(stdout='PASS\n')
            raise AssertionError(f'unexpected command: {args}')
        return fake_run

    def test_both_targets_with_independent_elf_closure_pass(self):
        for target in ABIS:
            with self.subTest(target=target), tempfile.TemporaryDirectory() as tmp:
                fake_run = self._fake_bundle(Path(tmp), target)
                with patch('verify_sl101_video_bundle.subprocess.run', side_effect=fake_run):
                    self.assertEqual(verify(Path(tmp), REV, DIGEST, target)['target'], target)

    def test_cross_libc_rejected_for_both_targets(self):
        for target, wrong in (
            ('armv7-unknown-linux-musleabihf', 'libc.so.6'),
            ('armv7-unknown-linux-gnueabihf', 'libc.musl-armv7.so.1'),
        ):
            with self.subTest(target=target), tempfile.TemporaryDirectory() as tmp:
                fake_run = self._fake_bundle(Path(tmp), target, needed_libc=wrong)
                with patch('verify_sl101_video_bundle.subprocess.run', side_effect=fake_run):
                    with self.assertRaisesRegex(ValueError, 'wrong libc ABI'):
                        verify(Path(tmp), REV, DIGEST, target)

    def test_target_mismatch_and_unknown_target_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            self._fake_bundle(Path(tmp), 'armv7-unknown-linux-musleabihf')
            with self.assertRaisesRegex(ValueError, 'provenance mismatch: target'):
                verify(Path(tmp), REV, DIGEST, 'armv7-unknown-linux-gnueabihf')
            with self.assertRaisesRegex(ValueError, 'unsupported bundle target'):
                verify(Path(tmp), REV, DIGEST, 'x86_64-unknown-linux-gnu')

    def test_unclosed_shared_dependency_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            fake_run = self._fake_bundle(Path(tmp), 'armv7-unknown-linux-gnueabihf', extra_needed=('libunexpected.so.1',))
            with patch('verify_sl101_video_bundle.subprocess.run', side_effect=fake_run):
                with self.assertRaisesRegex(ValueError, 'missing ELF closure'):
                    verify(Path(tmp), REV, DIGEST, 'armv7-unknown-linux-gnueabihf')

    def test_missing_tegra_drm_module_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            self._fake_bundle(Path(tmp), 'armv7-unknown-linux-musleabihf')
            meta_path = Path(tmp) / 'provenance.json'
            meta = json.loads(meta_path.read_text())
            meta['files'].pop('lib/libdrm_tegra.so.0')
            meta_path.write_text(json.dumps(meta))
            (Path(tmp) / 'lib/libdrm_tegra.so.0').unlink()
            with self.assertRaisesRegex(ValueError, 'missing required private GPU-video module'):
                verify(Path(tmp), REV, DIGEST)

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
            with self.assertRaisesRegex(ValueError, 'missing required private GPU-video module'):
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
