@projmgrauth
Set repo: nuraloumi

# NuraLoumi Wave 2 — Extensible Configuration Schema (M6)

Lane: config-schema
Priority: 95

## Mission

Evolve the existing `ShellConfig` (TOML/JSON, XDG discovery via
`load_config_or_default`) into a modular per-surface configuration that new
binaries inherit without code changes. This is foundational — all other wave 2
surfaces depend on the config contract being extensible.

Read:
- AGENTS.md, DESIGN.md, ROADMAP.md (patch releases + M6 section)
- docs/ARCHITECTURE.md, docs/CONTRACTS.md
- docs/CONFIGURATION.md
- crates/nuraloumi-shell/src/lib.rs (current ShellConfig struct and load_config_or_default)
- examples/nuraloumi-config.toml

## Exclusive write scope

- crates/nuraloumi-shell/src/lib.rs (config types and loading only)
- crates/nuraloumi-shell/src/config.rs (new file if extracted)
- docs/CONFIGURATION.md
- examples/nuraloumi-config.toml

Do not edit rendering, Wayland backend, providers, core model, or binary entry
points beyond updating their config loading calls.

## Requirements

### 1. Per-surface config sections
Extend `ShellConfig` (or a new `NuraConfig` wrapper) with typed per-surface
sections keyed by surface name:

```toml
[surface.music]
show_album_art = false

[surface.resources]
refresh_interval_ms = 5000

[surface.calendar]
ical_path = "~/.local/share/khal/calendars"
```

Unknown surface sections must be preserved and silently ignored (forward
compatibility for third-party surfaces). Use `#[serde(flatten)]` with
`HashMap<String, toml::Value>` or equivalent for the extensible surface map.

### 2. Typed provider config
Add provider-level configuration:

```toml
[providers]
terminal = "foot"
editor = "nvim"
weather_url = "https://wttr.in/?format=j1"
weather_cache_seconds = 1800

[providers.media]
backend = "playerctl"

[providers.resources]
refresh_interval_ms = 5000
top_process_count = 10
```

### 3. Quick-launch slots
Panel touch targets mapped to surface binaries or .desktop entries:

```toml
[[panel.quick_launch]]
label = "Term"
glyph = ""
command = "nuraloumi-terminal"

[[panel.quick_launch]]
label = "Edit"
glyph = ""
command = "nuraloumi-editor"

[[panel.quick_launch]]
label = "Files"
desktop = "org.gnome.Nautilus.desktop"
```

### 4. Schema versioning
- Add `config_version = 1` field at top level.
- Clear error on unsupported version.
- Forward-compatible: unknown top-level keys warn but don't fail.
- Strict sections (like `[keybindings]`) keep `deny_unknown_fields`.

### 5. Config validation subcommand
Add a `nuraloumi-config` binary (or xtask subcommand) that parses a config
file and reports warnings/errors without launching a surface:

```
nuraloumi-config validate [--config path]
nuraloumi-config dump-defaults
```

### 6. Backward compatibility
- Existing `ShellConfig` fields remain at top level: `panel_edge`,
  `panel_height`, `menu_width`, `row_height`, `theme`, `reduced_motion`,
  `launcher`, `menu_dismissal`, `keybindings`.
- A config.toml without `config_version` is treated as version 0 (current
  schema) and loads without error.
- All existing binary entry points continue to work unchanged.

## Constraints
- ARMv7/musl cross-build must pass.
- No new crate dependencies beyond serde/toml (already present).
- Config parsing errors must be clear and actionable (file path, line number
  when available, what was expected).
- The 35 MiB RSS budget is not affected (config is small and loaded once).

## Acceptance
1. `examples/nuraloumi-config.toml` demonstrates all new sections.
2. `docs/CONFIGURATION.md` updated with the new schema.
3. Existing surfaces load the new config format without regression.
4. Unknown `[surface.xyz]` sections are preserved and ignored.
5. `nuraloumi-config validate` parses and reports errors.
6. `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` pass.
