# NuraBrowser R2 — Gate 4 supervisor integration

OCP: NURALOUMI-20261119-NURABROWSER-R2-SUPERVISOR-INTEGRATION-20261119-ACD56D

## Evidence and status

- PASS (exit 0): cargo test -p nuraloumi-shell --test browser_supervisor_lifecycle — 5/5 fake-process/session tests.
- PASS (exit 0): cargo clippy -p nuraloumi-shell --test browser_supervisor_lifecycle -- -D warnings.
- PENDING: production browser executable wiring, WPE process readiness, real process-tree/helper reaping, actual multi-tab replay against WebKit, compositor/pixel evidence and SL101 qualification.
- No remote CI, Git push/fetch, device or live compositor mutation.

## Implemented isolated contract

The new browser/supervisor.rs provides Supervisor::new(binary, args), start(), poll(readiness_probe), state(), stop(), and configure(). States are Stopped, Starting, Ready, Backoff and Failed(reason). Retry count is bounded (default two retries). Child runs in its own process group. Stop sends SIGTERM then bounded SIGKILL and waits for the direct child. A failed readiness probe is not represented as Ready.

Session { tabs, active } validates 1..=6 HTTP(S)/about:blank entries, bounds length, rejects controls, atomically saves in an owner-private mode-0700 directory with mode-0600 file, fsync and rename; load rejects malformed/oversized snapshots.

## Tabstrip-worker integration handoff (no cross-claim changes)

1. Add pub mod supervisor to browser/mod.rs after its active claim is released; wire browser.rs to create Supervisor using a fixed, packaged libexec path, no PATH lookup or arbitrary command from UI/URL.
2. In the event loop, call supervisor.poll with an authenticated NativeBrowser::request("tabs", None) round-trip (not socket existence). Ensure the socket belongs to the current child generation; a stale same-UID socket otherwise can give false readiness.
3. Surface EngineState::Starting/Backoff/Failed in BrowserUi.status. Never fall back to offline fixture on a failed real start.
4. On every successful bounded tabs snapshot, save Session; restore by issuing open/tab-new/tab-select sequentially after readiness. The current IPC has no atomic restore or per-tab open-URI operation. Add a typed engine-side restore RPC with verified six-tab ordering, active selection, and idempotency before claiming production multitab resume.
5. On shutdown, terminate the process group and verify WebProcess/bus-proxy descendants are gone. Current host tests exercise the direct child only; on real WPE, helper cleanup and the post-exit process-group behavior require explicit E2E validation.
6. Preserve existing private runtime/ARMHF/sandbox gates. Live WPE/sway and target device remain PENDING.

Gate 4 cannot be declared closed until the integration worker wires and verifies these seams. The component is intentionally not imported into the production module while the Tabstrip worker owns mod.rs and bin/browser.rs.
