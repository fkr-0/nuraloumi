# Sway workspace switcher — host implementation

## Function and boundary

The new shipping binary is **nuraloumi-sway-workspace-switcher**, in
`crates/nuraloumi-shell/src/bin/sway_workspace_switcher.rs`. It has no EGL,
GPU, XWayland, or Sway configuration-writing dependency. The view is a
Wayland layer-shell ARGB8888/Cairo overlay on the current desktop.

On **Super+1** through **Super+9**, Sway starts the binary with the digit.
The first invocation opens a single temporary overlay; subsequent invocations
forward the new selection over a user-only Unix socket. Each selection also
sends a bounded **swaymsg -r -t command 'workspace number N'** request
immediately, without depending on the overlay redraw. The last requested
digit receives a **blue border**. The picker lists currently used numbered
Sway workspaces in the 1–9 shortcut range, including newly created targets.
Super release sends `--release` through the socket and records a bounded,
session-local release stamp for startup races. Release during slow capture is
honored before a surface can map. The socket is removed before the final
post-close screenshot so later key presses cannot target a terminating
overlay. Escape and a 15-second fail-safe also close it.

This integration does **not** replace the general workspace protocol provider
or change the existing desktop-switch action and renderer-switch helper.
Commands have exact argument lists, per-process timeouts and output limits;
digits are strictly validated to 1–9. The switcher does not spawn a shell or
execute arbitrary workspace names.

## Suggested Sway config

The ready-to-include snippet is
`packaging/sway/nuraloumi-workspace-switcher.conf`. Add its contents or include
it from the active Sway configuration, **replacing** the existing Super+1–9
bindings rather than adding duplicate bindsym entries:

```conf
set $ws_picker nuraloumi-sway-workspace-switcher
bindsym --no-repeat $mod+1 exec --no-startup-id $ws_picker 1
bindsym --no-repeat $mod+2 exec --no-startup-id $ws_picker 2
bindsym --no-repeat $mod+3 exec --no-startup-id $ws_picker 3
bindsym --no-repeat $mod+4 exec --no-startup-id $ws_picker 4
bindsym --no-repeat $mod+5 exec --no-startup-id $ws_picker 5
bindsym --no-repeat $mod+6 exec --no-startup-id $ws_picker 6
bindsym --no-repeat $mod+7 exec --no-startup-id $ws_picker 7
bindsym --no-repeat $mod+8 exec --no-startup-id $ws_picker 8
bindsym --no-repeat $mod+9 exec --no-startup-id $ws_picker 9

# Single Super-key release, whether left or right (XKB names).
bindsym --release --no-repeat Super_L exec --no-startup-id $ws_picker --release
bindsym --release --no-repeat Super_R exec --no-startup-id $ws_picker --release
```

The example assumes `set $mod Mod4`, an executable on Sway's PATH, the
standard `SWAYSOCK` and `XDG_RUNTIME_DIR` environment, plus `swaymsg`
and `grim`. The `--release` bindings must be tested on the actual keyboard:
the effective XKB Super keysyms and any other compositor bindings can change
their behavior. In particular, verify that Sway emits the standalone
`Super_L`/`Super_R` release binding **after** a `Mod4+number` binding, not
only after pressing Super alone. The overlay also recognizes Super key-up
when delivered directly to its Wayland surface, but does not assume those
events will always reach it. The 15-second fallback is not a substitute
for the required release gesture. Do not enable two competing release
bindings for the same physical key.

Before applying on a device, validate the config with `sway -C -c PATH` and
ensure the existing desktop can be recovered. **No device config changes or
restarts are performed by this task.**

## Screenshot correctness and limitations

Sway can provide `get_workspaces` metadata for inactive workspaces, but
`grim`/Wayland screencopy can capture only a visible compositor output,
not a hidden workspace. The picker **does not fake** an inactive desktop
screenshot or switch through other desktops to take screenshots.

Instead, it sends the workspace switch **first**, captures the selected
workspace **before** showing the overlay, then captures it again **after**
the overlay is removed. Downsampled ARGB previews (at most 320×220 each) persist in
user-private `$XDG_RUNTIME_DIR/nuraloumi-sway-switcher-*/` state, keyed by
the SWAYSOCK session. Previously seen workspaces show a screenshot; unseen
ones show **No cached screenshot**. If `grim` fails or times out, selection
and workspace switching still function normally. For multiple outputs,
the screenshot is restricted to the focused workspace's named output.
Stale previews may remain until that workspace is seen and captured again.

The preview capture has the same 4-second, 8-MiB and 320×220 limits as
NuraLoumi's existing screenshot provider. There is no continuous screen
capture or idle refresh loop. Capturing before first opening may delay the
overlay on slow hardware; device latency qualification is required. A brief
Super press released before the overlay appears will close it without mapping
a stale popup, as long as the release event reaches the corresponding
Sway binding.

## Host verification

```sh
cargo test --locked -p nuraloumi-shell --bin nuraloumi-sway-workspace-switcher
cargo fmt --all -- --check
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
python3 scripts/package-binaries.py
```

To inspect a deterministic model without contacting Sway:

```sh
cargo run --locked -p nuraloumi-shell --bin nuraloumi-sway-workspace-switcher -- --fixture
```

The repository's ARMv7 musl wrapper includes the new declared binary:

```sh
sh scripts/build-sl101-armv7.sh
scripts/inspect-armv7-elf.sh \
  target/armv7-unknown-linux-musleabihf/release/nuraloumi-sway-workspace-switcher
```

Use a **prepared** tablet-matched sysroot. Do not run `--prepare` without
separate authorization. No release tag, push, device deployment or Sway config
change is implied by host validation.

## Coordinator device acceptance

- Verify Super+1 opens the overlay, selects workspace 1, shows its blue border,
  and switches Sway to workspace 1 immediately.
- Holding Super and pressing 2, 3, 1 updates the blue selection, the workspace
  and the list without creating stacked overlays.
- Releasing either Super key closes the overlay, leaving the last selected
  workspace active. Escape closes it too. Sway bindings must still work with
  the panel running and the physical SL101 slider keyboard.
- A captured workspace shows a real screenshot; an unseen workspace shows the
  placeholder, never an unrelated screenshot. Reopen after visiting the new
  workspace to check its preview cache.
- Check no-SIGILL on Tegra20, memory/RSS, first frame, compositor fallback,
  input on fullscreen/floating surfaces, and absence of leftover socket after
  clean exit. No switcher test should require an actual renderer restart.
