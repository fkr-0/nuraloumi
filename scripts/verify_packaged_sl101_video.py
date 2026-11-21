#!/usr/bin/env python3
"""Verify the *final* source-pinned GPU release tar without extracting it.

This is a host-only artifact integrity check, never physical device proof.
"""

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import sys
import tarfile

from verify_sl101_video_bundle import ABIS, REQUIRED

SHA40 = re.compile(r'[0-9a-f]{40}\Z')
SHA64 = re.compile(r'[0-9a-f]{64}\Z')
VERSION = re.compile(r'[0-9]+(?:\.[0-9]+){2}(?:-(?:dev|rc)\.[0-9]+)?\Z')
PREFIX = 'lib/nuraloumi/video/'


def require(condition, message):
    if not condition:
        raise ValueError(message)


def verify(tar_path: Path, source_sha: str, expected_version: str,
           producer_revision: str, target='armv7-unknown-linux-musleabihf'):
    require(target in ABIS, 'unsupported target')
    require(bool(SHA40.fullmatch(source_sha)) and
            bool(SHA40.fullmatch(producer_revision)) and
            bool(VERSION.fullmatch(expected_version)), 'invalid source/version pin')
    files = {}
    with tarfile.open(tar_path, 'r:') as archive:
        for member in archive.getmembers():
            name = member.name.removeprefix('./')
            rel = PurePosixPath(name)
            require(name and not rel.is_absolute() and '..' not in rel.parts
                    and not member.issym() and not member.islnk()
                    and (member.isfile() or member.isdir()), 'unsafe GPU tar member')
            if member.isfile():
                require(name not in files, 'duplicate GPU tar member')
                stream = archive.extractfile(member)
                require(stream is not None, 'unreadable GPU tar member')
                files[name] = stream.read()

    require('MANIFEST.txt' in files, 'missing GPU package manifest')
    lines = files['MANIFEST.txt'].decode('utf-8').splitlines()
    require(lines.count('files:') == 1, 'invalid package manifest files marker')
    fields = {}
    for line in lines[:lines.index('files:')]:
        key, sep, value = line.partition('=')
        require(sep and key not in fields, 'invalid package manifest header')
        fields[key] = value
    for key, expected in {
        'schema': 'nuraloumi-package-manifest-v1',
        'git_commit': source_sha,
        'git_dirty': 'false',
        'target': target,
        'version': expected_version,
    }.items():
        require(fields.get(key) == expected, f'package {key} mismatch')

    manifest_files = {}
    for line in lines[lines.index('files:') + 1:]:
        digest, sep, name = line.strip().partition('  ')
        path = PurePosixPath(name)
        require(sep and SHA64.fullmatch(digest) and name and
                not path.is_absolute() and '..' not in path.parts and
                name not in manifest_files, 'invalid package file record')
        manifest_files[name] = digest
    require(set(files) == set(manifest_files) | {'MANIFEST.txt'},
            'partial or extra GPU package files')
    for name, digest in manifest_files.items():
        require(hashlib.sha256(files[name]).hexdigest() == digest,
                f'package file SHA mismatch: {name}')

    for binary in ('nuraloumi-player', 'nuraloumi-playerd'):
        data = files.get('bin/' + binary, b'')
        require(data[:5] == b'\x7fELF\x01' and data[18:20] == b'\x28\x00',
                f'missing ARM ELF player binary: {binary}')

    provenance_path = PREFIX + 'provenance.json'
    require(provenance_path in files, 'GPU producer provenance absent')
    meta = json.loads(files[provenance_path].decode('utf-8'))
    for key, expected in {
        'schema': 'sl101-gpu-video-bundle-v1',
        'project': 'sl101_linux',
        'git_revision': producer_revision,
        'target': target,
        'abi': ABIS[target],
    }.items():
        require(meta.get(key) == expected, f'GPU provenance {key} mismatch')
    producer_files = meta.get('files')
    require(isinstance(producer_files, dict) and
            set(REQUIRED).issubset(producer_files), 'incomplete GPU producer inventory')
    packaged_gpu = {name.removeprefix(PREFIX) for name in files if name.startswith(PREFIX)}
    require(packaged_gpu == set(producer_files) | {'provenance.json'},
            'GPU producer modules dropped or injected during packaging')
    for name, digest in producer_files.items():
        require(isinstance(name, str) and isinstance(digest, str),
                'invalid GPU producer record type')
        path = PurePosixPath(name)
        require(not path.is_absolute() and '..' not in path.parts and
                len(path.parts) >= 2 and path.parts[0] in ('gst', 'lib') and
                path.as_posix() == name and SHA64.fullmatch(digest),
                f'unsafe GPU producer record: {name}')
        require(hashlib.sha256(files[PREFIX + name]).hexdigest() == digest,
                f'GPU producer SHA mismatch: {name}')
    return {'source_sha': source_sha, 'version': expected_version,
            'producer_revision': producer_revision, 'gpu_modules': len(producer_files)}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('tar', type=Path)
    parser.add_argument('--source-sha', required=True)
    parser.add_argument('--expected-version', required=True)
    parser.add_argument('--producer-revision', required=True)
    parser.add_argument('--target', choices=ABIS, default='armv7-unknown-linux-musleabihf')
    args = parser.parse_args()
    try:
        result = verify(args.tar, args.source_sha, args.expected_version,
                        args.producer_revision, args.target)
    except (OSError, ValueError, tarfile.TarError, UnicodeError, json.JSONDecodeError) as error:
        sys.exit(f'PACKAGED_GPU_VIDEO=FAIL: {error}')
    print(f'PACKAGED_GPU_VIDEO=PASS {result} physical_device=PENDING')
