@projmgrauth
Set repo: nuraloumi

# NuraLoumi Wave 2 — IPC Socket and nuraloumi-ctl (M7)

Lane: ipc-socket
Priority: 94

## Mission

Add a Unix domain socket IPC interface so external scripts, status bars, and
automation tools can push data into NuraLoumi surfaces and query their state.
Ship a `nuraloumi-ctl` CLI client binary.

Read:
- AGENTS.md, DESIGN.md, ROADMAP.md (M7 section)
- docs/ARCHITECTURE.md, docs/CONTRACTS.md
- crates/nuraloumi-shell/src/lib.rs (ShellConfig, surface lifecycle)
- crates/nuraloumi-core/src/model.rs (MenuModel, MenuItem, MenuAction)

## Exclusive write scope

- crates/nuraloumi-shell/src/ipc.rs (new)
- crates/nuraloumi-shell/src/bin/ctl.rs (new — nuraloumi-ctl binary)
- crates/nuraloumi-shell/Cargo.toml (add [[bin]] entry for ctl)

Do not edit rendering, Wayland backend, providers, or core model.
Do not modify existing binary entry points in this task — IPC server
integration into panel/menu binaries is a follow-up.

## Protocol design

### Transport
- Unix domain socket at `$XDG_RUNTIME_DIR/nuraloumi.sock`.
- Fallback: `/tmp/nuraloumi-$UID.sock` if XDG_RUNTIME_DIR is unset.
- Line-delimited JSON: one request object per line, one response per line.
- Connection lifecycle: connect, send request, read response, disconnect
  (no persistent connections in v1).

### Request/response envelope
```json
{"id": "req-1", "method": "push-item", "params": {...}}
{"id": "req-1", "ok": true, "result": {...}}
{"id": "req-1", "ok": false, "error": {"code": "invalid_surface", "message": "..."}}
```

### Methods

1. **push-item** — inject a temporary menu item into a named surface.
   Params: `surface` (string), `label` (string), `glyph` (optional string),
   `subtitle` (optional string), `ttl_seconds` (optional, default 300).
   Pushed items expire after TTL. Max 64 pushed items per surface.

2. **set-status** — update a named status slot in the panel strip.
   Params: `slot` (string), `text` (string), `glyph` (optional string).

3. **notify** — emit a notification through the notification provider.
   Params: `summary` (string), `body` (optional string), `app` (optional string).

4. **surface-state** — query whether a surface is open, its current selection.
   Params: `surface` (string).
   Returns: `open` (bool), `selected_item` (optional string), `item_count` (int).

5. **navigate** — send a semantic navigation message to the focused surface.
   Params: `action` (string: "open", "close", "up", "down", "select", "back").

6. **reload-config** — trigger a config file re-read without restarting.
   Params: none.

7. **list-surfaces** — enumerate registered surfaces and their state.
   Returns: array of `{name, open, pid}`.

8. **ping** — health check. Returns `{version, uptime_seconds}`.

### Bounds
- Max 4 KiB per request message.
- 1-second timeout per request processing.
- Max 64 pushed items per surface; oldest evicted when full.
- Socket permission: owner-only (0o600) by default.

## nuraloumi-ctl binary

Thin CLI client wrapping the IPC protocol:

```
nuraloumi-ctl push-item --surface panel --label "VPN: up" --glyph ""
nuraloumi-ctl set-status --slot vpn --text "Connected"
nuraloumi-ctl notify --summary "Build passed" --body "All 42 tests green"
nuraloumi-ctl surface-state --surface launcher
nuraloumi-ctl navigate --action close
nuraloumi-ctl reload-config
nuraloumi-ctl list-surfaces
nuraloumi-ctl ping
```

Exit codes: 0 success, 1 error (prints JSON error to stderr), 2 connection failed.
`--json` flag outputs raw JSON response.
`--socket PATH` overrides the default socket path.

## IPC server module (ipc.rs)

The IPC server module provides:
- `IpcServer::bind(path)` — creates and binds the socket.
- `IpcServer::accept_one()` — accepts a connection, reads request, returns
  parsed `IpcRequest`.
- `IpcServer::respond(response)` — writes JSON response and closes.
- `IpcServer::cleanup()` — removes the socket file.

The server is non-blocking and designed to be polled from the surface's event
loop alongside Wayland fd polling. It must not block the rendering thread.

Surface binaries will integrate the IPC server in a follow-up by calling
`IpcServer::bind()` at startup and polling `accept_one()` in their event loop.
This task delivers the server module, protocol types, and the client binary.

## Constraints
- No new system dependencies beyond std Unix socket (already in std).
- ARMv7/musl cross-build must pass.
- Serde JSON for serialization (already a workspace dependency).
- No async runtime — use std::os::unix::net::UnixListener in non-blocking mode.
- The IPC module must be independently testable with unit tests (bind to a
  temp path, send request, verify response).

## Acceptance
1. `nuraloumi-ctl ping` connects to a test server and gets a response.
2. All 8 methods have request/response types with serde derive.
3. IPC server module has unit tests covering each method's parse and response.
4. `nuraloumi-ctl --help` documents all subcommands.
5. `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` pass.
6. Cross-build for armv7-unknown-linux-musleabihf passes ELF audit.
