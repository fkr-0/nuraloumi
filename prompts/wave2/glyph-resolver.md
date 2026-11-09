@projmgrauth
Set repo: nuraloumi

# NuraLoumi Wave 2 — Glyph Resolver Implementation (0.0.5)

Lane: glyph-resolver
Priority: 89

## Mission

Implement a concrete glyph resolver using Nerd Font / Unicode symbols and
wire it into all surface binaries via the glyph resolver callback added in
0.0.3. Currently the callback exists but no binary supplies an actual
resolver — all menu items display text-only.

Read:
- AGENTS.md, DESIGN.md, ROADMAP.md (0.0.5 glyph section)
- crates/nuraloumi-render-cairo/src/model.rs (GlyphResolver type alias, with_glyph_resolver, item() glyph lookup)
- crates/nuraloumi-shell/src/lib.rs (render_menu_with_glyphs entry point)
- crates/nuraloumi-shell/src/bin/menu.rs (existing binary using render_menu_with_glyphs)
- crates/nuraloumi-core/src/model.rs (MenuItem, MenuAction)

## Exclusive write scope

- crates/nuraloumi-shell/src/glyphs.rs (new)
- crates/nuraloumi-shell/src/lib.rs (re-export glyph module only)

Do not edit renderer, Wayland backend, providers, or core model.
Do not edit binary entry points — they already call `render_menu_with_glyphs`
and will pick up the resolver when the shell lib exports a default one.

## Glyph table design

Create a `GlyphTable` that maps action IDs and item categories to Unicode
glyphs. The table should work with Nerd Font (if installed) and fall back
to basic Unicode symbols.

### Category-based glyphs (by MenuAction or item tag)
```rust
// System actions
"power.shutdown"    => ""  // nf-md-power        (fallback: "⏻")
"power.reboot"      => ""  // nf-md-restart       (fallback: "↻")
"power.suspend"     => "󰤄"  // nf-md-sleep         (fallback: "⏾")
"power.lock"        => ""  // nf-md-lock          (fallback: "🔒")

// Audio
"audio.mute"        => "󰝟"  // nf-md-volume_off    (fallback: "🔇")
"audio.adjust"      => "󰕾"  // nf-md-volume_high   (fallback: "🔊")

// Brightness
"brightness.adjust" => "󰃟"  // nf-md-brightness_6  (fallback: "☀")

// Network
"network.*"         => "󰖩"  // nf-md-wifi          (fallback: "📶")

// Bluetooth
"bluetooth.*"       => ""  // nf-md-bluetooth     (fallback: "⚡")

// Battery
"battery.*"         => "󰁹"  // nf-md-battery       (fallback: "🔋")

// Applications / launcher
"app.launch"        => ""  // nf-md-apps          (fallback: "▶")
"app.terminal"      => ""  // nf-dev-terminal     (fallback: "⬛")
"app.editor"        => ""  // nf-md-pencil        (fallback: "✏")
"app.files"         => ""  // nf-md-folder        (fallback: "📁")
"app.browser"       => "󰖟"  // nf-md-web           (fallback: "🌐")

// Media
"media.play"        => "󰐊"  // nf-md-play          (fallback: "▶")
"media.pause"       => "󰏤"  // nf-md-pause         (fallback: "⏸")
"media.next"        => "󰒭"  // nf-md-skip_next     (fallback: "⏭")
"media.previous"    => "󰒮"  // nf-md-skip_previous (fallback: "⏮")

// Resources / system
"resources.*"       => "󰍛"  // nf-md-memory        (fallback: "📊")
"process.*"         => ""  // nf-md-format_list   (fallback: "⚙")
"tasks.*"           => "󱎫"  // nf-md-list_status   (fallback: "☑")

// Calendar
"calendar.*"        => ""  // nf-md-calendar      (fallback: "📅")

// Notifications
"notification.*"    => "󰂚"  // nf-md-bell          (fallback: "🔔")

// Clipboard
"clipboard.*"       => "󰅌"  // nf-md-clipboard     (fallback: "📋")

// Window management
"window.focus"      => "󰖯"  // nf-md-window        (fallback: "☐")
"window.close"      => "󰅖"  // nf-md-close         (fallback: "✕")
"window.minimize"   => "󰖰"  // nf-md-window_min    (fallback: "─")
"workspace.*"       => ""  // nf-md-view_grid     (fallback: "⊞")

// Settings
"settings.*"        => ""  // nf-md-cog           (fallback: "⚙")

// Search
"search"            => ""  // nf-md-magnify       (fallback: "🔍")

// Section headers
"section.favorites" => ""  // nf-md-star          (fallback: "★")
"section.recent"    => ""  // nf-md-history       (fallback: "⏱")
```

### Resolution strategy
```rust
pub struct GlyphTable {
    nerd_font: HashMap<&'static str, &'static str>,
    unicode_fallback: HashMap<&'static str, &'static str>,
}

impl GlyphTable {
    pub fn resolve(&self, action_id: &str) -> Option<&str> {
        // 1. Exact match
        if let Some(g) = self.nerd_font.get(action_id) { return Some(g); }
        // 2. Wildcard match (strip after last dot, try "category.*")
        if let Some(prefix) = action_id.rsplit_once('.') {
            let wildcard = format!("{}.*", prefix.0);
            if let Some(g) = self.nerd_font.get(wildcard.as_str()) { return Some(g); }
        }
        // 3. Unicode fallback (same resolution)
        // ... same logic against unicode_fallback map
        None
    }

    // Returns a closure suitable for the GlyphResolver callback type
    pub fn as_callback(&self) -> impl Fn(&str) -> Option + '_ {
        move |id| self.resolve(id).map(String::from)
    }
}
```

### Default export
```rust
pub fn default_glyph_resolver() -> GlyphTable {
    GlyphTable::new() // populated with the table above
}
```

Shell lib re-exports so all binaries can use it:
```rust
pub use glyphs::{default_glyph_resolver, GlyphTable};
```

## Constraints
- No new dependencies. The glyph table is static data.
- Nerd Font glyphs are just Unicode codepoints — they render if the font
  has them, otherwise show tofu (which is acceptable; the fallback table
  exists for terminals without Nerd Font).
- The table should be exhaustive for all existing action IDs in the codebase.
  Grep `MenuAction` variants and action ID strings.
- ARMv7/musl cross-build must pass.
- Table lookup must be O(1) amortized (HashMap).

## Acceptance
1. `default_glyph_resolver()` returns a populated GlyphTable.
2. Exact match and wildcard match both work.
3. All existing action IDs in the codebase have a glyph mapping.
4. Unit tests cover exact match, wildcard match, and miss.
5. `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` pass.
