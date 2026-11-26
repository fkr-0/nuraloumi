# NuraBrowser live Canary process/sandbox baseline (read-only)

Observation: 2026-11-26, approximately 01:28 CET, SSH `user@192.168.23.106`.
**NO-GO for production sandbox qualification.**
This inspects the pre-existing WPE 2.54.2 canary, **not** the new hash-pinned
Mesa26/LLVM23/Wayland26 + corrected crypto combined root. Nothing was restarted,
killed, installed or reconfigured on the device.

## Identity and runtime directory

- Tablet architecture: ARMv7 (`armv7l`). `user` is UID/GID **10000**.
- `/run/user/10000` exists, owned by `user:user`, mode `0700`,
  but exposes no direct child socket in this observation.
- Active compositor and application sockets are instead under
  **`/run/sl101-desktop-user`** (`wayland-1`, `sl101-desktop-bus`,
  `nuraloumi.sock`, `wayvncctl`, `pipewire-0`, etc.).
- UI, Web and NetworkProcess all inherited
  `XDG_RUNTIME_DIR=/run/sl101-desktop-user` and
  `WAYLAND_DISPLAY=wayland-1`, `GALLIUM_DRIVER=softpipe`,
  `LIBGL_ALWAYS_SOFTWARE=1`.
- This is **not** the intended conventional `/run/user/10000` socket
  contract. Do not silently change compositor/system activation while
  collecting baseline evidence. A reviewed socket migration or sandbox
  bind-policy is required.

## Observed live WPE process protection

Values below are from `/proc/<pid>/status` and
`/proc/<pid>/ns/{user,mnt,net,pid}`. PIDs are observations and will change
after a restart.

| Process | PID | PPid | NoNewPrivs | Seccomp | Filters | UID | Namespaces |
|---|---:|---:|---:|---:|---:|---:|---|
| UI `nurabrowser-core` | 14184 | 14182 | 1 | **0** | 0 | 10000 | Outer bwrap set |
| `WPENetworkProcess` | 14188 | 14184 | 1 | **0** | 0 | 10000 | **Same user/mnt/net/pid as UI** |
| `WPEWebProcess` | 14209 | 14202 | 1 | **2 (FILTER)** | **1** | 10000 | **Own user/mnt/net/pid** |

All three had `CapEff=0000000000000000`.
Outer UI/Network namespace inodes:
`user:[4026532614]`, `mnt:[4026532613]`,
`net:[4026531833]`, `pid:[4026531836]`.
Web namespace inodes:
`user:[4026532669]`, `mnt:[4026532622]`,
`net:[4026532625]`, `pid:[4026532624]`.
The WebProcess nested bwrap wrappers were PID 14201 and 14202;
WebProcess PPid=14202, NSpid was `14209 2`. Its Seccomp mode=2/filter=1
proves a filter is **loaded**, but no denied-syscall negative control has yet
proven that filter's effective policy. Its isolated network namespace differs
from the UI/Network namespace.

## Session bus/Wayland exposure

- UI and NetworkProcess:
  `DBUS_SESSION_BUS_ADDRESS=unix:path=/run/sl101-desktop-user/sl101-desktop-bus`
  (same direct full-session bus address).
- WebProcess:
  `DBUS_SESSION_BUS_ADDRESS=unix:path=/run/sl101-desktop-user/wpe/bus`.
  The `wpe/bus` socket is visible via
  `/proc/14209/root/run/sl101-desktop-user/wpe/bus` **inside the WebProcess
  mount namespace**, but not from the UI/Network host-visible path.
  `xdg-dbus-proxy` PID 14197 is alive, though this alone does not prove its
  allowed-name filter contents.
- The canary's outer UI bwrap uses `--ro-bind / /`, target-specific private
  library binds, and environment overrides. It is **not** the final minimal
  UI-/Web-/NetworkProcess mount and socket policy.

## Qualification verdict and next controls

- **WebProcess:** `NoNewPrivs`, one loaded Seccomp filter and distinct
  user/mount/network/PID namespaces are evidenced. Still require negative
  syscall tests, exact mount/socket allowlist and WebKit behavior tests.
- **NetworkProcess: NO-GO.** `Seccomp=0`, zero filters, shares the UI
  namespaces and direct D-Bus session bus. It needs an intentional, separate
  bwrap/namespace/seccomp/socket policy whose networking remains functional
  under negative tests.
- **Runtime path: NO-GO for conventional state.** Plan explicit
  `/run/user/10000` integration and Wayland/D-Bus proxy bind policy.
- **Browser release: NO-GO.** This baseline does not qualify the new D16
  combined root, native page pixels, touch, soak, clean exit or rollback.
  Do not promote or replace the running productive/test device as a result.

All checks were read-only; the existing canary/compositor and productive
NuraLoumi process were left running.
