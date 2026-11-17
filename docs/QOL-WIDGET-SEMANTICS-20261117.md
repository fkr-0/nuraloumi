# QoL R1-01: compact widget semantics (local implementation)

## Existing behavior / root cause

- `crates/nuraloumi-core/src/model.rs`: `MenuItemKind` represents six full-width rows; `MenuAction::Adjust` only conveys a relative delta. Existing menu JSON is preserved unchanged.
- `crates/nuraloumi-render-cairo/src/layout.rs`: default row layout uses 48-pixel full-width interactive rows, while the horizontal tab strip is hardwired to `launcher` and `browser` prefix IDs. No general volume slider or grouped controls exist.
- `crates/nuraloumi-render-cairo/src/model.rs`: `Scene::hit_test` returns only a stable item ID; new absolute slider positioning therefore needs a resolver that also receives the pointer X coordinate.

## New additive API

Core: `nuraloumi_core::model::{CompactControl, CompactButton, CompactTab, CompactSliderKey, validate_compact_controls}`. Renderer: `nuraloumi_render_cairo::{layout_compact_controls, compact_action_at}`. Import from those paths; existing `MenuModel` and `layout_menu` remain untouched.

`CompactControl` serde-tagged variants: `button`, `toolbar`, `slider`, `tabs`, `breadcrumb`, `status`. A slider's typed control has `id`, `label`, `value_id`, `min`, `max`, `step`, `value`, `enabled`; callers validate controls before layout. Invalid ranges/steps are rejected; out-of-range runtime input is clamped; a non-divisible maximum is reachable. Values are integer only; providers can express scaled quantities through units defined at the call site.

`CompactControl::slider_action(value)` and `slider_key_action(CompactSliderKey::{Decrease,Increase,Minimum,Maximum})` return `MenuAction::SetValue { id, value }` or `None`; no system calls are executed by core or renderer. For touch/pointer, call `compact_action_at(&controls, &scene, x, y)` after layout. Rendering uses current provider-supplied values, not optimistic action state.

Toolbar children and tabs have individual semantic IDs and 48-logical-pixel hit rectangles; narrow viewports wrap them to multiple lines instead of shrinking targets. The slide track is 48px hit-height with 12px visual chrome. Non-ASCII icon fallbacks use labels to avoid tofu boxes. Scroll offsets are finite-clamped, and the focused hit is brought into the viewport. DPI changes physical pixels, not logical hit size.

## Integration examples

- **Volume:** toolbar `mute`, `volume.minus`, `volume.plus` with typed `Toggle` and `Adjust`, plus `Slider { value_id: "audio.volume", min: 0, max: 100, step: 1 }`; shell/provider handles `SetValue`.
- **Brightness:** same schema with `display.brightness`; renderer receives authoritative snapshot after mutation.
- **Player:** toolbar with previous/play/pause/stop/next/loop; `Slider { value_id: "player.seek_seconds", min: 0, max: duration_seconds, step: 1 }` for bounded media with known duration.
- **Loop selection:** tabs `Off`, `One`, `All` with `Navigate` or domain-specific `Activate` actions as appropriate; selected tab never invokes an action.

## Boundaries and remaining integration

The additional controls are a separate compact scene, not yet embedded inside legacy `MenuModel` row layouts or the shell/Wayland input dispatch. UI lanes must explicitly call this API and handle the new `SetValue` arm; do not interpret this implementation as proof that end-user volume and seek sliders now appear. Drag capture, release/cancel and live backlight/audio provider E2E belong to input/provider lanes. Existing shell menu behavior is preserved. The new API adds no Grate, GStreamer, NEON, EGL or remote-device dependency.

## Fixture and tests

- `docs/fixtures/qol-widget-controls.png`: 440×192 pixel deterministic Cairo raster fixture produced by ignored renderer unit fixture test. Regeneration needs moving the file from crate-relative `crates/nuraloumi-render-cairo/docs/fixtures` to repository-root `docs/fixtures` because Cargo test sets crate CWD.
- Test: `cargo test -p nuraloumi-core -p nuraloumi-render-cairo`, including serde round trip, extreme-range quantization, 1×/1.5×/2× DPI hit geometry, toolbar wrapping, disabled hits and duplicate semantic IDs.
- Shell integration, hardware qualification, drag lifecycle and independent ABC reviewers are NOT proven by these tests.
