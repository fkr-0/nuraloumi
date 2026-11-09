@projmgrauth
Set repo: nuraloumi

# NuraLoumi Wave 2 — Music Player and Notification Center Surfaces (0.0.6)

Lane: music-notifications
Priority: 88

## Mission

Create two new dedicated surface binaries — `nuraloumi-music` (MPRIS media
player control) and `nuraloumi-notifications` (notification center) — using
the existing `MediaProvider` and `NotificationProvider` infrastructure.

Read:
- AGENTS.md, DESIGN.md, ROADMAP.md (0.0.6 section)
- docs/ARCHITECTURE.md, docs/CONTRACTS.md
- crates/nuraloumi-providers/src/media.rs (MediaProvider, MediaSnapshot, MediaPlayer, MediaAction)
- crates/nuraloumi-providers/src/notifications.rs (NotificationProvider, NotificationEntry)
- crates/nuraloumi-shell/src/bin/windows.rs (dedicated binary pattern)
- crates/nuraloumi-render-cairo/src/model.rs (CoreMenuAdapter)

## Exclusive write scope

- crates/nuraloumi-shell/src/bin/music.rs (new)
- crates/nuraloumi-shell/src/bin/notifications.rs (new)
- crates/nuraloumi-shell/Cargo.toml ([[bin]] entries only)

Do not edit providers, renderer, Wayland backend, or core model.

## nuraloumi-music binary

Media player control surface backed by the existing MPRIS/playerctl
MediaProvider.

### Layout
- **Now playing section**: artist and title of current track (from
  `MediaSnapshot.current()`). Status indicator: Playing/Paused/Stopped.
- **Transport controls**: three large touch rows (>= 48 px each):
  - Previous (glyph: ⏮ or Nerd Font equivalent)
  - Play/Pause (glyph: ⏯, toggles based on current status)
  - Next (glyph: ⏭)
- **Player selector section** (only when multiple MPRIS sources):
  list all `MediaSnapshot.players` with their id and status. Selecting
  a player makes it the target for transport actions.
- **Status row**: backend name, issue count if any.

### Interaction
- Touch/Enter on transport rows: execute `MediaAction::PlayPause`,
  `MediaAction::Previous`, `MediaAction::Next` via
  `MediaProvider.execute_action()`.
- Arrow keys to navigate between transport controls and player list.
- `r` or F5 to refresh media state.
- `q` or Escape to close.
- Space bar: play/pause shortcut.

### Data flow
1. On open: `MediaProvider::system().snapshot()`.
2. Build `MenuModel` with now-playing info row (text/status kind) +
   transport action rows + player selector rows.
3. On action: call `provider.execute_action(MediaAction::*)`, then
   re-snapshot and rebuild model.

## nuraloumi-notifications binary

Notification center showing notification history with dismiss and action
support.

### Layout
- **Header**: "Notifications" with count badge and "Clear all" action.
- **Notification list**: scrollable, most recent first. Each entry shows:
  - App name (secondary text)
  - Summary (primary text, bold)
  - Body (subtitle, truncated to 2 lines / MAX_TEXT_CHARS)
  - Per-notification action buttons (if entry has actions)
  - Dismiss button per entry
- **Empty state**: "No notifications" text row when list is empty.

### Interaction
- Arrow keys: navigate notification list.
- Enter on a notification action: execute the action via
  `NotificationProvider.execute_action()`.
- `d` or Delete on selected notification: dismiss it.
- `c` or Ctrl+L: clear all with confirmation gate.
  Confirmation: "Clear all N notifications? Enter to confirm."
- `r` or F5 to refresh.
- `q` or Escape to close.
- Search/filter by app name or summary text.

### Data flow
1. On open: `NotificationProvider::system().snapshot()`.
2. Convert entries to `MenuModel` — each notification becomes a submenu-like
   group with summary as title, body as subtitle, actions as child items.
3. Bounded to 64 entries (existing provider cap).
4. Dismiss/clear actions via provider, then re-snapshot.

## Shared patterns
- CLI args: `--config PATH`, `--fixture`.
- Fixture mode with hardcoded demo data (3 fake players, 10 fake
  notifications).
- Layer-shell popup role, dismiss backdrop, existing render pipeline.
- Follow windows.rs/files.rs binary structure.

## Constraints
- No new dependencies. Use existing provider APIs.
- ARMv7/musl cross-build must pass.
- No album art / image loading (stays within wl_shm text/glyph budget).
- Transport control touch targets >= 48 logical px.
- Notification body truncated to MAX_TEXT_CHARS (240, existing provider cap).

## Acceptance
1. `nuraloumi-music --fixture` shows demo player with transport controls.
2. `nuraloumi-notifications --fixture` shows demo notification list.
3. Transport actions work in music surface (play/pause/next/prev).
4. Dismiss and clear-all work in notifications surface.
5. Player selector appears when fixture has multiple players.
6. `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` pass.
