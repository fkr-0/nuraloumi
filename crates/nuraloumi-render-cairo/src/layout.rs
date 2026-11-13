use crate::{
    model::{
        HitRegion, InteractionState, MenuSource, PaintNode, Point, Rect, RowKind, Scene,
        ScrollWindow, TextMetrics, TextStyle, Viewport,
    },
    render::TextMeasurer,
    theme::{Color, Fill, Theme},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutMetrics {
    pub outer_margin: f64,
    pub content_insets: crate::Insets,
    pub header_height: f64,
    pub primary_row_height: f64,
    pub compact_row_height: f64,
    pub separator_row_height: f64,
    pub target_width: f64,
    pub min_target_width: f64,
    pub max_target_width: f64,
    pub icon_size: f64,
    pub icon_gap: f64,
}

impl Default for LayoutMetrics {
    fn default() -> Self {
        Self::from_core_metrics(&nuraloumi_core::UI_METRICS)
    }
}

impl LayoutMetrics {
    pub fn from_core_metrics(metrics: &nuraloumi_core::UiMetrics) -> Self {
        Self {
            // SL101 is a 1280x800 touch display: keep the 48px actionable
            // contract while trimming non-interactive chrome and side padding.
            outer_margin: 8.0,
            content_insets: crate::Insets::all(f64::from(metrics.spacing.md)),
            header_height: 48.0,
            primary_row_height: 48.0,
            compact_row_height: 32.0,
            separator_row_height: 12.0,
            target_width: 448.0,
            min_target_width: 400.0,
            max_target_width: 468.0,
            icon_size: 18.0,
            icon_gap: f64::from(metrics.spacing.sm),
        }
    }

    fn for_text_scale(mut self, scale: f64) -> Self {
        // Structural chrome remains compact; only rows and headers grow as
        // required by the actual scaled line heights.
        self.header_height = self.header_height.max(18.0 * scale * 1.2 + 24.0);
        self.compact_row_height = self.compact_row_height.max(13.0 * scale * 1.2 + 16.0);
        self
    }

    fn row_height(self, kind: RowKind, has_subtitle: bool, scale: f64) -> f64 {
        match kind {
            RowKind::Action | RowKind::Submenu | RowKind::Checkable { .. } => {
                let one_line = 15.0 * scale * 1.2 + 20.0;
                let two_lines = (15.0 + 11.0) * scale * 1.2 + 16.0;
                self.primary_row_height.max(48.0).max(if has_subtitle {
                    two_lines
                } else {
                    one_line
                })
            }
            RowKind::Section | RowKind::Status => self.compact_row_height,
            RowKind::Separator => self.separator_row_height,
        }
    }
}

fn panel_width(viewport: Viewport, metrics: LayoutMetrics) -> f64 {
    let available = (viewport.width - metrics.outer_margin * 2.0).max(1.0);
    if available < metrics.min_target_width {
        available
    } else {
        metrics
            .target_width
            .clamp(metrics.min_target_width, metrics.max_target_width)
            .min(available)
    }
}

fn centered_baseline<T: TextMeasurer>(row: Rect, text: &T, style: TextStyle) -> f64 {
    let line = text.measure("Mg", style);
    row.y + (row.height - line.height).max(0.0) / 2.0 + line.ascent
}

fn action_baselines<T: TextMeasurer>(
    row: Rect,
    text: &T,
    selected: bool,
    subtitle: bool,
    scale: f64,
) -> (f64, f64) {
    let main = text.measure(
        "Mg",
        TextStyle {
            size: 15.0 * scale,
            bold: selected,
        },
    );
    if !subtitle {
        return (
            row.y + (row.height - main.height).max(0.0) / 2.0 + main.ascent,
            0.0,
        );
    }
    let secondary = text.measure(
        "Mg",
        TextStyle {
            size: 11.0 * scale,
            bold: false,
        },
    );
    let gap = 4.0;
    let top = row.y + (row.height - main.height - secondary.height - gap).max(0.0) / 2.0;
    (
        top + main.ascent,
        top + main.height + gap + secondary.ascent,
    )
}

/// Retains the legacy layout contract at 1x text scale.
pub fn layout_menu<M: MenuSource, T: TextMeasurer>(
    menu: &M,
    interaction: &InteractionState,
    viewport: Viewport,
    theme: &Theme,
    text: &T,
    constrained: bool,
) -> Scene {
    layout_menu_scaled(menu, interaction, viewport, theme, text, constrained, 1.0)
}

/// Build a measured, scrollable menu scene with an independent text scale.
///
/// The scale is finite-clamped to 1.0..=2.0. Output scale still controls only
/// device pixels; 48 logical-pixel touch targets never shrink.
pub fn layout_menu_scaled<M: MenuSource, T: TextMeasurer>(
    menu: &M,
    interaction: &InteractionState,
    viewport: Viewport,
    theme: &Theme,
    text: &T,
    constrained: bool,
    requested_text_scale: f64,
) -> Scene {
    let scale = if requested_text_scale.is_finite() {
        requested_text_scale.clamp(1.0, 2.0)
    } else {
        1.0
    };
    let metrics = LayoutMetrics::default().for_text_scale(scale);
    let fullscreen_launcher = menu.id() == "launcher" && viewport.width >= 850.0;
    let wide_browser = menu.id() == "browser" && viewport.width >= 650.0;
    let width = if fullscreen_launcher || wide_browser {
        (viewport.width - metrics.outer_margin * 2.0).max(1.0)
    } else {
        panel_width(viewport, metrics)
    };
    let x = ((viewport.width - width) / 2.0).max(0.0);
    let tab_prefix = match menu.id() {
        "launcher" => Some("overview.mode."),
        "browser" => Some("browser.tab."),
        _ => None,
    };
    let tab_count = if let Some(prefix) = tab_prefix {
        (0..menu.len())
            .take_while(|&index| menu.item(index).id.starts_with(prefix))
            .count()
    } else {
        0
    };
    let tab_height = if tab_count > 0 { 52.0 } else { 0.0 };
    let max_panel_height =
        (viewport.height - metrics.outer_margin * 2.0).max(metrics.header_height + tab_height);

    // Responsive overview grid: section/empty rows span the width; actions
    // become three columns only on the optional wide launcher surface. These
    // are measured scene rectangles, not a separate GPU or widget hierarchy.
    let columns = if fullscreen_launcher || wide_browser {
        3
    } else {
        1
    };
    let mut content_rows = Vec::with_capacity(menu.len().saturating_sub(tab_count));
    let mut content_y = 0.0_f64;
    let mut group_column = 0_usize;
    let mut group_height = 0.0_f64;
    for index in tab_count..menu.len() {
        let item = menu.item(index);
        let height = metrics.row_height(item.kind, item.subtitle.is_some(), scale);
        if columns > 1 && item.kind.actionable() && item.id != "browser.address" {
            let column_width = width / columns as f64;
            content_rows.push((
                index,
                Rect::new(
                    x + group_column as f64 * column_width,
                    content_y,
                    column_width,
                    height,
                ),
            ));
            group_column += 1;
            group_height = group_height.max(height);
            if group_column == columns {
                content_y += group_height;
                group_column = 0;
                group_height = 0.0;
            }
        } else {
            if group_column > 0 {
                content_y += group_height;
                group_column = 0;
                group_height = 0.0;
            }
            content_rows.push((index, Rect::new(x, content_y, width, height)));
            content_y += height;
        }
    }
    let content_height = content_y + if group_column > 0 { group_height } else { 0.0 };
    let natural_height = metrics.header_height + tab_height + content_height;
    let panel_height = natural_height.min(max_panel_height);
    let panel = Rect::new(
        x,
        metrics
            .outer_margin
            .min((viewport.height - panel_height).max(0.0)),
        width,
        panel_height,
    );
    let body = Rect::new(
        panel.x,
        panel.y + metrics.header_height + tab_height,
        panel.width,
        // Tabs occupy their own fixed strip above the scrollable content.
        // Excluding them from the viewport also fixes max_offset and keeps
        // the last actionable row inside the actual Wayland surface.
        (panel.height - metrics.header_height - tab_height).max(0.0),
    );

    let max_offset = (content_height - body.height).max(0.0);
    let mut offset = if interaction.scroll_offset.is_finite() {
        interaction.scroll_offset.clamp(0.0, max_offset)
    } else {
        0.0
    };
    // Keep keyboard focus in view, even when a caller supplies a stale
    // scroll offset after navigation or a text-size change.
    if let Some(selected_id) = interaction.selected_id.as_deref() {
        for (index, rect) in &content_rows {
            let index = *index;
            let item = menu.item(index);
            let top = rect.y;
            let height = rect.height;
            if item.id == selected_id && item.kind.actionable() {
                if top < offset || height > body.height {
                    offset = top;
                } else if top + height > offset + body.height {
                    offset = top + height - body.height;
                }
                offset = offset.clamp(0.0, max_offset);
                break;
            }
        }
    }
    let scroll = ScrollWindow {
        viewport: body,
        content_height,
        offset,
        max_offset,
        clipped: content_height > body.height,
    };

    let mut paint = Vec::with_capacity(menu.len() * 5 + 8);
    paint.push(PaintNode::FillRect {
        rect: viewport.logical_rect(),
        color: theme.base,
    });

    let panel_fill = if constrained {
        Fill::Solid(theme.raised)
    } else {
        Fill::Solid(theme.overlay)
    };
    paint.push(PaintNode::RoundedRect {
        rect: panel,
        radius: 0.0,
        fill: panel_fill,
        stroke: Some((theme.border, theme.border_width)),
    });

    let title_style = TextStyle {
        size: 18.0 * scale,
        bold: true,
    };
    let title = text.ellipsize(
        menu.title(),
        panel.width - metrics.content_insets.left - metrics.content_insets.right,
        title_style,
    );
    let title_metrics = text.measure(&title, title_style);
    let title_baseline = panel.y
        + (metrics.header_height - title_metrics.height).max(0.0) / 2.0
        + title_metrics.ascent;
    paint.push(PaintNode::Text {
        origin: Point::new(panel.x + metrics.content_insets.left, title_baseline),
        text: title,
        style: title_style,
        color: theme.primary_text,
    });
    paint.push(PaintNode::Line {
        from: Point::new(panel.x + 1.0, body.y),
        to: Point::new(panel.right() - 1.0, body.y),
        width: 1.0,
        color: theme.separator,
    });

    // Visual tab row is outside the scrolling body. The selected tab is
    // disabled in the semantic model; it remains visibly active but cannot
    // dispatch redundant navigation actions.
    let mut hits = Vec::with_capacity(menu.len());
    if tab_count > 0 {
        let tab_width = ((panel.width - 16.0) / tab_count as f64).max(1.0);
        for index in 0..tab_count {
            let item = menu.item(index);
            let active = !item.enabled;
            let focused = interaction.selected_id.as_deref() == Some(item.id);
            let rect = Rect::new(
                panel.x + 8.0 + index as f64 * tab_width,
                panel.y + metrics.header_height + 2.0,
                tab_width,
                48.0,
            );
            paint.push(PaintNode::RoundedRect {
                rect: Rect::new(rect.x + 1.0, rect.y, (rect.width - 2.0).max(1.0), 46.0),
                radius: 0.0,
                fill: Fill::Solid(if active { theme.selected } else { theme.card }),
                stroke: if focused {
                    Some((theme.accent, 2.0))
                } else {
                    None
                },
            });
            if active {
                paint.push(PaintNode::FillRect {
                    rect: Rect::new(
                        rect.x + 4.0,
                        rect.bottom() - 4.0,
                        (rect.width - 8.0).max(1.0),
                        3.0,
                    ),
                    color: theme.accent,
                });
            }
            let style = TextStyle {
                size: 12.0 * scale,
                bold: active,
            };
            let label = text.ellipsize(item.label, (rect.width - 10.0).max(1.0), style);
            let label_width = text.measure(&label, style).width;
            paint.push(PaintNode::Text {
                origin: Point::new(
                    rect.x + (rect.width - label_width).max(0.0) / 2.0,
                    centered_baseline(rect, text, style),
                ),
                text: label,
                style,
                color: if active {
                    theme.primary_text
                } else {
                    theme.secondary_text
                },
            });
            hits.push(HitRegion {
                item_id: item.id.to_owned(),
                rect,
                actionable: true,
                enabled: item.enabled,
                kind: item.kind,
            });
        }
    }
    paint.push(PaintNode::ClipPush(body));
    for (index, geometry) in content_rows {
        let item = menu.item(index);
        let row = Rect::new(
            geometry.x,
            body.y + geometry.y - offset,
            geometry.width,
            geometry.height,
        );

        let Some(visible) = row.intersection(body) else {
            continue;
        };

        let selected = interaction.selected_id.as_deref() == Some(item.id);
        let pressed = interaction.pressed_id.as_deref() == Some(item.id);
        let enabled = item.enabled && item.kind.actionable();

        if item.kind.actionable() {
            let fill = if pressed {
                Color::rgba(theme.accent.r, theme.accent.g, theme.accent.b, 58)
            } else if selected {
                theme.selected
            } else {
                theme.card
            };
            let row_box = Rect::new(
                row.x + 6.0,
                row.y + 2.0,
                (row.width - 12.0).max(0.0),
                (row.height - 4.0).max(0.0),
            );
            paint.push(PaintNode::RoundedRect {
                rect: row_box,
                radius: 0.0,
                fill: Fill::Solid(fill),
                // A 2px accent outline makes keyboard focus independent of
                // the fill colour, for both light and dark themes.
                stroke: selected.then_some((theme.accent, 2.0)),
            });
            if selected {
                paint.push(PaintNode::FillRect {
                    rect: Rect::new(
                        row_box.x,
                        row_box.y + 6.0,
                        4.0,
                        (row_box.height - 12.0).max(1.0),
                    ),
                    color: theme.accent,
                });
            }
        }

        match item.kind {
            RowKind::Separator => {
                let y = row.y + row.height / 2.0;
                paint.push(PaintNode::Line {
                    from: Point::new(row.x + metrics.content_insets.left, y),
                    to: Point::new(row.right() - metrics.content_insets.right, y),
                    width: 1.0,
                    color: theme.separator,
                });
            }
            RowKind::Section => {
                let style = TextStyle {
                    size: 12.0 * scale,
                    bold: true,
                };
                let label = text.ellipsize(
                    item.label,
                    row.width - metrics.content_insets.left - metrics.content_insets.right,
                    style,
                );
                paint.push(PaintNode::Text {
                    origin: Point::new(
                        row.x + metrics.content_insets.left,
                        centered_baseline(row, text, style),
                    ),
                    text: label,
                    style,
                    color: theme.accent,
                });
            }
            RowKind::Status => {
                let label_style = TextStyle {
                    size: 13.0 * scale,
                    bold: false,
                };
                let value_style = TextStyle {
                    size: 12.0 * scale,
                    bold: false,
                };
                let right = row.right() - metrics.content_insets.right;
                let value_max = row.width * 0.42;
                let fitted_value = item
                    .subtitle
                    .map(|value| text.ellipsize(value, value_max, value_style));
                let value_width = fitted_value
                    .as_deref()
                    .map(|value| text.measure(value, value_style).width)
                    .unwrap_or(0.0);
                let label_x = row.x + metrics.content_insets.left;
                let label_max =
                    (right - value_width - f64::from(fitted_value.is_some()) * 12.0 - label_x)
                        .max(0.0);
                let label = text.ellipsize(item.label, label_max, label_style);
                paint.push(PaintNode::Text {
                    origin: Point::new(label_x, centered_baseline(row, text, label_style)),
                    text: label,
                    style: label_style,
                    color: theme.secondary_text,
                });
                if let Some(value) = fitted_value {
                    paint.push(PaintNode::Text {
                        origin: Point::new(
                            right - value_width,
                            centered_baseline(row, text, value_style),
                        ),
                        text: value,
                        style: value_style,
                        color: theme.hint,
                    });
                }
            }
            RowKind::Action | RowKind::Submenu | RowKind::Checkable { .. } => {
                if item.glyph.is_some() {
                    let glyph_box = Rect::new(
                        row.x + metrics.content_insets.left,
                        row.y + (row.height - metrics.icon_size) / 2.0,
                        metrics.icon_size,
                        metrics.icon_size,
                    );
                    paint.push(PaintNode::RoundedRect {
                        rect: glyph_box,
                        radius: 0.0,
                        fill: Fill::Solid(Color::rgba(
                            theme.accent.r,
                            theme.accent.g,
                            theme.accent.b,
                            20,
                        )),
                        stroke: Some((
                            Color::rgba(theme.accent.r, theme.accent.g, theme.accent.b, 64),
                            1.0,
                        )),
                    });
                    if let Some(glyph) = item.glyph {
                        paint.push(PaintNode::Text {
                            origin: Point::new(
                                glyph_box.x + 3.0,
                                centered_baseline(
                                    glyph_box,
                                    text,
                                    TextStyle {
                                        size: 13.0 * scale,
                                        bold: true,
                                    },
                                ),
                            ),
                            text: glyph.to_string(),
                            style: TextStyle {
                                size: 13.0 * scale,
                                bold: true,
                            },
                            color: theme.accent,
                        });
                    }
                }

                let text_x = row.x
                    + metrics.content_insets.left
                    + if item.glyph.is_some() {
                        metrics.icon_size + metrics.icon_gap
                    } else {
                        0.0
                    };
                let (title_y, subtitle_y) =
                    action_baselines(row, text, selected, item.subtitle.is_some(), scale);
                let affordance_right = row.right() - metrics.content_insets.right;
                let shortcut_style = TextStyle {
                    size: 12.0 * scale,
                    bold: false,
                };
                let fitted_shortcut = if matches!(item.kind, RowKind::Action) {
                    item.shortcut
                        .map(|shortcut| text.ellipsize(shortcut, 96.0, shortcut_style))
                } else {
                    None
                };
                let shortcut_width = fitted_shortcut
                    .as_deref()
                    .map(|shortcut| text.measure(shortcut, shortcut_style).width)
                    .unwrap_or(0.0);
                let label_right = match item.kind {
                    RowKind::Submenu => affordance_right - 25.0,
                    RowKind::Checkable { .. } => affordance_right - 32.0,
                    RowKind::Action if fitted_shortcut.is_some() => {
                        affordance_right - shortcut_width - 12.0
                    }
                    RowKind::Action => affordance_right,
                    _ => affordance_right,
                };
                let text_max_width = (label_right - text_x).max(0.0);
                let title_style = TextStyle {
                    size: 15.0 * scale,
                    bold: selected,
                };
                let fitted_label = text.ellipsize(item.label, text_max_width, title_style);
                let primary_color = if item.enabled {
                    theme.primary_text
                } else {
                    theme.hint
                };
                paint.push(PaintNode::Text {
                    origin: Point::new(text_x, title_y),
                    text: fitted_label,
                    style: title_style,
                    color: primary_color,
                });
                if let Some(subtitle) = item.subtitle {
                    let subtitle_style = TextStyle {
                        size: 11.0 * scale,
                        bold: false,
                    };
                    let fitted_subtitle = text.ellipsize(subtitle, text_max_width, subtitle_style);
                    paint.push(PaintNode::Text {
                        origin: Point::new(text_x, subtitle_y),
                        text: fitted_subtitle,
                        style: subtitle_style,
                        color: if item.enabled {
                            theme.secondary_text
                        } else {
                            theme.hint
                        },
                    });
                }

                match item.kind {
                    RowKind::Submenu => {
                        paint.push(PaintNode::Chevron {
                            rect: Rect::new(
                                affordance_right - 13.0,
                                row.y + (row.height - 18.0) / 2.0,
                                12.0,
                                18.0,
                            ),
                            color: if item.enabled {
                                theme.secondary_text
                            } else {
                                theme.hint
                            },
                        });
                    }
                    RowKind::Checkable { checked } => {
                        paint.push(PaintNode::CheckMark {
                            rect: Rect::new(
                                affordance_right - 20.0,
                                row.y + (row.height - 20.0) / 2.0,
                                20.0,
                                20.0,
                            ),
                            checked,
                            color: theme.secondary_text,
                            accent: theme.accent,
                        });
                    }
                    RowKind::Action => {
                        if let Some(shortcut) = fitted_shortcut {
                            let measured: TextMetrics = text.measure(&shortcut, shortcut_style);
                            paint.push(PaintNode::Text {
                                origin: Point::new(
                                    affordance_right - measured.width,
                                    centered_baseline(row, text, shortcut_style),
                                ),
                                text: shortcut,
                                style: shortcut_style,
                                color: theme.hint,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }

        // A partially clipped actionable row is visible but is deliberately
        // not touch-active: exposing a smaller clipped region would violate
        // the 48 logical-pixel minimum target contract.
        if !item.kind.actionable() || visible.height >= 48.0 {
            hits.push(HitRegion {
                item_id: item.id.to_owned(),
                rect: visible,
                actionable: item.kind.actionable(),
                enabled,
                kind: item.kind,
            });
        }
    }
    if scroll.clipped && body.height > 0.0 {
        // A finite scroll indicator communicates off-screen content without
        // reducing actionable hit geometry.
        let thumb_height = (body.height * body.height / content_height)
            .clamp(12.0_f64.min(body.height), body.height);
        let travel = body.height - thumb_height;
        let thumb_y = body.y
            + if max_offset > 0.0 {
                travel * offset / max_offset
            } else {
                0.0
            };
        paint.push(PaintNode::FillRect {
            rect: Rect::new(panel.right() - 4.0, thumb_y, 2.0, thumb_height),
            color: theme.accent,
        });
    }
    paint.push(PaintNode::ClipPop);

    Scene {
        viewport,
        menu_id: menu.id().to_owned(),
        panel_rect: panel,
        paint,
        hits,
        scroll,
    }
}

#[cfg(test)]
mod launcher_tab_tests {
    use super::*;
    use crate::{MenuItemView, MenuView, ToyText};

    fn launcher_fixture() -> MenuView {
        let mut items = Vec::new();
        for (index, name) in ["Home", "Apps", "Windows", "Spaces", "Tasks"]
            .iter()
            .enumerate()
        {
            items.push(MenuItemView {
                id: format!("overview.mode.{name}"),
                label: (*name).into(),
                subtitle: None,
                shortcut: None,
                glyph: None,
                kind: RowKind::Action,
                enabled: index != 0,
            });
        }
        items.push(MenuItemView {
            id: "section.apps".into(),
            label: "Applications".into(),
            subtitle: None,
            shortcut: None,
            glyph: None,
            kind: RowKind::Section,
            enabled: false,
        });
        for index in 0..9 {
            items.push(MenuItemView::action(
                format!("application.{index}"),
                format!("App {index}"),
            ));
        }
        MenuView {
            id: "launcher".into(),
            title: "Launcher".into(),
            items,
        }
    }

    #[test]
    fn launcher_last_action_scrolls_fully_into_clipped_body() {
        let mut menu = launcher_fixture();
        for index in 9..80 {
            menu.items.push(MenuItemView::action(
                format!("application.{index}"),
                format!("App {index}"),
            ));
        }
        for width in [448.0, 1264.0] {
            let scene = layout_menu(
                &menu,
                &InteractionState {
                    selected_id: Some("application.79".into()),
                    ..InteractionState::default()
                },
                Viewport::new(width, 320.0, 1.0),
                &Theme::dark(),
                &ToyText,
                false,
            );
            let body = scene.scroll.viewport;
            let last = scene
                .hits
                .iter()
                .find(|hit| hit.item_id == "application.79")
                .expect("last action should be reachable");
            assert!(scene.scroll.clipped);
            assert!(scene.scroll.max_offset > 0.0);
            assert!(body.y >= scene.panel_rect.y);
            assert!(
                body.bottom() <= scene.panel_rect.bottom() + f64::EPSILON,
                "scroll body must fit inside menu: width {width}"
            );
            assert!(
                last.rect.y >= body.y && last.rect.bottom() <= body.bottom() + f64::EPSILON,
                "last selected action must remain fully tappable at width {width}"
            );
            assert_eq!(
                scene.hit_test(
                    last.rect.x + last.rect.width / 2.0,
                    last.rect.y + last.rect.height / 2.0,
                ),
                Some("application.79"),
            );
        }
    }

    #[test]
    fn tabs_have_persistent_horizontal_hit_regions_and_grid_only_when_wide() {
        let menu = launcher_fixture();
        let compact = layout_menu(
            &menu,
            &InteractionState::default(),
            Viewport::new(448.0, 700.0, 1.0),
            &Theme::dark(),
            &ToyText,
            false,
        );
        let wide = layout_menu(
            &menu,
            &InteractionState::default(),
            Viewport::new(1264.0, 720.0, 1.0),
            &Theme::dark(),
            &ToyText,
            false,
        );
        for scene in [&compact, &wide] {
            let tabs: Vec<_> = scene
                .hits
                .iter()
                .filter(|hit| hit.item_id.starts_with("overview.mode."))
                .collect();
            assert_eq!(tabs.len(), 5);
            assert!(tabs.iter().all(|hit| hit.rect.height >= 48.0));
            assert!(tabs.windows(2).all(|pair| {
                pair[0].rect.y == pair[1].rect.y
                    && pair[0].rect.right() <= pair[1].rect.x + f64::EPSILON
            }));
            assert!(!tabs[0].enabled);
            assert!(tabs[1].enabled);
            assert_eq!(
                scene.hit_test(tabs[1].rect.x + 20.0, tabs[1].rect.y + 20.0),
                Some("overview.mode.Apps"),
            );
        }
        let app = |scene: &Scene, id: &str| {
            scene
                .hits
                .iter()
                .find(|hit| hit.item_id == id)
                .unwrap()
                .rect
        };
        assert!(compact.panel_rect.width < 500.0);
        assert!(wide.panel_rect.width > 1200.0);
        assert!(app(&compact, "application.1").y > app(&compact, "application.0").y);
        assert_eq!(app(&wide, "application.1").y, app(&wide, "application.0").y);
        assert!(app(&wide, "application.1").x > app(&wide, "application.0").x);
        assert!(app(&wide, "application.0").height >= 48.0);
    }
    #[test]
    fn browser_tabs_are_natively_drawn_in_fixed_touch_strip() {
        let menu = MenuView {
            id: "browser".into(),
            title: "Nura Browser".into(),
            items: vec![
                MenuItemView {
                    id: "browser.tab.0".into(),
                    label: "First".into(),
                    subtitle: None,
                    shortcut: None,
                    glyph: None,
                    kind: RowKind::Action,
                    enabled: false,
                },
                MenuItemView::action("browser.tab.1", "Second"),
                MenuItemView::action("browser.address", "URL: https://example.org/"),
                MenuItemView::action("browser.go", "Go"),
            ],
        };
        let scene = layout_menu(
            &menu,
            &InteractionState::default(),
            Viewport::new(480.0, 360.0, 1.0),
            &Theme::dark(),
            &ToyText,
            false,
        );
        let first = scene
            .hits
            .iter()
            .find(|hit| hit.item_id == "browser.tab.0")
            .unwrap();
        let second = scene
            .hits
            .iter()
            .find(|hit| hit.item_id == "browser.tab.1")
            .unwrap();
        let address = scene
            .hits
            .iter()
            .find(|hit| hit.item_id == "browser.address")
            .unwrap();
        assert_eq!(first.rect.y, second.rect.y);
        assert_eq!(first.rect.height, 48.0);
        assert!(second.rect.width >= 48.0);
        assert!(!first.enabled);
        assert!(second.enabled);
        assert!(address.rect.y >= scene.scroll.viewport.y);
        assert_eq!(
            scene.hit_test(second.rect.x + 20.0, second.rect.y + 20.0),
            Some("browser.tab.1")
        );
    }
}
