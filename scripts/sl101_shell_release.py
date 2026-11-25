#!/usr/bin/env python3
"""Transactional SL101 shell-only release; WebKit/GPU remain separate.

Execute as root while holding the external Nura release flock. The staged
package is complete and hash-verified; only an explicitly pinned video closure
is inherited from the previous release. Sway, DRM, kernel and WebKit are not
restarted. The Sway panel hook owns same-session IPC socket recovery.
"""
from __future__ import annotations

import argparse
import hashlib
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import time


VIDEO = "lib/nuraloumi/video"
VIDEO_PINS = {
    "gst/libgstv4l2codecs.so": "79ee247faf6397829d908136ad16e04dc936437a1c71b6d49120e470ec452608",
    "lib/libgallium-25.0.7.so": "84af7822128f8a08a03c460164b00cd92b9058f22fdbe094f2811119d1d13cf8",
    "lib/libnuraloumi-video.so": "848386bbbd2042c7a881a079368a5368e85a2040d8a3b847abba3d557c19add6",
}
REQUIRED = {
    "nuraloumi-panel", "nuraloumi-menu", "nuraloumi-notificationd",
    "nuraloumi-ctl", "nuraloumi-player", "nuraloumi-playerd",
}


def require(condition: bool, reason: str) -> None:
    if not condition:
        raise RuntimeError("SHELL_RELEASE_BLOCKED " + reason)


def digest(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def read_manifest(root: Path) -> tuple[dict[str, str], dict[str, str]]:
    manifest = root / "MANIFEST.txt"
    require(manifest.is_file() and not manifest.is_symlink(), "manifest")
    header, files = {}, {}
    in_files = False
    for line in manifest.read_text().splitlines():
        if line == "files:":
            require(not in_files, "duplicate-files-marker")
            in_files = True
        elif in_files:
            match = re.fullmatch(r"  ([0-9a-f]{64})  (.+)", line)
            require(match is not None, "manifest-record")
            sha, name = match.groups()
            path = Path(name)
            require(not path.is_absolute() and path.parts
                    and all(part not in (".", "..") for part in path.parts)
                    and path.as_posix() == name and name not in files, "manifest-path")
            files[name] = sha
        else:
            key, sep, value = line.partition("=")
            require(sep == "=" and key not in header, "manifest-header")
            header[key] = value
    require(in_files, "manifest-files")
    for key, value in {
        "schema": "nuraloumi-package-manifest-v1",
        "target": "armv7-unknown-linux-musleabihf",
        "git_dirty": "false",
        "release_profile": "shell-only",
        "browser_runtime": "deferred",
    }.items():
        require(header.get(key) == value, "manifest-" + key)
    require(re.fullmatch(r"[0-9a-f]{40}", header.get("git_commit", "")) is not None,
            "manifest-commit")
    require(re.fullmatch(r"[0-9A-Za-z.+-]+", header.get("version", "")) is not None,
            "manifest-version")
    binaries = header.get("binaries", "").split(",")
    require(len(binaries) == len(set(binaries)) and len(binaries) >= 28
            and REQUIRED.issubset(binaries), "manifest-binaries")
    require(all(re.fullmatch(r"nuraloumi-[a-z0-9-]+", name) for name in binaries),
            "binary-name")
    require(all("bin/" + name in files for name in binaries), "binary-inventory")
    return header, files


def inventory(root: Path, *, root_owned: bool = True) -> dict[str, str]:
    require(root.is_dir() and not root.is_symlink(), "tree")
    found = {}
    for path in [root, *root.rglob("*")]:
        require(not path.is_symlink(), "tree-symlink")
        stat = path.stat()
        if root_owned:
            require(stat.st_uid == 0 and not stat.st_mode & 0o022, "tree-owner-mode")
        if path.is_file():
            found[path.relative_to(root).as_posix()] = digest(path)
        else:
            require(path.is_dir(), "tree-special")
    return found


def verify_stage(root: Path, *, inherited: bool = False, root_owned: bool = True) -> dict[str, str]:
    header, files = read_manifest(root)
    found = inventory(root, root_owned=root_owned)
    manifest = found.pop("MANIFEST.txt", None)
    require(manifest is not None, "missing-manifest")
    if inherited:
        inherited_file = root / "VIDEO-INHERITED.txt"
        require(inherited_file.is_file(), "missing-inherited-manifest")
        entries = {}
        for line in inherited_file.read_text().splitlines():
            match = re.fullmatch(r"([0-9a-f]{64})  (.+)", line)
            require(match is not None, "inherited-record")
            sha, rel = match.groups()
            require(rel.startswith(VIDEO + "/") and rel not in entries, "inherited-path")
            entries[rel] = sha
        found.pop("VIDEO-INHERITED.txt")
        require(set(entries).isdisjoint(files), "overlay-collision")
        files = files | entries
    require(found == files, "file-inventory-or-sha")
    for name in header["binaries"].split(","):
        file = root / "bin" / name
        # ELF32 little-endian ARM (e_machine = EM_ARM = 40).
        with file.open("rb") as stream:
            elf = stream.read(20)
        require(len(elf) == 20 and elf[:6] == b"\x7fELF\x01\x01"
                and elf[18:20] == b"\x28\x00"
                and file.stat().st_mode & 0o111, "binary-arm-" + name)
    return header


def video_files(old: Path) -> dict[str, str]:
    root = old / VIDEO
    if not root.exists():
        return {}
    found = inventory(root)
    for name, sha in VIDEO_PINS.items():
        require(found.get(name) == sha, "inherited-video-pin-" + name)
    return {VIDEO + "/" + name: sha for name, sha in sorted(found.items())}


def atomic_link(current: Path, target: Path) -> None:
    temporary = current.with_name(".current-shell-" + str(os.getpid()))
    require(not temporary.exists() and not temporary.is_symlink(), "link-temporary")
    os.symlink(str(target), temporary)
    os.replace(temporary, current)
    directory_fd = os.open(current.parent, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(directory_fd)
    finally:
        os.close(directory_fd)


def pids_for(executable: Path) -> list[int]:
    found = []
    for proc in Path("/proc").iterdir():
        if not proc.name.isdigit():
            continue
        try:
            if os.readlink(proc / "exe") == str(executable):
                found.append(int(proc.name))
        except OSError:
            pass
    return found


def stop_pid(pid: int) -> None:
    os.kill(pid, signal.SIGTERM)
    for _ in range(40):
        try:
            status = Path(f"/proc/{pid}/status").read_text()
        except FileNotFoundError:
            return
        if any(line.startswith("State:") and line.split()[1] in ("Z", "X")
               for line in status.splitlines()):
            return
        time.sleep(0.1)
    require(False, "daemon-still-running-" + str(pid))


def desktop_env(args: argparse.Namespace) -> dict[str, str]:
    return {
        "HOME": "/home/" + args.desktop_user,
        "XDG_RUNTIME_DIR": args.runtime_dir,
        "WAYLAND_DISPLAY": args.wayland_display,
        "DBUS_SESSION_BUS_ADDRESS": "unix:path=" + args.runtime_dir + "/sl101-desktop-bus",
        "PATH": "/usr/local/bin:/usr/bin:/usr/sbin:/bin:/sbin",
    }


def desktop_command(args: argparse.Namespace, program: str, *options: str) -> subprocess.CompletedProcess:
    env_args = [key + "=" + value for key, value in desktop_env(args).items()]
    result = subprocess.run(["sudo", "-n", "-u", args.desktop_user, "env",
                             *env_args, program, *options],
                            capture_output=True, text=True, timeout=25)
    return result


def restart_panel(args: argparse.Namespace, expected_exe: Path) -> None:
    result = desktop_command(args, args.panel_hook)
    require(result.returncode == 0, "panel-restart-" + result.stderr[-180:])
    require(len(pids_for(expected_exe)) == 1, "panel-executable-not-live")
    socket = Path(args.runtime_dir) / "nuraloumi.sock"
    require(socket.is_socket() and str(socket) in Path("/proc/net/unix").read_text(),
            "panel-ipc-not-listening")


def notify_activation(args: argparse.Namespace, expected_exe: Path) -> None:
    result = desktop_command(args, "notify-send", "-a", "NuraLoumi",
                             "NuraLoumi shell updated")
    require(result.returncode == 0, "notification-activation-" + result.stderr[-180:])
    require(len(pids_for(expected_exe)) == 1, "notificationd-executable-not-live")


def ensure_command_links(binaries: str, current: Path) -> list[Path]:
    """Add only missing command names; never replace another installed tool."""
    created = []
    for name in binaries.split(","):
        link = Path("/usr/local/bin") / name
        target = str(current / name)
        if link.is_symlink():
            require(os.readlink(link) == target, "foreign-command-link-" + name)
        elif link.exists():
            require(False, "foreign-command-file-" + name)
        else:
            os.symlink(target, link)
            created.append(link)
    return created


def preflight(args: argparse.Namespace) -> tuple[dict[str, str], Path, list[int], list[int]]:
    require(os.geteuid() == 0 and os.environ.get("NURA_RELEASE_LOCK_HELD") == "1",
            "root-and-lock")
    require(os.uname().nodename == args.expected_hostname
            and os.uname().release == args.expected_kernel, "device")
    require(Path("/sys/block/mmcblk1/device/cid").read_text().strip() == args.sd_cid, "sd-cid")
    require(Path("/sys/block/mmcblk0/device/cid").read_text().strip() == args.emmc_cid,
            "emmc-cid")
    require(args.current.is_symlink() and os.readlink(args.current) == args.previous,
            "current-drift")
    previous = Path(args.previous)
    require(previous.parent.parent == args.releases and previous.name == "bin"
            and previous.is_dir(), "previous-release")
    require(Path(args.runtime_dir, args.wayland_display).is_socket(), "wayland-socket")
    require(Path(args.panel_hook).is_file(), "panel-hook")
    for name in header_names_for_links(args.stage):
        link = Path("/usr/local/bin") / name
        require(not link.exists() or
                (link.is_symlink() and os.readlink(link) == str(args.current / name)),
                "command-link-conflict-" + name)
    sway = pids_for(Path("/usr/local/bin/sl101-sway"))
    require(len(sway) == 1, "sway-identity")
    header = verify_stage(args.stage)
    require(header["git_commit"] != previous.parent.name[:40], "same-commit")
    old_notification = pids_for(previous / "nuraloumi-notificationd")
    require(len(old_notification) <= 2, "notificationd-count")
    video_files(previous.parent)
    print(f"SHELL_PREFLIGHT_PASS version={header['version']} commit={header['git_commit']} "
          f"old={args.previous} sway={sway[0]} notificationd={old_notification}", flush=True)
    return header, previous, sway, old_notification


def header_names_for_links(stage: Path) -> list[str]:
    header, _ = read_manifest(stage)
    return header["binaries"].split(",")


def activate(args: argparse.Namespace) -> None:
    header, old, sway, old_notifications = preflight(args)
    if args.dry_run:
        return
    commit = header["git_commit"]
    new = args.releases / (commit + "-shell-only")
    incoming = args.releases / (".incoming-" + commit + "-" + str(os.getpid()))
    require(not incoming.exists(), "incoming-already-exists")
    switched = False
    created_links: list[Path] = []
    try:
        if new.exists():
            require(verify_stage(new, inherited=True)["git_commit"] == commit,
                    "existing-release-drift")
        else:
            shutil.copytree(args.stage, incoming)
            video = video_files(old.parent)
            if video:
                shutil.copytree(old.parent / VIDEO, incoming / VIDEO)
            (incoming / "VIDEO-INHERITED.txt").write_text(
                "".join(f"{sha}  {name}\n" for name, sha in video.items()))
            verify_stage(incoming, inherited=True)
            os.rename(incoming, new)
        target = new / "bin"
        atomic_link(args.current, target)
        switched = True
        created_links = ensure_command_links(header["binaries"], args.current)
        restart_panel(args, target / "nuraloumi-panel")
        for pid in old_notifications:
            stop_pid(pid)
        notify_activation(args, target / "nuraloumi-notificationd")
        require(pids_for(Path("/usr/local/bin/sl101-sway")) == sway, "sway-changed")
        require(os.readlink(args.current) == str(target), "current-changed")
        print(f"SHELL_DEPLOY_PASS commit={commit} old={old} new={target} "
              "panel=live notificationd=live rollback=available", flush=True)
    except Exception:
        if switched:
            atomic_link(args.current, old)
            for link in created_links:
                if link.is_symlink() and os.readlink(link) == str(args.current / link.name):
                    link.unlink()
            for pid in pids_for(new / "bin/nuraloumi-notificationd"):
                stop_pid(pid)
            try:
                restart_panel(args, old / "nuraloumi-panel")
                notify_activation(args, old / "nuraloumi-notificationd")
                print(f"SHELL_ROLLBACK_COMPLETE current={old}", file=sys.stderr)
            except Exception as rollback_error:
                print(f"SHELL_ROLLBACK_NEEDS_ATTENTION {rollback_error}", file=sys.stderr)
        raise
    finally:
        if incoming.exists():
            shutil.rmtree(incoming)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stage", required=True, type=Path)
    parser.add_argument("--previous", required=True)
    parser.add_argument("--sd-cid", required=True)
    parser.add_argument("--emmc-cid", required=True)
    parser.add_argument("--expected-hostname", required=True)
    parser.add_argument("--expected-kernel", required=True)
    parser.add_argument("--releases", type=Path, default=Path("/opt/nuraloumi/releases"))
    parser.add_argument("--current", type=Path, default=Path("/opt/nuraloumi/current"))
    parser.add_argument("--desktop-user", default="user")
    parser.add_argument("--runtime-dir", default="/run/sl101-desktop-user")
    parser.add_argument("--wayland-display", default="wayland-1")
    parser.add_argument("--panel-hook", default="/usr/local/bin/sl101-sway-panel")
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--dry-run", action="store_true")
    mode.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    activate(args)


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"SHELL_RELEASE_FAIL {error}", file=sys.stderr)
        sys.exit(7)
