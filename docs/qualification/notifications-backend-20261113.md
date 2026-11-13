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
- UI lane must explicitly adopt the additive API; it is not wired by this
  backend-only task.

## Agent coordination

The UI lane was informed on `.wsbridge:lane-coordination` by note
`d33cece6-d614-4167-a9e9-f81b85d23851`. The preexisting
`NotificationProvider` snapshots/actions remain available; no UI source
was edited in this task.
