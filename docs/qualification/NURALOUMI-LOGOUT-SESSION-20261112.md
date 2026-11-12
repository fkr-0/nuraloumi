# NuraLoumi: logout to console (VT1) qualification

Date: 2026-11-12. Scope: SL101 Debian (.100) and Alpine/postmarketOS (.106).
This is a read-only preflight and acceptance contract, **not** device qualification.

## User-visible contract

"Log out to console (VT1)" confirms before action; closes the graphical desktop session, including its Sway compositor, NuraLoumi panel, and applications belonging to that desktop. It then exposes the usable text console on VT1. Do **not** terminate the Linux user account's other sessions (independent SSH or terminals on other VTs). Do **not** run `loginctl terminate-user`, `killall sway`, `pkill -u`, an unchecked process-tree kill, or a privileged arbitrary `chvt` wrapper.

Identify the graphical session on each device first; for a dedicated logind session with a proven session ID, a session-specific termination may be valid. Where Sway was launched from a VT1 login shell, a graceful `swaymsg exit` scoped to the right `SWAYSOCK` is generally preferable. Whether logout returns to a shell or login prompt depends on the launcher/getty design. Do not assume graphical session = SSH session or that a user service owns a logind session.

## Read-only commands to record on each device

```sh
uname -a
cat /sys/class/tty/tty0/active
loginctl list-sessions --no-legend
loginctl show-session "$XDG_SESSION_ID" -p Name -p Type -p Class -p Seat -p TTY -p VTNr -p Leader -p State  # only if set
ps -eo pid,ppid,sid,tty,comm,args | grep -E '[s]way|[n]uraloumi|[g]etty|[l]ogin'
command -v loginctl; command -v chvt
```

When inspecting via SSH, `$XDG_SESSION_ID` refers to the SSH connection, **not** automatically the graphical session. Inspect IDs from `list-sessions`, and inspect the compositor's process environment/session ownership without exposing secrets. Confirm VT1 is a real usable getty or shell before switching.

## Current network preflight (2026-11-12, host)

- `ssh -o BatchMode=yes -o ConnectTimeout=4 root@192.168.23.100`: `No route to host`.
- `ssh -o BatchMode=yes -o ConnectTimeout=4 root@192.168.23.106`: `Permission denied (publickey,password,keyboard-interactive)`.
- Therefore **neither device topology nor installed build/version is verified**. These addresses and root user originate from repository scripts, not a confirmed current inventory. Do not deploy or terminate a session based solely on this note.

## Acceptance gates

1. The power menu action is visible; confirmation rejects accidental touch, and Cancel has zero system effects.
2. Dry-run/fixture verifies exact command and capability; execution targets only the proven graphical session (never caller's SSH session or all sessions of a UID).
3. Logout request is not blocked by waiting for its own Wayland control surface to exit; post-session cleanup is handled by session supervisor or VT1's getty.
4. SSH and unrelated system services stay alive; VT1 presents the expected shell/login prompt; no stranded blank VT.
5. An actual device logout requires an operator with physical recovery and a working alternate control path. Record kernel/DTB, session graph, service manager, exact installed NuraLoumi binary/version, and successful VT1 transition.
6. Test both Debian/systemd and Alpine/OpenRC+seatd/elogind paths independently. If session ownership cannot be proved, fail closed and display a diagnostic; no generic `terminate-session` fallback.

Related: docs/VT-SWITCHING.md, docs/qualification/SL101-BOOT-SUSPEND-20261002.md, AGENTS.md.
