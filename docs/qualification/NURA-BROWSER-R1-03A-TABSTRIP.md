# NuraBrowser R1-03A — fixed tab strip and browser scene

Date: 2026-11-19. Scope: host-only Rust/Cairo scene and deterministic tests.
OCP: NURALOUMI-NURABROWSER-R1-03A-FIXED-TABSTRIP-LAYOUT-2026111-20261119-727871.
No device access, publication, commit, compositor restart, or external deployment.

## Implementation

- The browser binary calls `browser::scene::browser_scene` and renders the
  returned public `Scene` with `CairoRenderer::render_scene`, not the generic
  `render_menu_with_glyphs` layout.
- A 52-logical-pixel fixed horizontal tab strip, 56-pixel full-width address
  control, and 48-pixel action cells precede the clipped bookmark/status
  viewport. At 720–820 logical px, nine actions occupy three columns.
  A 480-px narrow case uses two columns; a 1024-px wide case uses four.
- Up to six tabs divide the whole width deterministically; selected tabs are
  disabled in the semantic menu and excluded from actionable hit testing.
  Text is ellipsized by deterministic glyph width and clipped to its cell.
  Action identity, URLs and payloads are never truncated by layout.
- The bookmark viewport can follow keyboard selection or scroll by same-contact
  touch drag; a drag cancels pending tap activation. Fixed controls are outside
  the scrolling clip. Pointer/touch press-release semantics remain in the
  original browser input layer.
- Existing engine-facing RPC and transport signatures are unchanged. No
  engine supervisor or new renderer/Wayland primitive was introduced.

## Hit-region table (fixture, 720 × 312 logical px)

Coordinates are `x,y,width,height` in logical pixels. All below derive from
the public scene geometry and are asserted by deterministic tests.

| ID | Rectangle | Actionable |
| --- | --- | --- |
| browser.tab.0 (active) | 0,0,360,52 | no |
| browser.tab.1 | 360,0,360,52 | yes |
| browser.address | 0,52,720,56 | yes |
| browser.go | 0,108,240,48 | yes |
| browser.back | 240,108,240,48 | yes |
| browser.forward (disabled in fixture) | 480,108,240,48 | no |
| browser.reload | 0,156,240,48 | yes |
| browser.stop (disabled in fixture) | 240,156,240,48 | no |
| browser.tab.new | 480,156,240,48 | yes |
| browser.tab.close | 0,204,240,48 | yes |
| browser.bookmark.toggle | 240,204,240,48 | yes |
| browser.bookmark.expand | 480,204,240,48 | yes |
| bookmark/status viewport | 0,252,720,60 | clipped, scrollable |

At 820 logical px, tab widths are 410 px and action columns are 820/3
px each; all Y positions and heights are unchanged. With six tabs at 720 px,
each tab is 120 × 52 px. The tests also exercise 480 and 1024 px widths,
clipped bookmark row hits, stability under scroll offsets, disabled tab
hit-testing, ellipsis/target separation, and unsaved address editing.

## PROVEN (host)

- `CARGO_TARGET_DIR=/tmp/nuraloumi-browser-r1-03A-target cargo test -p nuraloumi-shell --test browser_scene_layout`: **5 passed**.
- `CARGO_TARGET_DIR=/tmp/nuraloumi-browser-r1-03A-target cargo test -p nuraloumi-shell browser`: **exit 0**.
- `CARGO_TARGET_DIR=/tmp/nuraloumi-browser-r1-03A-target cargo test -p nuraloumi-shell`: **exit 0**.
- `CARGO_TARGET_DIR=/tmp/nuraloumi-browser-r1-03A-target cargo test --workspace`: **exit 0**.
- `CARGO_TARGET_DIR=/tmp/nuraloumi-browser-r1-03A-target cargo run -p nuraloumi-shell --bin nuraloumi-browser -- --fixture --headless`: **exit 0**, rendered software scene.
- `CARGO_TARGET_DIR=/tmp/nuraloumi-browser-r1-03A-target cargo clippy -p nuraloumi-shell --lib --bin nuraloumi-browser --test browser_scene_layout -- -D warnings`: **exit 0**.

## UNPROVEN / follow-up

- `cargo clippy --workspace --all-targets -- -D warnings` did **not**
  pass: unrelated, out-of-scope `src/bin/notifications.rs` reports
  `clippy::let_and_return`. Do not modify that file from this lane.
- No live nested Wayland compositor, touch device, real WPE pixels, ARMv7
  VFPv3-D16/no-NEON closure, sandbox startup, or GPU/device qualification was
  performed. Neither fixture rendering nor host unit tests prove those gates.
- The predecessor's qualification file
  `docs/qualification/NURA-BROWSER-R1-03-WAYLAND-E2E.md` was absent from
  this checkout when R1-03A inspected it; no predecessor proof is inferred.
- `nuraloumi-wayland` currently exposes no pointer-wheel/axis event in
  `PlatformEvent`. Pointer-wheel scrolling would require a separate scoped
  backend request; keyboard selection and touch drag work without it.

## Changed paths (all within R1-03A ownership)

- `crates/nuraloumi-shell/src/browser/mod.rs`
- `crates/nuraloumi-shell/src/browser/scene.rs` (new)
- `crates/nuraloumi-shell/src/bin/browser.rs`
- `crates/nuraloumi-shell/tests/browser_scene_layout.rs` (new)
- `docs/qualification/NURA-BROWSER-R1-03A-TABSTRIP.md` (new)
