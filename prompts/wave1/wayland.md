@projmgrauth
Set repo: nuraloumi

# NuraLoumi Wave 1 — Native Wayland SHM Backend

Canonical OCP task: NURALOUMI-WAYLAND-R1-20260930
Lane: wayland
Priority: 92

Preserve the exact OCP identity and project workflow authority. Claim only this lane.

## Mission

Build the native Wayland presentation/input backend that makes NuraLoumi viable on the SL101 without XWayland or EGL. It must display caller-supplied software pixel buffers using wl_shm and support a layer-shell panel/popup on wlroots/labwc.

Read:
- AGENTS.md
- ROADMAP.md
- DESIGN.md
- docs/ARCHITECTURE.md
- docs/CONTRACTS.md
- docs/SL101-QUALIFICATION.md

## Exclusive write scope

- crates/nuraloumi-wayland/**
- examples/wayland-*

Do not edit shell/core/renderer/providers/root manifests.

## Required implementation

1. Connection/registry
   - connect from environment.
   - bind wl_compositor, wl_shm, wl_seat, wl_output and wlr-layer-shell when available.
   - explicit capability/error reporting if layer-shell or required shm format is absent.

2. Software buffers
   - safe shm allocation strategy suitable for low-memory ARMv7.
   - double buffering or another backpressure-safe scheme.
   - ARGB8888/XRGB8888 support.
   - configure/resize without leaks.
   - damage/attach/commit and release lifecycle.
   - no EGL/GLES initialization anywhere.

3. Surface roles
   - top panel: top/left/right anchored, configurable exclusive zone/height.
   - transient sheet/menu: anchored layer surface with keyboard interactivity appropriate for menus.
   - clean surface teardown.
   - avoid stealing keyboard focus while only the panel is visible.

4. Input
   - pointer enter/motion/button.
   - touch down/motion/up/cancel with stable IDs.
   - keyboard key press/release sufficient for arrows, Enter, Escape, Backspace and text when possible.
   - normalized logical coordinates after output scale/transform.
   - expose PlatformEvent-like API from docs/CONTRACTS.md.

5. Output
   - scale tracking.
   - output transform metadata including rotated displays.
   - deterministic coordinate-transform helpers with tests.

6. Demo
   - a minimal example that opens a layer surface, fills wl_shm pixels with a visible test pattern, reacts to pointer/touch/key input, and exits cleanly.
   - if no Wayland compositor is available in CI, unit-test protocol-independent helpers and make live smoke opt-in rather than failing all tests.

## Library choice

Use current maintained Rust Wayland crates and layer-shell protocol bindings with the smallest practical feature set. Prefer stable public APIs over custom raw FFI. Do not add Smithay compositor/server internals merely to make a client.

## Acceptance

- cargo test -p nuraloumi-wayland passes.
- cargo clippy -p nuraloumi-wayland --all-targets -- -D warnings passes.
- no X11/XCB/EGL/GLES dependency.
- a live example can display software pixels under an available wlroots compositor.
- buffer release/resize logic is tested enough to rule out unbounded allocation.
- coordinate transform tests cover normal + 90/180/270 transforms.
- touch and keyboard events are represented without requiring GUI toolkit types.
- CRATE_READY placeholder removed.

## Handoff

Document exact crate versions/protocol choices, demo invocation, required Wayland globals, shm format contract for renderer/shell, and any live-environment limitation. Submit typed phase result/checkpoint.
