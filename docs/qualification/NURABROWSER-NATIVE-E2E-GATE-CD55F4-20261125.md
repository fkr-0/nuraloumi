# NuraBrowser native E2E hardware gate — CD55F4

Date: 2026-11-25. OCP: `NURALOUMI-20261125-NURABROWSER-NATIVE-E2E-HARDWARE-20261125-CD55F4`.
Checkout at inspection: `af2ac55fa57ee963034c33c6019cf6f319c3ad95` (`main`), initially clean.

## Decision

**BLOCKED for new native/device exercises.** The prerequisite private ARMv7/musl D16 runtime and NetworkProcess sandbox are not qualified. Preserve `/opt/nuraloumi/current`, the working private canary, Sway and WayVNC unchanged. No browser promotion, live swap, restart, reboot, or device state mutation was performed in this task.

The evidence below is historical evidence from `NURABROWSER-LIVE-WAYVNC-20261125.md`, **not a newly executed hardware run**. That report records private runtime inventory JSON SHA256 `7e11aed1c9d2a8ed623fabc34ecd40b43e04dfb388595250bfc7f56aa7fd34a3` with 30 findings, including missing private LLVM/DRM closure and non-D16 crypto attributes; live NetworkProcess had `Seccomp=0` and inherited broad mount visibility. Real maps and source SHA information appear in that parent report.

## Evidence and gate matrix

| Gate | Result | Attribution / evidence needed next |
| --- | --- | --- |
| Existing WPE fixture page rendered in WayVNC | PASS (prior run only) | 1280×800 capture and purple/green oracle in parent report; distinguish actual pixel evidence from new browser chrome. |
| Clicking fixture `Open second page` | PASS (prior run only) | `GET /index.html`, `GET /next.html` HTTP 200 and WPE `load-finished (failed=no)` in parent device logs. Input origin was not isolated. |
| Existing isolated engine clean exit | PASS (prior run only) | `engine-exit=0`, no remaining WPE helpers, Sway/WayVNC persisted, per parent report. Not proof of crash recovery. |
| Private loader, D16 ISA, full ELF/plugin/maps closure | FAIL qualification / BLOCKED | 30 runtime inventory findings. Require complete immutable staged candidate, source-verified replacements, interpreter and loaded-map/inode proofs. |
| NetworkProcess namespace, socket, D-Bus, seccomp negative policy | BLOCKED | Broad root and desktop-runtime visibility; `NoNewPrivs=1` is insufficient. Final negative sandbox fixtures and device proof are prerequisite. |
| Native browser chrome tabs, activation, close/focus | NOT TESTED | Need actual independent WPEWebViews and per-tab process/surface/state logs; fake-WebKit host IPC does not prove this. |
| Back/forward, typed address, HTTP/HTTPS success | NOT TESTED | Require each URL/result with navigation events, TLS result, frame oracle, and tab identity. |
| Invalid cert, unreachable host, network failure UI | NOT TESTED | Isolate TLS/network failures from IPC/UI/sandbox errors with WPE and NetworkProcess logs. |
| Keyboard/pointer via WayVNC | NOT TESTED in this task | Pair event source with screenshot and Wayland seat logs. Prior user click lacks input provenance. |
| Physical `wl_touch`, taps/gestures | NOT TESTED | Capture `wl_touch`/libinput source and touch ID tied to expected browser action; VNC pointer is a distinct gate. |
| RSS, thermals, frame latency, bounded soak | NOT TESTED | After both prerequisite gates pass, record monotonic elapsed samples, per-process PID/RSS, thermal sensor path/value and p95 interaction latency. Predeclare soak duration and stop thresholds before device run. |
| Child start, clean exit, zombies, sockets | PASS (prior narrow exit only) / NOT TESTED for full chrome | Recheck UI/Web/Network PIDs, exit codes, wait status and socket inventory per scenario. |
| Supervised crash recovery and compositor survival | NOT TESTED | Explicit isolated fault injection only after qualified staging/rollback and compositor PID baseline; do not kill normal release. |

## Host-only verification in this task

`python3 -m unittest scripts.test_inventory_nurabrowser_live_runtime scripts.test_verify_nurabrowser_runtime scripts.test_qualify_browser_nested_wayland` → **33 tests passed**, exit 0, approximately 0.415 s test runtime. These validate host fixtures/logic, not native pages, TLS, physical input or compositor pixels.

## Safe resumption contract

1. Obtain independently reviewed PASS receipts for a **separately staged**, complete ARMv7/musl D16 runtime and the final NetworkProcess sandbox negative controls. Do not mutate the known-good canary to achieve either prerequisite.
2. Record hashes/paths for immutable candidate, current release and rollback. Supervise exactly one isolated UI/Web/Network run on the designated target; record timestamps, PID identities and `/proc` namespace/map/mountinfo, then demonstrate successful clean exit and unchanged compositor.
3. Capture before/after WayVNC frames and corresponding WPE navigation/load/error logs for each fixture, tab and HTTPS case. Do not mark a case PASS from either pixels or logs alone.
4. Separate VNC pointer/keyboard input from physical touchscreen `wl_touch`. Only physical source-linked input counts for touch PASS.
5. Collect bounded memory/thermal/latency soak plus crash-recovery evidence; specify abort thresholds before test start. No promotion follows automatically from a Canary PASS.

No live device command was issued during this gate assessment.
