# Implement bidirectional swipe-to-dismiss for NuraLoumi list items

## Goal
Add a reusable touch gesture named **Swipe**: dragging an explicitly swipeable
list item horizontally left or right moves the actual item with the finger,
then slides it completely out of the list and collapses the vacated space
when the gesture is committed. Both directions work symmetrically. This is
different from vertical list scrolling and must not become a ghost click.

## Context
- Owner requested this for the native NuraLoumi touch UI on SL101/Tegra20.
- Start by inspecting the current nuraloumi main HEAD, AGENTS.md,
  docs/CONTRACTS.md, the Wayland touch routing, Cairo scene and hit-region
  model, menu scrolling/selection, and notification list/action boundaries.
- Existing task nuraloumi-20261125-sl101-menu-scroll-qualification concerns
  vertical scrolling and touch click suppression, not this dismiss gesture.
  Inspect its landed code and coordinate around any active claims.
- Keep 48px touch targets, Cairo/wl_shm software rendering, no NEON, bounded
  redraw and reduced-motion semantics. No always-on animation when idle.

## Scope and behavior
1. Add a generic, testable horizontal swipe state machine on list entries:
   touch-down, pending gesture, horizontal capture, live signed x-offset,
   release commit/cancel, finite off-screen exit and vertical gap collapse.
   Use a clear activation threshold and horizontal-versus-vertical intent
   arbitration. Restore the row on short/slow swipes or canceled gestures.
2. Swiping left removes the row toward the left; right removes it toward the
   right. The gesture carries direction in typed semantics. A row may register
   a left-swipe and/or right-swipe action, but neither action is required:
   **swiping must visually remove the item even when no action is registered**.
   Separate the explicit swipeable capability from optional action callbacks.
   Never infer swipeability from action presence, and do not silently make
   unrelated menu entries swipeable. A no-action swipe updates only the local
   list/view state; it must not invent a backend operation or mutate durable
   provider data. Define the lifecycle of locally hidden rows on refresh.
3. Integrate at least one real list, preferably the notification history.
   Show both actionless swipe-to-hide and an optional directional action when
   one is available. If an action is registered, invoke it on commit with the
   item ID and direction, and preserve/restore state on backend failure;
   durable provider removal is only performed following confirmed success.
   If the notification provider cannot support this contract, expose an
   explicit fixture list plus a precise integration blocker.
4. Preserve tap, keyboard, wheel and vertical swipe scrolling. A committed
   horizontal swipe must not activate the row or synthesize a second pointer
   click. Handle multi-touch conflict, out-of-bounds release, rapid repeated
   swipes, reordering/provider refresh during gestures, empty lists, and
   stale item IDs. Preserve selection/focus and scroll bounds after collapse.
5. When reduced-motion is enabled, perform the semantic dismissal immediately
   after a qualifying gesture. Otherwise bound animation duration and redraw
   scheduling; avoid heavy effects on Tegra20.

## Acceptance
- Deterministic unit tests for actionless left/right commit and local row
  removal; optional per-direction action callback delivery; undershoot
  cancellation; vertical-scroll arbitration; tap/pointer suppression;
  multi-touch cancel; provider failure/retry; stale ID; repeated swipes;
  list shortening and reduced-motion. Verify no backend action is emitted
  when no handler is registered.
- Cairo fixture or deterministic scene test demonstrates signed horizontal
  row offset and the off-screen/collapse transition without a screenshot-only
  assertion.
- Run targeted shell/Wayland/renderer tests, then cargo test --workspace,
  cargo fmt --all -- --check, and cargo clippy --workspace --all-targets
  -- -D warnings when practical. Record exact failures and unrelated blockers.
- No device mutation, no Sway/WayVNC restart, no /opt/current switch,
  no automatic reboot/poweroff, no remote Git push. Physical touch acceptance
  remains pending until separately supervised on Nura.

## Coordination and delivery
- Use authenticated projmgrauth, one workspace, root instructions and
  path claims; preserve unrelated edits and active owners.
- Implement on a focused branch/commit or a clearly reviewable scoped diff.
  Include a concise UX/API note: swipe event semantics, row capability,
  thresholds, state transitions and platform limitations.
- Send in_progress agent_checkpoint at meaningful milestones; finish with a
  terminal complete/blocked checkpoint using the injected OCP task identity,
  exact files/tests and self-contained follow-ups.
