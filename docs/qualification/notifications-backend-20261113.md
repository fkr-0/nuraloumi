# Notifications backend qualification — 2026-11-13

## Identity and scope

- OCP task: `NURALOUMI-01-NOTIFICATIONS-BACKEND-E2E-20261113-0C7114`
- Continuation: `NURALOUMI-MUSIC-NOTIFICATIONS-20261103-3CA6C8`, not duplicate work
- Owned runtime file: `crates/nuraloumi-providers/src/notifications.rs`
- No edits to shell UI, panel, `lib.rs`, resources, Cargo manifests, or system Mesa/Sway.
- Host: `archlinux` (x86_64); **SL101 .106 was not qualified**.

## Actual implementation and public API

The previous `NotificationProvider<R>` and `NotificationEntry` fields remain
backward compatible. The new, additive
`nuraloumi_providers::notifications::NotificationDelivery<R>` supports
`system()`, `new(runner)`, `poll()`, `snapshot()`, `mark_read(id)`
and `mark_all_read()`. It produces immutable
`NotificationDeliverySnapshot` with a bounded set of
`DeliveredNotification` values, an unread count and per-poll
`Added`, `Updated`, `Removed`, `DisplayExpired` events. The time
injection entry point is private and used by deterministic in-module tests.

**Transport boundary:** this is a *passive Dunst history observer*, not a
replacement for `org.freedesktop.Notifications` and not an immediate D-Bus
Notify event subscriber. Dunst may only place notifications into history after
display/close. Calling `poll()` only observes history transitions, so it
cannot prove low-latency delivery of every active notification.

Limits: maximum 64 retained notifications, 16 parse issues, 16 action keys,
240 display characters, 256 KiB parsed history and 32 JSON nesting levels.
The command runner already enforces an argv-only 2-second/256-KiB history
request and 3-second/32-KiB action request. Production `poll()` uses an
internal monotonic clock to limit history subprocesses to one per second; it
does not spawn worker threads or hold the process in a busy loop. A Dunst
display-timeout field is converted from microseconds and capped at 120 seconds.
**This is not notification-history expiry.** The emitted display-expired
event is an approximation from *first observation*, not an authoritative
signal that a compositor toast disappeared.

Security: display texts are bounded and sanitized as plain text, removing
HTML-like tags, C0/ANSI sequences and bidi override/isolate characters before
known entity decoding. Action IDs allow only ASCII alphanumerics, `_`,
`-`, `.`, `:` and at most 128 bytes. No user-controlled shell
interpolation is used. History remove/clear use explicit `dunstctl` argv.
Dunst cannot atomically invoke a selected history entry action using its ID:
the old history-pop then action-at-position-0 sequence is vulnerable to races.
Therefore `NotificationAction::Invoke` fails closed unless explicitly
opted into `with_unsafe_best_effort_actions(true)`; callers should
not enable the opt-in for production/UI use.

## Host verification

| Check | Recorded result |
| --- | --- |
| Targeted `cargo test -p nuraloumi-providers --lib notifications --locked` (initial shared target) | 11 tests PASS before subsequent changes |
| `cargo test --workspace --locked` before concurrent UI edits | PASS, exit 0 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` before concurrent UI edits | PASS, exit 0 |
| `rustfmt --edition 2021 --check crates/nuraloumi-providers/src/notifications.rs` | PASS after latest edits |
| `CARGO_TARGET_DIR=/tmp/nuraloumi-notifications-0c7114-target cargo test -p nuraloumi-providers --lib notifications --locked` | 12/12 PASS on current source (including action-label alignment) |
| `cargo fmt --check` after concurrent UI edits | BLOCKED by unrelated `render-cairo/src/layout.rs`, `shell/src/lib.rs`, `shell/src/bin/panel.rs` changes |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` after concurrent UI edits | BLOCKED by unused imports in unrelated `shell/src/bin/panel.rs` |
| `CARGO_TARGET_DIR=/tmp/nuraloumi-notifications-0c7114-target cargo test --workspace --locked` after concurrent UI edits | BLOCKED: missing `DARK_THEME` import in unrelated `shell/src/bin/menu.rs` test |
| `CARGO_TARGET_DIR=/tmp/nuraloumi-notifications-0c7114-target cargo clippy -p nuraloumi-providers --all-targets --locked -- -D warnings` | PASS, exit 0 |

Independent reviewer automation was attempted but the advertised review tools were unavailable (`Tool llm_review_code not found` and `Tool spawn_llm_agent not found`); the code has **not** received independent approval. A separate governed workflow review remains a release gate.

The shared `target` directory was concurrently rebuilt: a later scoped test
command unexpectedly executed only five older tests despite twelve current
test functions in the source. A dedicated `CARGO_TARGET_DIR` resolved this
build artifact race and demonstrably executed all twelve current tests. Do not
rely on the shared target for final qualification until builds are serialized.

Deterministic tests cover baseline versus new history, Added/Removed,
unread and mark-read, display-timeout retention, no repeated expiration,
failed transports retaining cached history, malformed JSON and size limits,
markup/control stripping, forged actions and the fail-closed default.

## Non-invasive live E2E attempt — **NOT PASS**

1. Read-only host inspection initially identified a preexisting
   `org.freedesktop.Notifications` owner, Dunst PID 659879 with 35 stored
   notifications; no ownership was requested by NuraLoumi.
2. A test-only Rust consumer was compiled at
   `/tmp/nuraloumi-notify-e2e-0c7114` with no repo-owned harness files.
3. A private `dbus-run-session` and `xvfb-run` launched a separate
   `dunst` in an isolated session. The private owner query returned
   `:1.1`. An `notify-send -a nuraloumi-e2e` message was sent, then
   `dunstctl close-all` requested in that private bus.
4. The consumer saw `HEALTH=Healthy HISTORY_TOTAL=0 MATCH=0 FEED=0`
   and failed its explicit assertion. Therefore **sender -> D-Bus ->
   history -> backend -> actions was NOT proven**, and no removal/clear
   end-to-end success should be claimed.
5. A later read-only host inspection briefly showed no original Dunst owner
   or history, followed by another active owner PID 2872542. Causality is
   unknown amid parallel session work. No further attempt was made to seize,
   restart or alter the production notification service.

## Follow-up: private D-Bus live E2E and scoped Clear race (2026-11-13)

This section supersedes the earlier **NOT PASS** assessment *only* for the
isolated host-to-backend path. The unsuccessful first attempt above is retained
as historical diagnostic evidence; the SL101 and UI end-to-end gate remain
unqualified.

- A fresh, isolated `xvfb-run -a dbus-run-session` started a Dunst instance
  using an explicit test configuration with `history_length=50` and short
  timeouts. The process ID obtained for the private
  `org.freedesktop.Notifications` owner matched the spawned test daemon.
  No production notification daemon was claimed or controlled.
- `notify-send -p` returned ID `2`; after natural expiry the real
  `NotificationProvider::system()` read exactly one notification with the
  expected application, summary and body. `NotificationDelivery::system().poll()`
  read the same history. `NotificationAction::Invoke` rejected the unsafe
  default action, then `NotificationAction::Remove { id: 2 }` removed exactly
  that entry, confirmed by a second snapshot.
- A second message with ID `3` entered the history. Legacy
  `NotificationAction::Clear` cleared the history in the private test.
  **Result: sender → private D-Bus → Dunst → history → real Rust provider
  → remove/clear PASS on the host.**
- A separate test delivered a notification with ID `2`, then another with
  ID `3` *after the simulated confirmation boundary*. The new
  `NotificationAction::ClearKnown { ids: vec![2] }` removed only ID `2`,
  preserving ID `3`, as verified by a fresh provider snapshot.
  This is a real private-bus, concurrent-arrival boundary regression, not just
  a mock runner.
- Backend `Clear` no longer calls Dunst's global `history-clear` and
  instead issues bounded exact-ID `history-rm` requests for entries present
  when execution begins. `ClearKnown` accepts explicitly confirmed IDs,
  validates their existence and uniqueness before any mutations, and refuses
  degraded/unavailable history or oversized sets. A partial clear remains
  possible if Dunst fails mid-sequence. Calls are bounded to 64 IDs.
- The 3 added deterministic tests cover confirmed-ID preservation, legacy
  scoped clear, and duplicate/unknown/oversized/unavailable rejection.
  Notification tests: **15/15 PASS**.

**Important residual UI TOCTOU:** the existing Notification Center still
dispatches legacy `NotificationAction::Clear` after confirmation, so arrivals
between the UI's *ask* and the provider's snapshot may still be cleared.
The UI owner must capture IDs when the confirmation screen is created and
pass precisely those IDs via `ClearKnown`. This backend-only follow-up did
**not** modify `crates/nuraloumi-shell/src/bin/notifications.rs`, `panel.rs`
or `lib.rs`. Production acceptance must not claim that this UI race has
been fully closed until that integration and its tests are complete.

The private test establishes functional host transport and action plumbing,
**not** immediate D-Bus event subscriptions, complete GUI/touch end-to-end,
hardware SL101 .106 CPU/RSS, or human-observed notification toasts.

Host worktree verification at base `0a821ac` with this follow-up applied:

- `cargo test -p nuraloumi-providers --lib notifications --locked`: **15/15 PASS**.
- `cargo test --workspace --locked`: **PASS**, all unit and doc tests.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: **PASS**.
- `rustfmt --edition 2021 --check crates/nuraloumi-providers/src/notifications.rs`: **PASS**.
- `cargo fmt --check`: **FAIL**, unrelated pre-existing module declaration
  order in `crates/nuraloumi-shell/src/lib.rs` (moving `pub mod browser;`
  above `config_migration`). This backend-only lane did not edit the
  shell's lib.rs. Assign to shell/UI owner for final all-green fmt gate.

---

## Notification Center confirmation closure — subsequent local integration

Implemented in `crates/nuraloumi-shell/src/bin/notifications.rs` on top of
backend commit `5c927aa`:

- The destructive *Ask* action freezes the IDs in the UI's current bounded
  notification snapshot as `Option<Vec<u32>>`. The confirmation menu displays
  the **captured count**, not the possibly refreshed history count.
- *Confirm* dispatches `NotificationAction::ClearKnown { ids }`; the legacy
  broad `NotificationAction::Clear` is no longer used by the notification
  center. A direct clear without arming a confirmation is inert.
- Keyboard Escape cancels and discards the captured IDs. Re-arming explicitly
  chooses a new set. After a successful action the confirmation state resets.
- Fixture-mode clear removes only captured IDs, mirroring the real backend
  contract instead of unconditionally emptying the entire snapshot.
- Two new deterministic UI regressions establish that an item arriving after
  Ask remains present after Confirm, and that a deliberate re-Ask updates the
  captured set. The existing rendering/truncation test is preserved.
- One additional `#[ignore]` integration test invokes the actual UI
  `act()` implementation against `NotificationProvider::system()` on a
  **fresh private D-Bus and dedicated Xvfb/Dunst**. An explicit opt-in and
  ephemeral bus-address guard prevent it from affecting the production
  notification service. On the host, the real sender→D-Bus→Dunst→UI
  confirmation→provider→history sequence **PASS**; the notification arriving
  after Ask survives ClearKnown. This test exercises UI action logic, **not**
  Wayland-rendered pixels, pointer/touch input or a display session on SL101.
- One unrelated formatting-only ordering fix in `crates/nuraloumi-shell/src/lib.rs`
  restores the workspace-wide `cargo fmt --check` gate.

On the isolated worktree based on `5c927aa`, the tests and checks return:

| Gate | Result |
| --- | --- |
| `cargo test -p nuraloumi-shell --bin nuraloumi-notifications --locked` | **3/3 PASS**, one private-bus test ignored by default |
| Opt-in ignored UI→real private D-Bus→provider confirmation test | **1/1 PASS**, dedicated test Dunst |
| `cargo test --workspace --locked` | **PASS**, including 15 backend notification tests |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | **PASS** |
| `cargo fmt --check` | **PASS** |
| `git diff --check` | **PASS** |

This closes the confirmed-ID UI→provider arrival race **at the tested
contract boundary** and confirms real isolated UI-action-to-D-Bus behavior.
A complete live Wayland GUI/pixel/touch demonstration and actual
SL101 .106 CPU/RSS/interaction numbers remain separately unverified.
The historical "residual UI TOCTOU" statement above refers to the
previous backend-only commit and is superseded by this section.

Independent reviewer execution was attempted via both the managed OpenCode
review preset and prompt-based LLM agent; their servers reported unavailable
backends/tools. No independent approval or physical-device release is claimed.

## P1 notification interaction and clear-budget hardening — 2026-11-13

Implemented on top of commit c159b073 in the isolated P1 follow-up:

1. The UI hides **Invoke** controls for Dunst historical actions because
   the provider safely rejects the non-atomic operation by default.
   Forged invoke activations fail closed. Backend errors now render a visible
   notice instead of terminating the Wayland menu. Failed clear operations
   report **Clear incomplete** and refresh the remaining history.
2. Only primary pointer BTN_LEFT (0x110) and matching touch contacts can
   activate on *release*. Crossing to another target, PointerLeave, cancel,
   wrong touch ID, secondary buttons and multitouch are rejected. Outside
   dismiss also waits for release; absent backdrop is explicitly guarded.
3. ClearKnown and legacy Clear retain exact-ID removal, with at most 64
   IDs. All subprocesses now share a single **3-second wall-clock deadline**,
   passing the remaining budget as each subprocess timeout. The previous
   64 * 3 seconds worst-case is removed. Partial clears remain possible
   because Dunst does not expose atomic multi-ID removal.
4. Live ClearKnown runs on one bounded worker thread and one-result channel.
   Wayland polls only while a clear is in flight; idle still blocks without
   periodic wakeups. Additional clear work is rejected while pending.
5. An opt-in private D-Bus test now exercises the background worker as
   well as the original confirmation-time ID UI/backend path. The test
   verifies unrelated notifications remain after a clear operation.

### Qualification evidence (isolated local worktree)

| Check | Result |
| --- | --- |
| Notification-Center deterministic UI tests | **7 PASS**, plus 1 ignored-by-default integration test |
| Backend notification tests | **16 PASS**, includes global-budget regression |
| cargo test --workspace --locked | **PASS** |
| cargo clippy --workspace --all-targets --locked -- -D warnings | **PASS** |
| cargo fmt --check and git diff --check | **PASS** |
| Private Xvfb + dbus-run-session + Dunst + notify-send + UI/provider test | **1 PASS**, verified isolated owner |

Remaining: Numeric Dunst ID reuse after daemon restart is a separate
P2 issue. The newly safe UI does not magically enable atomic historical
action invocation. No complete Wayland pointer/touch pixel E2E, real SL101
CPU/RSS, independent reviewer approval or hardware release is claimed.

## P1 review remediation: backdrop drag and post-clear verification (2026-11-13)

This follow-up addresses both findings from the prior review of commit
`039f60a7`, workflow packet `2090cb83`:

- A dedicated backdrop press state machine now records the initial pointer or
  finger coordinates and cancels the armed close action when movement exceeds
  **12 logical pixels**. Small finger jitter remains a valid tap. A down event
  followed by a drag and release never dismisses the center, including when
  Wayland retains touch focus on the original backdrop surface.
  Unmatched releases, touch cancel, multitouch and non-primary buttons are
  rejected. The existing in-menu release matching is retained.
- The real background `ClearKnown` worker now evaluates the **post-clear
  history snapshot** before returning a successful UI result. It requires
  `Health::Healthy` **and** verifies that each confirmed ID is absent from
  the history returned by Dunst. If the backend read fails or reports degraded
  or unavailable history, removal is reported as **unverified** rather than
  as success. The UI also distinguishes unavailable history from a genuinely
  empty inbox. A failed command still reports failure or possible partial
  completion. No atomicity is claimed.
- The worker execution is factored into a generic, injectable function for
  regression tests, without changing the production Dunst command runner.
  A mock runner tests a successful history-rm followed by a failed
  history read, a healthy but still-present notification, and a confirmed
  successful removal. Gesture tests cover small-jitter tap, drag away and
  return, wrong finger, cancellation, multitouch and pointer motion.

These checks do not prove SL101 .106 touch calibration or release behavior on
hardware; they validate the input state machine and system-command result
contract. Real Dunst numeric ID reuse after restart remains a separate P2
identity issue. All work remains local; no production notification service,
Wayland session or device was modified.

## Remaining external qualification gates

- In a controlled test bus with explicit Dunst config and lifecycle, prove
  Notify call/ID, active and expired notification transitions, history
  visibility, and remove/clear with captured before/after snapshots.
- If **immediate** notification delivery is required, design an authorized
  D-Bus signal/monitor adapter or explicit integration with the current
  notification daemon; history polling alone cannot guarantee it.
- Verify on the actual SL101 .106 session with backend, transport,
  concurrent senders, bounded CPU/RSS and action identity. Without that
  evidence, report the hardware/live E2E requirement **BLOCKED**.
- The notification center now uses `ClearKnown` with confirmation-time IDs;
  repeat the UI-to-D-Bus live action check and SL101 interaction/power
  qualification against the integrated release revision.

## Agent coordination

The UI lane was informed on `.wsbridge:lane-coordination` by note
`d33cece6-d614-4167-a9e9-f81b85d23851`. The preexisting
`NotificationProvider` snapshots/actions remain available; no UI source
was edited in this task.
