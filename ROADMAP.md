# NuraLoumi roadmap

## Mission

Ship a coherent set of small, operating binaries for the SL101/Nura rapidly, while preserving a software-rendered recovery path and keeping every subsystem independently testable.

### Nura Browser migration — local integration, 2026-11-13

- [x] Move the native C/WPEPlatform engine source into NuraLoumi-owned
      components/nura-browser-engine; preserve the previous SL101 source.
- [x] Provide nuraloumi-browser with Rust/Cairo/wl_shm address editing,
      navigation, horizontal tabs and durable bookmarks, without GTK.
- [x] Implement independent native WebKitWebView tabs and bounded
      authenticated Unix-socket commands; keep NuraLoumi's recovery shell
      independent of WebKit.
- [x] Install and use genuine host WPE 2.54.2 development headers for a
      native C compile/link (not an ARMHF/device qualification).
- [ ] Validate real WPE sandbox and Wayland pixels/touch, correct focus and
      compositing, and the ARMHF VFPv3-D16/no-NEON dependency closure.
- [ ] Integrate supervised startup/crash recovery; qualify packaging and
      independently review release ownership before device replacement.

See docs/architecture/NURA-BROWSER-20261113.md and
docs/ux/NURA-BROWSER-20261113.md for current contracts and release gates.

## Patch releases — 0.0.x

### 0.0.4 — on-device validation and binary deployment

Context: 0.0.3 shipped XKB keymap integration, dedicated binaries
(`nuraloumi-run`, `nuraloumi-files`, `nuraloumi-windows`), glyph resolver
callback, and panel-scope keybinding validation. None of the new surface
binaries have been exercised on the real SL101 beyond the no-SIGILL smoke.

- [ ] Copy 0.0.3 release binaries to SL101 and run each dedicated binary
      under labwc: confirm layer-shell role, touch input, and keyboard
      navigation on all five executables.
- [ ] Validate XKB keymap integration on the SL101 slider keyboard with
      `XKB_DEFAULT_LAYOUT=de`: verify German QWERTZ keybinding positions,
      shifted symbols, and menu text-search characters.
- [ ] Verify that `labwc <windowRule identifier="*" serverDecoration="yes"/>`
      does not interfere with NuraLoumi layer-shell overlays.
- [ ] Measure idle RSS for the panel binary on SL101 and compare against
      the 35 MiB combined budget; record first-interactive-frame latency.
- [ ] Fix any regressions or crash bugs found during on-device validation.
- [ ] Update qualification notes in CHANGELOG.md with real SL101 test
      results and RSS/latency numbers.

Exit: every shipping binary runs correctly on the real SL101 with the
German slider keyboard, and performance numbers are recorded.

### 0.0.5 — resource monitor and process manager surfaces

Context: `ResourceProvider` already delivers per-core CPU, memory, swap,
root-filesystem, thermal, network I/O, disk I/O, and top-process data.
`ProcessProvider` already delivers a ranked process list with PID, state,
CPU %, and memory. Neither has a dedicated surface — the data only shows
in the panel's status row and the menu's diagnostic tab.

- [ ] Add `nuraloumi-resources` binary: full-surface resource monitor.
      Per-core CPU bars, memory/swap arc or bar, disk usage, thermal
      readings, network rx/tx rates. Cairo-rendered, no continuous
      animation — poll-on-demand with a configurable refresh keybinding.
- [ ] Add `nuraloumi-tasks` binary: process/task manager surface.
      Sortable process list (CPU, memory, name), SIGTERM action on
      selected process with confirmation gate, search/filter.
- [ ] Wire both into the system sheet and panel as launchable entries.
- [ ] Icon glyphs: implement a small Nerd Font / Unicode glyph table and
      wire it into the launcher, system sheet, and new surface binaries
      via the glyph resolver callback.
- [ ] Cross-build and ELF-audit for armv7-unknown-linux-musleabihf.

Exit: resource monitor and task manager run on SL101 with live data,
glyphs display on all surfaces, within the performance budget.

### 0.0.6 — music player and notification center

Context: `MediaProvider` already does MPRIS/playerctl discovery with
play/pause/next/previous actions. `NotificationProvider` already reads
notification history with per-notification actions. Both are wired into
the panel status strip but have no dedicated interaction surface.

- [ ] Add `nuraloumi-music` binary: media player surface. Shows current
      track artist/title, playback status, play/pause/prev/next touch
      targets (>= 48 px), player selector when multiple MPRIS sources
      exist. No album art (stays within wl_shm budget).
- [ ] Add `nuraloumi-notifications` binary: notification center surface.
      Scrollable notification list, per-notification dismiss, action
      buttons, clear-all with confirmation. Bounded to the most recent
      64 entries (existing provider cap).
- [ ] Validate battery, brightness, and network providers against SL101
      sysfs/nmcli; fix any parse errors or stale-state bugs.
- [ ] Stress-test transient menu dismiss: rapid outside-press, touch
      sequences, and keyboard escape across all surfaces.
- [ ] Cross-build, ELF-audit, and SL101 smoke.

Exit: music player controls MPRIS sources on SL101, notification center
shows and dismisses real notifications, core providers return live data.

### 0.0.7 — quick-launch terminal and editor surfaces

Context: the SL101 slider keyboard makes it a keyboard-first device when
open. Quick access to a terminal (foot) and an editor (neovim or emacs
-nw) through NuraLoumi surfaces removes friction.

- [ ] Add `nuraloumi-terminal` binary: launches `foot` (or configured
      terminal) with a specific layer-shell-aware geometry. Touch-friendly
      font size default for SL101 screen. Close-on-exit lifecycle.
- [ ] Add `nuraloumi-editor` binary: launches the configured `$EDITOR`
      inside foot (or directly if the editor is graphical). Supports
      `foot -e nvim`, `foot -e emacs -nw`, or direct `emacs` when
      running as a Wayland native client.
- [ ] Add configurable quick-launch slots to the panel: terminal and
      editor get dedicated panel touch targets.
- [ ] Clipboard history surface: read from `wl-copy`/`wl-paste`, show
      recent clipboard entries, paste-on-tap.
- [ ] Section-header and separator rendering refinements for the compact
      dark theme (border opacity, section label weight).

Exit: terminal and editor launch from the panel on SL101 with correct
geometry; clipboard surface shows history.

### 0.0.8 — calendar widget and weather

Context: the SL101 is a personal tablet. A date-aware calendar grid and
weather display turn the panel clock into a useful information surface.

- [ ] Add `nuraloumi-calendar` binary: month-grid calendar surface.
      Cairo-rendered day grid with today highlighted. Optional integration
      with `khal` or local `.ics` files to show event dots on days.
      Event list for selected day. Touch navigation between months.
- [ ] Add weather display to the system sheet: fetch from `wttr.in` or
      local weather provider via bounded HTTP (curl with timeout).
      Temperature, condition icon (glyph), and forecast summary.
      Cached with configurable refresh interval.
- [ ] Provider hardening: ensure provider snapshot refresh stays bounded
      (no continuous polling while idle), re-measure idle RSS with all
      new surfaces loaded.
- [ ] Cross-build, ELF-audit, and SL101 smoke.

Exit: calendar grid renders on SL101 with optional khal event integration;
weather shows in system sheet.

## Release 0.1.0 — operating vertical slice

### What 0.1.0 means

All six crates build, the panel and launcher are usable on the SL101 as a
daily touch shell on top of labwc, providers report live data, the config
system is extensible enough that new surfaces slot in without code changes,
external tools can push data into NuraLoumi via IPC, and the xtask pipeline
produces a qualified deployment bundle. This is the first version intended
for daily use on the device.

### Upstream blockers (grate GPU driver, informational)

NuraLoumi itself is software-rendered and unaffected, but the SL101
compositor stack benefits from these grate driver fixes being developed in
parallel:

- **Texture pitch alignment** — grate rounds internal texture storage to
  64-byte pitch, causing horizontal distortion in labwc context menus and
  foot terminal when client SHM buffer stride does not match. A
  glTexSubImage2D patch is in progress (sl101_linux task 42959).
- **GR3D multi-context fencing** — grate lacks dma_fence/dma_resv
  integration, so concurrent GR3D contexts (compositor + GPU client)
  corrupt each other. Kernel + Mesa patches are being designed
  (sl101_linux task 2ed89). Required for sway on grate.
- **Sway EGL shim** — wlroots 0.20 attempts EGL_PLATFORM_DEVICE before
  GBM; the device-path failure is a suspected cause of broken Grate fallback.
  On SL101 (2026-11-05), the existing wrapper libEGL.so.1 was **confirmed to
  resolve via LD_LIBRARY_PATH under musl** (ldd), and a read-only ctypes probe
  verified its eglGetProcAddress interception: eglQueryDevicesEXT returns
  EGL_TRUE with zero devices. The older claim that musl cannot load it is
  disproven. This is not yet proof of Sway renderer/DRM startup or correct
  pixels: isolate and test the complete pinned EGL/GBM/wlroots library group
  without taking the active labwc KMS/seat ownership. See
  sl101_linux/docs/sl101-sway-egl-no-upstream-review-20261105.md.
  NuraLoumi's Cairo/wl_shm software path remains unaffected.

None of these block 0.1.0. They affect the broader SL101 desktop experience.

### Milestone detail

### M0 — contracts and repository skeleton
- [x] Register project and Git repository.
- [x] Establish six non-overlapping crates.
- [x] Freeze initial cross-crate contracts.
- [x] Establish independent-review agent workflow.
- [ ] All crates build on the host with placeholder implementations.

### M1 — semantic core
Deliver `nuraloumi-core`.
- MenuModel/MenuItem/MenuAction with stable IDs.
- Row kinds: action, submenu, checkable, text/status/section/separator.
- Selection/navigation invariants.
- Search/filter and quick-select.
- confirmation flow for destructive actions.
- dark/light design tokens with 48 px touch minimum.
- motion primitives with reduced-motion mode.
- pure deterministic tests and serde fixtures.

Exit: core crate has no GUI/window-system dependency.

### M2 — software renderer
Deliver `nuraloumi-render-cairo`.
- renderer-neutral scene/layout tree.
- Cairo image-surface renderer.
- rounded panels/cards, borders, separators, selected rows.
- icon placeholder/glyph path, title/subtitle/shortcut geometry.
- deterministic hit regions.
- bounded shadow/gradient effects designed for Tegra20.
- PNG fixture renderer for review and CI.
- no X11/XCB dependency.

Exit: a menu fixture renders to a deterministic PNG from the command line.

### M3 — native Wayland backend
Deliver `nuraloumi-wayland`.
- connection/registry/output/seat discovery.
- wl_shm double-buffered surfaces.
- layer-shell anchored panel and popup roles.
- keyboard, pointer and touch events mapped to a small platform-neutral event enum.
- scale/transform awareness.
- configure/resize/damage/commit lifecycle.
- clean shutdown and reconnectable error model.
- no EGL/GLES requirement.

Exit: an example binary opens, paints, receives input, and exits under a normal Wayland compositor.

### M4 — operating shell
Deliver `nuraloumi-shell`.
Initial executable surfaces:
- `nuraloumi-panel`: small top strip with Apps / Network / Audio / Battery / Clock.
- `nuraloumi-menu`: generic menu runner driven by JSON/TOML fixtures.
- launcher sheet with search.
- system sheet with brightness, volume, suspend/restart/power confirmation rows.
- keyboard + touch parity.
- one-menu-at-a-time lifecycle.
- reduced-motion mode.

Exit: both binaries run with fixture providers before hardware providers are available.

### M5 — providers
Deliver `nuraloumi-providers`.
- battery/power from sysfs.
- brightness from sysfs with explicit writable capability.
- network snapshot/action adapter around NetworkManager/nmcli or DBus.
- audio snapshot/action adapter with a minimal backend boundary.
- clock/session providers.
- bounded command runner, timeouts, error/stale states.
- fixture backend for all providers.

Exit: `nuraloumi-probe` emits a stable JSON snapshot and does not mutate state unless explicitly given an action command.

### M6 — extensible configuration schema

Evolve the existing `ShellConfig` (TOML/JSON, XDG discovery) into a
modular per-surface configuration that new binaries inherit without
code changes.

- Shared base config section: panel geometry, theme, reduced motion,
  keybindings — loaded by every surface binary via `load_config_or_default`.
- Per-surface config sections keyed by surface name (`[surface.music]`,
  `[surface.resources]`, `[surface.calendar]`, …). Unknown sections are
  preserved and ignored so third-party surfaces can add their own.
- Typed provider config: refresh intervals, backend selection, command
  overrides (e.g. `terminal = "foot"`, `editor = "nvim"`).
- Quick-launch slot definitions: panel touch targets mapped to surface
  binaries or arbitrary `.desktop` entries.
- Config schema versioning: version field, forward-compatible unknown-key
  policy, clear error on unsupported version.
- `nuraloumi-config validate` subcommand (in xtask or standalone): parse
  and report warnings/errors without launching a surface.
- Serde-based schema with `#[serde(deny_unknown_fields)]` on strict
  sections, `#[serde(flatten)]` on extensible sections.

Exit: a single config.toml drives all surface binaries; adding a new
surface does not require config code changes.

### M7 — IPC socket

External scripts, status bars, and automation tools can push data into
NuraLoumi and query surface state.

- Unix domain socket at `$XDG_RUNTIME_DIR/nuraloumi.sock`.
- Line-delimited JSON protocol (one request, one response per line).
- Commands:
  - `push-item`: inject a temporary menu item into a named surface
    (e.g. a script adds a "Deploy status: OK" row to the panel).
  - `set-status`: update a named status slot in the panel strip.
  - `notify`: emit a notification through the notification provider.
  - `surface-state`: query whether a surface is open, its current
    selection, and provider health.
  - `navigate`: send a semantic navigation/action message to the
    focused surface (open, close, select, scroll).
  - `reload-config`: trigger a config re-read without restarting.
- Bounded: max 64 pushed items, max 4 KiB per message, 1-second
  timeout per request. Pushed items expire after a configurable TTL.
- Auth: socket permission (owner-only by default). No token scheme.
- `nuraloumi-ctl` binary: thin CLI client for the IPC socket.
  `nuraloumi-ctl push-item --surface panel --label "VPN: up" --glyph ""`
  `nuraloumi-ctl notify --summary "Build passed"`

Exit: `nuraloumi-ctl push-item` injects a visible row into a running
panel; `surface-state` returns structured JSON.

### M8 — build and target qualification
Deliver `nuraloumi-xtask`.
- dependency/features audit.
- release build and size report.
- ARMv7 target configuration and no-NEON audit helpers.
- package staging directory.
- host nested-Wayland smoke script.
- SL101 deployment manifest, rollback notes and resource probe integration.
- reproducible fixture screenshots.

Exit: one command produces host release binaries plus an SL101 qualification bundle.

## Release 0.2 — daily-driver surfaces

The tablet becomes a usable daily device with its own personality.

### New surfaces
- Offline map viewer: pre-cached OSM tile renderer via Cairo. Bounded
  tile cache on SD card. Pan/zoom via touch drag and pinch (if
  compositor reports touch gestures) or keyboard arrows/+/-. Tile fetch
  via curl when online; works fully offline once cached. No WebView.
- RSS/feed reader: text-based, Cairo-rendered. OPML import, bounded
  entry list, mark-read, open-in-browser action. Fits the dark compact
  aesthetic and is uniquely suited to a small keyboard tablet.
- Screenshot tool: invoke `grim` with output selection, show preview
  in a NuraLoumi surface, copy to clipboard or save to ~/Pictures.
- On-screen keyboard surface for when the slider is closed (simple
  grid layout, Wayland input-method-v2 protocol).
- Notification toast overlay: small transient layer-shell popup for
  incoming notifications when the notification center is closed.

### Infrastructure
- Four-corner touch calibration and rotated output testing.
- On-device RSS/CPU/startup latency measurements across all surfaces.
- Application discovery through `.desktop` entries with icon theme
  lookup and bounded icon caches.
- Suspend/resume recovery for all surfaces and providers.
- NetworkManager, audio, and bluetooth backend hardening.
- Packaging for the Nura userspace (Alpine APK or plain tarball).

## Release 0.3 — polished daily shell

### New surfaces and features
- Favorite applications and recent items in the launcher.
- Workspace/window adapter when compositor IPC (labwc/ext-foreign-toplevel)
  is available: workspace switcher surface, window-to-workspace move.
- Lock/session screen affordances.
- Configurable top/bottom/side panel position.
- File manager surface: directory browser backed by `ls`/`stat`,
  open-with action via `.desktop` xdg-open, basic rename/delete with
  confirmation. Not a full file manager — a quick browser for ~/
  and removable media.
- Timer/stopwatch surface: useful for a tablet on a desk.
- Optional GPU-backed compositor operation with identical software fallback.

### Quality
- Accessibility pass: contrast ratios, text scaling, focus visibility.
- Config migration tooling for schema version upgrades.
- Theme variants: at least one light theme alongside the default dark.
- Config live-reload via IPC `reload-config` without surface restart.

## Release 0.4 — connectivity and integration (exploratory)
- Bluetooth file transfer surface (obexftp).
- Wi-Fi captive portal detection and browser launch.
- CalDAV/CardDAV sync for calendar and a minimal contacts surface.
- Graphical Emacs (pgtk) or neovim-qt launcher with NuraLoumi-aware
  window placement (preferred geometry, layer-shell adjacency).
- Composable panel widgets: user picks which segments appear via config.
- Plugin/extension contract: third-party surfaces register via a
  manifest and appear in the launcher automatically.
- IPC event subscriptions: external tools subscribe to surface events
  (menu open/close, selection change, provider updates) for scripting.

## Non-goals through 0.4
- replacing labwc/wlroots;
- browser engine or WebView embedding;
- full desktop settings daemon;
- animated blur, shader effects, or EGL-only rendering;
- Phosh/GNOME/Plasma compatibility layers;
- XWayland as a mandatory dependency;
- streaming video playback (delegate to mpv).
