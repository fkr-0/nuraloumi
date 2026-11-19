# NuraBrowser R2 Gate 3A — status

OCP `NURALOUMI-20261119-NURABROWSER-R2-TABSTRIP-UI-20261119-A514CE`.

Host tests: layout 5/5 PASS (exit 0); browser-scoped strict Clippy PASS (exit 0); offline headless fixture PASS (exit 0, 2 tabs / 2 bookmarks). Broad shell Clippy FAIL (exit 101): unrelated `bin/notifications.rs:97` `let_and_return`.

Observed geometry: tab strip 52 logical px, address 56 logical px full width, action cells 48 logical px; body is clipped/scrolling. For 6 tabs at width below 288 logical px equal-width tabs cannot retain 48 px each. Deterministic off-target release / touch cancel / refresh-during-press and Ctrl+L/Enter/Escape parity require final verification.

R1-03A owns active claims for `browser.rs`, `browser/mod.rs`, `browser_scene_layout.rs` (expiry ~01:25Z), and `browser/scene.rs`. R2 did not modify those paths. Real nested compositor pixels/touch: PENDING; host fixture is not device evidence. Gate4 owns supervisor/transport, not changed. No push, remote CI, device SSH, or live compositor mutation.
