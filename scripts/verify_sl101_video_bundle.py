#!/usr/bin/env python3
"""Fail-closed source/ABI verifier for an independently produced SL101 bundle."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys

ABIS = {
    'armv7-unknown-linux-musleabihf': 'armv7-musleabihf-vfpv3-d16-no-neon-no-idiv',
    'armv7-unknown-linux-gnueabihf': 'armv7-gnueabihf-vfpv3-d16-no-neon-no-idiv',
}
REQUIRED = (
    'lib/libnuraloumi-video.so',
    'lib/libgallium-25.0.7.so',
    'lib/libEGL.so.1',
    'lib/libGLESv2.so.2',
    'lib/libgbm.so.1',
    'lib/libdrm.so.2',
    'lib/libdrm_tegra.so.0',
    'gst/libgstv4l2codecs.so',
)
SYSTEM = {
    'armv7-unknown-linux-musleabihf': {'libc.musl-armv7.so.1', 'libgcc_s.so.1'},
    'armv7-unknown-linux-gnueabihf': {'libc.so.6', 'ld-linux-armhf.so.3', 'libm.so.6', 'libgcc_s.so.1'},
}


def verify(root, revision, digest, target='armv7-unknown-linux-musleabihf'):
    if target not in ABIS:
        raise ValueError(f'unsupported bundle target: {target}')
    if not re.fullmatch('[a-f0-9]{40}', revision) or not re.fullmatch('[a-f0-9]{64}', digest):
        raise ValueError('unpinned producer revision or archive digest')
    if not root.is_dir() or root.is_symlink():
        raise ValueError('bundle directory absent or symlinked')
    meta = json.loads((root / 'provenance.json').read_text())
    for key, value in {'schema':'sl101-gpu-video-bundle-v1', 'project':'sl101_linux', 'git_revision':revision, 'target':target, 'abi':ABIS[target]}.items():
        if meta.get(key) != value:
            raise ValueError(f'provenance mismatch: {key}')
    files = meta.get('files')
    if not isinstance(files, dict) or not all(x in files for x in REQUIRED):
        raise ValueError('missing required private GPU-video module')
    if {p.relative_to(root).as_posix() for p in root.rglob('*') if p.is_file()} != set(files) | {'provenance.json'}:
        raise ValueError('unlisted or missing files')
    sonames, needed = set(), set()
    for name, sha in files.items():
        rel = Path(name)
        if rel.is_absolute() or '..' in rel.parts or rel.parts[0] not in ('lib', 'gst'):
            raise ValueError(f'unsafe path: {name}')
        path = root / rel
        if path.is_symlink() or not re.fullmatch('[a-f0-9]{64}', str(sha)) or hashlib.sha256(path.read_bytes()).hexdigest() != sha:
            raise ValueError(f'file SHA mismatch: {name}')
        with path.open('rb') as stream:
            data = stream.read(20)
        if data[:5] != b'\x7fELF\x01' or data[18:20] != b'\x28\x00':
            raise ValueError(f'not ELF32 ARM: {name}')
        h, a, d = (subprocess.run(('readelf', flag, str(path)), capture_output=True, text=True, check=True).stdout for flag in ('-h', '-A', '-d'))
        if ('hard-float ABI' not in h or 'Tag_ABI_VFP_args: VFP registers' not in a
                or 'Tag_FP_arch: VFPv3-D16' not in a or 'NEON' in a or 'Advanced_SIMD' in a):
            raise ValueError(f'wrong ARM ABI: {name}')
        if any(x in d for x in ('(RPATH)', '(RUNPATH)')):
            raise ValueError(f'unsafe loader: {name}')
        # The two ARM hard-float targets share an instruction set, not a
        # dynamic linker or libc. Reject cross-ABI contamination explicitly.
        forbidden_libc = 'libc.so.6' if target.endswith('musleabihf') else 'libc.musl-armv7.so.1'
        if forbidden_libc in d:
            raise ValueError(f'wrong libc ABI: {name}')
        for soname in re.findall(r'\(SONAME\).*\[([^]]+)\]', d):
            if soname in sonames:
                raise ValueError(f'duplicate SONAME: {soname}')
            sonames.add(soname)
        needed.update(re.findall(r'\(NEEDED\).*\[([^]]+)\]', d))
        subprocess.run(('sh', 'scripts/inspect-armv7-elf.sh', str(path)), check=True, capture_output=True)
    if needed - sonames - SYSTEM[target]:
        raise ValueError(f'missing ELF closure: {sorted(needed - sonames - SYSTEM[target])}')
    required_libc = 'libc.musl-armv7.so.1' if target.endswith('musleabihf') else 'libc.so.6'
    if required_libc not in needed:
        raise ValueError(f'bundle has no {required_libc} dependency')
    return meta


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('bundle', type=Path)
    parser.add_argument('--producer-revision', required=True)
    parser.add_argument('--archive-sha256', required=True)
    parser.add_argument('--target', choices=tuple(ABIS), default='armv7-unknown-linux-musleabihf')
    args = parser.parse_args()
    try:
        verify(args.bundle, args.producer_revision, args.archive_sha256, args.target)
    except (OSError, ValueError, subprocess.CalledProcessError, json.JSONDecodeError) as exc:
        sys.exit(f'GPU_VIDEO_BUNDLE=FAIL: {exc}')
    print('GPU_VIDEO_BUNDLE=PASS; physical hardware qualification pending')
