# SL101 top-right quick-status sheet (2026-11-25)

## Design and behavior

The panel's rightmost power button opens a **generic quick-status sheet**.
On a top-edge panel it is a fixed-width Wayland layer surface anchored to
**Top + Right**, immediately below the compositor-reserved panel strip. The
launcher and other transient menus keep their existing positioning. Switching
between Power and another menu recreates the anchored surface; it is never
silently repainted at the stale top-left anchor.

The screen uses NuraLoumi's Cairo/wl_shm software rendering, no icon font or
GPU requirement. Four touch-sized vector-icon tiles form a two-column grid;
an enabled toggle is visibly **lit by a bottom accent bar**. The grid data
structure is extendable. State is read from the corresponding provider where
available; an unavailable Wi-Fi/Bluetooth/audio source is not displayed as
an active control. The Wi-Fi tile uses the radio-enabled flag, not the
connected-to-SSID flag.

Below the tiles are two **segmented slider tracks**, brightness and volume.
Volume supports six 20%-spaced choices (0%, 20%, ..., 100%); brightness
supports six choices (10%, 25%, 40%, 60%, 80%, 100%). The brightness floor
avoids accidentally blacking out an unqualified tablet. Both tracks use the
existing typed SetValue action. Chosen values go to BacklightProvider and
AudioProvider respectively; no system commands run in the renderer. A future
continuous drag gesture can refine this deliberate touch-sized first pass.

The bottom action list is, in order: Sleep, Restart, Shut down, Hibernate.
Sleep, restart and shutdown retain the existing two-activation confirmation
model, and a pending choice is highlighted with a visible *Tap again to
confirm* warning. Hibernate is **disabled**, not an executable placeholder:
the SL101 has no qualified hibernation image/swap/resume path. Sleep remains
dry-run unless the separately qualified unsafe-suspend capability is enabled;
reboot and poweroff retain their existing explicit action policy.

If the output has less than 570 logical px of usable popup height, the sheet
falls back to the ordinary scrollable menu rather than clipping destructive
actions. On the expected SL101 landscape/portrait display the compact sheet
is 574 px tall, with touch regions at least 48 logical px wide.

## Display idle mode

The **Always on** tile defaults to lit. Turning it off starts one
panel-owned `swayidle` child in the active Sway user session:

```text
swayidle -w timeout 600 'swaymsg "output * power off"' resume 'swaymsg "output * power on"'
```

Ten minutes of compositor-observed inactivity turn outputs off; activity
turns them on again. This is **display blanking only** — never CPU suspend,
hibernate, `rtcwake`, direct sysfs state writes or ignore-inhibitor loginctl.
The documented upstream swayidle command contract is followed. When the
Always on tile is re-enabled or the panel exits, it sends SIGTERM **only to
its own child**; swayidle runs pending resume handlers on graceful exit. An
800 ms bounded stop fallback prevents an orphaned child. A failed launch or
early child exit leaves/reverts the tile to Always on, with an error log.
An unexpected exit or forced stop also attempts one bounded `swaymsg output *
power on` restoration; no repeated recovery loop is started.

This mode is currently **session-scoped**, not persisted or autostarted by a
system service. Restarting the panel defaults to Always on. It requires
`swayidle` and `swaymsg` on the active device and compatible Sway output
power control. It cannot be claimed functional until both programs and
Wayland idle resume behavior are observed on the real SL101. A read-only SSH
check in this implementation session was denied, so no device-side timer,
power or wake behavior was exercised.
The standalone `nuraloumi-menu` executable does not own this timer; invoking
its Always on action reports an explicit unsupported-action error.

## Qualification gates

1. Host: deterministic pure scene tests for tile layout, bottom lights,
   brightness/volume ordering, hit targets, disabled/confirmation states;
   right-edge layer-shell anchor; typed SetValue dispatch; isolated idle
   command contract; workspace test and Clippy gates.
2. On SL101, physically witness top-right placement below the panel and no
   clipping/overlap with the status strip on actual output scale/orientation.
3. Qualify all toggles with real readback. Particularly check Wi-Fi radio
   shutdown does not remove the **only** recovery channel during a test.
4. Qualify brightness at safe visible levels first; 0% and the hardware
   minimum remain unavailable in this UI until separately proven. Qualify
   volume changes against the default sink.
5. With recovery physically present, select 10 min, observe output blanking,
   press a key/touch and verify display, compositor, input and Wi-Fi all
   return. Re-enable Always on and verify child termination and output on.
6. Before releasing, test startup/no-helper fallback and unexpected helper
   exit, repeated opening/dismissal, touched/keyboard confirmation, and
   native ARMv7/musl/no-NEON source-matched package. Do not conflate screen
   off/on with full suspend/hibernate qualification.

No live SL101 state, installed package, service configuration or GitHub CI
was changed while preparing this source revision.
