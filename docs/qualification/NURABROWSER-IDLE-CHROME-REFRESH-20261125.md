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
RPC, then redraws only when the browser model changes. At idle this adds at
most roughly 0.67 refresh requests per second, while preserving immediate
Wayland input dispatch. The interval and redraw decision are regression-checked
explicitly.

## Executed host checks

| Check | Result |
| --- | --- |
| `cargo test -p nuraloumi-shell --bin nuraloumi-browser` | PASS, 8/8 (after repaint tests) |
| `cargo test -p nuraloumi-shell --test browser_scene_layout` | PASS, 5/5 |
| `cargo run -q -p nuraloumi-shell --bin nuraloumi-browser -- --fixture --headless` | PASS, two tabs/two bookmarks |
| `cargo clippy -p nuraloumi-shell --bin nuraloumi-browser -- -D warnings` | PASS |
| `rustfmt --check crates/nuraloumi-shell/src/bin/browser.rs` | PASS |

## Independent model transition regression

The browser chrome now has a deterministic fake-RPC sequence covering an
external WebKit link with no chrome input: initial page, loading next page,
finished next page, updated address/title/Back/Stop/Reload actions and a
second tab. It also verifies that a user-typed draft remains intact across a
subsequent background refresh and that Escape restores the current page URI.
This checks the model update seam; it does not simulate Wayland events or prove
native WebKit pixel delivery.

Additional executed checks after adding this regression:

- `cargo test -p nuraloumi-shell browser::`: **PASS, 8/8 browser library tests**.
- `cargo test -p nuraloumi-shell --test browser_supervisor_lifecycle`: **PASS, 6/6**.
- `cargo clippy -p nuraloumi-shell --lib --bin nuraloumi-browser -- -D warnings`: **PASS**.
- `rustfmt --check crates/nuraloumi-shell/src/browser/mod.rs`: **PASS**.

## Idle Cairo repaint suppression

The 1,500 ms idle poll (350 ms during loading) is still needed to notice a
WebKit navigation that does not generate an event on the chrome's Wayland
connection. Previously every timeout scheduled a full Cairo redraw, even when
the RPC snapshot was identical. The loop now compares the bounded `BrowserUi`
state before and after refresh and redraws only on a model transition; Wayland
input and other explicit damage still request redraw independently. The
comparison includes the active URI, tab/history state, address draft,
bookmarks, loading indicators and transport status.

The browser binary tests exercise 64 consecutive identical RPC snapshots with
no scheduled redraw; a changed URL and a new tab each require one, while
subsequent unchanged snapshots do not. Repeated identical transport failures
cause one initial status redraw, not a repaint loop. An unchanged idle poll
also preserves an in-progress touch press until release.

For an otherwise static UI this suppresses approximately **40 unnecessary
Cairo repaint requests per minute** by event-loop contract. It does **not**
eliminate the 0.67 Hz RPC polling, prove an ARMv7 CPU/RSS win, measure WayVNC
pixels or certify native WPE behavior. No target installation or runtime
mutation was performed for this change.

## Qualification boundary

This is a **host-only UI correctness fix**, not a WPE render, GPU/FPS,
NetworkProcess policy, physical-touch or release certification. On-device
WayVNC pixels, URI/history transitions following an in-page link and clean
process exit still need a native browser-shell E2E run against an eligible
runtime root. Do not promote the existing canary as a result of this change.
