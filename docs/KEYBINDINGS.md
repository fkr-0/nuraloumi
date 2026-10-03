# Transparent keybinding design

Status: design contract for a later implementation stage. No configurable
keybinding runtime is enabled by this document alone.

## Goal

NuraLoumi keybindings should be **transparent**:

- every effective binding can be enumerated and explained;
- built-in and user-defined bindings use the same data model;
- configuration uses the same notation that the UI and diagnostics display;
- input normalization is separate from action execution;
- conflicts are deterministic and visible rather than silently resolved;
- bindings point to registered typed actions, never arbitrary shell commands;
- future combined actions compose those same typed actions without bypassing
  confirmation or provider safety gates.

The physical SL101 keyboard must remain fully usable when no user
configuration exists. The first implementation should therefore reproduce the
current keyboard behavior from a built-in binding registry before adding any
new shortcuts.

## Architecture

~~~text
Wayland key event
    |
    v
physical key normalization
    |
    v
KeyStroke { modifiers, key }
    |
    v
canonical notation: "Ctrl+Alt+Left"
    |
    v
binding resolver
    |          |
    |          +--> introspection view / shortcut UI / diagnostics
    v
ActionSpec
    |
    v
existing shell typed-action dispatch
    |
    +--> confirmation/provider/capability gates remain authoritative
~~~

The Wayland crate should normalize physical key state. The shell should own
binding resolution and action dispatch. The renderer must not own shortcuts,
and providers must not parse keyboard notation.

## Canonical key notation

One canonical textual notation is used in configuration, diagnostics and UI.

### Chords

~~~text
Enter
Escape
Super+Space
Ctrl+Alt+Left
Ctrl+Shift+F12
XF86AudioRaiseVolume
~~~

Rules:

1. modifiers are written in canonical order:
   Ctrl+Alt+Shift+Super;
2. modifier names are exactly Ctrl, Alt, Shift, and Super;
3. named keys use stable names such as Enter, Escape, Backspace, Space, Tab,
   Up, Down, Left, Right, and F1 through F24;
4. media/system keys use their conventional XKB/XF86 names when a stable name
   exists;
5. printable letter key names are normalized to uppercase (A, not a);
6. textual input remains text input and is not represented as hundreds of
   implicit bindings;
7. raw numeric keycodes are diagnostic data, not normal user configuration.

Equivalent spellings such as "super + space", "Meta+Space", or
"Control+Alt+Left" may be accepted by a future parser as conveniences, but
introspection must always emit the canonical spelling above.

### Sequences

Multi-stroke key sequences are deliberately out of scope for the first
implementation. The grammar should reserve comma as a future sequence
separator:

~~~text
Super+K, W
~~~

Until sequence support is implemented, such input must fail configuration
validation instead of being partially interpreted.

## Binding identity

Every binding has a stable semantic ID independent of its current chord.

~~~text
menu.up
menu.down
menu.activate
menu.back
launcher.toggle
audio.volume-up
user.browser
~~~

This is important for transparency: changing launcher.toggle from Super+Space
to Super+A should still be recognizable as an override of the same binding
rather than a new anonymous entry.

Binding IDs are not action IDs. A binding says **when** to dispatch; an action
descriptor says **what** to dispatch.

## Scopes

Bindings are resolved inside an explicit scope:

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

The initial resolver should use a small fixed scope set rather than arbitrary
string matching. A future extension may add namespaced family scopes.

Specific scopes take precedence over global only when the collision is
explicitly valid. Two effective bindings with the same normalized chord and
same scope are an error unless one explicitly replaces the other.

## Action descriptors

Bindings target a registry of typed action descriptors. They must not carry
shell snippets.

Conceptual representation:

~~~rust
struct ActionSpec {
    id: String,
    args: BTreeMap<String, Value>,
}
~~~

Examples:

~~~toml
action = { id = "menu.navigate", direction = "up" }
action = { id = "menu.open", family = "launcher" }
action = { id = "audio.adjust", delta = 5 }
~~~

The action registry resolves these descriptors to the same semantic/provider
paths already used by pointer, touch and Enter activation. Capability checks,
confirmation models and dry-run/safety policy continue to happen after
resolution.

Unknown action IDs or invalid arguments are configuration errors.

## Configuration model

The preferred TOML shape keeps stable binding IDs visible:

~~~toml
[keybindings]
enabled = true

[[keybindings.bindings]]
id = "launcher.toggle"
keys = "Super+Space"
scope = "global"
action = { id = "menu.open", family = "launcher" }

[[keybindings.bindings]]
id = "menu.escape"
keys = "Escape"
scope = "menu"
action = { id = "menu.back-or-close" }

[[keybindings.bindings]]
id = "user.audio-up"
keys = "XF86AudioRaiseVolume"
scope = "global"
action = { id = "audio.adjust", delta = 5 }
~~~

Built-in defaults should be loaded through the same BindingDefinition
structure before user configuration is applied.

For a built-in binding, a user entry with the same id is an explicit override.
Disabling should also be explicit:

~~~toml
[[keybindings.bindings]]
id = "launcher.toggle"
enabled = false
~~~

A custom user binding must use a unique ID. Different IDs that collide on the
same effective chord/scope should fail validation with both IDs named in the
error. Avoid order-dependent "last one wins" behavior.

## Resolution and precedence

~~~text
built-in registry
    |
    v
user overrides by stable binding ID
    |
    v
validation + conflict detection
    |
    v
effective registry
    |
    v
scope-aware runtime lookup
~~~

CLI flags may select or disable a configuration file but should not become a
second shortcut-definition language.

The effective registry must be immutable during one dispatch operation. A
future live-reload feature may replace the entire validated registry
atomically.

## Introspection contract

The effective keymap should be available as structured data, not reconstructed
from help text.

~~~rust
struct BindingView {
    id: String,
    keys: String,
    scope: BindingScope,
    action: ActionView,
    source: BindingSource, // BuiltIn or UserConfig
    enabled: bool,
    effective: bool,
    replaces: Option<String>,
}
~~~

Useful consumers:

- a future **Keyboard Shortcuts** settings page;
- CLI/debug JSON such as --dump-keybindings;
- contextual shortcut hints in menus;
- documentation generated from the built-in registry;
- conflict/error diagnostics that name both bindings and their sources.

An introspection dump should expose both normalized notation and source
provenance. It should never need to inspect Rust match arms to discover the
current keymap.

## Search/text-input interaction

Text entry remains a special semantic mode. While launcher search owns text
focus:

- printable text is delivered to the search state;
- navigation bindings such as Up, Down, Enter, Escape and Backspace keep their
  semantic behavior;
- global bindings requiring modifiers may still resolve;
- an unmodified printable keybinding must not steal ordinary search text.

This rule should be encoded in resolver policy and be visible through
introspection.

## Future combined actions

Combined actions are a later-stage extension, not part of the first
configurable-keybinding implementation.

The data model should nevertheless leave room for:

~~~toml
action = { sequence = [
  { id = "audio.unmute" },
  { id = "audio.adjust", delta = 5 },
  { id = "menu.open", family = "audio" },
] }
~~~

Initial composition rules should be deliberately boring:

- only registered typed actions may appear;
- bounded sequence length, for example at most 8 steps;
- execute in order;
- stop on first failure by default;
- no loops, recursion, arbitrary commands or embedded scripting;
- a step requiring confirmation pauses composition and uses the normal
  confirmation model;
- destructive/provider safety gates cannot be weakened by composition;
- introspection expands the sequence so the user can see every step.

Conditionals, parallel actions, delays and general scripting should remain out
of scope until there is a concrete use case and a separate safety/design
review.

## Suggested implementation stages

### Stage 1 — registry without user customization

- introduce KeyStroke, BindingDefinition, BindingScope, and ActionSpec;
- express the current built-in navigation keys as data;
- resolve them through one registry;
- add a deterministic introspection dump;
- prove behavior parity on keyboard tests and SL101.

### Stage 2 — configuration

- parse canonical notation;
- merge built-ins with explicit user overrides;
- validate collisions and action arguments;
- expose effective bindings in settings/diagnostics;
- document the schema in docs/CONFIGURATION.md.

### Stage 3 — richer discoverability

- keyboard-shortcut settings surface;
- contextual hints sourced from the same registry;
- optional atomic config reload.

### Stage 4 — combined actions

- add bounded ActionSpec::Sequence;
- preserve confirmation and provider safety semantics;
- add expanded introspection and failure reporting.

## Non-goals

- arbitrary shell-command shortcuts;
- compositor-global shortcuts that require bypassing NuraLoumi's Wayland
  authority;
- remapping printable text behind the search field without explicit modifier
  semantics;
- hidden precedence based on declaration order;
- implementing combined actions before single-action binding introspection and
  configuration are stable.
