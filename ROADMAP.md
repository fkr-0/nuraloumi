# NuraLoumi roadmap

## Mission

Ship a coherent set of small, operating binaries for the SL101/Nura rapidly, while preserving a software-rendered recovery path and keeping every subsystem independently testable.

## Release 0.1 — operating vertical slice

### M0 — contracts and repository skeleton
- [x] Register project and Git repository.
- [x] Establish six non-overlapping crates.
- [x] Freeze initial cross-crate contracts.
- [x] Establish independent-review agent workflow.
- [ ] All crates build on the host with placeholder implementations.

### M1 — semantic core
Deliver `nuraloumi-core`.
- MenuModel/MenuItem/MenuAction with stable IDs.
- Row kinds: action, submenu, checkable, text/status/section/separator.
- Selection/navigation invariants.
- Search/filter and quick-select.
- confirmation flow for destructive actions.
- dark/light design tokens with 48 px touch minimum.
- motion primitives with reduced-motion mode.
- pure deterministic tests and serde fixtures.

Exit: core crate has no GUI/window-system dependency.

### M2 — software renderer
Deliver `nuraloumi-render-cairo`.
- renderer-neutral scene/layout tree.
- Cairo image-surface renderer.
- rounded panels/cards, borders, separators, selected rows.
- icon placeholder/glyph path, title/subtitle/shortcut geometry.
- deterministic hit regions.
- bounded shadow/gradient effects designed for Tegra20.
- PNG fixture renderer for review and CI.
- no X11/XCB dependency.

Exit: a menu fixture renders to a deterministic PNG from the command line.

### M3 — native Wayland backend
Deliver `nuraloumi-wayland`.
- connection/registry/output/seat discovery.
- wl_shm double-buffered surfaces.
- layer-shell anchored panel and popup roles.
- keyboard, pointer and touch events mapped to a small platform-neutral event enum.
- scale/transform awareness.
- configure/resize/damage/commit lifecycle.
- clean shutdown and reconnectable error model.
- no EGL/GLES requirement.

Exit: an example binary opens, paints, receives input, and exits under a normal Wayland compositor.

### M4 — operating shell
Deliver `nuraloumi-shell`.
Initial executable surfaces:
- `nuraloumi-panel`: small top strip with Apps / Network / Audio / Battery / Clock.
- `nuraloumi-menu`: generic menu runner driven by JSON/TOML fixtures.
- launcher sheet with search.
- system sheet with brightness, volume, suspend/restart/power confirmation rows.
- keyboard + touch parity.
- one-menu-at-a-time lifecycle.
- reduced-motion mode.

Exit: both binaries run with fixture providers before hardware providers are available.

### M5 — providers
Deliver `nuraloumi-providers`.
- battery/power from sysfs.
- brightness from sysfs with explicit writable capability.
- network snapshot/action adapter around NetworkManager/nmcli or DBus.
- audio snapshot/action adapter with a minimal backend boundary.
- clock/session providers.
- bounded command runner, timeouts, error/stale states.
- fixture backend for all providers.

Exit: `nuraloumi-probe` emits a stable JSON snapshot and does not mutate state unless explicitly given an action command.

### M6 — build and target qualification
Deliver `nuraloumi-xtask`.
- dependency/features audit.
- release build and size report.
- ARMv7 target configuration and no-NEON audit helpers.
- package staging directory.
- host nested-Wayland smoke script.
- SL101 deployment manifest, rollback notes and resource probe integration.
- reproducible fixture screenshots.

Exit: one command produces host release binaries plus an SL101 qualification bundle.

## Release 0.2 — target hardening
- Four-corner touch calibration and rotated output testing.
- On-device RSS/CPU/startup latency measurements.
- on-screen keyboard integration.
- notification/status toast surface.
- application discovery through desktop entries.
- icon theme lookup with bounded caches.
- suspend/resume recovery.
- NetworkManager and audio backend hardening.
- packaging for the Nura userspace.

## Release 0.3 — polished daily shell
- favorite applications and recent items.
- workspace/window adapter when compositor IPC is available.
- lock/session affordances.
- accessibility pass: contrast, text scaling, focus visibility.
- configurable top/bottom/side panel.
- optional GPU-backed compositor operation with identical software fallback.
- stable configuration schema and migration.

## Non-goals through 0.3
- replacing labwc/wlroots;
- browser engine;
- full desktop settings daemon;
- animated blur, shader effects, or EGL-only rendering;
- Phosh/GNOME/Plasma compatibility layers;
- XWayland as a mandatory dependency.
