use crate::theme::{Color, Fill};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl From<&nuraloumi_core::MenuState> for InteractionState {
    fn from(state: &nuraloumi_core::MenuState) -> Self {
        Self {
            selected_id: state.selected_id.clone(),
            pressed_id: None,
            scroll_offset: 0.0,
        }
    }
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(self) -> f64 {
        self.y + self.height
    }

    pub fn contains(self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    pub fn intersection(self, other: Self) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        (right > x && bottom > y).then(|| Self::new(x, y, right - x, bottom - y))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Insets {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

impl Insets {
    pub const fn all(value: f64) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    /// Logical width in compositor coordinates.
    pub width: f64,
    /// Logical height in compositor coordinates.
    pub height: f64,
    /// Output scale. Geometry and hit testing remain logical.
    pub scale: f64,
}

impl Viewport {
    pub fn new(width: f64, height: f64, scale: f64) -> Self {
        Self {
            width: width.max(1.0),
            height: height.max(1.0),
            scale: scale.max(1.0),
        }
    }

    pub fn logical_rect(self) -> Rect {
        Rect::new(0.0, 0.0, self.width, self.height)
    }

    pub fn device_size(self) -> (i32, i32) {
        let width = (self.width * self.scale).ceil().clamp(1.0, i32::MAX as f64) as i32;
        let height = (self.height * self.scale)
            .ceil()
            .clamp(1.0, i32::MAX as f64) as i32;
        (width, height)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Action,
    Submenu,
    Checkable { checked: bool },
    Section,
    Status,
    Separator,
}

impl RowKind {
    pub fn actionable(self) -> bool {
        matches!(self, Self::Action | Self::Submenu | Self::Checkable { .. })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuItemView {
    pub id: String,
    pub label: String,
    pub subtitle: Option<String>,
    pub shortcut: Option<String>,
    /// Optional cheap leading glyph. No icon theme/image stack is required.
    pub glyph: Option<char>,
    pub kind: RowKind,
    pub enabled: bool,
}

impl MenuItemView {
    pub fn action(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            subtitle: None,
            shortcut: None,
            glyph: None,
            kind: RowKind::Action,
            enabled: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuView {
    pub id: String,
    pub title: String,
    pub items: Vec<MenuItemView>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuItemRef<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub subtitle: Option<&'a str>,
    pub shortcut: Option<&'a str>,
    pub glyph: Option<char>,
    pub kind: RowKind,
    pub enabled: bool,
}

/// Renderer-side seam for the immutable semantic menu model.
///
/// This intentionally requires only stable renderer facts. A wrapper around
/// nuraloumi_core::MenuModel can implement it without renderer changes.
pub trait MenuSource {
    fn id(&self) -> &str;
    fn title(&self) -> &str;
    fn len(&self) -> usize;
    fn item(&self, index: usize) -> MenuItemRef<'_>;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl MenuSource for MenuView {
    fn id(&self) -> &str {
        &self.id
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn len(&self) -> usize {
        self.items.len()
    }

    fn item(&self, index: usize) -> MenuItemRef<'_> {
        let item = &self.items[index];
        MenuItemRef {
            id: &item.id,
            label: &item.label,
            subtitle: item.subtitle.as_deref(),
            shortcut: item.shortcut.as_deref(),
            glyph: item.glyph,
            kind: item.kind,
            enabled: item.enabled,
        }
    }
}

/// Read-only renderer adapter over the current semantic-core menu level.
///
/// It consumes the core-owned visible-row projection without copying semantic
/// strings or introducing presentation state into nuraloumi-core.
pub struct CoreMenuAdapter<'a> {
    model: &'a nuraloumi_core::MenuModel,
    visible_items: Vec<&'a nuraloumi_core::MenuItem>,
}

impl<'a> CoreMenuAdapter<'a> {
    pub fn new(model: &'a nuraloumi_core::MenuModel, state: &nuraloumi_core::MenuState) -> Self {
        Self {
            model,
            visible_items: state.visible_items(model),
        }
    }
}

impl MenuSource for CoreMenuAdapter<'_> {
    fn id(&self) -> &str {
        &self.model.id
    }

    fn title(&self) -> &str {
        &self.model.title
    }

    fn len(&self) -> usize {
        self.visible_items.len()
    }

    fn item(&self, index: usize) -> MenuItemRef<'_> {
        let item = self.visible_items[index];
        MenuItemRef {
            id: &item.id,
            label: &item.label,
            subtitle: item.subtitle.as_deref(),
            shortcut: None,
            glyph: None,
            kind: RowKind::from(item),
            enabled: item.enabled,
        }
    }
}

impl From<&nuraloumi_core::MenuItem> for RowKind {
    fn from(item: &nuraloumi_core::MenuItem) -> Self {
        match item.kind {
            nuraloumi_core::MenuItemKind::Action => Self::Action,
            nuraloumi_core::MenuItemKind::Submenu => Self::Submenu,
            nuraloumi_core::MenuItemKind::Checkable => Self::Checkable {
                checked: item.checked.unwrap_or(false),
            },
            nuraloumi_core::MenuItemKind::Section => Self::Section,
            nuraloumi_core::MenuItemKind::Status => Self::Status,
            nuraloumi_core::MenuItemKind::Separator => Self::Separator,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InteractionState {
    pub selected_id: Option<String>,
    pub pressed_id: Option<String>,
    pub scroll_offset: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub size: f64,
    pub bold: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextMetrics {
    pub width: f64,
    pub height: f64,
    pub ascent: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PaintNode {
    FillRect {
        rect: Rect,
        color: Color,
    },
    RoundedRect {
        rect: Rect,
        radius: f64,
        fill: Fill,
        stroke: Option<(Color, f64)>,
    },
    Line {
        from: Point,
        to: Point,
        width: f64,
        color: Color,
    },
    Text {
        origin: Point,
        text: String,
        style: TextStyle,
        color: Color,
    },
    Chevron {
        rect: Rect,
        color: Color,
    },
    CheckMark {
        rect: Rect,
        checked: bool,
        color: Color,
        accent: Color,
    },
    ClipPush(Rect),
    ClipPop,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HitRegion {
    pub item_id: String,
    pub rect: Rect,
    pub actionable: bool,
    pub enabled: bool,
    pub kind: RowKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollWindow {
    pub viewport: Rect,
    pub content_height: f64,
    pub offset: f64,
    pub max_offset: f64,
    pub clipped: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub viewport: Viewport,
    pub menu_id: String,
    pub panel_rect: Rect,
    pub paint: Vec<PaintNode>,
    pub hits: Vec<HitRegion>,
    pub scroll: ScrollWindow,
}

impl Scene {
    /// Returns a stable item identity only for enabled actionable rows.
    pub fn hit_test(&self, x: f64, y: f64) -> Option<&str> {
        self.hits
            .iter()
            .find(|hit| hit.enabled && hit.actionable && hit.rect.contains(x, y))
            .map(|hit| hit.item_id.as_str())
    }
}
