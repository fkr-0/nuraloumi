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

[menu_dismissal]
outside_press = true
after_one_shot_action = true

[keybindings]
enabled = true

[[keybindings.bindings]]
id = "user.audio-up"
keys = "Ctrl+Alt+Up"
scope = "global"
action = { id = "audio.adjust", delta = 5 }

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

## Transient menu dismissal

The persistent panel is never part of transient-menu dismissal. When a
panel-owned menu opens, NuraLoumi can place a transparent layer-shell backdrop
behind that menu while leaving the panel strip uncovered.

### `menu_dismissal.outside_press`

Default: `true`.

When enabled:

- pointer press or touch-down on the transparent backdrop closes the transient
  menu;
- pointer press or touch-down on blank, non-actionable space inside the menu
  also closes it;
- the static panel remains visible and interactive;
- presses are ignored for dismissal while menu entry animation is still
  blocking hit testing, preventing an opening animation from being mistaken for
  a blank-space click.

Set it to `false` to keep transient menus open until an explicit menu action,
Escape/back navigation, or compositor close.

### `menu_dismissal.after_one_shot_action`

Default: `true`.

When enabled, a successfully completed action that navigates away from the
current menu closes the transient menu:

- application launch;
- window focus;
- desktop/workspace switch.

Continuous or in-menu controls stay open. This includes brightness and volume
adjustments, toggles, `menu.open`, launcher overview-mode changes, and
control-center tab changes.

This policy is based on semantic action kind rather than individual UI rows, so
pointer, touch and keyboard activation share the same close behavior.

## Keybindings

Keybindings use one validated registry shared by built-in navigation and user
configuration. The same canonical notation appears in configuration and in
structured introspection.

```toml
[keybindings]
enabled = true

# Override a built-in by stable ID. Omitted scope/action inherit the built-in.
[[keybindings.bindings]]
id = "menu.down"
keys = "Ctrl+J"

# Add a new typed binding.
[[keybindings.bindings]]
id = "user.audio-up"
keys = "Ctrl+Alt+Up"
scope = "global"
action = { id = "audio.adjust", delta = 5 }

# Disable one built-in explicitly.
[[keybindings.bindings]]
id = "menu.escape"
enabled = false
```

### Canonical notation

Modifiers are emitted in `Ctrl+Alt+Shift+Super` order. The parser accepts
`Control` as an alias for `Ctrl`, `Meta` as an alias for `Super`, and
whitespace around components.

Named keys include navigation keys, `Tab`, `Space`, `F1` through `F24`,
letters/digits, stable punctuation names, and the supported XF86 audio and
display-brightness keys. Raw numeric keycodes cannot be configured.

Multi-stroke sequences such as `Super+K, W` are reserved for a later design
stage and currently fail validation.

### Scopes and precedence

Supported scopes are:

```text
global
panel
menu
launcher
control-center
windows
applications
tasks
desktops
```

Specific launcher/control scopes resolve before `menu`, and `menu` resolves
before `global`. Different IDs with the same canonical key and same scope are
an error; declaration order never chooses a winner.

`global` means every **keyboard-focused NuraLoumi transient menu**. It is not
a compositor-global hotkey. The persistent panel intentionally has keyboard
interactivity disabled, so desktop-wide shortcuts still belong in the
compositor. The `panel` scope is reserved for a future explicitly
keyboard-interactive panel mode.

### Typed actions

User bindings can target only the typed whitelist documented in
`docs/KEYBINDINGS.md`: menu navigation/activation/backspace, menu-family
transitions, launcher modes, control-center tabs, bounded audio/brightness
adjustments, mute, Wi-Fi radio, and Bluetooth radio.

There is no shell-command or generic arbitrary-action field. Resolved provider
actions still pass through the same live/dry-run capability boundaries used by
menu rows.

### Search interaction

While launcher search has text focus, unmodified printable keys remain text
input; Shift may alter case/punctuation. `Ctrl`, `Alt`, or `Super` chords
can still resolve as bindings, and navigation keys continue to use the built-in
registry.

### Overrides, disabling, and limits

- A user entry whose `id` matches a built-in overrides that stable binding.
  Missing `keys`, `scope`, or `action` inherit from the built-in.
- An enabled custom ID must supply all three of those fields.
- `enabled = false` on a binding disables that binding.
- `[keybindings] enabled = false` disables user customization while retaining
  the built-in keyboard baseline.
- At most 128 user binding entries are accepted.
- IDs are bounded to 128 bytes and use letters, digits, `.`, `-`, or `_`.
- Key notation is bounded to 96 bytes.

### Introspection

Both shell binaries print the validated effective registry without opening a
Wayland surface:

```sh
nuraloumi-menu --dump-keybindings
nuraloumi-menu --config ~/.config/nuraloumi/config.toml --dump-keybindings
nuraloumi-panel --dump-keybindings
```

The JSON schema `nuraloumi-keybindings/v1` contains binding ID, canonical
keys, scope, typed action, source, enabled/effective state, and replacement
provenance.

See `docs/KEYBINDINGS.md` for the complete implemented single-action
contract. Combined actions remain a later-stage design.

## Launcher preferences

### `launcher.pinned`

A list of discovered desktop IDs in desired favorite order. Matching entries are shown once in a dedicated **Favorites** section before the general **Applications** section. Non-pinned applications retain their deterministic desktop-entry discovery order.

An ID that is not currently discovered does not create an application entry. Pinned entries remain ordinary discovered XDG applications; the config only changes launcher presentation.

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
