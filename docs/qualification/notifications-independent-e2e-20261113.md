# Notifications and Notification Center — independent acceptance (2026-11-13)

- **OCP authority:** `NURALOUMI-03-NOTIFICATIONS-INDEPENDENT-E2E-20261113-E7A388`
- **Role:** independent review of lanes `NURALOUMI-01-NOTIFICATIONS-BACKEND-E2E-20261113-0C7114` and `NURALOUMI-02-NOTIFICATION-CENTER-UI-20261113-307117`.
- **Baseline:** `df6182b5dc6014b8001b0db480686a6f9cc0ba2e` (2026-11-13). Independent clean checkout branch: `review/nuraloumi-notifications-e2e-20261113`.
- **Overall acceptance:** **BLOCKED / NOT RELEASE QUALIFIED**. This is an independent *baseline* audit, not a verdict on unfinished implementation commits. Do not label fixture screenshots or source tests as physical E2E.
- **Mutations:** no implementation changes, no panel/daemon/session replacement, no Sway/Mesa changes, no reboot/flash/suspend. This report is the only intended tracked write.

## Evidence and provenance

The source repository `/home/user/code/nuraloumi` was dirty at review start (uncommitted `crates/nuraloumi-shell/src/bin/{panel.rs}`, `src/lib.rs`, feed fixture Cargo files, and unrelated untracked docs/prompts); it was not reset or reformatted. Separate managed worktree: `/home/user/.ws-bridge/worktrees/nuraloumi-a0909801`. Main's active UX workflow claim covers panel and lib and was respected.

The two notification OCP implementation tasks were **running**, not terminal, at initial review. Thus the source at this baseline still contains the `dunstctl` provider, rather than a verified owned `org.freedesktop.Notifications` bus endpoint. This cannot establish the sender → session bus → provider → surface → action/dismiss/closed signal chain.

| Check | Scope/command | Result | Meaning |
| --- | --- | --- | --- |
| Baseline clean formatting | `cargo fmt --check` | **PASS** | Clean worktree, not dirty main |
| Clean workspace tests | `cargo test --workspace` | **PASS** | Existing host regression baseline |
| Clean strict Clippy | `cargo clippy --workspace --all-targets -- -D warnings` | **PASS** | Existing committed source only |
| Targeted provider | `cargo test -p nuraloumi-providers notifications -- --nocapture` | **PASS**, 5 notification tests | Includes forged action, JSON decode, missing dunstctl, exact argv; mocked command fixture, *not* DBus |
| Notification surface | `cargo test -p nuraloumi-shell --bin nuraloumi-notifications` | **PASS**, 1 test | Model/truncation only; not UI input E2E |
| Shell notification references | `cargo test -p nuraloumi-shell notification` | **PASS**, 3 matching tests | Control-center model, timer behavior; no real notification bus |
| One-shot toast | `cargo test -p nuraloumi-shell --bin nuraloumi-toast` | **PASS**, 11 tests | Noteworthy release-on-same-target and second-finger checks *for toast*, not center |
| Dirty main formatting | `cargo fmt --check` | **FAIL** at uncommitted `panel.rs:84-89` | Belongs to active UX owner; clean branch passes |
| Dirty main strict Clippy | `cargo clippy --workspace --all-targets -- -D warnings` | **FAIL**, `panel.rs:72-73` | `#[test]` embedded in inner scope, `unnameable_test_items` and `dead_code`; owner informed |
| Dirty main workspace test | `cargo test --workspace` | **PASS** | It does **not** discover the nested panel test |
| `cargo test -p nuraloumi-shell notify` | Pattern-filtered | **NO COVERAGE** (zero matching tests) | Not counted as a security pass; rerun with `notification` above |

Raw host logs (ignored build evidence, not committed) from the isolated checkout:
`target/notifications-e2e-20261113/{fmt.log,workspace.log,clippy.log,target-provider.log,target-ui.log,notification-security.log,toast.log}`.
Dirty main logs: `target/qualification-notifications-20261113/{summary.log,workspace-test.log,clippy.log}`.
`CARGO_TARGET_DIR=/home/user/code/nuraloumi/target` reused host compilation products; source crate paths in the clean logs resolve into the independent worktree.

## Findings requiring owner response

| Severity | Result | Location | Reproduction / impact / required remediation |
| --- | --- | --- | --- |
| **P0 (interaction)** | **FAIL** | `crates/nuraloumi-shell/src/bin/notifications.rs:424-430` | Current center sends `requested=hit(...)` on `PointerButton {pressed: true}` and `TouchDown`; actions can fire *on press*, regardless of later drag/cancel/outside release. Violates `DESIGN.md` release-inside-hit-region contract. A confirm-clear action may fire on touch-down. Track pointer button/contact ID, pressed origin, motion/cancel, and activate on matching release only, rejecting unrelated second contacts. Add positive/negative tests. |
| **P1 (action target)** | **FAIL / unqualified race** | `crates/nuraloumi-providers/src/notifications.rs:157-166` | `dunstctl history-pop ID` followed by `dunstctl action 0 ACTION` is two calls; another arriving notification may become topmost between them, changing target. Unit test `invoke_validates_history_id_and_action_before_two_step_dunst_call` only verifies fixture argv. Do not claim atomic targeting. Prefer D-Bus action signal keyed to immutable notification ID / daemon-owned atomic state. |
| **P1 (delivery)** | **FAIL at baseline** | `crates/nuraloumi-providers/src/notifications.rs:85-114`, `crates/nuraloumi-shell/src/bin/notifications.rs:284-289,327-329,449-455` | Snapshot pulls dunst history at startup and explicitly on refresh; center has no live arrival subscription. Verify bus service ownership, notification arrived/updated/closed events, callback or bounded UI invalidation and visual state updates from incoming sender without manual F5. |
| **P1 (error handling)** | **FAIL at baseline** | `crates/nuraloumi-shell/src/bin/notifications.rs:245-280,445-446,487-501` | Provider action failure returns `Err` through `run()` and terminates surface; no persistent user-visible feedback/retry state. Add fail-closed in-surface error treatment; test unavailable service, action races and invalid IDs. |
| **P1 (daemon ownership)** | **BLOCKED** | `crates/nuraloumi-providers/src/notifications.rs`; new lane 01 unmerged | No source evidence on baseline for `org.freedesktop.Notifications` ownership, competing dunst/mako handling, or `Notify/GetCapabilities/GetServerInformation/CloseNotification` and `ActionInvoked/NotificationClosed` contract. Verify isolated session-bus roundtrip, explicit loss-of-name behavior and no dual owner. |
| **P1 (state contract)** | **BLOCKED** | Provider model `crates/nuraloumi-providers/src/notifications.rs:11-27` | Existing entry lacks explicit unread/read, urgency, expiry and closed reason. Check new lane implements bounded (64) history, replacement IDs, expiration, mark-read, action+dismiss, persistence policy, no stale selection/duplicate notifications. |
| **P2 (text limits)** | **PARTIAL** | `crates/nuraloumi-shell/src/bin/notifications.rs:159-168,195-216,418-420`, provider `:255-308` | Summary/body bounds and Unicode surrogate fixture pass. Search query input has no visible cap; action label/payload can carry untrusted display text. Check overlong input, Unicode combining characters, RTL, malformed markup, control/bidi characters and markup escaping; no shell interpolation should occur. Cairo text path does not establish that all DBus XML/markup consumers escape appropriately. |
| **P2 (surface responsiveness)** | **BLOCKED** | Center `crates/nuraloumi-shell/src/bin/notifications.rs:327-484` | No current on-device memory/frame-time samples, 64-notification fixture is not a load test. Check 1-GiB Tegra20 64 entries × max actions, redraw latency, scroll/key/touch parity and absence of idle wakeups. |

**Important:** For the current one-shot toast *only*, `nuraloumi-shell/src/toast.rs` plus `bin/toast.rs` have explicit bounded lifetime, Unicode truncation, release/cancel/multitouch tests; this does not cure the independent center's press-down handler.

## Device read-only safety preflight (.106)

A single non-mutating SSH command ran with `BatchMode=yes`, 5-second connect timeout and strict host-key verification. Observed:

- Host `sl101-nura`; device-tree model `ASUS Eee Pad Slider SL101`.
- Kernel `7.0.1-postmarketos-grate-dmavde1`.
- AC power-supply `ac-adapter-detect`: `Mains`, `online=1`; battery `sbs-5-000b` reports `Full`.
- Active `/usr/local/bin/sl101-sway` PID 1436, `wayvnc` PID 1485, `swaybar` PID 1781, live `/opt/nuraloumi/current/nuraloumi-panel --live --enable-renderer-switch` PID 1792.
- Default user-bus socket `/run/user/$(id -u)/bus` was **absent** for the SSH account. This does not exclude a nondefault session bus but blocks treating this SSH context as a proven DBus integration environment.

**Hardware E2E: BLOCKED / NOT ATTEMPTED.** No new notification backend binary was deployed on the tablet, the existing panel and Sway were active, and an owner-confirmed safe bus/test seat was unavailable. No injection, screenshot, VNC input, touch replay, panel/daemon restart or process kill was attempted. The SSH preflight is hardware *identity/safety context*, not a notification E2E PASS.

## Acceptance matrix for lanes 01 and 02

| Required behavior | Grade at this baseline | Exact follow-up proof |
| --- | --- | --- |
| Unique `org.freedesktop.Notifications` owner; no competing daemon | **BLOCKED** | Isolated bus well-known-name ownership and rejection/fallback when dunst/mako owns name |
| Sender → bus → provider → center/toast | **BLOCKED** | Independent `dbus-send` or `notify-send` roundtrip + receiver observations |
| Updating/replacing notification and sender/ID isolation | **BLOCKED** | Two senders, replacement ID, close reason and edge/error cases |
| History cap, unread, TTL, expiration | **BLOCKED** | Monotonic fake clock tests plus durable user-visible UI observation |
| Invoke/dismiss actions | **FAIL on baseline**, new lane **BLOCKED** | Immutable ID/action handling and matched NotificationClosed/ActionInvoked signals; race test |
| Keyboard, pointer, touch, multitouch, release/cancel | **FAIL on baseline** | Center event simulation + real SL101 touch and slider keyboard proof |
| Unicode / markup / shell injection | **PARTIAL** | Adversarial tagged payload, surrogate/RTL/combining chars, bounded length, escaped renderer output |
| Error containment / degraded provider | **FAIL on baseline** | Missing daemon and bus disconnect leave UI operable and report status |
| Low memory, first frame and idle CPU | **BLOCKED** | Device RSS/latency/CPU measurements and pixel evidence, no production restart |
| Clean baseline host tests / Clippy / fmt | **PASS** | Rerun against exact *implementation merge* SHA, not previous baseline |
| Currently dirty integration main fmt/Clippy | **FAIL** | Panel owner corrects formatting and nested `#[test]`; retest at a fixed SHA |

## Reproduction commands (safe host-only)

Run on a clean, documented Git revision; capture `git rev-parse HEAD`, `git status --porcelain`, stdout/stderr, exit codes and test counts:

```sh
cargo fmt --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p nuraloumi-providers notifications -- --nocapture
cargo test -p nuraloumi-shell --bin nuraloumi-notifications
cargo test -p nuraloumi-shell notification
cargo test -p nuraloumi-shell --bin nuraloumi-toast
```

A future DBus E2E must use a **separate isolated session bus**, explicit owner identity, bounded test duration, and test-only daemon binary. Do not bind over the current user bus, kill `dunst`/`mako`, replace the active NuraLoumi panel, or relabel fixture tests as hardware acceptance. For .106, obtain a separately approved safe test seat/session plus confirmed actual DBus bus and active binary hashes before attempting pixel/input capture.

## Coordination and decision

- Initial gate finding (dirty panel test/format) delivered on `.wsbridge:coordination` note **8496**.
- Independent notification backend and UI issue list delivered on `.wsbridge:coordination` note **8498**.
- **No lane 01/02 approval** from this report. Re-review their exact landed commits, repeat all baseline gates and specifically reproduce/fix the issues above; only then consider a guarded device test.
- No release/tag/push/deployment authorization granted. Continue through a separate reviewed integration checkpoint.
