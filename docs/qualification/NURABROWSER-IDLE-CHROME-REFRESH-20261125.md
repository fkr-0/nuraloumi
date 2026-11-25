# NuraBrowser chrome refresh after in-page navigation — host gate

Date: 2026-11-25. Scope: native Cairo browser controls only; no device mutation.

## Defect and change

The browser chrome (`nuraloumi-browser`) uses its own Wayland connection,
separate from the WPE page. Previously, when the active page reported
`is_loading = false`, the chrome called `blocking_dispatch()` indefinitely.
A subsequent click on a WebKit page could change its URI, tab title or history
without producing any Wayland event on the chrome connection. That left the
address bar and Back/Forward state stale until an unrelated chrome interaction.

The event loop now waits with `poll()` in both states: 350 ms while loading and
1,500 ms when idle. A timeout invokes the existing bounded `BrowserUi::refresh`
RPC, then redraws. At idle this adds at most roughly 0.67 refresh requests per
second, while preserving immediate Wayland input dispatch. The interval is
defined and regression-checked explicitly.

## Executed host checks

| Check | Result |
| --- | --- |
| `cargo test -p nuraloumi-shell --bin nuraloumi-browser` | PASS, 5/5 |
| `cargo test -p nuraloumi-shell --test browser_scene_layout` | PASS, 5/5 |
| `cargo run -q -p nuraloumi-shell --bin nuraloumi-browser -- --fixture --headless` | PASS, two tabs/two bookmarks |
| `cargo clippy -p nuraloumi-shell --bin nuraloumi-browser -- -D warnings` | PASS |
| `rustfmt --check crates/nuraloumi-shell/src/bin/browser.rs` | PASS |

## Qualification boundary

This is a **host-only UI correctness fix**, not a WPE render, GPU/FPS,
NetworkProcess policy, physical-touch or release certification. On-device
WayVNC pixels, URI/history transitions following an in-page link and clean
process exit still need a native browser-shell E2E run against an eligible
runtime root. Do not promote the existing canary as a result of this change.
