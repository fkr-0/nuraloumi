use std::collections::BTreeSet;

use crate::ToplevelCapabilities;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SurfaceId(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct OutputId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PixelFormat {
    Argb8888,
    Xrgb8888,
}

impl PixelFormat {
    pub const fn bytes_per_pixel(self) -> usize {
        4
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Argb8888 => "ARGB8888",
            Self::Xrgb8888 => "XRGB8888",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OutputTransform {
    #[default]
    Normal,
    Rotate90,
    Rotate180,
    Rotate270,
    Flipped,
    Flipped90,
    Flipped180,
    Flipped270,
}

impl OutputTransform {
    pub const fn swaps_axes(self) -> bool {
        matches!(
            self,
            Self::Rotate90 | Self::Rotate270 | Self::Flipped90 | Self::Flipped270
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OutputInfo {
    pub id: OutputId,
    pub name: Option<String>,
    pub description: Option<String>,
    pub physical_width_mm: i32,
    pub physical_height_mm: i32,
    pub mode_width: i32,
    pub mode_height: i32,
    pub refresh_mhz: i32,
    pub scale: i32,
    pub transform: OutputTransform,
}

impl OutputInfo {
    pub(crate) fn new(id: OutputId) -> Self {
        Self {
            id,
            name: None,
            description: None,
            physical_width_mm: 0,
            physical_height_mm: 0,
            mode_width: 0,
            mode_height: 0,
            refresh_mhz: 0,
            scale: 1,
            transform: OutputTransform::Normal,
        }
    }

    /// Best-effort logical output size derived from the current wl_output mode.
    ///
    /// wl_output mode dimensions are physical-mode units rather than compositor
    /// logical coordinates. This helper applies the advertised integer scale and
    /// rotation so callers do not accidentally size portrait surfaces from the
    /// unrotated mode width. xdg-output logical_size remains authoritative when a
    /// compositor-specific integration later exposes it.
    pub fn logical_size(&self) -> Option<(u32, u32)> {
        let width = u32::try_from(self.mode_width)
            .ok()
            .filter(|value| *value > 0)?;
        let height = u32::try_from(self.mode_height)
            .ok()
            .filter(|value| *value > 0)?;
        let scale = self.scale.max(1) as u32;
        let width = (width / scale).max(1);
        let height = (height / scale).max(1);

        if self.transform.swaps_axes() {
            Some((height, width))
        } else {
            Some((width, height))
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BackendCapabilities {
    pub compositor: bool,
    pub shm: bool,
    pub layer_shell: bool,
    pub seat: bool,
    pub output_count: usize,
    pub argb8888: bool,
    pub xrgb8888: bool,
    pub toplevel: ToplevelCapabilities,
    pub workspace: crate::WorkspaceCapabilities,
}

impl BackendCapabilities {
    pub fn supports_format(&self, format: PixelFormat) -> bool {
        match format {
            PixelFormat::Argb8888 => self.argb8888,
            PixelFormat::Xrgb8888 => self.xrgb8888,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Escape,
    Backspace,
    Text(String),
    Raw(u32),
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlatformEvent {
    Configure {
        width: u32,
        height: u32,
        scale: i32,
    },
    PointerEnter {
        x: f64,
        y: f64,
    },
    PointerMove {
        x: f64,
        y: f64,
    },
    PointerLeave,
    PointerButton {
        x: f64,
        y: f64,
        pressed: bool,
        button: u32,
    },
    TouchDown {
        id: i32,
        x: f64,
        y: f64,
    },
    TouchMotion {
        id: i32,
        x: f64,
        y: f64,
    },
    TouchUp {
        id: i32,
    },
    TouchCancel {
        ids: Vec<i32>,
    },
    Key {
        key: Key,
        pressed: bool,
    },
    Close,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BackendEvent {
    pub surface: Option<SurfaceId>,
    pub event: PlatformEvent,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PanelEdge {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
}

impl PanelEdge {
    pub const fn is_horizontal(self) -> bool {
        matches!(self, Self::Top | Self::Bottom)
    }
}

#[derive(Clone, Debug)]
pub struct PanelConfig {
    pub height: u32,
    pub exclusive_zone: i32,
    pub output: Option<OutputId>,
    pub namespace: String,
}

impl Default for PanelConfig {
    fn default() -> Self {
        Self {
            height: 48,
            exclusive_zone: 48,
            output: None,
            namespace: "nuraloumi-panel".to_owned(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct MenuConfig {
    pub width: u32,
    pub height: u32,
    pub margin_top: i32,
    pub margin_left: i32,
    pub output: Option<OutputId>,
    pub namespace: String,
}

impl Default for MenuConfig {
    fn default() -> Self {
        Self {
            width: 480,
            height: 640,
            margin_top: 48,
            margin_left: 0,
            output: None,
            namespace: "nuraloumi-menu".to_owned(),
        }
    }
}

/// Transparent overlay surface intended to sit immediately below a transient
/// menu and receive pointer/touch events that land outside the menu itself.
///
/// With all margins at zero, the compositor sizes the surface to the complete
/// selected output. A shell may reserve its persistent panel strip by setting
/// the corresponding non-negative edge margin.
#[derive(Clone, Debug)]
pub struct DismissBackdropConfig {
    pub margin_top: i32,
    pub margin_right: i32,
    pub margin_bottom: i32,
    pub margin_left: i32,
    pub output: Option<OutputId>,
    pub namespace: String,
}

impl Default for DismissBackdropConfig {
    fn default() -> Self {
        Self {
            margin_top: 0,
            margin_right: 0,
            margin_bottom: 0,
            margin_left: 0,
            output: None,
            namespace: "nuraloumi-dismiss-backdrop".to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Frame<'a> {
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub format: PixelFormat,
    pub pixels: &'a [u8],
}

impl<'a> Frame<'a> {
    pub fn packed(width: u32, height: u32, format: PixelFormat, pixels: &'a [u8]) -> Self {
        Self {
            width,
            height,
            stride: (width as usize).saturating_mul(format.bytes_per_pixel()),
            format,
            pixels,
        }
    }
}

/// Convert a point expressed in physical output pixels into unscaled logical
/// coordinates while applying the wl_output transform.
///
/// The input extents are the untransformed physical output width/height.
/// Continuous coordinates are used, so (width, height) is a valid edge.
pub fn normalize_output_point(
    point: Point,
    physical_width: u32,
    physical_height: u32,
    scale: i32,
    transform: OutputTransform,
) -> Point {
    let scale = f64::from(scale.max(1));
    let width = f64::from(physical_width) / scale;
    let height = f64::from(physical_height) / scale;
    let x = point.x / scale;
    let y = point.y / scale;

    match transform {
        OutputTransform::Normal => Point { x, y },
        OutputTransform::Rotate90 => Point {
            x: height - y,
            y: x,
        },
        OutputTransform::Rotate180 => Point {
            x: width - x,
            y: height - y,
        },
        OutputTransform::Rotate270 => Point { x: y, y: width - x },
        OutputTransform::Flipped => Point { x: width - x, y },
        OutputTransform::Flipped90 => Point { x: y, y: x },
        OutputTransform::Flipped180 => Point { x, y: height - y },
        OutputTransform::Flipped270 => Point {
            x: height - y,
            y: width - x,
        },
    }
}

pub(crate) fn text_key(key: u32, shift: bool) -> Option<String> {
    let base = match key {
        2 => {
            if shift {
                '!'
            } else {
                '1'
            }
        }
        3 => {
            if shift {
                '@'
            } else {
                '2'
            }
        }
        4 => {
            if shift {
                '#'
            } else {
                '3'
            }
        }
        5 => {
            if shift {
                '$'
            } else {
                '4'
            }
        }
        6 => {
            if shift {
                '%'
            } else {
                '5'
            }
        }
        7 => {
            if shift {
                '^'
            } else {
                '6'
            }
        }
        8 => {
            if shift {
                '&'
            } else {
                '7'
            }
        }
        9 => {
            if shift {
                '*'
            } else {
                '8'
            }
        }
        10 => {
            if shift {
                '('
            } else {
                '9'
            }
        }
        11 => {
            if shift {
                ')'
            } else {
                '0'
            }
        }
        12 => {
            if shift {
                '_'
            } else {
                '-'
            }
        }
        13 => {
            if shift {
                '+'
            } else {
                '='
            }
        }
        16 => 'q',
        17 => 'w',
        18 => 'e',
        19 => 'r',
        20 => 't',
        21 => 'y',
        22 => 'u',
        23 => 'i',
        24 => 'o',
        25 => 'p',
        26 => {
            if shift {
                '{'
            } else {
                '['
            }
        }
        27 => {
            if shift {
                '}'
            } else {
                ']'
            }
        }
        30 => 'a',
        31 => 's',
        32 => 'd',
        33 => 'f',
        34 => 'g',
        35 => 'h',
        36 => 'j',
        37 => 'k',
        38 => 'l',
        39 => {
            if shift {
                ':'
            } else {
                ';'
            }
        }
        40 => {
            if shift {
                '"'
            } else {
                '\''
            }
        }
        41 => {
            if shift {
                '~'
            } else {
                '`'
            }
        }
        43 => {
            if shift {
                '|'
            } else {
                '\\'
            }
        }
        44 => 'z',
        45 => 'x',
        46 => 'c',
        47 => 'v',
        48 => 'b',
        49 => 'n',
        50 => 'm',
        51 => {
            if shift {
                '<'
            } else {
                ','
            }
        }
        52 => {
            if shift {
                '>'
            } else {
                '.'
            }
        }
        53 => {
            if shift {
                '?'
            } else {
                '/'
            }
        }
        57 => ' ',
        _ => return None,
    };

    let ch = if shift && base.is_ascii_lowercase() {
        base.to_ascii_uppercase()
    } else {
        base
    };
    Some(ch.to_string())
}

pub(crate) fn semantic_key(key: u32, shift: bool) -> Key {
    match key {
        1 => Key::Escape,
        14 => Key::Backspace,
        28 => Key::Enter,
        103 => Key::Up,
        105 => Key::Left,
        106 => Key::Right,
        108 => Key::Down,
        _ => text_key(key, shift).map(Key::Text).unwrap_or(Key::Raw(key)),
    }
}

pub(crate) fn sorted_touch_ids(ids: &BTreeSet<i32>) -> Vec<i32> {
    ids.iter().copied().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_point(actual: Point, expected: Point) {
        assert!(
            (actual.x - expected.x).abs() < 1e-9,
            "{actual:?} != {expected:?}"
        );
        assert!(
            (actual.y - expected.y).abs() < 1e-9,
            "{actual:?} != {expected:?}"
        );
    }

    #[test]
    fn output_transform_normal() {
        assert_point(
            normalize_output_point(
                Point { x: 100.0, y: 40.0 },
                800,
                600,
                2,
                OutputTransform::Normal,
            ),
            Point { x: 50.0, y: 20.0 },
        );
    }

    #[test]
    fn output_transform_90() {
        assert_point(
            normalize_output_point(
                Point { x: 100.0, y: 40.0 },
                800,
                600,
                2,
                OutputTransform::Rotate90,
            ),
            Point { x: 280.0, y: 50.0 },
        );
    }

    #[test]
    fn output_transform_180() {
        assert_point(
            normalize_output_point(
                Point { x: 100.0, y: 40.0 },
                800,
                600,
                2,
                OutputTransform::Rotate180,
            ),
            Point { x: 350.0, y: 280.0 },
        );
    }

    #[test]
    fn output_transform_270() {
        assert_point(
            normalize_output_point(
                Point { x: 100.0, y: 40.0 },
                800,
                600,
                2,
                OutputTransform::Rotate270,
            ),
            Point { x: 20.0, y: 350.0 },
        );
    }

    #[test]
    fn logical_output_size_applies_scale_and_rotation() {
        let mut output = OutputInfo::new(OutputId(1));
        output.mode_width = 1920;
        output.mode_height = 1080;
        output.scale = 2;
        assert_eq!(output.logical_size(), Some((960, 540)));

        output.transform = OutputTransform::Rotate90;
        assert_eq!(output.logical_size(), Some((540, 960)));

        output.transform = OutputTransform::Flipped270;
        assert_eq!(output.logical_size(), Some((540, 960)));
    }

    #[test]
    fn logical_output_size_requires_a_valid_mode() {
        let output = OutputInfo::new(OutputId(1));
        assert_eq!(output.logical_size(), None);
    }

    #[test]
    fn panel_edge_orientation_is_stable() {
        assert!(PanelEdge::Top.is_horizontal());
        assert!(PanelEdge::Bottom.is_horizontal());
        assert!(!PanelEdge::Left.is_horizontal());
        assert!(!PanelEdge::Right.is_horizontal());
    }

    #[test]
    fn semantic_keys_cover_navigation_and_text() {
        assert_eq!(semantic_key(103, false), Key::Up);
        assert_eq!(semantic_key(28, false), Key::Enter);
        assert_eq!(semantic_key(1, false), Key::Escape);
        assert_eq!(semantic_key(14, false), Key::Backspace);
        assert_eq!(semantic_key(30, true), Key::Text("A".to_owned()));
    }
}
