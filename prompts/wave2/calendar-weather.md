@projmgrauth
Set repo: nuraloumi

# NuraLoumi Wave 2 — Calendar Widget and Weather Provider (0.0.8)

Lane: calendar-weather
Priority: 84

## Mission

Create a `nuraloumi-calendar` surface binary with a month-grid calendar view
and optional khal/ics integration, plus a weather provider and display
integrated into the system sheet.

Read:
- AGENTS.md, DESIGN.md, ROADMAP.md (0.0.8 section)
- docs/ARCHITECTURE.md, docs/CONTRACTS.md
- crates/nuraloumi-providers/src/command.rs (CommandRunner, CommandSpec)
- crates/nuraloumi-providers/src/common.rs (Provider, SnapshotMeta, Health)
- crates/nuraloumi-providers/src/clock.rs (existing clock provider pattern)
- crates/nuraloumi-shell/src/bin/windows.rs (dedicated binary pattern)
- crates/nuraloumi-render-cairo/src/model.rs (CoreMenuAdapter)

## Exclusive write scope

- crates/nuraloumi-providers/src/calendar.rs (new)
- crates/nuraloumi-providers/src/weather.rs (new)
- crates/nuraloumi-providers/src/lib.rs (register new modules)
- crates/nuraloumi-shell/src/bin/calendar.rs (new)
- crates/nuraloumi-shell/Cargo.toml ([[bin]] entry only)

Do not edit renderer, Wayland backend, core model, or existing provider files.

## Calendar provider (crates/nuraloumi-providers/src/calendar.rs)

### CalendarSnapshot

Struct `CalendarSnapshot`:
- meta: SnapshotMeta
- today: CalendarDate
- month_days: Vec of CalendarDay
- events: Vec of CalendarEvent
- issues: Vec of String

Struct `CalendarDate`:
- year: u16
- month: u8
- day: u8
- weekday: u8 (0=Mon..6=Sun)

Struct `CalendarDay`:
- date: CalendarDate
- has_events: bool
- event_count: u8

Struct `CalendarEvent`:
- date: CalendarDate
- time: Option of String ("14:30" or None for all-day)
- summary: String
- location: Option of String

### Data sources (tried in order)
1. **khal** (if installed): `khal list --format "{start-date} {start-time} {title} {location}" TODAY 31d`
   Parse output lines into CalendarEvent entries. Bounded: 2s timeout, 64 KiB.
2. **Local .ics files**: if `[surface.calendar] ical_path` is configured,
   scan that directory for .ics files and parse VEVENT entries with basic
   ical parsing (DTSTART, SUMMARY, LOCATION). No recurrence expansion in v1.
3. **No backend**: calendar grid still works with just date arithmetic;
   events list is empty.

### Date arithmetic
Use standard library or minimal date calculation (no chrono dependency).
The year/month/day/weekday can be computed from Unix timestamp with basic
arithmetic. Need: days-in-month, day-of-week for first-of-month, today's date.

## nuraloumi-calendar binary

### Layout
Month grid rendered as a menu-like surface:

- **Header row**: `< November 2026 >` with left/right navigation.
- **Weekday header**: `Mo Tu We Th Fr Sa Su` (compact, fixed-width).
- **Day grid**: 6 rows x 7 columns of day numbers. Today highlighted with
  selected style. Days with events show a dot indicator (glyph or bullet).
  Days outside current month shown in hint color.
- **Event list section** (below grid): events for the selected day, showing
  time and summary. Empty: "No events".

### Interaction
- Arrow keys: move selection between days in the grid.
- Left/Right on header row (or PageUp/PageDown): previous/next month.
- Enter on a day: scroll to its event list.
- `t`: jump to today.
- `r` or F5: refresh events from provider.
- `q` or Escape: close.

### Rendering approach
The day grid does not map cleanly to a flat MenuModel list. Two options:
1. **Preferred**: render the grid as 6 text/status rows with 7 fixed-width
   day cells per row, using the subtitle for event dots. Selection tracks
   which cell within the row.
2. **Fallback**: render as a flat list of week rows where each row's label
   is `"01  02  03  04  05  06  07"` — simpler but less interactive.

Use option 1 if the renderer supports inline selection within a row's text;
otherwise use option 2 with the row representing the selected week.

## Weather provider (crates/nuraloumi-providers/src/weather.rs)

### WeatherSnapshot

Struct `WeatherSnapshot`:
- meta: SnapshotMeta
- backend: String
- temperature_c: Option of i8
- condition: Option of String ("Sunny", "Cloudy", "Rain", etc.)
- condition_glyph: Option of String (weather glyph)
- humidity_percent: Option of u8
- wind_kmh: Option of u16
- forecast: Vec of ForecastDay
- location: Option of String
- issues: Vec of String

Struct `ForecastDay`:
- label: String ("Today", "Tomorrow", "Wed")
- high_c: i8
- low_c: i8
- condition: String
- glyph: String

### Data source
- **wttr.in**: `curl -s "wttr.in/?format=j1"` — returns JSON with current
  conditions and 3-day forecast. Bounded: 5s timeout, 64 KiB.
- Config: `[providers.weather] url`, `location` (default: auto-detect by IP),
  `cache_seconds` (default: 1800 = 30 min).
- Cached: store last successful response and serve from cache until
  `cache_seconds` expires. Weather provider never blocks the UI.
- Offline: if fetch fails and cache exists, serve stale cache with
  "last updated N min ago" in issues. If no cache, report unavailable.

### Condition-to-glyph mapping
Map wttr.in condition codes to weather glyphs:
- Sunny/Clear: ☀ /
- Partly cloudy: ⛅ /
- Cloudy: ☁ /
- Rain: 🌧 /
- Snow: ❄ /
- Thunder: ⚡ /
- Fog: 🌫 /

## Weather display integration

The weather display will be integrated into the system sheet by the shell
lane in a follow-up. This task delivers the provider and snapshot types.
Include a `nuraloumi-weather` fixture surface for testing:

```
nuraloumi-calendar --fixture     # demo month grid with fake events
nuraloumi-weather --fixture      # demo weather display (temp provider test)
```

The weather fixture surface is a minimal surface showing current conditions
and 3-day forecast as menu rows.

## Constraints
- No chrono or time crate dependency. Date arithmetic from std or manual.
- No HTTP client dependency. Use `curl` via CommandRunner (already the
  pattern for playerctl/nmcli).
- ical parsing: minimal VEVENT extraction only (DTSTART, SUMMARY, LOCATION,
  DTEND). No recurrence (RRULE), no timezone conversion, no VALARM. Unknown
  properties are skipped.
- khal output parsing: line-oriented, bounded.
- ARMv7/musl cross-build must pass.
- Weather fetch must not block UI (check cache first, fetch in background
  or on explicit refresh).

## Acceptance
1. `nuraloumi-calendar --fixture` shows month grid with fake events.
2. Day navigation and month switching work.
3. CalendarProvider with khal backend parses sample khal output correctly.
4. WeatherProvider with fixture data produces valid WeatherSnapshot.
5. Weather cache logic: serves stale cache when fetch fails.
6. Both new provider modules registered in lib.rs.
7. `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` pass.
