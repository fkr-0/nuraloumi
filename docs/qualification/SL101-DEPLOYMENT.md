# SL101 copy-only deployment and rollback

## Baseline

Current Nura evidence establishes a musl ARMv7 userspace and a working
labwc/wlroots pixman recovery desktop. GPU-backed labwc is not a prerequisite
for NuraLoumi and must not be enabled by this qualification procedure.

The tablet's known recovery model is:

- keep SSH or a physical terminal available;
- keep the existing pixman labwc baseline independently startable;
- copy NuraLoumi into a disposable staging directory;
- start binaries manually for qualification;
- never install into /usr and never enable a NuraLoumi service in Wave 1.

## Stage

On the build host:

    cargo xtask build-release --target armv7-unknown-linux-musleabihf
    cargo xtask qualify-armv7
    cargo xtask package --target armv7-unknown-linux-musleabihf

Copy the produced tar or stage directory to the tablet. On the tablet, use a
versioned path under the user's home directory, for example:

    mkdir -p "$HOME/.local/opt/nuraloumi-staging"
    cd "$HOME/.local/opt/nuraloumi-staging"
    tar -xf /path/to/nuraloumi-0.1.0-armv7-unknown-linux-musleabihf.tar

Before starting anything, record the baseline:

    uname -a
    cat /etc/os-release
    pgrep -a labwc || true
    ps -eo pid,rss,%cpu,etime,comm,args | grep -E 'labwc|nuraloumi' || true
    free -m
    cat /proc/meminfo | sed -n '1,12p'

Confirm the existing recovery desktop independently before the test. If the
installation provides the known sl101-desktop OpenRC service, only inspect it
at first:

    rc-service sl101-desktop status || true

## Manual start

Run from the terminal or SSH session and keep the PID visible. Prefer the
fixture/safe mode until provider actions are separately qualified.

    cd "$HOME/.local/opt/nuraloumi-staging/<unpacked-directory>"
    WLR_RENDERER=pixman ./bin/nuraloumi-panel [fixture/safe options] &
    panel_pid=$!
    ps -o pid,rss,%cpu,etime,args -p "$panel_pid"

Do not modify WAYLAND_DISPLAY globally and do not stop the user's compositor.
NuraLoumi's runtime path is Cairo/software buffer to wl_shm; WLR_RENDERER is
shown only as a reminder that the compositor recovery baseline is pixman.

## Rollback

Primary rollback is process-only:

    pkill -TERM -x nuraloumi-menu 2>/dev/null || true
    pkill -TERM -x nuraloumi-panel 2>/dev/null || true
    sleep 1
    pkill -KILL -x nuraloumi-menu 2>/dev/null || true
    pkill -KILL -x nuraloumi-panel 2>/dev/null || true

Verify labwc survived:

    pgrep -a labwc
    ps -eo pid,rss,%cpu,etime,comm,args | grep -E 'labwc|nuraloumi' || true

If the pre-existing baseline desktop itself needs recovery and the tablet uses
the known OpenRC service, the operator may restart that existing service:

    rc-service sl101-desktop restart

That command is recovery of the established baseline, not part of NuraLoumi
installation. A reboot must return to the pre-existing baseline because no boot
service or /usr file was changed.

After evidence is copied off-device, the staging directory may be removed. No
filesystem surgery should be required.

## Resource evidence

Record idle and menu-open snapshots:

    ps -C nuraloumi-panel,nuraloumi-menu -o pid,rss,%cpu,etime,args
    grep -E 'VmRSS|VmHWM|Threads' /proc/<pid>/status
    cat /proc/<pid>/stat
    cat /proc/meminfo | grep -E 'MemAvailable|MemFree|Buffers|Cached'

For startup timing, capture a monotonic timestamp immediately before exec and
the first runtime readiness/frame log emitted by the final shell binary. Keep
the raw timestamps in the evidence record.

For 100 open/close cycles, use the final shell's documented non-destructive
open/close command or input harness after it lands. Record pre/post RSS and
labwc PID; do not substitute 100 process crashes/restarts for menu lifecycle.

For suspend/resume, **do not** use unattended `rtcwake -m mem`, direct
`/sys/power/state` writes, or inhibitor-bypass flags. The safe progression is:

1. record the exact kernel, boot-image/DTB identity, labwc/panel PID/RSS, network
   state and available physical recovery path;
2. run `loginctl --dry-run suspend`;
3. with a human present, constrain elogind to shallow `freeze` and request
   suspend through `loginctl` so inhibitors remain active;
4. after physical wake, verify display, touch, slider keyboard, labwc/panel,
   provider refresh, menu lifecycle and Wi-Fi/network recovery;
5. only after shallow recovery passes may a separately authorized,
   human-present `mem/deep` experiment be considered.

If SSH is the only recovery path, stop before step 3. RTC wake alone is not a
resume PASS. See `SL101-BOOT-SUSPEND-20261002.md` for the failed deep-suspend
incident and its kernel/DTB boundary.
