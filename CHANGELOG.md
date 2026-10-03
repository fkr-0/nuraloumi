# Changelog

All notable user-visible changes to NuraLoumi are recorded here.

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
