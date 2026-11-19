#!/usr/bin/env python3
"""Extract only flat, regular producer files into an empty staging directory."""
from pathlib import Path
import sys
import tarfile


def extract(archive, destination):
    if any(destination.iterdir()):
        raise ValueError('destination is not empty')
    with tarfile.open(archive, 'r:gz') as tar:
        seen = set()
        for item in tar.getmembers():
            name = item.name
            rel = Path(name)
            if rel.is_absolute() or '..' in rel.parts or name in seen:
                raise ValueError(f'unsafe archive member: {name}')
            seen.add(name)
            if item.isdir():
                continue
            if not item.isfile() or not (name == 'provenance.json' or rel.parts[0] in ('lib', 'gst')):
                raise ValueError(f'unsupported archive member: {name}')
            target = destination / rel
            target.parent.mkdir(parents=True, exist_ok=True)
            with tar.extractfile(item) as src, target.open('xb') as dst:
                while chunk := src.read(1024 * 1024):
                    dst.write(chunk)


if __name__ == '__main__':
    if len(sys.argv) != 3:
        sys.exit('usage: extract_sl101_video_bundle.py ARCHIVE EMPTY_DIR')
    try:
        extract(Path(sys.argv[1]), Path(sys.argv[2]))
    except (OSError, ValueError, tarfile.TarError) as error:
        sys.exit(f'GPU_ARCHIVE=FAIL: {error}')
