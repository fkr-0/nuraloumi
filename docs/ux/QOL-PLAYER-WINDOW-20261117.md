# Player full-window lane 10 — bounded implementation and backend gate

Canonical task: `NURALOUMI-20261117-QOL-R1-10-PLAYER-EXPANDED-WINDOW-20261117-AF7E58`.

## Defect audit

- `crates/nuraloumi-shell/src/bin/player.rs::run` invokes
  `WaylandBackend::create_menu`, a layer-shell role with fixed 690 px height,
  a dismissal backdrop and no window-manager decorations. Increasing this size
  cannot produce a Sway toplevel, keyboard focus/close/reopen lifecycle or
  independent window placement.
- `crates/nuraloumi-wayland/src/backend.rs` binds
  `zwlr_layer_shell_v1`; its exported backend has no xdg_wm_base binding or
  `create_toplevel` and no xdg configure/ack/window close lifecycle.
- `crates/nuraloumi-shell/src/player_engine.rs` already owns the single
  shared playlist and `Command::{Status,Seek,SeekRelative,Toggle,Stop,...}`;
  `playerd` is the only permitted media session.

## Implemented without crossing lane ownership

- New `src/player_window.rs`: pure high-resolution seek mapping, reversible
  fraction calculation, shortcut translation, explicit capability gate and
  three isolated regression tests. Each UI control must occupy at least
  48 logical px; moving the slider maps a finite local x coordinate into the
  daemon's absolute millisecond seek value, clamped to the known duration.
- New `src/bin/player_window.rs`: fail-closed skeleton that explicitly
  refuses a false layer-shell managed window. No daemon is spawned or stopped
  and thus opening/closing this stub cannot interrupt compact playback.
- This is **not** a working Sway window, nor is video handoff implemented.

## Required lane 02 + lane 15 handoff

1. Lane 02: add `xdg_wm_base` and `xdg_surface/xdg_toplevel` bind,
   configure/ack, close, destroy, wl_shm frame and seat focus/keyboard/touch
   routing to `nuraloumi-wayland`, with compositor-proven smoke evidence.
2. Lane 15: register `[[bin]] name = "nuraloumi-player-window"` in shell
   Cargo.toml, and expose the pure model in shell lib (or keep it local to
   the binary). Replace the skeleton capability check with a real xdg
   window creation; draw with existing Cairo/wl_shm, use the existing
   `XDG_RUNTIME_DIR/nuraloumi-player.sock` request/reply protocol and its
   bounded read/write deadline, never start another playback pipeline.
3. Lane 09: expand launches this exact binary via the shell's approved bounded
   launcher API. It must never send Stop on compact-surface dismissal.
4. All lanes: preserve offline/error UX when playerd socket is absent, avoid
   duplicate full windows (window focus or singleton activation), and confirm
   on Sway that close/reopen retains playback, seek and playlist state.

## Current gates

Host-isolated pure model tests are runnable using:
`rustc --edition=2021 --test crates/nuraloumi-shell/src/player_window.rs -o /tmp/nuraloumi-player-window-tests && /tmp/nuraloumi-player-window-tests`.
Cargo workspace checks do not currently compile the new unregistered binary.
Never claim on-device pixels, native toplevel or media playback from these tests.
