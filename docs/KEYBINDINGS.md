# Transparent keybindings

Status: the single-action keybinding registry, configuration, resolver, and
introspection contract is implemented for the 0.0.3 line. Combined actions
remain a later stage.

## Goal

NuraLoumi keybindings are **transparent**:

- every effective binding can be enumerated and explained;
- built-in and user-defined bindings use the same data model;
- configuration uses the same notation that diagnostics emit;
- input normalization is separate from action execution;
- conflicts are deterministic and visible rather than silently resolved;
- bindings point to registered typed actions, never arbitrary shell commands;
- future combined actions must compose those same typed actions without
  bypassing confirmation or provider safety gates.

The physical SL101 keyboard remains usable with no user configuration because
the normal navigation keys are themselves entries in the built-in registry.

## Architecture

~~~text
Wayland key event
    |
    v
physical key + modifier normalization
    |
    v
KeyStroke { modifiers, key }
    |
    v
canonical notation: "Ctrl+Alt+Left"
    |
    v
scope-aware binding resolver
    |          |
    |          +--> --dump-keybindings / future shortcut UI
    v
ActionSpec
    |
    v
existing shell semantic / typed-action dispatch
    |
    +--> provider capability, dry-run, and confirmation gates remain authoritative
~~~

The Wayland crate normalizes physical key and modifier state. The shell owns
notation parsing, binding resolution, configuration, and action dispatch. The
renderer owns no shortcuts, and providers do not parse keyboard notation.

No XKB runtime dependency is introduced for this implementation. The Wayland
backend maps the bounded Linux input-code set NuraLoumi needs for navigation,
US-layout printable search input, F1-F24, and supported XF86 media/brightness
keys. Unknown codes remain raw diagnostic keys and cannot be configured.

## Canonical key notation

Examples:

~~~text
Enter
Escape
Super+Space
Ctrl+Alt+Left
Ctrl+Shift+F12
XF86AudioRaiseVolume
~~~

Rules:

1. modifiers are emitted in canonical order: `Ctrl+Alt+Shift+Super`;
2. canonical modifier names are exactly `Ctrl`, `Alt`, `Shift`, and
   `Super`;
3. the parser also accepts `Control` → `Ctrl`, `Meta` → `Super`, and
   whitespace around components;
4. stable named keys include `Enter`, `Escape`, `Backspace`, `Tab`,
   `Space`, arrows, and `F1` through `F24`;
5. letters are canonicalized to uppercase; digits retain their digit spelling;
6. punctuation uses stable names such as `Minus`, `Equal`,
   `BracketLeft`, `BracketRight`, `Semicolon`, `Apostrophe`,
   `Grave`, `Backslash`, `Comma`, `Period`, and `Slash`;
7. supported system keys use `XF86AudioMute`, `XF86AudioLowerVolume`,
   `XF86AudioRaiseVolume`, `XF86AudioPrev`, `XF86AudioPlay`,
   `XF86AudioNext`, `XF86AudioStop`, `XF86MonBrightnessDown`, and
   `XF86MonBrightnessUp`;
8. raw numeric keycodes are not normal configuration.

### Sequences

Multi-stroke sequences are reserved for a later stage:

~~~text
Super+K, W
~~~

A comma currently makes configuration validation fail. NuraLoumi never
partially interprets a sequence as a single chord.

## Binding identity

Every binding has a stable semantic ID independent of its current chord:

~~~text
menu.up
menu.down
menu.activate
menu.escape
audio.volume-up
user.audio-up
~~~

Changing `menu.down` from `Down` to `Ctrl+J` remains an override of the
same binding rather than creating a new anonymous shortcut.

Binding IDs are not action IDs. A binding says **when** to dispatch; an action
descriptor says **what** to dispatch.

## Built-in bindings

The initial built-in registry contains:

| Binding ID | Keys | Scope | Action |
| --- | --- | --- | --- |
| `menu.up` | `Up` | `menu` | navigate up |
| `menu.down` | `Down` | `menu` | navigate down |
| `menu.left` | `Left` | `menu` | navigate left/back |
| `menu.right` | `Right` | `menu` | navigate right/enter |
| `menu.activate` | `Enter` | `menu` | activate |
| `menu.escape` | `Escape` | `menu` | back-or-close |
| `menu.backspace` | `Backspace` | `menu` | search backspace |
| `audio.volume-down` | `XF86AudioLowerVolume` | `global` | audio −5 |
| `audio.volume-up` | `XF86AudioRaiseVolume` | `global` | audio +5 |
| `audio.mute` | `XF86AudioMute` | `global` | toggle mute |
| `display.brightness-down` | `XF86MonBrightnessDown` | `global` | brightness −10 |
| `display.brightness-up` | `XF86MonBrightnessUp` | `global` | brightness +10 |

Backspace remains harmless outside focused search because the existing shell
semantic layer rejects search input when search does not own text focus.

## Scopes

Supported scopes are:

~~~text
global
panel
menu
launcher
control-center
windows
applications
tasks
desktops
~~~

Resolution is most-specific first. Launcher overview scopes
(`windows`, `applications`, `tasks`, `desktops`) precede
`launcher`; launcher/control-center precede `menu`; `menu` precedes
`global`.

Two enabled bindings with the same normalized chord and same scope are a
configuration error. Declaration order never chooses a winner. The same chord
may intentionally exist at different scope levels because the more-specific
scope resolves first.

`global` is deliberately **not compositor-global**. It means any
keyboard-focused NuraLoumi transient menu. The persistent panel itself requests
no keyboard focus, so NuraLoumi cannot make `Super+Space` a desktop-wide
launcher hotkey without compositor cooperation.

The `panel` scope is reserved for a future explicitly keyboard-interactive
panel mode; it does not cause the current static panel to consume keyboard
input.

## Typed action descriptors

Bindings target a bounded typed descriptor:

~~~rust
struct ActionSpec {
    id: String,
    direction: Option<String>,
    family: Option<String>,
    mode: Option<String>,
    tab: Option<String>,
    delta: Option<i32>,
}
~~~

Implemented action IDs are:

| Action ID | Arguments | Result |
| --- | --- | --- |
| `menu.navigate` | `direction = "up"|"down"|"left"|"right"` | semantic navigation |
| `menu.activate` | none | activate selected row |
| `menu.back-or-close` | none | back/escape semantics |
| `menu.backspace` | none | search backspace |
| `menu.open` | `family = "..."` | existing menu-family transition |
| `overview.mode` | `mode = "..."` | launcher overview mode |
| `control.tab` | `tab = "..."` | control-center tab |
| `audio.adjust` | bounded non-zero `delta` | `audio.volume` provider action |
| `audio.toggle-mute` | none | `audio.mute` provider action |
| `display.adjust` | bounded non-zero `delta` | `system.brightness` provider action |
| `network.toggle-wifi` | none | `network.wifi` provider action |
| `bluetooth.toggle-radio` | none | `bluetooth.radio` provider action |

Audio/display deltas are bounded to non-zero values with absolute value at most
20 per activation. Unknown action IDs, missing/extra arguments, and invalid
family/mode/tab values fail startup validation.

There is deliberately no generic `command`, `exec`, or arbitrary custom
action field. Provider actions continue through the existing live/dry-run
capability boundary.

## Configuration model

~~~toml
[keybindings]
enabled = true

# Override one built-in by stable ID. Missing fields inherit.
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
~~~

Built-ins and user entries are merged into one registry. For a user entry whose
ID matches a built-in, omitted `keys`, `scope`, and `action` inherit from
that built-in.

An enabled custom ID must provide `keys`, `scope`, and `action`.
Duplicate user IDs are rejected. Disabling a custom ID that has no built-in
definition is also rejected because it has nothing to disable.

`[keybindings] enabled = false` disables **user customization**, not the
keyboard baseline: built-in bindings remain active.

Bounds:

- at most 128 user binding entries;
- binding IDs: 1–128 bytes, letters/digits plus `.`, `-`, and `_`;
- key notation: at most 96 bytes.

## Resolution and precedence

~~~text
built-in registry
    |
    v
user overrides by stable binding ID
    |
    v
notation/action validation + same-scope conflict detection
    |
    v
effective registry
    |
    v
specific scope -> menu -> global runtime lookup
~~~

The effective registry is immutable during a dispatch operation. Live reload
is not implemented in 0.0.3.

## Introspection

Both shell binaries expose the effective keymap without opening a Wayland
surface:

~~~sh
nuraloumi-menu --dump-keybindings
nuraloumi-menu --config ~/.config/nuraloumi/config.toml --dump-keybindings
nuraloumi-panel --dump-keybindings
~~~

The JSON schema is `nuraloumi-keybindings/v1`. Each binding reports:

- stable `id`;
- canonical `keys`;
- `scope`;
- typed `action`;
- `source` as `built_in` or `user_config`;
- `enabled` and `effective`;
- `replaces` when a user entry overrides a built-in ID.

This data can later feed a Keyboard Shortcuts settings page, contextual menu
hints, documentation generation, and conflict diagnostics.

## Search/text interaction

Text input remains special while launcher search owns focus:

- an unmodified printable key is delivered to search text;
- Shift may change case/punctuation and still counts as text input;
- Ctrl, Alt, or Super makes the physical key eligible for chord resolution;
- navigation keys still resolve through their built-in binding entries;
- Backspace resolves through the built-in registry and then existing search
  focus semantics.

This prevents a user-defined unmodified `A` shortcut from stealing normal
typing while search is focused, while `Ctrl+A` remains usable as a binding.

## Future combined actions

Combined actions are **not implemented in 0.0.3**. A future representation may
look like:

~~~toml
action = { sequence = [
  { id = "audio.unmute" },
  { id = "audio.adjust", delta = 5 },
  { id = "menu.open", family = "audio" },
] }
~~~

Any later composition design should keep these rules:

- only registered typed actions may appear;
- bounded sequence length;
- ordered execution and stop-on-failure by default;
- no loops, recursion, arbitrary commands, or embedded scripting;
- confirmation pauses composition and uses the normal confirmation model;
- provider/destructive safety gates cannot be weakened;
- introspection expands every step.

Conditionals, parallel actions, delays, and general scripting remain out of
scope until a concrete use case and separate safety/design review.

## Remaining work after 0.0.3

- a dedicated Keyboard Shortcuts settings surface;
- contextual shortcut hints sourced from the same registry;
- optional atomic configuration reload;
- bounded combined actions in a separately reviewed later stage.

## Non-goals

- arbitrary shell-command shortcuts;
- pretending NuraLoumi owns compositor-global shortcuts;
- remapping ordinary search text behind the focused search field;
- hidden precedence based on declaration order;
- combined actions before the single-action model has field use.
