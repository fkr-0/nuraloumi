# SL101 Super+Shift+R panel restart — host qualification (2026-11-25)

## Source and failure path

- NuraLoumi starting HEAD: `a0f428794afaf3ff3e3b1f9e6aecf4a6ff3ad06e` (contains `95bb56bc`).
- Binding source: sibling `sl101_linux/docs/sl101-device-config/home/user/.config/sway/config`: `$mod+Shift+r reload`; `exec_always /usr/local/bin/sl101-sway-panel`.
- Previous `sl101-sway-panel` terminated all `nuraloumi-panel` PIDs for the UID, waited up to 2 seconds, and considered a replacement healthy after merely 200 ms of `kill -0`. An IPC bind failure, late process exit, or wrong session was not excluded. This is a proven **failure-capable code path**, not an observed reproduction on tablet.
- Legacy `sl101-restart-shell` uses broad `pkill -f` and labwc HUP; it is not the Sway binding. Do not install it as the Sway fix.

## Proposed source and install mapping (not installed)

| Source in `sl101_linux` | Destination on SL101 |
| --- | --- |
| `docs/sl101-device-config/usr/local/bin/sl101-sway-panel` | `/usr/local/bin/sl101-sway-panel` (executable, mode 0755) |
| `docs/sl101-device-config/home/user/.config/sway/config` | Desktop user's `~/.config/sway/config` (reviewed merge, never blind overwrite) |

New binding directly invokes the restart hook; `Super+Shift+C` remains Sway reload. `exec_always` still ensures a panel on explicit reload. The script serializes with an exclusive runtime flock, checks panel executable and Wayland socket *before* termination, scopes old PIDs to the same UID, `XDG_RUNTIME_DIR`, and `WAYLAND_DISPLAY`, waits boundedly for exit, then checks new IPC socket readiness within 3 seconds. Logs to `${XDG_STATE_HOME:-$HOME/.local/state}/nuraloumi-panel.log` and stderr (Sway exec log). It never removes another listener's IPC path or sends process-group signals.

For rollback recovery during a release, provision a separately qualified known-good executable and export `SL101_SWAY_PANEL_FALLBACK_BIN=/path/to/known-good-panel` in the Sway session environment *before* the canary. Failed primary startup attempts this fallback if the IPC path is free. Without a pinned fallback, recovery after a real panel crash is not guaranteed; retain a VT/SSH manual recovery plan. IPC socket existence is a readiness signal, not proof of successfully rendered layer-shell pixels.

## Host checks

From `sl101_linux`:

    python3 -m unittest discover -s tests -p test_sl101_sway_panel_restart.py -v
    sh -n docs/sl101-device-config/usr/local/bin/sl101-sway-panel
    git diff --check

Fixture creates a temporary fake Wayland Unix socket and a Python process with a unique Linux process name, matching real `/proc` session environment. No live compositor/panel is touched. Cases cover fresh launch, repeated restart, crashed replacement, configured fallback, invalid fallback, unavailable Wayland, and invalid primary path.

## On-tablet acceptance: separately owner-approved supervised canary only

1. Read-only inspect the *installed* Sway config, `swaymsg -t get_config`, `command -v sl101-sway-panel`, `readlink -f /opt/nuraloumi/current`, `pgrep -af nuraloumi-panel`; compare to these sources. Record session values from the real Sway/panel process: `WAYLAND_DISPLAY`, `XDG_RUNTIME_DIR`, `PATH`, `XDG_STATE_HOME` and IPC socket owner; record shell and panel log paths.
2. Preserve current Sway config and hook verbatim, hashes, mode and a known-good panel executable. Arrange explicit rollback command from a separate VT/SSH session; confirm user physical access before any hotkey test.
3. Install the reviewed script and merge binding only with explicit owner approval. Validate Sway config without mutating the running compositor; never replace the complete user config blindly.
4. With owner physically supervising, press Super+Shift+R once; confirm original PID exits, exactly one replacement remains, IPC listener responds, top panel is visibly drawn and touch/keyboard interaction works. Inspect bounded log and process/session environment. Repeat three rapid presses and a no-panel case.
5. Simulate a launch failure only under a separately approved canary with pinned fallback and safe rollback. Verify fallback identity, visible pixels, IPC response and zero unexpected child panels/toast orphans. Roll back immediately on blank desktop.

**Device status: BLOCKED / NOT TESTED.** No SSH session, live hotkey, service, Sway reload, package install or device mutation performed by this task. No on-device PASS is claimed.
