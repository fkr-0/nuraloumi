#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub fn components(self) -> (f64, f64, f64, f64) {
        (
            f64::from(self.r) / 255.0,
            f64::from(self.g) / 255.0,
            f64::from(self.b) / 255.0,
            f64::from(self.a) / 255.0,
        )
    }
}

impl From<nuraloumi_core::UiColor> for Color {
    fn from(color: nuraloumi_core::UiColor) -> Self {
        Self::rgba(color.r, color.g, color.b, color.a)
    }
}

impl From<&nuraloumi_core::ThemeTokens> for Theme {
    fn from(tokens: &nuraloumi_core::ThemeTokens) -> Self {
        let border: Color = tokens.border.color.into();
        Self {
            base: tokens.surfaces.base.into(),
            raised: tokens.surfaces.raised.into(),
            overlay: tokens.surfaces.overlay.into(),
            card: tokens.surfaces.card.into(),
            selected: tokens.surfaces.selected.into(),
            primary_text: tokens.text.primary.into(),
            secondary_text: tokens.text.secondary.into(),
            hint: tokens.text.hint.into(),
            accent: tokens.text.accent.into(),
            warning: tokens.text.warning.into(),
            error: tokens.text.error.into(),
            border,
            border_width: f64::from(tokens.border.width),
            separator: Color::rgba(border.r, border.g, border.b, 80),
            panel_radius: f64::from(tokens.border.radius_menu),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fill {
    Solid(Color),
    VerticalGradient { top: Color, bottom: Color },
}

/// Renderer tokens. Values match DESIGN.md's initial dark palette.
///
/// The type is intentionally plain-data so final semantic-core theme tokens
/// can be mapped into it without Cairo leaking into core.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub base: Color,
    pub raised: Color,
    pub overlay: Color,
    pub card: Color,
    pub selected: Color,
    pub primary_text: Color,
    pub secondary_text: Color,
    pub hint: Color,
    pub accent: Color,
    pub warning: Color,
    pub error: Color,
    pub border: Color,
    pub border_width: f64,
    pub separator: Color,
    pub panel_radius: f64,
}

impl Theme {
    /// High-contrast light palette for the same square Cairo scene grammar.
    pub const fn light() -> Self {
        Self {
            base: Color::rgb(0xf7, 0xf8, 0xfa),
            raised: Color::rgb(0xff, 0xff, 0xff),
            overlay: Color::rgb(0xf8, 0xf9, 0xfc),
            card: Color::rgb(0xf0, 0xf2, 0xf7),
            selected: Color::rgb(0xdf, 0xe5, 0xf4),
            primary_text: Color::rgb(0x16, 0x1d, 0x2c),
            secondary_text: Color::rgb(0x39, 0x45, 0x58),
            hint: Color::rgb(0x48, 0x55, 0x6b),
            accent: Color::rgb(0x30, 0x4a, 0x86),
            warning: Color::rgb(0x86, 0x54, 0x00),
            error: Color::rgb(0x9a, 0x24, 0x28),
            border: Color::rgba(0x24, 0x30, 0x46, 68),
            border_width: 1.0,
            separator: Color::rgba(0x24, 0x30, 0x46, 65),
            panel_radius: 0.0,
        }
    }

    pub const fn dark() -> Self {
        Self {
            base: Color::rgb(0x11, 0x12, 0x18),
            raised: Color::rgb(0x18, 0x19, 0x21),
            overlay: Color::rgb(0x1c, 0x1d, 0x27),
            card: Color::rgb(0x1f, 0x20, 0x2b),
            selected: Color::rgb(0x3a, 0x2d, 0x5c),
            primary_text: Color::rgb(0xf4, 0xf4, 0xf8),
            secondary_text: Color::rgb(0xbe, 0xbf, 0xcc),
            hint: Color::rgb(0x8e, 0x90, 0xa0),
            accent: Color::rgb(0xa7, 0x8b, 0xfa),
            warning: Color::rgb(0xfb, 0xbf, 0x24),
            error: Color::rgb(0xf8, 0x71, 0x71),
            border: Color::rgba(0xff, 0xff, 0xff, 36),
            border_width: 1.0,
            separator: Color::rgba(0xff, 0xff, 0xff, 28),
            panel_radius: 12.0,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}
