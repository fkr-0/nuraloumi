# NuraBrowser WPE 2.54.2 — real WayVNC navigation and process boundary

Date: 2026-11-25. Target: owner-designated SL101 Nura, `armv7l`, postmarketOS/musl. This is an **isolated, bounded canary**, not a regular browser release or a production activation.

## Outcome

| Gate | Result and independently observed evidence |
| --- | --- |
| Existing desktop | Sway PID 1444 and WayVNC PID 1480 survived each bounded run. |
| Visible page | WPE rendered the blue/purple local `index.html` on the real Wayland display, captured over an SSH-tunneled WayVNC session. |
| User-activated navigation | **PASS:** browser log has `load-finished (failed=no)` at 20:23:07 and 20:23:20 local time; fixture HTTP log has `GET /index.html` and `GET /next.html`, both HTTP 200. |
| Visible next page | **PASS:** captured 1280×800 WayVNC frame contained 950,295 green pixels, zero purple pixels; the `next` color oracle passed. The earlier capture in that recording was already green because the user had clicked before the automated RFB pointer probe. |
| Process termination | **PASS:** bounded run returned `engine-exit=0`; a later process inventory found no `nurabrowser-core`, `WPEWebProcess` or `WPENetworkProcess`. |
| Physical touchscreen | **PENDING:** the owner confirmed pressing the link; the input source (actual `wl_touch` versus VNC pointer) has not yet been established. Do not claim `wl_touch` qualification. |

The successful user-navigation run produced device evidence at:

`/home/user/wpe2542-canary-20261124/results-live-wayvnc-d16-20261125-cEkCda`

Both `engine.log` and `http.log` are retained there. The diagnostic WayVNC `next` frame passed `scripts/sl101-mesa-d16-pixel-oracle.py --page next` with `green_pixels=950295`, `purple_pixels=0`, `pixel_oracle_pass=true`.

The canary used private WPE/WebProcess/NetworkProcess binaries, private musl loader and ICU 76 data, pinned Mesa 26.2.4 D16/softpipe components, and the existing `/run/sl101-desktop-user/wayland-1` socket. The normal `/opt/nuraloumi/current` target was not changed by the browser test. The specific active release was the separate, already-installed video hotfix, not the outdated software-only path cited by earlier preflight documents.

## Live subprocess boundary inspection

A second bounded live run retained logs in `results-live-wayvnc-d16-20261125-bLhihL` and returned `engine-exit=0`. On-device `/proc` inspections were made while all three processes were running; the namespace inodes were read via `sudo readlink` and the seccomp status directly from each process.

| Process / observed PID | `NoNewPrivs` | `Seccomp` / filters | Namespace relationship |
| --- | ---: | ---: | --- |
| UI `nurabrowser-core` / 4988 | 1 | 0 / 0 | Outer private mount/user namespace; shares network/PID/UTS with NetworkProcess. |
| `WPENetworkProcess` / 4992 | 1 | 0 / 0 | Same mount, network, PID, user, UTS and IPC namespace inodes as UI. |
| `WPEWebProcess` / 4999 | 1 | 2 / 1 | Distinct mount `4026532616`, net `4026532619`, PID `4026532618`, user `4026532663`, UTS `4026532617` from UI; shared IPC `4026531839`. |

NetworkProcess's missing seccomp filter is **not** evidence that WebKit's WebProcess sandbox failed. WPE WebKit's GLib Bubblewrap launcher deliberately considers the NetworkProcess trusted and spawns it without the inner Bubblewrap sandbox. In this canary it inherits the outer mount namespace, which read-only binds the entire host root and bind-mounts the whole conventional desktop runtime directory. This is too broad for a qualified production NetworkProcess filesystem/socket policy. `NoNewPrivs=1` alone is not a seccomp filter.

## Remaining release blockers

1. Construct a source-matched, complete ARMv7/musl loader/ICU/ELF/plugin/dlopen closure and independently reviewed `runtime.json`. This private canary remains **UNQUALIFIED** and must not be promoted into the normal release.
2. Narrow UI/NetworkProcess file and socket visibility; reserve outbound networking for NetworkProcess without exposing the entire host root or desktop session bus. Test D-Bus restrictions through a controlled `xdg-dbus-proxy` policy and negative controls.
3. Repeat the WebProcess namespace/seccomp proof under the final installed layout; test syscall denial and refusal when the required sandbox helper is unavailable. The present live inspection proves a filter was loaded, not its full behavioral policy.
4. Exercise the actual Rust browser chrome (tabs, history, address navigation), keyboard, physical touchscreen `wl_touch`, cancellation and recovery. The local two-page click proves visible WebKit navigation only.
5. Preserve the exact active release and rollback, and run full installer/activation and post-activation health gates before promoting a browser binary.

Verdict: **live rendering and link navigation PASS; WebProcess sandbox presence PASS; NetworkProcess policy, native browser shell, physical touch and production package PENDING.**
