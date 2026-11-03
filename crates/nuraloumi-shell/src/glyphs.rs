//! Lightweight, allocation-free-at-lookup glyph resolution for shell menu row IDs.
//! The renderer passes `MenuItem.id`, which can differ from the typed action ID.

use std::collections::HashMap;

/// Glyph preference; Nerd Font glyphs require a suitable installed font.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GlyphStyle {
    NerdFont,
    Unicode,
}

/// Hash-based lookup table; a resolver returns a single Unicode scalar
/// because the Cairo callback is `Fn(&str) -> Option<char>`.
#[derive(Clone, Debug)]
pub struct GlyphTable {
    nerd_font: HashMap<&'static str, char>,
    unicode_fallback: HashMap<&'static str, char>,
    style: GlyphStyle,
}

const GLYPHS: &[(&str, char, char)] = &[
    ("power.shutdown", '󰐥', '⏻'),
    ("power.poweroff", '󰐥', '⏻'),
    ("power.reboot", '󰜉', '↻'),
    ("power.restart", '󰜉', '↻'),
    ("power.suspend", '󰤄', '⏾'),
    ("power.lock", '󰌾', '🔒'),
    ("audio.mute", '󰝟', '🔇'),
    ("audio.adjust", '󰕾', '🔊'),
    ("audio.volume", '󰕾', '🔊'),
    ("audio.down", '󰕾', '🔉'),
    ("audio.up", '󰕾', '🔊'),
    ("brightness.adjust", '󰃟', '☀'),
    ("network.*", '󰖩', '📶'),
    ("control.network.*", '󰖩', '📶'),
    ("control.bluetooth.*", '󰂯', '⚡'),
    ("control.display.*", '󰃟', '☀'),
    ("control.media.*", '󰕾', '🔊'),
    ("control.notifications.*", '󰂚', '🔔'),
    ("control.system.suspend", '󰤄', '⏾'),
    ("control.system.restart", '󰜉', '↻'),
    ("control.system.poweroff", '󰐥', '⏻'),
    ("control.system.battery", '󰁹', '🔋'),
    ("control.system.*", '󰒓', '⚙'),
    ("control.tabs", '󰒓', '⚙'),
    ("control.*", '󰒓', '⚙'),
    ("bluetooth.*", '󰂯', '⚡'),
    ("battery.*", '󰁹', '🔋'),
    ("app.launch", '󰀻', '▶'),
    ("app.terminal", '', '▣'),
    ("app.editor", '󰏫', '✏'),
    ("app.files", '󰉋', '📁'),
    ("app.browser", '󰖟', '🌐'),
    ("media.play", '󰐊', '▶'),
    ("media.pause", '󰏤', '⏸'),
    ("media.play_pause", '󰐊', '▶'),
    ("media.next", '󰒭', '⏭'),
    ("media.previous", '󰒮', '⏮'),
    ("resources.*", '󰍛', '📊'),
    ("process.*", '󰉺', '⚙'),
    ("tasks.*", '󱎫', '☑'),
    ("calendar.*", '󰃭', '📅'),
    ("notification.*", '󰂚', '🔔'),
    ("clipboard.*", '󰅌', '📋'),
    ("window.focus", '󰖯', '☐'),
    ("window.close", '󰅖', '✕'),
    ("window.minimize", '󰖰', '─'),
    ("window.*", '󰖯', '☐'),
    ("windows.*", '󰖯', '☐'),
    ("display.*", '󰃟', '☀'),
    ("workspace.*", '󰕰', '⊞'),
    ("settings.*", '󰒓', '⚙'),
    ("search", '󰍉', '🔍'),
    ("section.favorites", '󰓎', '★'),
    ("section.recent", '󰋚', '⏱'),
    ("launcher.favorites", '󰓎', '★'),
    ("launcher.windows", '󰖯', '☐'),
    ("launcher.apps", '󰀻', '▶'),
    ("launcher.tasks", '󱎫', '☑'),
    ("launcher.desktops", '󰕰', '⊞'),
    ("launcher.more", '󰍜', '☷'),
    ("launcher.*", '󰀻', '▶'),
    ("desktop.*", '󰕰', '⊞'),
    ("power.*", '󰐥', '⏻'),
    ("system.*", '󰒓', '⚙'),
    ("audio.*", '󰕾', '🔊'),
    ("brightness.*", '󰃟', '☀'),
    ("renderer.*", '󰹑', '▣'),
    ("files.*", '󰉋', '📁'),
    ("file.*", '󰉋', '📁'),
    ("run.*", '󰍉', '▶'),
    ("exec.*", '', '▣'),
    ("win-*", '󰖯', '☐'),
];

impl Default for GlyphTable {
    fn default() -> Self {
        Self::new()
    }
}

impl GlyphTable {
    pub fn new() -> Self {
        Self::with_style(GlyphStyle::NerdFont)
    }

    pub fn with_style(style: GlyphStyle) -> Self {
        let mut nerd_font = HashMap::with_capacity(GLYPHS.len());
        let mut unicode_fallback = HashMap::with_capacity(GLYPHS.len());
        for &(key, nerd, unicode) in GLYPHS {
            nerd_font.insert(key, nerd);
            unicode_fallback.insert(key, unicode);
        }
        Self {
            nerd_font,
            unicode_fallback,
            style,
        }
    }

    /// Use an exact row ID first, then the longest matching dotted category
    /// wildcard. This also supports dynamic IDs such as `notification.17.remove`.
    pub fn resolve(&self, id: &str) -> Option<char> {
        let map = match self.style {
            GlyphStyle::NerdFont => &self.nerd_font,
            GlyphStyle::Unicode => &self.unicode_fallback,
        };
        if let Some(&glyph) = map.get(id) {
            return Some(glyph);
        }
        let mut remaining = id;
        while let Some((prefix, _)) = remaining.rsplit_once('.') {
            let key = format!("{prefix}.*");
            if let Some(&glyph) = map.get(key.as_str()) {
                return Some(glyph);
            }
            remaining = prefix;
        }
        None
    }

    pub fn as_callback(&self) -> impl Fn(&str) -> Option<char> + '_ {
        move |id| self.resolve(id)
    }

    pub fn len(&self) -> usize {
        self.nerd_font.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nerd_font.is_empty()
    }
}

pub fn default_glyph_resolver() -> GlyphTable {
    GlyphTable::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_table_is_populated() {
        assert!(default_glyph_resolver().len() > 40);
    }

    #[test]
    fn exact_match_and_callback() {
        let table = GlyphTable::new();
        assert_eq!(table.resolve("audio.mute"), Some('󰝟'));
        assert_eq!(table.as_callback()("power.suspend"), Some('󰤄'));
    }

    #[test]
    fn wildcard_and_nested_ids() {
        let table = GlyphTable::with_style(GlyphStyle::Unicode);
        assert_eq!(table.resolve("network.ssid.3"), Some('📶'));
        assert_eq!(table.resolve("notification.17.remove"), Some('🔔'));
        assert_eq!(table.resolve("launcher.more.apps"), Some('▶'));
        assert_eq!(table.resolve("control.network.wifi.state"), Some('📶'));
        assert_eq!(table.resolve("control.system.suspend"), Some('⏾'));
        assert_eq!(table.resolve("windows.list"), Some('☐'));
    }

    #[test]
    fn unicode_fallback_and_miss() {
        let table = GlyphTable::with_style(GlyphStyle::Unicode);
        assert_eq!(table.resolve("power.shutdown"), Some('⏻'));
        assert_eq!(table.resolve("unknown.category"), None);
    }
}
