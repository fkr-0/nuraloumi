//! Fixed browser chrome composed from the public Cairo scene primitives.
use nuraloumi_core::{MenuItem, MenuModel, MenuState, ThemeTokens};
use nuraloumi_render_cairo::{
    DeterministicText, HitRegion, PaintNode, Point, Rect, RowKind, Scene, ScrollWindow, TextStyle,
    Theme, Viewport,
};

pub const TAB_HEIGHT: f64 = 52.0;
pub const ADDRESS_HEIGHT: f64 = 56.0;
pub const ACTION_HEIGHT: f64 = 48.0;
const ACTIONS: [&str; 9] = [
    "browser.go",
    "browser.back",
    "browser.forward",
    "browser.reload",
    "browser.stop",
    "browser.tab.new",
    "browser.tab.close",
    "browser.bookmark.toggle",
    "browser.bookmark.expand",
];

pub fn action_columns(width: f64) -> usize {
    if width >= 900.0 {
        4
    } else if width >= 640.0 {
        3
    } else if width >= 320.0 {
        2
    } else {
        1
    }
}

fn bounded_text(label: &str, width: f64, style: TextStyle) -> String {
    if width <= 0.0 {
        return String::new();
    }
    if DeterministicText::run_width(label, style) <= width {
        return label.into();
    }
    let mut result = String::new();
    for c in label.chars() {
        let candidate = format!("{result}{c}…");
        if DeterministicText::run_width(&candidate, style) > width {
            break;
        }
        result.push(c);
    }
    if result.is_empty() && DeterministicText::run_width("…", style) > width {
        String::new()
    } else {
        format!("{result}…")
    }
}

fn control(
    paint: &mut Vec<PaintNode>,
    hits: &mut Vec<HitRegion>,
    item: &MenuItem,
    rect: Rect,
    selected: bool,
    theme: &Theme,
) {
    paint.push(PaintNode::FillRect {
        rect,
        color: if selected {
            theme.selected
        } else {
            theme.raised
        },
    });
    paint.push(PaintNode::Line {
        from: Point::new(rect.x, rect.bottom()),
        to: Point::new(rect.right(), rect.bottom()),
        width: 1.0,
        color: theme.separator,
    });
    let style = TextStyle {
        size: 16.0,
        bold: selected,
    };
    let inset = 12.0_f64.min(rect.width / 4.0);
    let text_rect = Rect::new(
        rect.x + inset,
        rect.y,
        (rect.width - inset * 2.0).max(0.0),
        rect.height,
    );
    paint.push(PaintNode::ClipPush(text_rect));
    paint.push(PaintNode::Text {
        origin: Point::new(text_rect.x, rect.y + rect.height / 2.0 + 5.0),
        text: bounded_text(&item.label, text_rect.width, style),
        style,
        color: if item.enabled || selected {
            theme.primary_text
        } else {
            theme.hint
        },
    });
    paint.push(PaintNode::ClipPop);
    hits.push(HitRegion {
        item_id: item.id.clone(),
        rect,
        actionable: item.is_actionable(),
        enabled: item.enabled && item.is_actionable(),
        kind: RowKind::from(item),
    });
}

/// Fixed strip and address, three action columns at 720–820, clipped lower list.
/// The lower list follows the selected item for keyboard-only navigation.
pub fn browser_scene(
    menu: &MenuModel,
    state: &MenuState,
    viewport: Viewport,
    tokens: &ThemeTokens,
    requested_offset: f64,
) -> Scene {
    let theme = Theme::from(tokens);
    let width = viewport.width;
    let mut paint = vec![PaintNode::FillRect {
        rect: viewport.logical_rect(),
        color: theme.base,
    }];
    let mut hits = Vec::new();
    let tabs: Vec<_> = menu
        .items
        .iter()
        .filter(|item| {
            item.id
                .strip_prefix("browser.tab.")
                .is_some_and(|suffix| suffix.parse::<usize>().is_ok())
        })
        .take(super::MAX_TABS)
        .collect();
    let tab_width = width / tabs.len().max(1) as f64;
    for (i, item) in tabs.iter().enumerate() {
        control(
            &mut paint,
            &mut hits,
            item,
            Rect::new(i as f64 * tab_width, 0.0, tab_width, TAB_HEIGHT),
            !item.enabled,
            &theme,
        );
    }
    if let Some(item) = menu.item_by_id("browser.address") {
        control(
            &mut paint,
            &mut hits,
            item,
            Rect::new(0.0, TAB_HEIGHT, width, ADDRESS_HEIGHT),
            state.selected_id.as_deref() == Some("browser.address"),
            &theme,
        );
    }
    let columns = action_columns(width);
    let cell = width / columns as f64;
    let actions_top = TAB_HEIGHT + ADDRESS_HEIGHT;
    for (i, id) in ACTIONS.iter().enumerate() {
        if let Some(item) = menu.item_by_id(id) {
            control(
                &mut paint,
                &mut hits,
                item,
                Rect::new(
                    (i % columns) as f64 * cell,
                    actions_top + (i / columns) as f64 * ACTION_HEIGHT,
                    cell,
                    ACTION_HEIGHT,
                ),
                state.selected_id.as_deref() == Some(id),
                &theme,
            );
        }
    }
    let body_top = actions_top + ACTIONS.len().div_ceil(columns) as f64 * ACTION_HEIGHT;
    let body = Rect::new(0.0, body_top, width, (viewport.height - body_top).max(0.0));
    let body_items: Vec<_> = menu
        .items
        .iter()
        .filter(|item| {
            (item.id.starts_with("browser.bookmark.")
                && item.id != "browser.bookmark.toggle"
                && item.id != "browser.bookmark.expand")
                || item.id == "browser.status"
        })
        .collect();
    let row_height = |item: &MenuItem| if item.is_actionable() { 48.0 } else { 36.0 };
    let content_height: f64 = body_items.iter().map(|item| row_height(item)).sum();
    let max_offset = (content_height - body.height).max(0.0);
    let mut offset = requested_offset.clamp(0.0, max_offset);
    let mut cursor = 0.0;
    for item in &body_items {
        let h = row_height(item);
        if state.selected_id.as_deref() == Some(item.id.as_str()) && body.height > 0.0 {
            if cursor < offset {
                offset = cursor;
            } else if cursor + h > offset + body.height {
                offset = cursor + h - body.height;
            }
            offset = offset.clamp(0.0, max_offset);
        }
        cursor += h;
    }
    paint.push(PaintNode::ClipPush(body));
    cursor = 0.0;
    for item in body_items {
        let h = row_height(item);
        let rect = Rect::new(0.0, body_top + cursor - offset, width, h);
        cursor += h;
        if let Some(visible) = rect.intersection(body) {
            if item.is_actionable() {
                let index = hits.len();
                control(
                    &mut paint,
                    &mut hits,
                    item,
                    rect,
                    state.selected_id.as_deref() == Some(item.id.as_str()),
                    &theme,
                );
                hits[index].rect = visible;
            } else {
                paint.push(PaintNode::FillRect {
                    rect,
                    color: theme.overlay,
                });
                let style = TextStyle {
                    size: 14.0,
                    bold: false,
                };
                paint.push(PaintNode::Text {
                    origin: Point::new(12.0, rect.y + h / 2.0 + 5.0),
                    text: bounded_text(&item.label, width - 24.0, style),
                    style,
                    color: theme.secondary_text,
                });
            }
        }
    }
    paint.push(PaintNode::ClipPop);
    paint.push(PaintNode::Line {
        from: Point::new(0.0, body_top),
        to: Point::new(width, body_top),
        width: 1.0,
        color: theme.border,
    });
    Scene {
        viewport,
        menu_id: menu.id.clone(),
        panel_rect: viewport.logical_rect(),
        paint,
        hits,
        scroll: ScrollWindow {
            viewport: body,
            content_height,
            offset,
            max_offset,
            clipped: max_offset > 0.0,
        },
    }
}
