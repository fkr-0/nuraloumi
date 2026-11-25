# SL101 NuraLoumi notification usage and qualification — 2026-11-25

- OCP: `NURALOUMI-20261125-NURALOUMI-NOTIFICATIONS-E2E-20261125-329BEF`.
- Starting HEAD: `056a591532037b47e0564f686c717d7adc06469b`; `95bb56bc20033cb9db6b2d1570af18598d86dcfe` is an ancestor. Concurrent writers advance HEAD.
- No tablet notifications sent, no SL101 desktop modified. Device delivery, toast pixels and history **UNVERIFIED**.

## Two independent paths

| Interface | Destination | Effect |
| --- | --- | --- |
| `nuraloumi-ctl notify` | panel's Unix IPC socket | one-shot Wayland toast child, **not** D-Bus/history |
| `notify-send` | desktop user's D-Bus session | active `org.freedesktop.Notifications` daemon and its history, **not** automatically a toast |

## IPC: exact flags and local/SSH commands

The parser supports `notify --summary TEXT [--body TEXT] [--app TEXT]`; only `--summary` is mandatory and it must be nonblank. Global flags: `--json`, `--socket PATH`. No `notify --timeout`, `--urgency`, `--icon`, or `--id`. Duplicate/unknown flags are errors; NUL/CR/LF are invalid in required fields and maximum IPC request is 4096 bytes.

```sh
# As the local Wayland-panel user:
nuraloumi-ctl --json ping
nuraloumi-ctl --json notify --summary 'Hello flo' --body 'Local shell' --app Terminal

# Read-only SSH preflight through trusted SSH alias, as desktop session user:
ssh SL101_ALIAS 'ls -ld /run/sl101-desktop-user; ls -l /run/sl101-desktop-user/nuraloumi.sock'
ssh SL101_ALIAS 'XDG_RUNTIME_DIR=/run/sl101-desktop-user nuraloumi-ctl --json ping'

# ONLY with explicit opt-in to a visible tablet canary:
ssh SL101_ALIAS 'nuraloumi-ctl --json --socket /run/sl101-desktop-user/nuraloumi.sock notify --summary "SSH canary" --body "One-shot toast" --app Qualification'
```

`/run/sl101-desktop-user` is a known session-directory *candidate*, not freshly verified in this task. Confirm the actual panel process environment before use. The CLI defaults to `$XDG_RUNTIME_DIR/nuraloumi.sock` if the runtime directory is nonempty and absolute; otherwise `/tmp/nuraloumi-$UID.sock`. SSH logins can inherit an unrelated runtime directory: use `--socket` to target the confirmed one. Socket permissions require the owning desktop user; do not escalate privileges to force delivery.

With `--json` and a successful spawn, expect a response containing `"ok":true` and `"result":{"accepted":true,"presentation":"one_shot_started_unverified"}` (plus request `id`). Without `--json`, stdout prints the pretty `result` only. Exit 0 means child **spawned**, not visible pixels. The panel owns the IPC listener and launches only executable sibling `nuraloumi-toast`, with at most one child active. Open notification center/control center suppresses the toast (`accepted:false, reason:notification_center_open`); unknown notification-center state fails `center_state_unknown`. Other failures: `toast_unavailable`, `toast_busy`, `toast_spawn_failed`. An unavailable/denied socket yields `connection_failed` on stderr, exit 2; other failures exit 1.

Safe no-UI/parser probe deliberately targets a nonexistent socket (never sends anything):

```sh
nuraloumi-ctl --socket /tmp/nuraloumi-not-present-329bef.sock notify --summary 'No UI probe'
```

## Freedesktop D-Bus: local and SSH

`nuraloumi-notificationd` is a distinct session-bus owner. It acquires `org.freedesktop.Notifications` with `DoNotQueue`; a competing Dunst/Mako owner must **not** be replaced, and a second NuraLoumi daemon exits unsuccessfully. It provides standard Notify/CloseNotification/GetCapabilities/GetServerInformation and action/closed signals; NuraLoumi adds GetHistory, DismissHistory, InvokeAction and NotificationAdded. In-memory history is bounded to 64 records and **does not survive daemon restart**. The center reads the native history if server identity says NuraLoumi, otherwise can use Dunst history via `dunstctl`; absent backend gives unavailable state.

```sh
# Only in the correct desktop-user session environment:
printf 'runtime=%s\nbus=%s\n' "$XDG_RUNTIME_DIR" "$DBUS_SESSION_BUS_ADDRESS"
busctl --user status org.freedesktop.Notifications
busctl --user call org.freedesktop.Notifications /org/freedesktop/Notifications org.freedesktop.Notifications GetServerInformation

# Explicitly approved visible canary only:
notify-send --app-name=NuraLoumi --expire-time=5000 'D-Bus canary' 'Check center history'
# NuraLoumi-daemon-only history extension, not generic Dunst:
busctl --user call org.freedesktop.Notifications /org/freedesktop/Notifications org.freedesktop.Notifications GetHistory
```

For SSH, use the **same UID, `DBUS_SESSION_BUS_ADDRESS` and actual private bus socket** as the running desktop. The candidate `unix:path=/run/sl101-desktop-user/bus` is **unverified**: inspect process environment and socket first. The panel's `nuraloumi.sock` is never a D-Bus socket. `dbus-run-session` starts a private test bus, *not* the tablet's current session. An installed `share/dbus-1/services/org.freedesktop.Notifications.service` descriptor can activate the daemon if executable and visible to the bus; alternatively one user-session service may manage it. Merely building `nuraloumi-notificationd` does not establish bus ownership. Startup order for IPC: panel -> bound IPC -> sibling toast; for D-Bus: established user bus -> single owner/activation -> sender -> provider/center refresh. D-Bus daemon has **no integrated on-screen toast listener** in this qualified path.

**No-UI fallback:** if Wayland/panel/toast is unavailable, IPC fails rather than quietly storing history; if there is no freedesktop owner, `notify-send` cannot prove reception. These transports do not automatically fall back to each other. If notification center is open, IPC intentionally suppresses the toast. Do not claim on-screen presentation from a successful D-Bus call alone.

## Host test evidence (exit codes)

| Command | Exit | Outcome |
| --- | ---: | --- |
| `cargo test -p nuraloumi-shell --bin nuraloumi-notificationd --locked` | 0 | 2 passed |
| `cargo test -p nuraloumi-shell --lib notification --locked` | 0 | 1 passed, 128 filtered |
| `cargo test -p nuraloumi-shell --bin nuraloumi-toast --locked` | 0 | 11 passed |
| `cargo test -p nuraloumi-shell --bin nuraloumi-ctl --locked` | 0 | 10 passed |
| `sh tests/notifications/private-session.sh` | 0 | Isolated D-Bus Notify/GetHistory PASS, duplicate owner rejected, CloseNotification preserved inactive history; nonfatal diagnostic on shutdown |
| `cargo run --quiet --locked -p nuraloumi-shell --bin nuraloumi-ctl -- notify --summary 'Host-only parser probe' --unknown value` | 1 | `invalid_arguments`, unknown option |
| `cargo run --quiet --locked -p nuraloumi-shell --bin nuraloumi-ctl -- --socket /tmp/nuraloumi-not-present-329bef.sock notify --summary 'Host-only parser probe' --body 'No delivery intended' --app Qualification` | 2 | `connection_failed`, ENOENT |

## Single remaining device qualification

With owner opt-in, first read-only verify the desktop UID, runtime/socket paths, NuraLoumi panel and sibling toast binary, desktop bus address and current D-Bus owner. Then deliver one visibly labeled IPC canary and one D-Bus canary *in that exact session*. Observe via WayVNC/screenshot or owner's eyes; inspect notification center history, which should contain the **D-Bus** canary, not automatically the IPC canary. Record commands, exit codes, owner, screenshots and timings. Until then: tablet toast **UNVERIFIED**, tablet D-Bus delivery **UNVERIFIED**, tablet notification-center history **UNVERIFIED**.
