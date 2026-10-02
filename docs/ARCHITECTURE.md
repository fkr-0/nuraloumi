# Architecture

## Dependency direction

```text
nuraloumi-shell
   ├── nuraloumi-core
   ├── nuraloumi-render-cairo
   ├── nuraloumi-wayland
   └── nuraloumi-providers

nuraloumi-render-cairo ──> nuraloumi-core
nuraloumi-providers      ──> no runtime crate dependency
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
7. shell translates validated semantic actions into provider-specific actions;
   providers never import menu/core semantics.

## Recovery path

The reference rendering path is CPU software:
`Cairo -> wl_shm -> labwc/wlroots pixman`.

No runtime crate may require EGL initialization to display the panel or menus.
If the compositor itself later uses Grate/GPU acceleration, NuraLoumi remains unchanged.

## Compositor toplevel boundary

Foreign-toplevel integration obeys this dependency rule:

```text
nuraloumi-wayland
    owns Wayland globals, proxy handles, protocol lifetimes and compositor requests
    does not know menu rows or semantic navigation

nuraloumi-shell
    depends on Wayland + core
    converts typed toplevel snapshots into WindowEntry values
    converts validated window semantic actions back into typed Wayland requests

nuraloumi-core
    owns generic menu/action semantics
    knows neither Wayland protocol objects nor labwc

nuraloumi-providers
    owns non-compositor OS/service providers
    must not become a second Wayland protocol stack
```

`zwlr_foreign_toplevel_manager_v1` is the preferred list/control backend because
it provides state, activation, fullscreen and close requests. The staging
`ext_foreign_toplevel_list_v1` protocol is a list-only degraded fallback. Window
identity is always an opaque NuraLoumi `ToplevelId`; titles and app IDs are
presentation metadata and must never be used as control identity.

Foreign-toplevel property changes are committed to shell-visible snapshots only
at the protocol `done` boundary. A close request does not remove a window
optimistically; removal follows the compositor's `closed` event.

## Resource discipline

- single process is preferred for panel + transient menu surfaces in 0.1.
- providers may use subprocesses only behind bounded adapters.
- no polling faster than 1 Hz for battery/clock; event-driven when practical.
- redraw only damaged/changed surfaces.
- use fixed-size or bounded caches.
- animations are finite and stop scheduling frames when complete.
