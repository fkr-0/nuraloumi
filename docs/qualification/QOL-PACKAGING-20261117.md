# QoL R1 Lane 14 — Packaging / CI qualification (2026-11-17)

Canonical task: `NURALOUMI-20261117-QOL-R1-14-PACKAGING-CI-20261117-9B5DBC`

## Source-grounded defects

- `crates/nuraloumi-xtask/src/package.rs` previously staged and required exactly one desktop launcher, `nuraloumi-player.desktop`; no files, notifications, file-manager launchers or D-Bus activation were in the package manifest.
- `packaging/alpine/APKBUILD.in` previously installed only that player launcher, with no activation descriptor.
- `scripts/build-ci-debian-deb.sh:33` still installs **only** the player launcher and no D-Bus activation descriptor. This script is OUTSIDE Lane14 exclusive ownership; Lane15 must integrate a separate patch. An APK PASS cannot imply DEB PASS.
- `crates/nuraloumi-shell/Cargo.toml` declares `nuraloumi-files`, `nuraloumi-player`, `nuraloumi-notifications` and `nuraloumi-notificationd`; bin completeness is already metadata-derived through `scripts/package-binaries.py`. The full-window file-manager executable `nuraloumi-file-manager` is **not yet declared** in the observed state. The full-window desktop entry deliberately references that distinct name so `package-check` rejects a falsely advertised compact fallback until Lane07 supplies it.
- Lane03 currently owns `notification_daemon.rs`. Its observed code uses a session bus with `RequestNameFlags::DoNotQueue`. Activation file uses the standard user-service name and exact `/usr/bin/nuraloumi-notificationd`. Only one activation descriptor is staged; there is no XDG autostart entry and no system service enabled.
- Icons are currently freedesktop-themed icon names (`system-file-manager`, `preferences-system-notifications`, `multimedia-player`), **not guaranteed bundled assets**. Asset creation and Debian installation remain release gaps.

## Bounded changes

- Stage four explicit desktop entries plus `share/dbus-1/services/org.freedesktop.Notifications.service` in the deterministic tar manifest.
- Require their presence and validate `Exec` and `TryExec` binaries against the declared Cargo runtime bin list during `package-check`. Validate D-Bus service name, daemon path and included daemon binary.
- Alpine APK copy recipe installs staged desktop entries and user-bus activation descriptor; no post-install service startup.
- Isolated manifest fixture tests add missing-activation and broken-Exec failure injections, with an explicit daemon and file-manager model binary.

## Verification matrix

| Check | Result | Evidence |
|---|---|---|
| Python packaging integrity + native CI fixtures | PASS, 13 tests (prior to final desktop name tightening) | `python3 -m unittest scripts.test_package_integrity scripts.test_native_packages_ci` |
| Python packaging integrity + manifest + native CI | 17 PASS, 1 FAIL | `python3 -m unittest scripts.test_package_integrity scripts.test_package_binaries scripts.test_native_packages_ci`: parallel Lane03 untracked `notification_daemon.rs` is declared in Cargo but not tracked by Git |
| Rust formatter | PASS at inspected state | `cargo fmt --all -- --check` |
| Workspace Cargo tests | FAIL, unrelated concurrent Lane08 | `Playlist` gained `loop_mode` while `player.rs:196` still constructs the old initializer |
| Strict clippy | FAIL, concurrent Lane01 | `crates/nuraloumi-render-cairo/src/layout.rs:1065,1082` manual div-ceil |
| ARMv7 ELF/NEON audit | NOT RUN | No new native ELF was produced; existing audit script and private video contract were not changed |
| Actual APK and DEB content | NOT VERIFIED | No native container build or remote CI run permitted |
| Real notification bus / SL101 deployment | NOT VERIFIED | Local-only boundary |
| Re-run tests after final desktop tightening | PENDING | Workspace execution rejected: 4 active process sessions, capacity reached |

The test source-only fixture models an available full file manager, but current repository Cargo metadata does not yet provide it. Thus the final release gate is **blocked**, not implicitly qualified. The per-file hash-integrity check and ELF-header check remain in place; no private Grate/GStreamer ABI paths were altered.

## Independent verifier tasks

- `prompts/20261117-qol-r1-14-apk-contents-verify.md`: APK and ELF omission/contents
- `prompts/20261117-qol-r1-14-dbus-owner-verify.md`: notification activation/duplicate-owner policy
- `prompts/20261117-qol-r1-14-debian-seam-verify.md`: Debian copy seam, full window and assets

All three were sent through `chatgpt-ops run ... --project nuraloumi`; sending is not completion. Wait for exact OCP checkpoint/test evidence before treating independent review as accepted.

## Handoffs / release blockers

1. Lane07: produce and register an actual full-window `nuraloumi-file-manager`, or explicitly negotiate another truthful executable/argument contract. Never substitute the compact menu silently.
2. Lane15: own the out-of-scope Debian script seam and ensure the same desktop/service paths in native DEB and APK content tests. Ensure icon assets or document a deliberate theme dependency.
3. Lane03: confirm `nuraloumi-notificationd` name and single-owner behavior in final merged source.
4. Rebuild and retest once concurrent source changes settle; validate true ARMv7 VFPv3-D16/no NEON on both musl and glibc artifacts, and inspect native archives rather than equating metadata with installed contents.
