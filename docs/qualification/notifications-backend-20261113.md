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
