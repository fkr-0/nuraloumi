# Parallel implementation lanes — Wave 1

All six lanes are intended to run concurrently. Their write scopes deliberately do not overlap.

## Lane A — Core semantics
Write: `crates/nuraloumi-core/**`
Goal: complete renderer-neutral semantic menu library with tests and fixtures.
Reference: DeskHalloumi `unilii/core/src/menu.rs`, `ui_tokens.rs`, `ui_motion.rs`, `docs/menu-design-system.md`.
Do not copy desktop integration or Iced dependencies.

## Lane B — Cairo renderer
Write: `crates/nuraloumi-render-cairo/**`, `tests/fixtures/render/**`
Goal: deterministic layout/scene + Cairo ImageSurface renderer + PNG example.
Reference: modrelease-2 `src/render/cairo_utils.rs`, `src/ui/widgets/menu_layout.rs`, relevant menu render block.
No XCB/X11.

## Lane C — Wayland backend
Write: `crates/nuraloumi-wayland/**`, `examples/wayland-*`
Goal: shm + layer-shell + touch/pointer/keyboard backend and paint demo.
No EGL requirement.

## Lane D — Shell binaries
Write: `crates/nuraloumi-shell/**`, `examples/menu-fixtures/**`
Goal: operating panel and generic menu binaries in fixture mode, then pluggable providers.
May depend on declared crate contracts; do not edit provider/backend/core crates.

## Lane E — Providers
Write: `crates/nuraloumi-providers/**`, `tests/fixtures/providers/**`
Goal: read-only snapshots first, safe explicit actions second; battery/backlight/network/audio/clock and JSON probe binary.
No GUI dependencies.

## Lane F — Build/qualification
Write: `crates/nuraloumi-xtask/**`, `.cargo/**`, `packaging/**`, `scripts/**`, `docs/qualification/**`
Goal: host build/test/package commands, ARMv7/no-NEON checks, nested Wayland smoke, SL101 deployment/rollback bundle.
Do not modify runtime crates.

## Merge order after Wave 1

1. core
2. renderer and providers
3. Wayland backend
4. shell adaptation to exact landed APIs
5. xtask/qualification full gate
6. independent integrated review
