# Configuration

NuraLoumi uses one shell configuration for both `nuraloumi-menu` and `nuraloumi-panel`. TOML is the recommended format; JSON is also accepted.

The configuration is deliberately small. It controls presentation and launcher preferences while executable application authority remains with XDG `.desktop` files discovered by the application provider.

## Discovery and precedence

An explicit CLI file always wins:

```sh
nuraloumi-menu --config /path/to/config.toml --family launcher
nuraloumi-panel --config /path/to/config.toml --live
```

Without `--config`, the shell selects one configuration root:

1. an **absolute** `$XDG_CONFIG_HOME`, when set;
2. otherwise `$HOME/.config`;
3. otherwise no user configuration root.

Inside that root NuraLoumi tries, in order:

```text
nuraloumi/config.toml
nuraloumi/config.json
```

If neither file exists, built-in defaults are used. If the selected file exists but is invalid, startup fails with a clear configuration error rather than silently ignoring it.

The CLI flag `--reduced-motion` can additionally force reduced motion on after the file is loaded.

## Complete TOML example

The repository copy at `examples/nuraloumi-config.toml` is intended to be copied and edited:

```toml
panel_edge = "top"
panel_height = 40
menu_width = 448
row_height = 48
theme = "dark"
reduced_motion = false

[launcher]
pinned = [
  "foot.desktop",
  "firefox.desktop",
]
hidden = [
  "org.example.LegacyTool.desktop",
]

[launcher.labels]
"firefox.desktop" = "Web"
"foot.desktop" = "Terminal"

[launcher.app_id_aliases]
"firefox.desktop" = ["firefox", "firefox-esr"]
```

Install it with:

```sh
config_root="${XDG_CONFIG_HOME:-$HOME/.config}"
case "$config_root" in
  /*) ;;
  *) config_root="$HOME/.config" ;;
esac
mkdir -p "$config_root/nuraloumi"
cp examples/nuraloumi-config.toml "$config_root/nuraloumi/config.toml"
```

## Shell fields

| Key | Type | Default | Validation |
| --- | --- | --- | --- |
| `panel_edge` | `top`, `bottom`, `left`, or `right` | `top` | enum |
| `panel_height` | integer | `40` | 40..72 logical px |
| `menu_width` | integer | `448` | 320..720 logical px |
| `row_height` | integer | `48` | 40..72 logical px |
| `theme` | `dark` or `light` | `dark` | enum |
| `reduced_motion` | boolean | `false` | boolean |

Unknown or malformed values fail configuration loading instead of being guessed.

## Launcher preferences

### `launcher.pinned`

A list of discovered desktop IDs in desired priority order. Matching entries move to the front while the original deterministic order of all other discovered entries is preserved.

An ID that is not currently discovered does not create an application entry.

### `launcher.hidden`

A list of discovered desktop IDs to omit from the launcher.

This is a presentation filter only. It does not uninstall, disable, or modify the underlying desktop file.

### `launcher.labels`

A TOML table mapping desktop ID to a display label:

```toml
[launcher.labels]
"firefox.desktop" = "Web"
```

Only the visible label changes. Launch authority and executable metadata still come from the discovered desktop file.

### `launcher.app_id_aliases`

A TOML table mapping a desktop ID to additional exact Wayland toplevel `app_id` strings:

```toml
[launcher.app_id_aliases]
"sl101-firefox-debian.desktop" = ["firefox-esr"]
```

NuraLoumi normally derives the expected `app_id` from the desktop ID stem. Aliases are useful when an application publishes a different compositor `app_id`.

Window reuse is intentionally conservative:

- only focusable toplevels are considered;
- matching is exact, not substring or fuzzy matching;
- a single unambiguous matching window may be focused;
- if it is already focused, no duplicate launch is requested;
- zero matches or multiple matches fall back to the ordinary desktop launch path;
- the opaque foreign-toplevel ID remains the only control identity.

Aliases therefore help reuse an existing window without turning titles or labels into control identifiers.

## Desktop IDs and command safety

Launcher preference keys must be desktop IDs ending in `.desktop`. They may not contain path separators, NULs, or line breaks.

The config file cannot define an `Exec` command. Applications continue to come from the XDG application directories and are launched through the provider's validated desktop-file path. Pinning, hiding, labels, and aliases only operate on entries that discovery already produced.

This keeps a user-editable menu configuration from becoming a second executable registry.

## Limits

To keep parsing bounded on constrained targets:

- each launcher preference collection is limited to 64 desktop IDs;
- each desktop ID is limited to 512 bytes and must end in `.desktop`;
- labels must be non-empty, single-line, and at most 128 characters;
- each desktop entry may define at most 8 `app_id` aliases;
- aliases must be simple single-line identifiers without `/`.

## Checking a configuration

The committed example should remain parseable by the same shell configuration loader:

```sh
cargo test -p nuraloumi-shell --locked
cargo run -p nuraloumi-shell --bin nuraloumi-menu -- --config examples/nuraloumi-config.toml --family launcher
```

For deterministic menu/provider development, the existing fixtures under `examples/menu-fixtures/` remain available separately from user configuration.
