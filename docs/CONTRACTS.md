# Cross-crate contracts

These contracts are frozen for the first parallel implementation wave. Agents may add fields/types compatibly but must not silently redefine another lane's boundary.

## nuraloumi-core

Must expose renderer-neutral equivalents of:

```rust
pub struct MenuModel { pub id: String, pub title: String, pub items: Vec<MenuItem> }
pub struct MenuItem { pub id: String, pub label: String, pub subtitle: Option<String>, pub kind: MenuItemKind, pub enabled: bool, pub action: Option<MenuAction>, pub children: Vec<MenuItem> }
pub enum MenuItemKind { Action, Submenu, Checkable, Section, Status, Separator }
pub enum MenuAction { Activate(String), Toggle(String), Adjust { id: String, delta: i32 }, Navigate(String), Confirm(Box<MenuAction>), Close, Custom { kind: String, payload: String } }
pub struct MenuState { pub selected_id: Option<String>, pub path: Vec<String>, pub query: String }
pub enum SemanticInput { Up, Down, Left, Right, Activate, Back, Text(String), Backspace }
```

Exact representation may be improved, but stable IDs, typed actions, non-actionable rows, deterministic selection, validation, and serde fixtures are mandatory.

Core also owns:
- design tokens;
- motion/easing primitives;
- semantic navigation helpers;
- model validation limits.

It must not depend on Cairo, Wayland, Iced, X11 or Tokio.

## nuraloumi-render-cairo

Input:
- immutable core model/state;
- viewport { width, height, scale };
- theme/tokens.

Output:
- deterministic Scene/Layout containing paint nodes and HitRegion values;
- render-to-Cairo entry point;
- render-to-PNG fixture helper.

It does not invoke actions or talk to Wayland.

## nuraloumi-wayland

Owns:
- Wayland connection/registry;
- shm buffer lifecycle;
- layer-shell/window role;
- outputs/scales/transforms;
- seats and normalized input.

Expose a small event loop API around:

```rust
pub enum PlatformEvent {
    Configure { width: u32, height: u32, scale: i32 },
    PointerMove { x: f64, y: f64 },
    PointerButton { x: f64, y: f64, pressed: bool, button: u32 },
    TouchDown { id: i32, x: f64, y: f64 },
    TouchMotion { id: i32, x: f64, y: f64 },
    TouchUp { id: i32 },
    Key { key: Key, pressed: bool },
    Close,
}
```

It must allow a client to submit ARGB/XRGB software buffers without EGL.

## nuraloumi-providers

Expose immutable snapshots plus explicit actions. Every backend has a fixture implementation.

```rust
pub trait Provider {
    type Snapshot;
    fn snapshot(&mut self) -> Result<Self::Snapshot, ProviderError>;
}

pub trait ActionProvider<A> {
    fn execute(&mut self, action: A) -> Result<ActionResult, ProviderError>;
}
```

The provider crate is deliberately below the semantic menu layer and has no
runtime dependency on `nuraloumi-core`. Mapping `MenuAction` values to
provider-specific action enums is a shell responsibility.

Do not let shell/render code construct shell commands.

## nuraloumi-shell

Owns:
- surface open/close/navigation state;
- conversion of PlatformEvent to SemanticInput/hit activation;
- provider snapshot -> MenuModel adapters;
- binaries and CLI/config.

It must support a fixture/demo mode requiring no real system services.

## nuraloumi-xtask

Owns build/packaging/qualification only. It must not become runtime authority.

## Foreign-toplevel/window-control contract

The compositor integration boundary is strict:

- `nuraloumi-wayland` owns foreign-toplevel protocol discovery, generated proxy
  handles, coherent protocol state, opaque `ToplevelId` allocation and typed
  activate/fullscreen/close requests. It must not depend on menu models.
- `nuraloumi-shell` is the only runtime crate allowed to map between typed
  toplevel snapshots and `WindowEntry`/semantic window actions.
- `nuraloumi-core` remains compositor-agnostic. It may carry generic/custom
  semantic actions but no Wayland/labwc types.
- `nuraloumi-providers` must not implement a second Wayland stack. Process/task
  inspection remains a separate provider concern and must not require a PID ↔
  window relationship.

Protocol policy:

1. prefer `zwlr_foreign_toplevel_manager_v1` for list + state + control;
2. fall back to `ext_foreign_toplevel_list_v1` for list/title/app-id only;
3. expose explicit capability flags so shell actions fail closed when control is
   unavailable;
4. never identify a window by title or app ID; use opaque non-reused
   `ToplevelId` values;
5. apply property updates only at protocol `done`;
6. treat close as a request and wait for compositor `closed` before emitting
   removal.
