# NuraBrowser R2 Gate 3A — tabstrip/UI qualification

Date: 2026-11-19. Canonical OCP:
`NURALOUMI-20261119-NURABROWSER-R2-TABSTRIP-UI-20261119-A514CE`.

## Result

**Host Gate 3A: PASS for the existing code; real Wayland compositor: PENDING.**
The previously integrated R1-03A browser scene and input path already implement
the requested fixed chrome; no runtime edits were needed in this follow-up.

- `browser::scene::browser_scene` places a fixed, horizontal **52-logical-pixel**
  tab strip, a **56-logical-pixel full-width** address row and **48-logical-pixel**
  action cells ahead of a clipped, independently scrolling bookmark/status list.
  At 720 logical px, the three action columns are 240 px each.
- Active tabs remain visible but are not actionable; remaining tabs keep stable
  `browser.tab.select` identities. The six-tab test at 720 px yields 120 × 52
  logical-pixel targets. At widths below 288 px, six equal columns necessarily
  violate a 48 px minimum; the actual Wayland surface requests 720–820 px.
- Pointer activation requires matching press/release; pointer-leave cancels.
  Touch activation requires the same contact ID, cancels on motion leaving the
  hit region or explicit cancel, and is reset by model/layout redraw. Scrolling
  beyond the drag threshold cancels the pending activation.
- Ctrl+L opens address editing, Enter submits, Escape cancels; Ctrl+T/W
  request new/close tab. `BrowserUi` preserves unsent edits across refresh.
  Keyboard selection scrolls the lower list into view.
- `--fixture --headless` exercises Cairo rendering without Wayland, engine
  socket or network access. It is not a pixel/touch proof of a compositor.

## Executed checks

| Check | Result | Exit |
| --- | --- | --- |
| `cargo test -p nuraloumi-shell --test browser_scene_layout` | PASS, 5/5 | 0 |
| `cargo test -p nuraloumi-shell --bin nuraloumi-browser` | PASS, 4/4 | 0 |
| `cargo run -q -p nuraloumi-shell --bin nuraloumi-browser -- --fixture --headless` | PASS, two tabs/two bookmarks | 0 |
| `CARGO_TARGET_DIR=/tmp/nurabrowser-a514ce-target cargo clippy -p nuraloumi-shell --lib --bin nuraloumi-browser --test browser_scene_layout -- -D warnings` | PASS | 0 |
| `CARGO_TARGET_DIR=/tmp/nurabrowser-a514ce-target cargo clippy --workspace --all-targets -- -D warnings` | PASS | 0 |
| `CARGO_TARGET_DIR=/tmp/nurabrowser-a514ce-target cargo test --workspace` | BLOCKED during linking: disk quota exceeded (OS error 122), not a test assertion failure | 101 |

A non-isolated Clippy run first failed with `E0432` on an existing `scene`
import despite the preceding integration test passing; the identical command
using an isolated target directory passed. Concurrent shared-target artifact
interference is plausible, but not independently proven.

## Remaining proof and handoff

- **PENDING**: real nested compositor Configure dimensions, wl_shm paint,
  pointer/touch hit coordinates, release/cancel paths and keyboard focus, with
  captured scene/pixel evidence. R2 nested E2E owns this proof; it must not be
  inferred from the offline fixture or prior host tests.
- **PENDING**: clean complete `cargo test --workspace` once disk quota allows
  linking; do not delete other lanes' target directories.
- Gate 4 supervisor/transport boundary is unchanged. Browser chrome uses
  `BrowserRpc` and stable `browser.tab.*` actions; Gate 4 must preserve
  refresh/event semantics and must not let a page initiate chrome actions.
- No device deployment, live Sway mutation, reboot, SSH, push or remote CI.

Scope of this follow-up: qualification document only. Runtime implementation
was already present and passed the targeted checks.
