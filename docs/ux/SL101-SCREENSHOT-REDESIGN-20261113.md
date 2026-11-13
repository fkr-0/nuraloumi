# SL101 screenshot-driven shell UX redesign — 2026-11-13

## Evidence / scope

Input: two current SL101 screenshots at 1280x~800 with the NuraLoumi top bar and
launcher over the green Sway desktop. Audit focuses on user-visible design,
not speculative conclusions about the live GPU/Wayland pipeline.

The displayed deployment shows `Clock · 22:…` and `Battery · 88% · Disc…`,
while the current source already renders `HH:MM` without `Clock` and draws
the battery with a proportional fill (panel source commits 351e7ab / df6182b).
**Compare installed binary SHA/version to source before any SL101 deployment.**
The screenshot also shows a visible browser behind the launcher while the
launcher reports no mapped windows. Verify the foreign-toplevel adapter,
compositor capability and refresh timing separately. The apparent green
overlay/translucency could also be capture/compositor damage or alpha
composition, and is not a palette problem until verified.

## UX principles

1. **One control, one job** — a real search field in the panel; no fake search
   text-row before the first launcher action.
2. **Visual semantics** — consistent treatment of heading, tab, section
   heading, actionable row, metadata, disabled row, and empty state. Never
   render debug diagnostics as secondary metadata in the normal launcher.
3. **Compact status, deep detail** — icon + meaningful short value in the bar.
   Full SSID, signal, output device, audio endpoint, battery state and power
   details belong in their own sheets. `Clock` is not displayed.
4. **Device constraints** — 48 logical-pixel primary touch targets;
   dependency-light native Cairo / wl_shm path; keyboard parity; no NEON,
   idle animation, GPU requirement, font dependency or unbounded thumbnail cache.
   The existing 40-px panel configuration is a known exception to the 48-px
   target and needs a coordinated geometry/exclusive-zone migration.
5. **Stable identity** — actions use the existing semantic IDs, never visible
   labels. UI cleanups must not alter launcher desktop IDs, action payloads,
   window identifiers or focus/activation safety.

## First landing (current task)

- Cairo-drawn Wi-Fi, speaker/mute and Apps glyphs in the panel, without font
  fallback risks. Battery stays a drawn percentage-filled icon; clock stays
  purely `HH:MM`; panel text hides redundant category names.
- Reallocate wide fixed slots toward centered search while preserving
  hit-region geometry and an explicit narrow-screen layout fallback.
- Remove the duplicate, non-interactive launcher search explanation row.
- Show a readable generic application name, not a desktop-entry filename or
  arbitrary keyword/tag list as a row subtitle.
- Do not show a misleading `Open windows: none` section on the overview
  when the compositor returns an empty list; in the dedicated Windows view,
  show a proper user-facing absence message.

## Follow-up: overview navigation and layout

Design a **two-state launcher** using existing `OverviewMode` values:

- **Compact** (default via Apps button): width ~480–620 logical px, a
  visually distinct header and search field followed by clearly separated
  **Favorites**, **Recent**, and **All apps** views. Window and workspace
  access are distinct top-level tabs, not interleaved within the app list.
- **Overview** (optional, GNOME-inspired): large 2–4-column responsive
  application tiles; tabs `Apps | Windows | Workspaces | Tasks` above content,
  search always visible. No compositor blur or animated wallpaper.
- Tab selection is visible by shape/underline **and** contrast, not colour
  alone. Tabs have keyboard navigation and ≥48 px hit regions.
- Group labels are nonactionable, 12–13 px semibold uppercase or small caps;
  actionable labels at least 15–16 px; optional one-line metadata at least
  12–13 px; regular foreground text contrast ≥4.5:1, critical non-text
  controls/focus boundaries ≥3:1.
- Empty states have an illustration/icon, a short human message, and a safe
  recovery action. Low-level failures go to diagnostics, not the main list.
- Avoid four distinct once-only text styles. Use explicit presentation roles:
  title, tab, section, item, metadata, empty, error.
- Keep one menu at a time and avoid mutating other applications' workspace
  focus except on deliberate user activation.

## Theme specification

Existing config supports `theme = "dark"` and `theme = "light"`;
the core, Cairo renderer and shell already carry corresponding tokens.
Current dark is a very close-valued grey-on-grey palette; selection and
structural boundaries are harder to distinguish on SL101.

Proposed next iteration, after snapshot/contrast tests:

| Variant | Base | Raised | Selected | Text | Accent |
| --- | --- | --- | --- | --- | --- |
| Slate Dark (recommended default) | #151b24 | #202938 | #354b64 | #f1f5fa | #83b7ee |
| Paper Light | #f5f7fa | #ffffff | #dce8f6 | #182433 | #2b609a |
| High Contrast | #090b0f | #191d24 | #375b8e | #ffffff | #ffce54 |

Keep palette mapping through `nuraloumi-core::ThemeTokens` as the
source of truth. Expose the available variants in a **Settings → Appearance**
sheet, persist via validated config or explicit IPC reload; do not implement
a second hardcoded palette in the panel. Check colour and grayscale
differentiation, renderer parity and touch focus against all variants.

## Acceptance and regression matrix

- Target 1280x800, and width fallback 1024/800; light/dark; menu with 0/1/20+
  apps and with 0/1/20+ windows; search focused/unfocused; German keyboard.
- Network connected/disconnected/stale; audio mute/output change; battery
  charging/discharging/unknown; correct HH:MM at minute boundary; correct
  click/tap targets after slot resizing.
- Host deterministic scene/hit-region and screenshot fixtures, then native
  ARMv7 musl + Debian armhf builds, no-NEON/static inspection, runtime RSS
  and input latency on SL101; compare installed binary commit before claiming
  the device shows new changes.
- Pixel oracle for the Sway/Grate opaque-vs-transparent background and
  screenshot/capture path. A screenshot alone does not localize GPU damage.
- Do not reboot, suspend, replace the active compositor, or deploy in this
  design and host-test task.
