# Changelog

All notable user-visible changes to NuraLoumi are recorded here.

## [Unreleased]

### ARMv7 on-device file-manager smoke fix (2026-11-11)

- Fixed an immediate `nuraloumi-files --live` startup failure on the ASUS
  SL101 (`pin browser root: Invalid argument (os error 22)`). Linux's
  `O_DIRECTORY` and `O_NOFOLLOW` flag values differ on ARM versus x86; the
  root pin and all secure `openat` mutation operations now use
  architecture-correct `libc::O_*` constants rather than hard-coded octal
  values. Directory/root symlink protections remain enabled.
- Verified the patched standalone ARMv7/musl executable on physical .106:
  ELF EABI5, hard-float VFPv3-D16, no missing runtime libraries,
  startup in the existing Sway/Wayland session, and 61 seconds of
  uninterrupted operation without logged errors (host-only source change;
  the pinned 0.0.4-dev.2 CI release on-device is **not** modified).
- Companion smoke: the existing image viewer, NetSurf GTK3 browser and
  software-rendered Wayland Cog/WPE browser each remained alive for 61 seconds.
  Cog loaded `about:blank` successfully; no interaction/pixel qualification
  is implied.

### CI repair

- The ARMv7 musl file-manager mutation path invokes the Linux
  `renameat2(RENAME_NOREPLACE)` syscall directly, avoiding a missing musl
  libc wrapper without weakening no-clobber semantics.
- Async file-open selection additionally compares inode change time and
  size; the replacement regression now pins the removed inode to avoid
  allocator-dependent false successes.
- Fixed the Debian CI command-line continuation so packaging receives its
  verified stage, output directory and Git SHA as separate arguments.
- Viewer/terminal child-slot reservation now uses a bounded atomic CAS loop,
  compatible with both older Rust and Rust 1.99 strict Clippy.

### Build and distribution

- Native CI packaging now assembles Debian 13 ARMHF `.deb` and Alpine 3.22
  ARMv7 `.apk` artifacts from the **exact, previously ELF-audited** staging
  binaries, alongside the original copy-only tar bundles. Build artifacts
  carry Git SHA, SHA256 checksums and manifest/ABI evidence.
- Added an exact-run GitHub Actions artifact verifier for CI-only deployment
  staging. Alpine development APK signatures use a CI-ephemeral key; production
  trust enrollment and physical SL101 qualification remain mandatory.
- Flatpak remains a separate desktop-only proposal: current standard
  Freedesktop runtimes do not target the SL101's 32-bit ARMv7 architecture.

## [0.0.4-dev.2] - 2026-11-10

Development prerelease; **not** a production-qualified SL101 release.

### Added

- Native, rooted Wayland file manager (`nuraloumi-files`) with exhaustive
  off-event-loop directory scans, 512-entry cursor batches, touch/keyboard
  paging, file-open actions, interactive Open With, explicit error/diagnostic
  states, and optional confirmed rename/delete. Directory results are bounded
  per batch rather than by truncating the directory scan.
- Shared XDG file opener with MIME associations, discoverable eligible desktop
  applications, exact-argv GIO launch requests, stable recent-file selection,
  and non-UTF-8-safe path handling. A helper launch request is not represented
  as proof that the application rendered a window.
- `nuraloumi-viewer` renders bounded PNG/JPEG images with Cairo/wl_shm and
  provides a readable `file -- PATH` diagnostic for unsupported inputs.
- A transient Sway workspace switcher with numbered previews, a blue selected
  border, immediate Super+1–9 switching, and dismissal on Super release.
  Never-captured inactive workspaces display a placeholder rather than a
  fabricated preview.

### Fixed

- Recent-file selection no longer silently discards failed launch requests;
  unavailable MIME defaults offer a real handler chooser or a diagnostic.
- File-manager confirmation/fallback dialogs accept touch release-inside
  instead of ignoring touch; Run/Open With also requires matching pointer
  release or retained touch contact rather than acting on initial press.
- File-manager keyboard navigation reaches chooser, fallback, and terminal
  actions; the terminal diagnostic uses `foot --hold` to keep output visible.
  Non-UTF-8 path arguments are preserved without panicking.
- Open With pagination keeps navigation and Cancel hit targets inside the
  SL101-height Cairo popup. Slow MIME/GIO/file-description work executes off
  the Run dialog's Wayland event thread.
- File opener helper capture is bounded without waiting indefinitely for
  inherited stdout/stderr pipe EOF.
- The standalone viewer creates its Wayland surface before image decoding or
  `file(1)` identification. A worker keeps the loading surface responsive;
  the diagnostic helper uses a two-second deadline with nonblocking pipes,
  bounded output, and process-group cleanup even when descendants retain pipes.
- Viewer JPEG EXIF orientation values 1–8 (rotations and reflections) render
  correctly; extreme image aspect ratios stay inside the Cairo-clipped image
  viewport. Only a matching primary-button release activates Back/X.
- The ARGB conversion traverses checked source rows/columns without per-pixel
  integer division/modulo, reducing software overhead on Tegra20 ARMv7.
  Host regression tests cover image decoding, EXIF, pathological dimensions,
  child-held pipes, slow loading, and input semantics. See `docs/VIEWER.md` and
  `docs/qualification/NURALOUMI-VIEWER-20261110.md`.
- Renderer indicator recognizes pinned Grate Gallium library layouts on Sway
  and does not claim a renderer is ready for unknown or failed probes.
  Renderer-switch arguments and authorization are unchanged.

### Compatibility and qualification

- All runtime surfaces retain the software Cairo/wl_shm fallback. EGL/GLES,
  NEON, and hardware video acceleration are **not** startup requirements.
- Rename/delete capabilities are disabled by default; user confirmation does
  not bypass rooted path or inode validation. Linux mutations use root-pinned
  descriptor-relative operations and a private staging/identity check with
  fail-closed recovery. Concurrent modification by an adversary with the same
  user permissions is not proven race-free; this is hardening, not a security
  guarantee against a malicious same-UID process.
- Host tests and CI checks are release gates, not proof of SL101 hardware
  qualification. ARMv7 build artifacts are separately audited by the GitHub
  CI workflows; device input, memory/latency, and compositor checks remain
  separate deployment prerequisites.
- No SL101 deployment or suspension is part of this release.

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
