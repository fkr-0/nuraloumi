# SL101 notification banner qualification — 2026-11-25

## Scope and baseline

- OCP: `NURALOUMI-20261125-NURALOUMI-TOAST-HISTORY-BANNER-20261125-82259E`.
- Start HEAD: `65ddad6ad61d737167238b2b04fe5524e3bb1402` (includes `95bb56bc20033cb9db6b2d1570af18598d86dcfe`).
- Host-only implementation. No SL101 deployment or physical visibility claim.

## Reproduction and root cause

`SurfaceCatalog::discover` registers an executable `nuraloumi-notifications` as `InstalledBinary`, whose `open_state` is *always* `Unknown` even when its window is not running. The old `notification_reply` returned `center_state_unknown` **before spawning** `nuraloumi-toast`. A separately persisted notification-history row says nothing about the IPC banner outcome. The deterministic `explicit_notify_unknown_installed_center_does_not_suppress_banner` test creates the registered-binary fixture and verifies that notification dispatch invokes the launch callback. Prior behavior rejected with `center_state_unknown` and no launch.

The new gate suppresses banners only for an **observed** `Open` `notifications` or `control-center` surface. `Unknown` is neither proof of open nor proof of closed. When another independently running notification center is truly open but unobservable to the panel, simultaneous display remains possible; a future accurate open-state observer may refine this policy without installation-based suppression.

## Distinguish stages

1. *History persisted*: provider/daemon history snapshot (not an IPC reply).
2. *IPC accepted*: `accepted=true`, `presentation=one_shot_started_unverified` means only that OS spawning succeeded.
3. *Child started*: panel stderr says `toast child spawned pid=...`.
4. *Wayland committed*: toast child stderr says `first Wayland buffer committed`; child stderr is inherited by panel.
5. *Physically visible*: requires human/WayVNC observation on the actual compositor/output; no host process or protocol log proves pixels are visible.
6. *Exited*: panel logs the exit code without message content. A nonzero exit before commit points to Wayland/session/config failure. No confidential notification summary/body is logged by these new diagnostics.

Only one toast child is active at once. An overlapping arrival receives `toast_busy`, not an unbounded queue. Absent executable receives `toast_unavailable`; spawn failures receive `toast_spawn_failed`. A closed/open control-center gate remains covered by the fixture. The one-shot model bounds TTL at 500–10000 ms and supports Escape/Enter/Space, pointer release and touch release to dismiss; touch cancellation/leave invalidates the gesture. Overlay input consumption/ghost-click prevention remains a **physical compositor gate**.

## Host verification

Run `cargo test -p nuraloumi-shell --lib surface_catalog` and `cargo test -p nuraloumi-shell --bin nuraloumi-toast`; capture exact exit codes separately. Also review `git diff --check` and only changed source paths.

Observed host results (no live Wayland or notification-history daemon fixture):

- `cargo test -p nuraloumi-shell --lib surface_catalog`: exit 0, 11 passed, 0 failed.
- `cargo test -p nuraloumi-shell --bin nuraloumi-toast`: exit 0, 11 passed, 0 failed.
- `git diff --check`: exit 0 at verification time.

The first test exercises the prior pre-spawn suppression and callback invocation. It does **not** persist a history entry: history is managed independently by the notification provider/daemon, so the full history-plus-banner E2E remains unverified here. Missing-executable, busy, open-center and injected spawn-error responses have fixture coverage. Real child startup errors, Wayland failure, DND transport suppression and physical ghost-click behavior require the manual canary.

## Safe local/SL101 canary (manual only, not performed)

Use the already-running session, its *own* runtime socket and WAYLAND_DISPLAY; do not launch an additional compositor or change `/opt/nuraloumi/current`. Verify the installed `nuraloumi-toast` sibling is executable and shares the panel's Wayland/session environment. From that same user session, send a harmless `nuraloumi-ctl notify --summary 'Banner canary'` request, inspect the response and local panel journal/stderr. Do not include sensitive payloads. Watch that a small floating banner appears below the top bar on the intended 1024×600 output, can be dismissed by touch and keyboard, and never activates an underlying launcher row. Repeat with control center closed and open, overlapping arrivals, and DND active; record whether DND is enforced by the **upstream notification transport** (this IPC path does not itself query DND). Check the history separately in the notification center.

If the IPC returns `accepted=true` but no pixels appear, inspect compositor layer-shell configuration, actual output selection/scale, panel exclusive zone, child inherited `WAYLAND_DISPLAY`/`XDG_RUNTIME_DIR`, stderr exit reason and first-commit log; stop at the first unverified stage. A first-commit log is still not visible-pixel proof. No automatic deployment or PASS claim is authorized by this host result.
