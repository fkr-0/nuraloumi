# Architecture

## Dependency direction

```text
nuraloumi-shell
   ├── nuraloumi-core
   ├── nuraloumi-render-cairo
   ├── nuraloumi-wayland
   └── nuraloumi-providers

nuraloumi-render-cairo ──> nuraloumi-core
nuraloumi-providers      ──> nuraloumi-core (typed action/result types only)
nuraloumi-wayland        ──> no provider dependency
nuraloumi-xtask          ──> build/test/package tooling; not linked into runtime
```

Cycles are forbidden.

## Runtime flow

1. provider snapshots are translated into MenuModel values.
2. shell owns navigation/focus/open-surface state.
3. renderer converts a model + viewport + interaction state into a Scene.
4. Cairo rasterizes Scene into an ARGB buffer.
5. Wayland backend copies/renders into wl_shm buffers and commits damage.
6. Wayland input becomes PlatformEvent; shell converts it into semantic navigation/action messages.
7. provider actions are invoked only after semantic action validation.

## Recovery path

The reference rendering path is CPU software:
`Cairo -> wl_shm -> labwc/wlroots pixman`.

No runtime crate may require EGL initialization to display the panel or menus.
If the compositor itself later uses Grate/GPU acceleration, NuraLoumi remains unchanged.

## Resource discipline

- single process is preferred for panel + transient menu surfaces in 0.1.
- providers may use subprocesses only behind bounded adapters.
- no polling faster than 1 Hz for battery/clock; event-driven when practical.
- redraw only damaged/changed surfaces.
- use fixed-size or bounded caches.
- animations are finite and stop scheduling frames when complete.
