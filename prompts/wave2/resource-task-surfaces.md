@projmgrauth
Set repo: nuraloumi

# NuraLoumi Wave 2 — Resource Monitor and Task Manager Surfaces (0.0.5)

Lane: resource-task-surfaces
Priority: 90

## Mission

Create two new dedicated surface binaries — `nuraloumi-resources` (system
resource monitor) and `nuraloumi-tasks` (process/task manager) — using the
existing `ResourceProvider` and `ProcessProvider` infrastructure. These are
the first "utility surface" binaries beyond the core panel/menu/launcher.

Read:
- AGENTS.md, DESIGN.md, ROADMAP.md (0.0.5 section)
- docs/ARCHITECTURE.md, docs/CONTRACTS.md
- crates/nuraloumi-providers/src/resources.rs (ResourceProvider, ResourceSnapshot, CpuUsage, MemoryStats, ThermalReading, DiskUsage, NetworkIoRate, DiskIoRate)
- crates/nuraloumi-providers/src/processes.rs (ProcessProvider, ProcessSnapshot, ProcessEntry)
- crates/nuraloumi-shell/src/bin/panel.rs (existing provider wiring pattern)
- crates/nuraloumi-shell/src/bin/windows.rs (existing dedicated binary pattern)
- crates/nuraloumi-render-cairo/src/model.rs (CoreMenuAdapter, Scene building)

## Exclusive write scope

- crates/nuraloumi-shell/src/bin/resources.rs (new)
- crates/nuraloumi-shell/src/bin/tasks.rs (new)
- crates/nuraloumi-shell/Cargo.toml ([[bin]] entries only)

Do not edit providers, renderer, Wayland backend, or core model. Use existing
APIs only. Follow the pattern established by windows.rs and files.rs for
dedicated surface binaries.

## nuraloumi-resources binary

Full-surface resource monitor showing live system data from ResourceProvider.

### Layout (single scrollable menu surface)
- **CPU section**: one row per core showing `core N: XX%` with a visual bar
  (use subtitle field for ASCII bar like `[████░░░░░░] 42%`). Aggregate CPU
  row at top.
- **Memory section**: total/used/available in MiB. Swap if present.
- **Disk section**: root filesystem usage percent and free space.
- **Thermal section**: one row per thermal zone with temperature in C.
- **Network I/O section**: rx/tx rates in human-readable units (KB/s, MB/s).
- **Disk I/O section**: read/write rates.
- **Top processes subsection**: top 10 processes by CPU, showing name, PID,
  CPU %, memory MiB.

### Interaction
- Keyboard: `r` or F5 to refresh snapshot (no continuous polling).
- `q` or Escape to close.
- Arrow keys to scroll through sections.
- Search/filter to find specific processes or readings.

### Data flow
1. On open: call `ResourceProvider::system().snapshot()`.
2. Convert `ResourceSnapshot` fields into `MenuModel` sections.
3. Render via existing `CoreMenuAdapter` and Cairo pipeline.
4. On refresh keybinding: re-snapshot and rebuild model.

## nuraloumi-tasks binary

Process/task manager surface for viewing and managing system processes.

### Layout
- Sortable process list showing: name, PID, state, CPU %, memory MiB.
- Default sort: CPU % descending.
- Section header showing total process count.

### Interaction
- Arrow keys: navigate process list.
- `s` to cycle sort mode: CPU / memory / name / PID.
- Search/filter: type to filter by process name.
- `k` or Delete on selected process: SIGTERM with confirmation gate.
  Confirmation row: "Kill [process name] (PID XXXXX)? Press Enter to confirm."
  The kill action uses the existing bounded CommandRunner with
  `kill -TERM PID`.
- `r` or F5 to refresh.
- `q` or Escape to close.

### Data flow
1. On open: call `ProcessProvider::system().snapshot()`.
2. Convert `ProcessSnapshot.processes` into `MenuModel` with sortable items.
3. Kill action: use `CommandRunner` with `kill -TERM PID`, bounded timeout.
4. After kill: auto-refresh to show updated process list.

## Shared patterns

Both binaries follow the existing dedicated binary pattern:
- CLI args: `--config PATH`, `--fixture` (use fixture/demo data).
- Fixture mode: hardcoded demo snapshot for testing without real system.
- Layer-shell popup role (same as windows/files binaries).
- Use `load_config_or_default()` for config.
- Use `render_menu_with_glyphs()` for rendering.
- Use `dismiss_backdrop_margins()` for dismiss behavior.

## Constraints
- No new dependencies. Use existing provider and shell APIs.
- ARMv7/musl cross-build must pass.
- No continuous polling — refresh on demand only.
- SIGTERM action requires confirmation (never SIGKILL).
- Process kill is bounded: 2-second timeout, max 16 KiB output.
- Fixture mode must work without procfs (for host testing).

## Acceptance
1. `nuraloumi-resources --fixture` opens and displays demo resource data.
2. `nuraloumi-tasks --fixture` opens and displays demo process list.
3. Sort cycling works in tasks surface.
4. Kill-with-confirmation flow works (test with fixture mode).
5. Both binaries added to Cargo.toml [[bin]] entries.
6. `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` pass.
