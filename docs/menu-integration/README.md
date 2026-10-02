# Menu integration R2

This pass turns the Wave 1 shell into a usable control-menu layer while keeping
rendering, system providers, and destructive actions separated.

## Built-in families

| CLI family | Purpose | Live backend |
| --- | --- | --- |
| `wifi` | radio, connection status, rescan, connect | NetworkProvider / nmcli |
| `bluetooth` / `bt` | adapter power, devices, connect/disconnect | BluetoothProvider / bluetoothctl |
| `display` | brightness and active-window fullscreen | brightness is live; fullscreen remains semantic until a toplevel-control source is wired |
| `audio` / `speaker` | speaker volume and mute | AudioProvider / wpctl |
| `power` | battery, suspend, restart, power off | SessionProvider; confirmation required and dry-run by default |
| `tasks` | conceptual task snapshot | inspection-only fixture/model contract |
| `windows` | conceptual window snapshot | focus/fullscreen/close semantics only |
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
action. Window focus/fullscreen/close remain semantic until a bounded
foreign-toplevel/compositor integration exists. Window close still requires
confirmation.

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

On a compositor exposing `zwlr_layer_shell_v1`:

    cargo run -p nuraloumi-shell --bin nuraloumi-menu -- --live --family system

The live menu can render without enabling system mutation. Real power actions
require the separate flag described above.

## Review findings / next wiring

Two integration details should stay explicit while the runtime convergence work
lands:

- live Wi-Fi should copy the complete `NetworkSnapshot.networks` scan list into
  the menu snapshot rather than reconstructing a one-row list from only the
  active SSID;
- the brightness value displayed by the menu should come from the same writable
  backlight device that the adjustment action will change.

Fullscreen, window focus/close, and task inspection remain semantic contracts
until a bounded compositor/process adapter exists. This is preferable to adding
ad-hoc shell commands that bypass the provider/runtime boundary.
