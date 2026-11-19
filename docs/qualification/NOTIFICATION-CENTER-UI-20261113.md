# Notification Center UI qualification — 2026-11-13

Task: NURALOUMI-02-NOTIFICATION-CENTER-UI-20261113-307117

## Ownership and integration

- Existing binary: crates/nuraloumi-shell/src/bin/notifications.rs; no duplicate.
- No changes to panel.rs, lib.rs, providers, core, renderer or Wayland crates.
- Uses existing render_menu_with_glyphs, Cairo scene and WaylandBackend::present
  with PixelFormat::Argb8888 and wl_shm; no EGL/GLES required.
- The binary is already registered in the shell Cargo manifest.
- Panel owner should launch the binary; backend owner is authoritative for
  live IDs and actions. Render/hit-test functions never execute commands.

## Behavior and limitations

| Behavior | Current UI semantics |
|---|---|
| Inbox | At most 64 entries grouped alphabetically by app with distinct section headers |
| Filter | Case-insensitive app, summary and body search; max 80 characters; empty/miss state |
| Read/unread | Filled/unfilled marker, explicit mark action; session-local, not persisted |
| Detail | Provider-bounded body, 46-char wrapped lines, five-line pages; touch and keyboard paging |
| Actions | Only actions advertised by snapshot, revalidated by provider before execution |
| Dismiss | Exact ID provider Remove; no mutation on pointer-down |
| Clear all | Explicit Yes/Cancel confirmation and Esc; verify expected ID set against a fresh provider snapshot |
| Errors | Backend unavailable, stale history, failed actions, retry; preserve old entries if refresh fails |
| Touch | 48px hit targets, matching release-inside, swipe navigation, multitouch and TouchCancel fail closed |
| Keyboard | Up/Down, Tab, Enter/Space, Esc, F5 refresh, PgUp/PgDn for long bodies, filter+Backspace |
| CPU | Blocking dispatch; redraw on input/configuration/action, no idle timer or GPU |

**Per-message time grouping deferred**: current provider NotificationEntry has no
timestamp, and backend history does not supply a persisted read/unread flag.
The UI does not invent ages or imply read status survives restart. Backend
integration must add source-backed timestamp/age metadata and durable read-state
semantics before these can be claimed. App grouping and session-local reads
function independently.

## Deterministic host checks

    rustfmt --edition 2021 crates/nuraloumi-shell/src/bin/notifications.rs
    cargo fmt --all --check
    cargo test -p nuraloumi-shell --bin nuraloumi-notifications
    cargo test --workspace
    cargo clippy --workspace --all-targets -- -D warnings
    git diff --check

Tests cover app grouping, body filtering, stable IDs, bounds/sanitization,
local reads, paging, clear/cancel/stale protection, forged action rejection,
pointer release-inside, swipe, multitouch cancel, keyboard semantics, hit
geometry and error/retry/empty states. All tests use fixtures; no live daemon.

## Source-backed E2E: independent follow-up

Task NURALOUMI-03-NOTIFICATIONS-INDEPENDENT-E2E-20261113-E7A388 owns
real notification daemon and physical-device qualification.

Pending:
- Dunst daemon and notification sender, live action IDs/races/errors.
- Actual SL101 touch and German slider-keyboard gestures, long-list scroll.
- Cairo/wl_shm pixel screenshots under Sway and software fallback.
- Real timestamp/read-state integration after backend contract is available.
- Tegra20 RSS/idle CPU, first frame latency; ARMv7 musl/armhf no-NEON cross-build.

Do not infer live SL101 E2E PASS from host tests or modify production
Mesa/Sway, reboot, flash or suspend on this UI task.
