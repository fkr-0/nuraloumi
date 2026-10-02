# Menu integration R2

This pass turns the Wave 1 shell into a usable control-menu layer while keeping
rendering, system providers, and destructive actions separated.

## Built-in families

| CLI family | Purpose | Live backend |
| --- | --- | --- |
| `wifi` | radio, connection status, rescan, connect | NetworkProvider / nmcli |
| `bluetooth` / `bt` | adapter power, devices, connect/disconnect | BluetoothProvider / bluetoothctl |
| `display` | brightness and active-window fullscreen | brightness provider + live foreign-toplevel fullscreen control |
| `audio` / `speaker` | speaker volume and mute | AudioProvider / wpctl |
| `power` | battery, suspend, restart, power off | SessionProvider; confirmation required and dry-run by default |
| `tasks` | conceptual task snapshot | inspection-only fixture/model contract |
| `windows` | compositor window list | wlr foreign-toplevel list/focus/fullscreen/close; ext-list degrades to read-only |
| `system` | quick status/control-center links and power rows | opens the families above |

Aliases such as `brightness`, `fullscreen`, `volume`, `task-viewer`, and
`window-list` resolve to the corresponding family. Checkable rows preserve an
explicit checked state where the provider/model can establish one.

## Live renderer and input

`nuraloumi-menu --live` now uses the native Wayland layer-shell backend and
Cairo software renderer directly:

    semantic menu -> Cairo ARGB32 -> wl_shm -> layer-shell surface
                          ^
                          |
               keyboard / pointer / touch

The path does not require EGL, GLES, XWayland, or a GUI toolkit. Pointer/touch
hit testing comes from the Cairo scene and is routed through the same semantic
state machine as physical keyboard input.

Without `--providers`, live mode probes current bounded system providers and
refreshes state after provider actions. Supplying a provider fixture keeps the
menu deterministic and disables real provider mutation.

## Action safety

Wi-Fi, Bluetooth, brightness, speaker volume, and mute are executed only by
their provider adapters. No shell command interpolation is used.

Power rows use the existing two-step confirmation state machine. Even after
confirmation, live power operations are provider dry-runs unless explicitly
enabled:

    nuraloumi-menu --live --family power --enable-power-actions

That flag permits the SessionProvider to execute confirmed suspend/reboot/
poweroff. It has no effect outside live mode.

Task rows emit only `task.inspect`; there is intentionally no process-kill
action. In normal live mode, window focus/fullscreen/close are translated by the
shell into typed `nuraloumi-wayland` foreign-toplevel requests. Every listed
window with control capability opens a per-window submenu containing bounded
Focus, Fullscreen and Close… actions, so control does not depend on the
compositor currently reporting an activated toplevel. The focused window also
keeps quick Fullscreen/Close rows. Window close always requires confirmation
and the row is not removed until the compositor emits its `closed` event.
Supplying `--providers` keeps window actions fixture-only and never mutates
real compositor state.

## Fixture schema additions

Provider snapshots may optionally include:

- `bluetooth`
- `wifi_networks[]`
- `bluetooth_devices[]`
- `tasks[]`
- `windows[]`

Older fixtures containing only network/audio/battery/clock/brightness still
deserialize: Bluetooth becomes unavailable and the structured lists are empty.

## Review commands

    cargo run -p nuraloumi-shell --bin nuraloumi-menu -- --family wifi --providers examples/menu-fixtures/providers.json
    cargo run -p nuraloumi-shell --bin nuraloumi-menu -- --family bt --providers examples/menu-fixtures/providers.json
    cargo run -p nuraloumi-shell --bin nuraloumi-menu -- --family display --providers examples/menu-fixtures/providers.json
    cargo run -p nuraloumi-shell --bin nuraloumi-menu -- --family power --input down,down,enter,enter
    cargo run -p nuraloumi-shell --bin nuraloumi-menu -- --family tasks --providers examples/menu-fixtures/providers.json
    cargo run -p nuraloumi-shell --bin nuraloumi-menu -- --family windows --providers examples/menu-fixtures/providers.json

Inspect compositor window-control capability without opening a menu:

    cargo run -p nuraloumi-shell --bin nuraloumi-menu -- --probe-toplevels

On a compositor exposing `zwlr_layer_shell_v1`:

    cargo run -p nuraloumi-shell --bin nuraloumi-menu -- --live --family system

The preferred control path is `zwlr_foreign_toplevel_manager_v1`. Activation
is advertised only when a usable `wl_seat` is also available, because the wlr
activate request requires a seat. Fullscreen and close remain independently
available when the compositor exposes them. In live mode, `--input` is applied
only after the compositor toplevel snapshot is loaded and is dispatched through
the same semantic/confirmation/control path as keyboard or touch input. The live
menu can render without enabling system mutation. Real power actions require the
separate flag described above.

## Integration state

The provider-to-menu wiring now preserves the complete live Wi-Fi scan list in
the menu snapshot rather than reconstructing only the active SSID. Brightness
display and adjustment also use the same selected writable backlight device, so
the value shown to the user corresponds to the device the action will change.

Window control now follows the bounded compositor adapter: wlr foreign-toplevel
management provides list/state/focus/fullscreen/close, while ext foreign-
toplevel-list provides a read-only fallback. Opaque `tl:<generation>` IDs are
used for actions; title/app-id are never control identity. Task inspection
remains a separate conceptual/process-provider concern and is intentionally not
coupled to Wayland window identity.
