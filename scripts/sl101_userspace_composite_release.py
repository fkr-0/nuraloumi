#!/usr/bin/env python3
"""Guarded SL101 0.0.8-rc.1 release activation with separately pinned 0.0.7 video.

Install only from the original, hash-verified musl CI package plus exact
on-device legacy video subtree. Run with root-held qualification flock.
The old release, kernel, Mesa and compositor are never replaced.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import time

RELEASES = Path('/opt/nuraloumi/releases')
CURRENT = Path('/opt/nuraloumi/current')
NEW_COMMIT = 'fb47cfc73c614440dcbf6aecb12642085fef796a'
OLD_COMMIT = 'a5bbcc766e9e29bbff8555c7ae35b690e1199258'
TARGET = 'armv7-unknown-linux-musleabihf'
OLD_LINK = str(RELEASES / OLD_COMMIT / 'bin')
VIDEO_PREFIX = 'lib/nuraloumi/video/'
VIDEO_PINS = {
    'lib/nuraloumi/video/gst/libgstv4l2codecs.so': '79ee247faf6397829d908136ad16e04dc936437a1c71b6d49120e470ec452608',
    'lib/nuraloumi/video/lib/libgallium-25.0.7.so': '84af7822128f8a08a03c460164b00cd92b9058f22fdbe094f2811119d1d13cf8',
    'lib/nuraloumi/video/lib/libnuraloumi-video.so': '848386bbbd2042c7a881a079368a5368e85a2040d8a3b847abba3d557c19add6',
}


def require(ok: bool, reason: str) -> None:
    if not ok:
        raise RuntimeError('COMPOSITE_RELEASE_BLOCKED ' + reason)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for data in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(data)
    return digest.hexdigest()


def manifest(root: Path, version: str, commit: str) -> tuple[dict[str, str], dict[str, str]]:
    manifest_path = root / 'MANIFEST.txt'
    require(manifest_path.is_file() and not manifest_path.is_symlink(), 'manifest-missing')
    lines = manifest_path.read_text().splitlines()
    require(lines.count('files:') == 1, 'manifest-files')
    boundary = lines.index('files:')
    header: dict[str, str] = {}
    for line in lines[:boundary]:
        key, separator, value = line.partition('=')
        require(separator == '=' and key not in header, 'manifest-header')
        header[key] = value
    for key, expected in (
        ('schema', 'nuraloumi-package-manifest-v1'),
        ('version', version),
        ('target', TARGET),
        ('git_commit', commit),
        ('git_dirty', 'false'),
    ):
        require(header.get(key) == expected, 'manifest-' + key)
    hashes: dict[str, str] = {}
    for line in lines[boundary + 1:]:
        require(line.startswith('  ') and '  ' in line[2:], 'manifest-record')
        digest, name = line.strip().split('  ', 1)
        entry = Path(name)
        require(len(digest) == 64 and all(c in '0123456789abcdef' for c in digest), 'manifest-digest')
        require(name not in hashes and not entry.is_absolute()
                and all(part not in ('.', '..') for part in entry.parts)
                and entry.as_posix() == name, 'manifest-path')
        hashes[name] = digest
    return header, hashes


def verify_tree(root: Path, expected: dict[str, str], root_owned: bool) -> None:
    require(root.is_dir() and not root.is_symlink(), 'tree-root')
    entries = [root, *root.rglob('*')]
    files: set[str] = set()
    for path in entries:
        require(not path.is_symlink(), 'tree-symlink-' + str(path))
        if root_owned:
            info = path.stat()
            require(info.st_uid == 0 and not info.st_mode & 0o022, 'tree-owner-' + str(path))
        if path.is_file():
            name = path.relative_to(root).as_posix()
            files.add(name)
            if name != 'MANIFEST.txt':
                require(expected.get(name) == sha256(path), 'tree-sha-' + name)
        else:
            require(path.is_dir(), 'tree-special-' + str(path))
    require(files == set(expected) | {'MANIFEST.txt'}, 'tree-inventory')


def make_release_traversable(root: Path) -> None:
    """Fix staging's private (0700) directories after copying as root.

    Never widen ownership or write bits. Files retain CI/legacy modes.
    """
    for directory in [root, *(path for path in root.rglob('*') if path.is_dir())]:
        info = directory.stat()
        require(info.st_uid == 0 and not info.st_mode & 0o022, 'publish-owner')
        if info.st_mode & 0o005 != 0o005:
            os.chmod(directory, (info.st_mode & 0o7777) | 0o055)
        require(directory.stat().st_mode & 0o005 == 0o005, 'publish-traverse')


def verify_composite(app: Path, combined: Path, old: Path, root_owned: bool) -> dict[str, str]:
    header, app_hashes = manifest(app, '0.0.8-rc.1', NEW_COMMIT)
    old_header, old_hashes = manifest(old, '0.0.7-dev.1', OLD_COMMIT)
    require(len(header.get('binaries', '').split(',')) == 28, 'new-binary-count')
    require(len(set(header['binaries'].split(','))) == 28, 'new-binary-duplicates')
    require(len(old_header.get('binaries', '').split(',')) == 28, 'old-binary-count')
    overlay = {name: digest for name, digest in old_hashes.items() if name.startswith(VIDEO_PREFIX)}
    require(len(overlay) == 23 and set(overlay).isdisjoint(app_hashes), 'overlay-inventory')
    for name, digest in VIDEO_PINS.items():
        require(overlay.get(name) == digest, 'video-pin-' + name)
    # The existing root release supplies only the GPU private subtree.
    for name, digest in overlay.items():
        path = old / name
        require(path.is_file() and not path.is_symlink() and sha256(path) == digest, 'old-video-sha-' + name)
    verify_tree(app, app_hashes, False)
    require((combined / 'MANIFEST.txt').read_bytes() == (app / 'MANIFEST.txt').read_bytes(), 'ci-manifest-modified')
    verify_tree(combined, app_hashes | overlay, root_owned)
    for binary in header['binaries'].split(','):
        path = combined / 'bin' / binary
        require(path.is_file() and path.stat().st_mode & 0o100
                and path.open('rb').read(5) == b'\x7fELF\x01', 'new-binary-' + binary)
    return header


def pids_for(exe: Path) -> list[int]:
    found: list[int] = []
    for proc in Path('/proc').iterdir():
        if proc.name.isdigit():
            try:
                if os.readlink(proc / 'exe') == str(exe):
                    found.append(int(proc.name))
            except OSError:
                pass
    return found


def alive(pid: int) -> bool:
    try:
        status = Path(f'/proc/{pid}/status').read_text()
    except FileNotFoundError:
        return False
    return not any(line.startswith('State:') and line.split()[1] in ('Z', 'X')
                   for line in status.splitlines())


def halt(pid: int) -> None:
    if alive(pid):
        os.kill(pid, signal.SIGTERM)
    for _ in range(60):
        if not alive(pid):
            return
        time.sleep(0.1)
    require(False, 'playerd-still-running')


def atomic_current(target: str) -> None:
    temp = CURRENT.with_name('.current-composite-' + str(os.getpid()))
    require(not temp.exists() and not temp.is_symlink(), 'symlink-temp-exists')
    os.symlink(target, temp)
    os.replace(temp, CURRENT)
    handle = os.open(str(CURRENT.parent), os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(handle)
    finally:
        os.close(handle)


def client_status() -> dict:
    command = ['sudo', '-n', '-u', 'user', 'env', 'HOME=/home/user',
               'XDG_RUNTIME_DIR=/run/sl101-desktop-user', 'WAYLAND_DISPLAY=wayland-1',
               '/usr/local/bin/nuraloumi-player', '--status']
    result = subprocess.run(command, text=True, capture_output=True, timeout=15)
    require(result.returncode == 0, 'client-status-' + result.stderr[-120:])
    return json.loads(result.stdout)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', required=True, type=Path, help='Original unmodified software-only CI extraction')
    parser.add_argument('--stage', required=True, type=Path, help='Composite staging, 0.0.8 app plus 0.0.7 video')
    parser.add_argument('--previous', required=True)
    parser.add_argument('--sd-cid', required=True)
    parser.add_argument('--emmc-cid', required=True)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--dry-run', action='store_true')
    mode.add_argument('--apply', action='store_true')
    args = parser.parse_args()

    require(os.geteuid() == 0 and os.environ.get('NURA_RELEASE_LOCK_HELD') == '1', 'root-lock')
    require(os.uname().nodename == 'sl101-nura'
            and os.uname().release == '7.0.1-postmarketos-grate-dmavde1', 'device')
    require(Path('/sys/block/mmcblk1/device/cid').read_text().strip() == args.sd_cid, 'sd-cid')
    require(Path('/sys/block/mmcblk0/device/cid').read_text().strip() == args.emmc_cid, 'emmc-cid')
    battery = int(Path('/sys/class/power_supply/sbs-5-000b/capacity').read_text())
    require(0 <= battery <= 100, 'battery-invalid')
    ac = Path('/sys/class/power_supply/ac-adapter-detect/online').read_text().strip()
    require(ac == '1' or battery >= 50, 'insufficient-power')
    require(args.previous == OLD_LINK, 'previous-not-pinned')
    require(CURRENT.is_symlink() and os.readlink(CURRENT) == OLD_LINK, 'current-drift')
    require(len(pids_for(Path('/usr/local/bin/sl101-sway'))) == 1, 'sway-count')
    sway = pids_for(Path('/usr/local/bin/sl101-sway'))[0]
    require(Path('/usr/local/bin/nuraloumi-player').is_symlink()
            and os.readlink('/usr/local/bin/nuraloumi-player') == '/opt/nuraloumi/current/nuraloumi-player',
            'player-symlink')
    status = client_status()
    stopped = status.get('status') == 'Stopped'
    failed_idle = (status.get('status') == 'Error' and status.get('position_ms') == 0
                   and status.get('duration_ms') == 0)
    require(stopped or failed_idle, 'player-busy')
    old_daemons = pids_for(Path(OLD_LINK) / 'nuraloumi-playerd')
    require(len(old_daemons) <= 4, 'old-daemon-count')
    old = RELEASES / OLD_COMMIT
    verify_composite(args.app, args.stage, old, False)
    new = RELEASES / NEW_COMMIT
    if new.exists():
        verify_composite(args.app, new, old, True)
    print('COMPOSITE_PREFLIGHT_PASS version=0.0.8-rc.1 source=CI-musl commit=' + NEW_COMMIT
          + ' video=0.0.7-on-device video_files=23 battery=' + str(battery)
          + ' ac=' + ac + ' rollback=' + OLD_LINK, flush=True)
    if args.dry_run:
        return

    temporary = RELEASES / ('.incoming-' + NEW_COMMIT + '-' + str(os.getpid()))
    require(not temporary.exists(), 'temp-exists')
    switched = False
    try:
        if not new.exists():
            shutil.copytree(args.stage, temporary)
            verify_composite(args.app, temporary, old, True)
            os.rename(temporary, new)
        # Private staging parents are 0700. A root-owned release must still
        # be readable/executable by the desktop's unprivileged user.
        make_release_traversable(new)
        atomic_current(str(new / 'bin'))
        switched = True
        for pid in old_daemons:
            halt(pid)
        status = client_status()
        require(status.get('status') == 'Stopped' and status.get('error') is None
                and 'VDE' in status.get('gpu_status', ''), 'new-player-unhealthy')
        require(pids_for(Path('/usr/local/bin/sl101-sway')) == [sway], 'sway-drift')
        require(os.readlink(CURRENT) == str(new / 'bin'), 'current-unexpected')
        print('COMPOSITE_DEPLOY_PASS version=0.0.8-rc.1 commit=' + NEW_COMMIT
              + ' gpu=0.0.7-qualified rollback=' + OLD_LINK, flush=True)
    except Exception:
        if switched:
            for pid in pids_for(new / 'bin/nuraloumi-playerd'):
                halt(pid)
            atomic_current(OLD_LINK)
            try:
                client_status()
            except Exception:
                pass
            print('COMPOSITE_ROLLBACK_COMPLETE current=' + OLD_LINK, file=sys.stderr)
        raise
    finally:
        if temporary.exists():
            shutil.rmtree(temporary)


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        print('COMPOSITE_RELEASE_FAIL ' + str(error), file=sys.stderr)
        sys.exit(7)
