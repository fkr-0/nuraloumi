//! Renderer-neutral visual design tokens.

use serde::{Deserialize, Serialize};

/// RGBA color using 8-bit channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl UiColor {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceColors {
    pub base: UiColor,
    pub raised: UiColor,
    pub overlay: UiColor,
    pub card: UiColor,
    pub selected: UiColor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextColors {
    pub primary: UiColor,
    pub secondary: UiColor,
    pub hint: UiColor,
    pub accent: UiColor,
    pub warning: UiColor,
    pub error: UiColor,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BorderTokens {
    pub width: f32,
    pub radius_small: f32,
    pub radius_menu: f32,
    pub color: UiColor,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowToken {
    pub offset_y: f32,
    pub blur_radius: f32,
    pub opacity: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ElevationTokens {
    pub none: ShadowToken,
    pub low: ShadowToken,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpacityTokens {
    pub popup: f32,
    pub overlay: f32,
    pub separator: f32,
    pub inactive: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpacingTokens {
    pub xxs: u16,
    pub xs: u16,
    pub sm: u16,
    pub md: u16,
    pub lg: u16,
    pub xl: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeTokens {
    pub title: u16,
    pub section: u16,
    pub body: u16,
    pub label: u16,
    pub metadata: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IconTokens {
    pub small: u16,
    pub medium: u16,
    pub large: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HitTargetTokens {
    /// Minimum primary touch target in logical pixels.
    pub minimum: u16,
    pub panel_height: u16,
    pub primary_row: u16,
    pub compact_row: u16,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemeTokens {
    pub surfaces: SurfaceColors,
    pub text: TextColors,
    pub border: BorderTokens,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiMetrics {
    pub spacing: SpacingTokens,
    pub type_scale: TypeTokens,
    pub icons: IconTokens,
    pub hit_targets: HitTargetTokens,
    pub elevation: ElevationTokens,
    pub opacity: OpacityTokens,
    pub outer_padding: u16,
}

pub const UI_METRICS: UiMetrics = UiMetrics {
    spacing: SpacingTokens {
        xxs: 2,
        xs: 4,
        sm: 8,
        md: 12,
        lg: 16,
        xl: 24,
    },
    type_scale: TypeTokens {
        title: 22,
        section: 15,
        body: 14,
        label: 13,
        metadata: 12,
    },
    icons: IconTokens {
        small: 16,
        medium: 20,
        large: 24,
    },
    hit_targets: HitTargetTokens {
        minimum: 48,
        panel_height: 48,
        primary_row: 52,
        compact_row: 40,
    },
    elevation: ElevationTokens {
        none: ShadowToken {
            offset_y: 0.0,
            blur_radius: 0.0,
            opacity: 0.0,
        },
        low: ShadowToken {
            offset_y: 2.0,
            blur_radius: 6.0,
            opacity: 0.16,
        },
    },
    opacity: OpacityTokens {
        popup: 0.96,
        overlay: 0.72,
        separator: 0.16,
        inactive: 0.55,
    },
    outer_padding: 16,
};

const _: () = {
    assert!(UI_METRICS.hit_targets.minimum >= 48);
    assert!(UI_METRICS.hit_targets.primary_row >= UI_METRICS.hit_targets.minimum);
    assert!(UI_METRICS.hit_targets.panel_height >= UI_METRICS.hit_targets.minimum);
    assert!(UI_METRICS.type_scale.title > UI_METRICS.type_scale.body);
    assert!(UI_METRICS.type_scale.body >= UI_METRICS.type_scale.metadata);
    assert!(UI_METRICS.elevation.low.blur_radius >= UI_METRICS.elevation.none.blur_radius);
};

pub const DARK_THEME: ThemeTokens = ThemeTokens {
    surfaces: SurfaceColors {
        base: UiColor::rgb(0x0b, 0x0d, 0x0f),
        raised: UiColor::rgb(0x10, 0x12, 0x15),
        overlay: UiColor::rgb(0x14, 0x17, 0x1b),
        card: UiColor::rgb(0x17, 0x1a, 0x1f),
        selected: UiColor::rgb(0x24, 0x29, 0x30),
    },
    text: TextColors {
        primary: UiColor::rgb(0xe8, 0xea, 0xed),
        secondary: UiColor::rgb(0xa9, 0xae, 0xb6),
        hint: UiColor::rgb(0x7b, 0x81, 0x8a),
        accent: UiColor::rgb(0x95, 0x9e, 0xac),
        warning: UiColor::rgb(0xc9, 0xa2, 0x5d),
        error: UiColor::rgb(0xd7, 0x78, 0x78),
    },
    border: BorderTokens {
        width: 1.0,
        radius_small: 0.0,
        radius_menu: 0.0,
        color: UiColor::rgb(0x2c, 0x31, 0x37),
    },
};

pub const LIGHT_THEME: ThemeTokens = ThemeTokens {
    surfaces: SurfaceColors {
        base: UiColor::rgb(0xf8, 0xf8, 0xfc),
        raised: UiColor::rgb(0xff, 0xff, 0xff),
        overlay: UiColor::rgb(0xfa, 0xfa, 0xff),
        card: UiColor::rgb(0xff, 0xff, 0xff),
        selected: UiColor::rgb(0xed, 0xe9, 0xfe),
    },
    text: TextColors {
        primary: UiColor::rgb(0x18, 0x18, 0x20),
        secondary: UiColor::rgb(0x4b, 0x4e, 0x5c),
        hint: UiColor::rgb(0x6b, 0x72, 0x80),
        accent: UiColor::rgb(0x6d, 0x28, 0xd9),
        warning: UiColor::rgb(0xb4, 0x53, 0x09),
        error: UiColor::rgb(0xb9, 0x1c, 0x1c),
    },
    border: BorderTokens {
        width: 1.0,
        radius_small: 0.0,
        radius_menu: 0.0,
        color: UiColor::rgb(0xd1, 0xd5, 0xdb),
    },
};

#[cfg(test)]
mod tests {
    use super::*;

    fn linear_channel(value: f32) -> f32 {
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }

    fn luminance(color: UiColor) -> f32 {
        let r = linear_channel(f32::from(color.r) / 255.0);
        let g = linear_channel(f32::from(color.g) / 255.0);
        let b = linear_channel(f32::from(color.b) / 255.0);
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    fn contrast_ratio(a: UiColor, b: UiColor) -> f32 {
        let a = luminance(a);
        let b = luminance(b);
        let lighter = a.max(b);
        let darker = a.min(b);
        (lighter + 0.05) / (darker + 0.05)
    }

    #[test]
    fn spacing_and_type_scales_are_ordered() {
        let spacing = [
            UI_METRICS.spacing.xxs,
            UI_METRICS.spacing.xs,
            UI_METRICS.spacing.sm,
            UI_METRICS.spacing.md,
            UI_METRICS.spacing.lg,
            UI_METRICS.spacing.xl,
        ];
        assert!(spacing.windows(2).all(|pair| pair[0] <= pair[1]));
    }

    #[test]
    fn primary_and_secondary_text_reach_body_contrast_on_base_and_card() {
        for theme in [DARK_THEME, LIGHT_THEME] {
            for surface in [theme.surfaces.base, theme.surfaces.card] {
                assert!(contrast_ratio(theme.text.primary, surface) >= 4.5);
                assert!(contrast_ratio(theme.text.secondary, surface) >= 4.5);
            }
        }
    }

    #[test]
    fn visual_corner_tokens_are_square() {
        assert_eq!(DARK_THEME.border.radius_small, 0.0);
        assert_eq!(DARK_THEME.border.radius_menu, 0.0);
        assert_eq!(LIGHT_THEME.border.radius_small, 0.0);
        assert_eq!(LIGHT_THEME.border.radius_menu, 0.0);
    }

    #[test]
    fn opacity_and_elevation_values_are_bounded() {
        for opacity in [
            UI_METRICS.opacity.popup,
            UI_METRICS.opacity.overlay,
            UI_METRICS.opacity.separator,
            UI_METRICS.opacity.inactive,
            UI_METRICS.elevation.none.opacity,
            UI_METRICS.elevation.low.opacity,
        ] {
            assert!((0.0..=1.0).contains(&opacity));
        }
    }
}
