#!/usr/bin/env python3
"""Atomic SL101 NuraLoumi userspace release activation (no flash/reboot).

Run under a root-held /run/lock/sl101-nura-qualification.lock. The incoming
release must be root-owned, immutable to unprivileged users, and SHA-pinned.
Use --dry-run before --apply. Never changes the kernel, Sway or system Mesa.
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
RUNTIME = '/run/sl101-desktop-user'
EXPECTED_KERNEL = '7.0.1-postmarketos-grate-dmavde1'
EXPECTED_HOSTNAME = 'sl101-nura'
EXPECTED_VERSION = '0.0.7-dev.1'
EXPECTED_COMMIT = '9753fae3895e34735cdb4e3269a86f6590911283'
EXPECTED_VIDEO = {
    'lib/nuraloumi/video/gst/libgstv4l2codecs.so': '79ee247faf6397829d908136ad16e04dc936437a1c71b6d49120e470ec452608',
    'lib/nuraloumi/video/lib/libgallium-25.0.7.so': '84af7822128f8a08a03c460164b00cd92b9058f22fdbe094f2811119d1d13cf8',
    'lib/nuraloumi/video/lib/libnuraloumi-video.so': '848386bbbd2042c7a881a079368a5368e85a2040d8a3b847abba3d557c19add6',
}


def require(ok: bool, why: str) -> None:
    if not ok:
        raise RuntimeError('USERSpace_RELEASE_BLOCKED ' + why)


def battery_reading_valid(capacity: int) -> bool:
    """Validate sensor input without imposing a userspace install threshold."""
    return 0 <= capacity <= 100


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open('rb') as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()


def verify_stage(root: Path, must_be_root_owned: bool = True) -> dict:
    require(root.is_dir() and not root.is_symlink(), 'stage-dir')
    require(not must_be_root_owned or (root.stat().st_uid == 0 and not root.stat().st_mode & 0o022), 'stage-owner')
    manifest = root / 'MANIFEST.txt'
    require(manifest.is_file() and not manifest.is_symlink(), 'manifest')
    lines = manifest.read_text().splitlines()
    require('files:' in lines, 'manifest-files')
    boundary = lines.index('files:')
    header = dict(line.split('=', 1) for line in lines[:boundary])
    for key, value in [('schema', 'nuraloumi-package-manifest-v1'),
                       ('target', 'armv7-unknown-linux-musleabihf'),
                       ('version', EXPECTED_VERSION), ('git_commit', EXPECTED_COMMIT),
                       ('git_dirty', 'false')]:
        require(header.get(key) == value, 'manifest-' + key)
    binaries = header.get('binaries', '').split(',')
    require(len(binaries) == 27 and len(set(binaries)) == 27, 'binary-list')
    expected = {}
    for line in lines[boundary + 1:]:
        require(line.startswith('  ') and '  ' in line[2:], 'manifest-record')
        digest, name = line.strip().split('  ', 1)
        candidate = Path(name)
        require(len(digest) == 64 and all(c in '0123456789abcdef' for c in digest), 'manifest-digest')
        require(name not in expected and not candidate.is_absolute()
                and all(part not in ('.', '..') for part in candidate.parts), 'manifest-path')
        expected[name] = digest
    present = set()
    for item in root.rglob('*'):
        require(not item.is_symlink(), 'stage-symlink')
        if must_be_root_owned:
            require(item.stat().st_uid == 0 and not item.stat().st_mode & 0o022, 'stage-permissions')
        if item.is_file():
            relative = item.relative_to(root).as_posix()
            present.add(relative)
            if relative != 'MANIFEST.txt':
                require(expected.get(relative) == sha256(item), 'file-sha-' + relative)
    require(present == set(expected) | {'MANIFEST.txt'}, 'file-inventory')
    for binary in binaries:
        item = root / 'bin' / binary
        require(item.is_file() and item.stat().st_mode & 0o100
                and item.open('rb').read(5) == b'\x7fELF\x01', 'binary-' + binary)
    for name, digest in EXPECTED_VIDEO.items():
        require(expected.get(name) == digest, 'video-sha-' + name)
    return header


def pids_for(exe: Path) -> list[int]:
    matches = []
    for proc in Path('/proc').iterdir():
        if not proc.name.isdigit():
            continue
        try:
            if os.readlink(proc / 'exe') == str(exe):
                matches.append(int(proc.name))
        except OSError:
            pass
    return matches


def atomic_current(link_target: str) -> None:
    tmp = CURRENT.with_name('.current-userspace-' + str(os.getpid()))
    require(not tmp.exists() and not tmp.is_symlink(), 'link-temp-conflict')
    os.symlink(link_target, tmp)
    os.replace(tmp, CURRENT)
    fd = os.open(str(CURRENT.parent), os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def process_running(pid: int) -> bool:
    # A child may remain as a zombie until its UI parent reaps waitpid().
    # SIGTERM already ended it; a zombie has no executable or resources and
    # must not prevent a valid playerservice deployment/rollback.
    try:
        status = Path(f'/proc/{pid}/status').read_text()
    except FileNotFoundError:
        return False
    return not any(line.startswith('State:') and line.split()[1] in ('Z', 'X')
                   for line in status.splitlines())


def halt(pid: int) -> None:
    if not process_running(pid):
        return
    os.kill(pid, signal.SIGTERM)
    for _ in range(60):
        if not process_running(pid):
            return
        time.sleep(0.1)
    require(False, 'playerd-still-running')


def client_status() -> dict:
    cmd = ['sudo', '-n', '-u', 'user', 'env', 'HOME=/home/user',
           'XDG_RUNTIME_DIR=' + RUNTIME, 'WAYLAND_DISPLAY=wayland-1',
           '/usr/local/bin/nuraloumi-player', '--status']
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=12)
    require(result.returncode == 0, 'player-status-' + result.stderr[-120:])
    return json.loads(result.stdout)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--stage', type=Path, required=True)
    parser.add_argument('--previous', required=True, help='Exact existing /opt/nuraloumi/current symlink target')
    parser.add_argument('--sd-cid', required=True)
    parser.add_argument('--emmc-cid', required=True)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--dry-run', action='store_true')
    mode.add_argument('--apply', action='store_true')
    args = parser.parse_args()
    require(os.geteuid() == 0 and os.environ.get('NURA_RELEASE_LOCK_HELD') == '1', 'root-lock')
    require(os.uname().nodename == EXPECTED_HOSTNAME and os.uname().release == EXPECTED_KERNEL, 'device')
    require(Path('/sys/block/mmcblk1/device/cid').read_text().strip() == args.sd_cid, 'sd-cid')
    require(Path('/sys/block/mmcblk0/device/cid').read_text().strip() == args.emmc_cid, 'emmc-cid')
    battery = int(Path('/sys/class/power_supply/sbs-5-000b/capacity').read_text().strip())
    require(battery_reading_valid(battery), 'invalid-battery-reading')
    ac_online = Path('/sys/class/power_supply/ac-adapter-detect/online').read_text().strip()
    charging = Path('/sys/class/power_supply/sbs-5-000b/status').read_text().strip()
    require(CURRENT.is_symlink() and os.readlink(CURRENT) == args.previous, 'current-drift')
    require(args.previous.startswith(str(RELEASES) + '/') and args.previous.endswith('/bin'), 'previous-path')
    existing = RELEASES / EXPECTED_COMMIT
    if existing.exists():
        # A previous attempt may have installed and verified the payload,
        # then rolled back only the current symlink. Reuse exact bytes.
        verify_stage(existing)
    require(len(pids_for(Path('/usr/local/bin/sl101-sway'))) == 1, 'sway-identity')
    sway = pids_for(Path('/usr/local/bin/sl101-sway'))[0]
    current_status = client_status()
    # A failed old VDE playback leaves a stable Error snapshot. Replacing that
    # broken daemon is precisely the intended recovery; reject active media.
    idle = current_status.get('status') == 'Stopped'
    failed_idle = (current_status.get('status') == 'Error'
                   and current_status.get('position_ms') == 0
                   and current_status.get('duration_ms') == 0)
    require(idle or failed_idle, 'player-active')
    if failed_idle:
        print('USERSPACE_OLD_PLAYER_ERROR ' + str(current_status.get('error')), flush=True)
    old_daemons = pids_for(Path(args.previous) / 'nuraloumi-playerd')
    # A failing old player may leave an orphaned daemon alongside the UI-owned
    # instance. They all have the exact old release executable and can be
    # stopped individually; do not blanket-kill by process name.
    require(len(old_daemons) <= 4, 'playerd-count')
    if len(old_daemons) > 1:
        print('USERSPACE_OLD_PLAYERD_DUPLICATES count=' + str(len(old_daemons)), flush=True)
    require(Path('/usr/local/bin/nuraloumi-player').is_symlink(), 'player-symlink')
    require(os.readlink('/usr/local/bin/nuraloumi-player') == '/opt/nuraloumi/current/nuraloumi-player', 'player-symlink-target')
    verify_stage(args.stage)
    print('USERSPACE_PREFLIGHT_PASS battery=' + str(battery) + ' ac=' + ac_online + ' status=' + charging + ' threshold=none old=' + args.previous + ' new=' + EXPECTED_COMMIT, flush=True)
    if args.dry_run:
        return
    temporary = RELEASES / ('.incoming-' + EXPECTED_COMMIT + '-' + str(os.getpid()))
    new = RELEASES / EXPECTED_COMMIT
    require(not temporary.exists(), 'temporary-conflict')
    switched = False
    try:
        if not new.exists():
            shutil.copytree(args.stage, temporary)
            verify_stage(temporary)
            os.rename(temporary, new)
        else:
            verify_stage(new)
        atomic_current(str(new / 'bin'))
        switched = True
        for pid in old_daemons:
            halt(pid)
        result = client_status()
        require(result.get('error') is None and 'VDE' in result.get('gpu_status', ''), 'new-player-unhealthy')
        require(len(pids_for(Path('/usr/local/bin/sl101-sway'))) == 1
                and pids_for(Path('/usr/local/bin/sl101-sway'))[0] == sway, 'sway-changed')
        require(os.readlink(CURRENT) == str(new / 'bin'), 'symlink-changed')
        print('USERSPACE_DEPLOY_PASS version=' + EXPECTED_VERSION + ' commit=' + EXPECTED_COMMIT
              + ' battery=' + str(battery) + ' rollback=' + args.previous
              + ' video_gpu_status=' + result['gpu_status'], flush=True)
    except Exception:
        if switched:
            for pid in pids_for(new / 'bin/nuraloumi-playerd'):
                halt(pid)
            atomic_current(args.previous)
            try:
                client_status()
            except Exception:
                pass
            print('USERSPACE_ROLLBACK_COMPLETE current=' + args.previous, file=sys.stderr)
        raise
    finally:
        if temporary.exists():
            shutil.rmtree(temporary)


if __name__ == '__main__':
    try:
        main()
    except Exception as exc:
        print('USERSPACE_RELEASE_FAIL ' + str(exc), file=sys.stderr)
        sys.exit(7)
