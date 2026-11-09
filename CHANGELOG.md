# Changelog

All notable user-visible changes to NuraLoumi are recorded here.

## [Unreleased] - 2026-11-09

### Fixed

- Renderer indicator identifies Grate on Sway/sl101-sway with date- or label-suffixed
  `/opt/grate-mesa25-musl-*/lib/libgallium-25.0.7.so` installations, the
  glibc clean-prefix layout, and legacy Grate libraries. Detection requires the
  actual mapped library pathname and the expected GLES2/Grate environment.
- Probe failure and unknown compositor state are shown as unavailable, not a
  ready renderer. The renderer switch UI now refers to the compositor rather
  than labwc only. The privileged renderer-switch helper and its arguments
  are unchanged.

### Verification

- Added a fixture from the coordinator's 2026-11-09 Sway session and regression
  tests for recognized layouts, false positives, missing/oversized/unreadable
  procfs inputs, and compositor PID reuse. Device deployment and validation
  remain coordinator-owned.

## [0.0.4-dev.1] - 2026-11-03

Development checkpoint toward 0.2 and 0.3; not SL101 release qualified.

### Added

- Resource, task, music, notification and calendar surfaces; terminal/editor launchers,
  clipboard history, glyphs, and bounded calendar/weather providers.
- Offline feed and map providers and initial Cairo reader/viewer implementations.
- Screenshot capture/preview, one-shot notification toasts and an explicit IPC trigger.
- Initial confirmed file operations, monotonic timer/stopwatch, persistent favorites
  storage, accessible renderer text scaling/light theme, and opt-in config migration.
- Package completeness/integrity checks and bounded performance qualification tools.

### Fixed

- IPC now distinguishes unobserved standalone surfaces from closed ones and enforces
  an overall monotonic client deadline.
- Screenshot capture and clipboard operations bound descendant-retained pipes.
- Rotated raw touch coordinate mapping and keyboard protocol capability reporting.

### Qualification and remaining work

- New feature source does not imply complete launcher/config/runtime integration.
  Config, maps, feeds and timer binaries are registered; broader launcher and IPC
  runtime integration still require followup.
- Host checks are recorded separately from release acceptance. ARMv7 qualification
  is blocked by the prepared sysroot missing libxkbcommon; physical SL101 input,
  software-rendered operation, memory and latency checks remain pending.
- This development tag does not certify 0.2 or 0.3 completion or authorize deployment.

## [0.0.3] - 2026-11-03

### Added

- Keyboard input now follows the compositor's XKB keymap instead of assuming
  US-QWERTY. German QWERTZ, French AZERTY, and other layouts work out of the
  box when the compositor sets `XKB_DEFAULT_LAYOUT`.
- Added dedicated `nuraloumi-run`, `nuraloumi-files`, and `nuraloumi-windows`
  binaries so the compositor can invoke each surface directly from keybindings.
- Added an optional glyph resolver callback to the render adapter, allowing
  menu binaries to supply per-item icon glyphs without coupling the core model
  to icon resolution.

### Fixed

- Enabled panel-scope keybindings now fail validation with a clear message
  instead of being silently accepted but never firing.
- Disabled keybinding overrides no longer require a valid scope-action pair,
  since they serve only as documentation.

### Compatibility

- New dependency: `xkbcommon` (with `wayland` feature) for XKB keymap loading.
  Requires `libxkbcommon` in the cross-compile sysroot.
- The hardcoded scancode table remains as a fallback when no compositor keymap
  has been received.
- The pixman/Cairo software-rendered path remains authoritative.

## [0.0.2] - 2026-10-03

### Added

- Added a bounded resource telemetry provider backed by procfs/sysfs/df for aggregate and per-core CPU usage, memory/swap, root-filesystem usage, bounded top-process samples, network and disk I/O rates, thermal readings, and explicit partial-data diagnostics.
- Added a real transparent layer-shell dismiss backdrop for transient menus. It covers the output behind a menu while deliberately leaving the persistent panel strip uncovered.
- Added configurable transient-menu policy through `[menu_dismissal]`, with `outside_press = true` and `after_one_shot_action = true` as the defaults.

### Changed

- Reworked the visual system into a restrained dark-in-dark graphite/steel treatment: square corners, flat surfaces, no decorative shadows/gradients, muted structured borders, and a narrow selection rail while preserving touch-target geometry.
- Transient menus now dismiss on outside pointer press or touch-down by default. Blank non-actionable space inside a menu follows the same policy.
- Successful application launch, window focus, and desktop switch now close a transient menu by default, while brightness/volume adjustments, toggles, menu navigation, overview changes, and control-center tab changes stay open.
- Updated the visual/interaction design guide to match the implemented square shell and current Favorites → Open windows → Applications → More launcher hierarchy.

### Fixed

- Launcher search now filters the complete discovered application/window/task/desktop snapshot before applying daily-use display caps, so matching applications beyond the initial list remain discoverable.
- Restored an explicit Tasks route in the launcher's More section.

### Design

- Added `docs/KEYBINDINGS.md` as the contract for a later transparent keybinding system: one canonical chord notation, stable binding identities, explicit scopes, deterministic conflict handling, configuration overrides, and structured effective-keymap introspection.
- Reserved bounded typed-action sequences for a later combined-action stage. The design explicitly excludes arbitrary shell commands, loops, implicit order-dependent overrides, and safety-gate bypasses.

### Compatibility

- The pixman/Cairo software-rendered path remains authoritative; EGL/GLES acceleration is optional.
- The SL101 target remains ARMv7 hard-float musl with VFPv3-D16 and no NEON, upper-D-register, or hardware-divide requirement.
- Existing `.desktop` authority, provider safety gates, confirmation behavior, and explicit unsafe-suspend opt-in remain unchanged.

### Verification

- Release qualification covers locked host format/check/test/clippy/release-build gates, ARMv7 cross build and ELF audit, real-SL101 copy-only no-SIGILL smoke, and a clean package built from the exact release commit.

## [0.0.1] - 2026-10-03

### Added

- Native Wayland panel and launcher surfaces designed for the ASUS SL101/Nura and similarly constrained Linux tablets.
- XDG desktop-entry application discovery with searchable launching, configurable favorites, hidden entries, labels, and exact Wayland app-id aliases.
- Foreign-toplevel window discovery and controls, including conservative reuse of an already-open application instead of blindly spawning duplicates.
- Workspace/desktop views, task inspection, a tabbed control center, media and notification integration, network/Bluetooth controls, brightness, audio, and guarded power actions where providers are available.
- TOML/JSON shell configuration with automatic XDG/HOME discovery and a documented example configuration.
- ARMv7/musl qualification tooling and copy-only SL101 deployment/smoke workflows.

### Changed

- The default launcher is now a daily-use surface rather than a diagnostic overview: Favorites, Open windows, Applications, then compact links to all applications, window controls, desktops, settings, and power.
- Advanced Windows, Applications, Tasks, and Desktops views remain available without crowding the default launcher.
- Search wording follows the active launcher view instead of claiming to search data that is not currently shown.
- Cairo + wl_shm software rendering remains the compatibility baseline; GPU acceleration is optional.

### Compatibility

- Release target: ARMv7 hard-float musl on Tegra20-class hardware, with VFPv3-D16 and no NEON requirement.
- The shell is designed to run on top of labwc/wlroots and does not replace the compositor.
- Application execution remains authoritative to validated XDG .desktop files; user configuration cannot inject arbitrary commands.

### Known limitations

- Live window previews depend on compositor capture protocol support and may be unavailable without affecting window listing/control.
- Deep suspend is not treated as release-qualified on the SL101; NuraLoumi keeps suspend behind a separate safety opt-in.
- Optional GPU-backed compositor operation does not replace the qualified pixman/software recovery path.

### Verification

- Host format, locked check, full workspace tests, strict clippy, and optimized release build passed on 2026-10-03.
- The release candidate cross-build passed for armv7-unknown-linux-musleabihf; all four runtime binaries passed the Tegra20 ELF audit with VFPv3-D16, no NEON attribute/disassembly hit, no upper-D-register use, and no hardware divide.
- The same ARMv7 runtime binaries passed the copy-only no-SIGILL smoke on the real SL101 before tagging.
