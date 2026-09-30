@projmgrauth
Set repo: nuraloumi

# NuraLoumi Wave 1 — Cairo Software Renderer

Canonical OCP task: NURALOUMI-CAIRO-R1-20260930
Lane: cairo
Priority: 90

Use the exact OCP task as durable identity. Adopt/bind its managed packet before writes when required. Work only in your lane and preserve concurrent agents.

## Mission

Produce a compact, deterministic software renderer that turns the NuraLoumi semantic menu model into polished pixels without X11, XCB, EGL, GLES, WGPU, Iced, or a compositor-specific renderer. The target path is Cairo ImageSurface -> Wayland wl_shm -> labwc/wlroots pixman.

Read:
- AGENTS.md
- DESIGN.md
- docs/ARCHITECTURE.md
- docs/CONTRACTS.md
- docs/AGENT-LANES.md

Reference:
- /home/user/code/modrelease-2/src/render/cairo_utils.rs
- /home/user/code/modrelease-2/src/ui/widgets/menu_layout.rs
- /home/user/code/modrelease-2/src/ui/widgets/menu.rs
Use ideas and primitives, not its XCB renderer/window ownership.

## Exclusive write scope

- crates/nuraloumi-render-cairo/**
- tests/fixtures/render/**

Do not edit nuraloumi-core or any sibling crate. Adapt to the declared core contract and current landed API; if it is incomplete, keep an adapter layer local to this crate rather than invading core.

## Required implementation

1. Scene/layout model
   - Viewport with logical size and scale.
   - Rect, Insets, text/icon metrics.
   - Scene/PaintNode or equivalent deterministic intermediate representation.
   - HitRegion containing stable menu item identity and geometry.
   - layout for header, section/status/separator/action/checkable/submenu rows.
   - scroll window metadata even if scrolling itself is shell-owned.

2. Touch-first geometry
   - 52px default primary rows, >=48px actionable hit targets.
   - compact non-action rows allowed around 40px.
   - 420-520px target menu width with viewport clamping.
   - correct high-DPI/scale multiplication while keeping logical hit coordinates.

3. Cairo drawing
   - ImageSurface ARGB32 software rendering.
   - rounded panel/card fills, 1px border, selected/pressed states.
   - cheap bounded shadow; constrained mode disables expensive shadow/gradient.
   - separators, text title/subtitle/shortcut, chevron/check marker.
   - DESIGN.md dark palette initially; support core theme tokens.
   - no blur pipeline and no continuous animation.

4. Text
   - provide a minimal text measurer/renderer abstraction.
   - Cairo toy text is acceptable for first operating binary if clearly isolated.
   - keep a clean future seam for PangoCairo without making it mandatory.

5. Fixture tooling
   - executable/example that renders canonical launcher/system fixtures to PNG.
   - deterministic scene/layout tests independent of PNG byte instability.
   - hit-test tests for touch boundaries and disabled/non-actionable rows.

6. Buffer handoff
   - expose raw/borrowed ARGB32 image bytes + stride/size safely enough for the Wayland lane to copy into wl_shm.
   - document pixel format/endian assumptions.

## Dependency ceiling

Prefer cairo-rs with the minimum needed features plus nuraloumi-core. Do not enable xcb/xlib/gl/gles features. Avoid large image/icon stacks in Wave 1.

## Acceptance

- cargo test -p nuraloumi-render-cairo passes.
- cargo clippy -p nuraloumi-render-cairo --all-targets -- -D warnings passes.
- fixture PNG command produces non-empty valid PNG(s).
- crate dependency tree has no X11/XCB/Iced/WGPU/EGL/GLES.
- deterministic layout tests cover 48+px touch targets, submenu/checkable/status/section rows and viewport clamping.
- raw buffer output can be consumed without a GPU.
- CRATE_READY placeholder removed.

## Handoff

Report exact PNG artifact paths, dimensions, dependency choices, public scene/buffer APIs, performance caveats, and remaining text/icon limitations. Submit a typed phase result/checkpoint.
