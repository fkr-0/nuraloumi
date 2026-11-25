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

/// Five Control Center categories use simple Cairo strokes, not private-use
/// Nerd Font characters. The tablet's minimal musl font set cannot guarantee
/// those codepoints and otherwise paints missing-glyph rectangles.
fn paint_control_category_icon(
    item_id: &str,
    bounds: Rect,
    color: Color,
    paint: &mut Vec<PaintNode>,
) -> bool {
    type Stroke = (f64, f64, f64, f64);
    let category = item_id.strip_prefix("control.tab.").or_else(|| {
        item_id
            .strip_prefix("control.")
            .and_then(|rest| rest.split_once('.').map(|(category, _)| category))
    });
    let segments: &[Stroke] = match category {
        Some("media") => &[
            (0.27, 0.18, 0.27, 0.82),
            (0.27, 0.18, 0.78, 0.5),
            (0.78, 0.5, 0.27, 0.82),
        ],
        Some("network") => &[
            (0.12, 0.36, 0.31, 0.23),
            (0.31, 0.23, 0.69, 0.23),
            (0.69, 0.23, 0.88, 0.36),
            (0.28, 0.55, 0.44, 0.45),
            (0.44, 0.45, 0.56, 0.45),
            (0.56, 0.45, 0.72, 0.55),
            (0.48, 0.76, 0.52, 0.76),
        ],
        Some("display") => &[
            (0.13, 0.18, 0.87, 0.18),
            (0.87, 0.18, 0.87, 0.68),
            (0.87, 0.68, 0.13, 0.68),
            (0.13, 0.68, 0.13, 0.18),
            (0.50, 0.68, 0.50, 0.87),
            (0.32, 0.87, 0.68, 0.87),
        ],
        Some("system") => &[
            (0.50, 0.08, 0.50, 0.29),
            (0.50, 0.71, 0.50, 0.92),
            (0.08, 0.50, 0.29, 0.50),
            (0.71, 0.50, 0.92, 0.50),
            (0.22, 0.22, 0.36, 0.36),
            (0.64, 0.64, 0.78, 0.78),
            (0.78, 0.22, 0.64, 0.36),
            (0.36, 0.64, 0.22, 0.78),
            (0.36, 0.36, 0.64, 0.36),
            (0.64, 0.36, 0.64, 0.64),
            (0.64, 0.64, 0.36, 0.64),
            (0.36, 0.64, 0.36, 0.36),
        ],
        Some("notifications") => &[
            (0.32, 0.31, 0.50, 0.17),
            (0.50, 0.17, 0.68, 0.31),
            (0.32, 0.31, 0.32, 0.67),
            (0.68, 0.31, 0.68, 0.67),
            (0.32, 0.67, 0.22, 0.75),
            (0.22, 0.75, 0.78, 0.75),
            (0.78, 0.75, 0.68, 0.67),
            (0.44, 0.84, 0.56, 0.84),
        ],
        _ => return false,
    };
    for &(x0, y0, x1, y1) in segments {
        paint.push(PaintNode::Line {
            from: Point::new(bounds.x + x0 * bounds.width, bounds.y + y0 * bounds.height),
            to: Point::new(bounds.x + x1 * bounds.width, bounds.y + y1 * bounds.height),
            width: 1.8,
            color,
        });
    }

    true
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
                    if !paint_control_category_icon(item.id, glyph_box, theme.accent, &mut paint) {
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

    #[test]
    fn control_center_categories_draw_icons_without_private_font_glyphs() {
        let mut menu = MenuView {
            id: "control-center".into(),
            title: "Control Center".into(),
            items: Vec::new(),
        };
        for category in ["media", "network", "display", "system", "notifications"] {
            let id = format!("control.tab.{category}");
            let mut strokes = Vec::new();
            assert!(paint_control_category_icon(
                &id,
                Rect::new(0.0, 0.0, 18.0, 18.0),
                Theme::dark().accent,
                &mut strokes,
            ));
            assert!(
                strokes.len() >= 3
                    && strokes
                        .iter()
                        .all(|node| matches!(node, PaintNode::Line { .. }))
            );
            let mut entry = MenuItemView::action(id, category);
            entry.glyph = Some('󰒓'); // Not present on the tablet's stock font.
            menu.items.push(entry);
        }
        let scene = layout_menu(
            &menu,
            &InteractionState::default(),
            Viewport::new(448.0, 400.0, 1.0),
            &Theme::dark(),
            &ToyText,
            false,
        );
        assert!(scene.paint.iter().all(|node| !matches!(
            node,
            PaintNode::Text { text, .. } if text.contains('󰒓')
        )));
    }

    #[test]
    fn control_icons_fit_bounds_and_rasterize_across_themes_widths_and_scales() {
        use crate::render::{CairoRenderer, DeterministicText};

        let categories = ["media", "network", "display", "system", "notifications"];
        for width in [448.0, 280.0] {
            for scale in [1.0, 2.0] {
                for theme in [Theme::dark(), Theme::light()] {
                    let mut menu = MenuView {
                        id: "control-center".into(),
                        title: "Control Center".into(),
                        items: Vec::new(),
                    };
                    for (index, category) in categories.iter().enumerate() {
                        let mut entry =
                            MenuItemView::action(format!("control.tab.{category}"), *category);
                        entry.glyph = Some('\u{f0c9}');
                        entry.enabled = index != 4;
                        menu.items.push(entry);
                    }
                    let interaction = InteractionState {
                        selected_id: Some("control.tab.media".into()),
                        ..InteractionState::default()
                    };
                    let viewport = Viewport::new(width, 400.0, scale);
                    let scene = layout_menu(&menu, &interaction, viewport, &theme, &ToyText, false);
                    let strokes: Vec<_> = scene
                        .paint
                        .iter()
                        .filter_map(|node| {
                            if let PaintNode::Line {
                                from,
                                to,
                                width,
                                color,
                            } = node
                            {
                                (*width == 1.8 && *color == theme.accent).then_some((*from, *to))
                            } else {
                                None
                            }
                        })
                        .collect();
                    assert!(
                        strokes.len() >= 30,
                        "missing category strokes at {width}px/{scale}x"
                    );
                    for (index, category) in categories.iter().enumerate() {
                        let hit = scene
                            .hits
                            .iter()
                            .find(|hit| hit.item_id == format!("control.tab.{category}"))
                            .unwrap();
                        assert_eq!(hit.enabled, index != 4);
                        let icon = Rect::new(
                            hit.rect.x + LayoutMetrics::default().content_insets.left,
                            hit.rect.y + (hit.rect.height - 18.0) / 2.0,
                            18.0,
                            18.0,
                        );
                        let matching: Vec<_> = strokes
                            .iter()
                            .filter(|(from, to)| {
                                icon.contains(from.x, from.y) && icon.contains(to.x, to.y)
                            })
                            .collect();
                        assert!(matching.len() >= 3, "{category} has no in-bounds strokes");
                        // Cairo's 1.8px round/cap footprint must also remain inside the icon box.
                        for (from, to) in matching {
                            for point in [from, to] {
                                assert!(point.x - 0.9 >= icon.x && point.x + 0.9 <= icon.right());
                                assert!(point.y - 0.9 >= icon.y && point.y + 0.9 <= icon.bottom());
                            }
                        }
                    }
                    assert!(!scene.paint.iter().any(|node| matches!(node,
                        PaintNode::Text { text, .. } if text.chars().any(|ch| ('\u{e000}'..='\u{f8ff}').contains(&ch)))));
                    let renderer = CairoRenderer::with_text_backend(DeterministicText);
                    let mut buffer = renderer.render_scene(&scene).unwrap();
                    let (pixel_count, _) = buffer
                        .with_argb32_bytes(|bytes, info| {
                            let stride = info.stride as usize;
                            let mut totals = [0_usize; 5];
                            for (index, category) in categories.iter().enumerate() {
                                let hit = scene
                                    .hits
                                    .iter()
                                    .find(|hit| hit.item_id == format!("control.tab.{category}"))
                                    .unwrap();
                                let x = ((hit.rect.x + 8.0) * scale) as usize;
                                let y = ((hit.rect.y + (hit.rect.height - 18.0) / 2.0) * scale)
                                    as usize;
                                let side = (18.0 * scale) as usize;
                                for py in y..y + side {
                                    for px in x..x + side {
                                        let offset = py * stride + px * 4;
                                        let raw = u32::from_ne_bytes(
                                            bytes[offset..offset + 4].try_into().unwrap(),
                                        );
                                        let red = ((raw >> 16) & 255) as i32;
                                        let green = ((raw >> 8) & 255) as i32;
                                        let blue = (raw & 255) as i32;
                                        let distance = (red - i32::from(theme.accent.r)).abs()
                                            + (green - i32::from(theme.accent.g)).abs()
                                            + (blue - i32::from(theme.accent.b)).abs();
                                        if distance < 12 {
                                            totals[index] += 1;
                                        }
                                    }
                                }
                            }
                            (totals, info)
                        })
                        .unwrap();
                    assert!(
                        pixel_count
                            .iter()
                            .all(|count| *count >= (3.0 * scale) as usize),
                        "invisible icon pixels: {pixel_count:?} at {width}px/{scale}x"
                    );
                }
            }
        }
    }

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

/// Render a compact horizontal control stack independently of legacy menu rows.
/// Hits are stable semantic IDs, never glyph-specific geometry; the caller must
/// resolve them with `compact_action_at` rather than executing in the renderer.
pub fn layout_compact_controls<T: TextMeasurer>(
    surface_id: &str,
    controls: &[nuraloumi_core::model::CompactControl],
    interaction: &InteractionState,
    viewport: Viewport,
    theme: &Theme,
    text: &T,
) -> Scene {
    use nuraloumi_core::model::CompactControl as Control;
    let panel = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    let mut paint = vec![PaintNode::FillRect {
        rect: panel,
        color: theme.overlay,
    }];
    let mut hits = Vec::new();
    let rows_for = |count: usize| -> usize {
        let columns = (viewport.width / 48.0).floor().max(1.0) as usize;
        count.div_ceil(columns).max(1)
    };
    let total_height: f64 = controls
        .iter()
        .map(|control| match control {
            Control::Toolbar { buttons, .. } => rows_for(buttons.len()) as f64 * 48.0,
            Control::Tabs { tabs, .. } => rows_for(tabs.len()) as f64 * 48.0,
            _ => 48.0,
        })
        .sum();
    let max_offset = (total_height - viewport.height).max(0.0);
    let mut offset = if interaction.scroll_offset.is_finite() {
        interaction.scroll_offset.clamp(0.0, max_offset)
    } else {
        0.0
    };
    // Keyboard focus must remain visible even with a stale scroll position.
    if let Some(focused) = interaction.selected_id.as_deref() {
        let mut top = 0.0;
        for control in controls {
            let (rows, index) = match control {
                Control::Button { id, .. } => (1, (id == focused).then_some(0)),
                Control::Slider { id, .. } => (1, (id == focused).then_some(0)),
                Control::Toolbar { buttons, .. } => (
                    rows_for(buttons.len()),
                    buttons.iter().position(|b| b.id == focused),
                ),
                Control::Tabs { tabs, .. } => (
                    rows_for(tabs.len()),
                    tabs.iter().position(|t| t.id == focused),
                ),
                _ => (1, None),
            };
            if let Some(index) = index {
                let cols = (viewport.width / 48.0).floor().max(1.0) as usize;
                let focus_top = top + (index / cols) as f64 * 48.0;
                if focus_top < offset {
                    offset = focus_top;
                } else if focus_top + 48.0 > offset + viewport.height {
                    offset = focus_top + 48.0 - viewport.height;
                }
                offset = offset.clamp(0.0, max_offset);
                break;
            }
            top += rows as f64 * 48.0;
        }
    }
    paint.push(PaintNode::ClipPush(panel));
    let mut y = -offset;
    for control in controls {
        let row_height = 48.0_f64;
        let row = Rect::new(0.0, y, viewport.width, row_height);
        y += row_height;
        let selected = |id: &str| interaction.selected_id.as_deref() == Some(id);
        let draw_text = |paint: &mut Vec<PaintNode>, rect: Rect, label: &str, active: bool| {
            let style = TextStyle {
                size: 13.0,
                bold: active,
            };
            let label = text.ellipsize(label, (rect.width - 12.0).max(0.0), style);
            paint.push(PaintNode::Text {
                origin: Point::new(rect.x + 6.0, centered_baseline(rect, text, style)),
                text: label,
                style,
                color: if active {
                    theme.primary_text
                } else {
                    theme.secondary_text
                },
            });
        };
        let mut button = |id: &str, label: &str, enabled: bool, rect: Rect, active: bool| {
            paint.push(PaintNode::RoundedRect {
                rect: Rect::new(
                    rect.x + 2.0,
                    rect.y + 4.0,
                    (rect.width - 4.0).max(0.0),
                    rect.height - 8.0,
                ),
                radius: 4.0,
                fill: Fill::Solid(if active { theme.selected } else { theme.card }),
                stroke: selected(id).then_some((theme.accent, 2.0)),
            });
            draw_text(&mut paint, rect, label, active);
            if let Some(visible) = rect.intersection(panel) {
                hits.push(HitRegion {
                    item_id: id.to_string(),
                    rect: visible,
                    actionable: true,
                    enabled,
                    kind: RowKind::Action,
                });
            }
        };
        match control {
            Control::Button {
                id,
                label,
                icon,
                enabled,
                ..
            } => {
                let fallback = if icon.is_ascii() && !icon.is_empty() {
                    icon.as_str()
                } else {
                    label.as_str()
                };
                button(id, fallback, *enabled, row, selected(id));
            }
            Control::Toolbar { buttons, .. } => {
                // A narrow viewport stacks instead of shrinking below 48 logical px.
                let columns = (viewport.width / 48.0).floor().max(1.0) as usize;
                let columns = columns.min(buttons.len()).max(1);
                let cell_width = viewport.width / columns as f64;
                for (index, item) in buttons.iter().enumerate() {
                    let line = index / columns;
                    let col = index % columns;
                    let rect = Rect::new(
                        col as f64 * cell_width,
                        row.y + line as f64 * 48.0,
                        cell_width,
                        48.0,
                    );
                    let fallback = if item.icon.is_ascii() && !item.icon.is_empty() {
                        &item.icon
                    } else {
                        &item.label
                    };
                    button(&item.id, fallback, item.enabled, rect, selected(&item.id));
                }
                y += (buttons.len().div_ceil(columns) - 1) as f64 * 48.0;
            }
            Control::Tabs { tabs, .. } => {
                let columns = (viewport.width / 48.0).floor().max(1.0) as usize;
                let columns = columns.min(tabs.len()).max(1);
                let cell_width = viewport.width / columns as f64;
                for (index, tab) in tabs.iter().enumerate() {
                    let line = index / columns;
                    let col = index % columns;
                    let rect = Rect::new(
                        col as f64 * cell_width,
                        row.y + line as f64 * 48.0,
                        cell_width,
                        48.0,
                    );
                    button(&tab.id, &tab.label, !tab.selected, rect, tab.selected);
                }
                y += (tabs.len().div_ceil(columns) - 1) as f64 * 48.0;
            }
            Control::Slider {
                id,
                label,
                min,
                max,
                step,
                value,
                enabled,
                ..
            } => {
                draw_text(
                    &mut paint,
                    Rect::new(0.0, row.y, (viewport.width * 0.32).min(140.0), 48.0),
                    label,
                    *enabled,
                );
                let track = compact_slider_rect(row);
                let ratio = if max > min {
                    (i64::from(*value.clamp(min, max)) - i64::from(*min)) as f64
                        / (i64::from(*max) - i64::from(*min)) as f64
                } else {
                    0.0
                };
                paint.push(PaintNode::RoundedRect {
                    rect: Rect::new(track.x, track.y + 18.0, track.width, 12.0),
                    radius: 6.0,
                    fill: Fill::Solid(theme.card),
                    stroke: None,
                });
                paint.push(PaintNode::RoundedRect {
                    rect: Rect::new(track.x, track.y + 18.0, track.width * ratio, 12.0),
                    radius: 6.0,
                    fill: Fill::Solid(theme.accent),
                    stroke: None,
                });
                let thumb_x = track.x + track.width * ratio;
                paint.push(PaintNode::RoundedRect {
                    rect: Rect::new(thumb_x - 5.0, track.y + 13.0, 10.0, 22.0),
                    radius: 4.0,
                    fill: Fill::Solid(theme.primary_text),
                    stroke: selected(id).then_some((theme.accent, 2.0)),
                });
                let _ = step; // Quantization is performed only by semantic actions.
                if let Some(visible) = track.intersection(panel) {
                    hits.push(HitRegion {
                        item_id: id.clone(),
                        rect: visible,
                        actionable: true,
                        enabled: *enabled,
                        kind: RowKind::Action,
                    });
                }
            }
            Control::Breadcrumb { parts, .. } | Control::Status { fields: parts, .. } => {
                draw_text(&mut paint, row, &parts.join(" / "), false);
            }
        }
    }
    paint.push(PaintNode::ClipPop);
    let body = panel;
    Scene {
        viewport,
        menu_id: surface_id.to_string(),
        panel_rect: panel,
        paint,
        hits,
        scroll: ScrollWindow {
            viewport: body,
            content_height: total_height,
            offset,
            max_offset,
            clipped: total_height > viewport.height,
        },
    }
}

/// Slider track is the entire accessible right-hand region (48px high);
/// actual visible chrome remains a 12px strip.
fn compact_slider_rect(row: Rect) -> Rect {
    let start = (row.width * 0.32).min(140.0);
    Rect::new(row.x + start, row.y, (row.width - start).max(1.0), 48.0)
}

/// Map exact pointer/touch positions to typed actions. No command execution.
pub fn compact_action_at(
    controls: &[nuraloumi_core::model::CompactControl],
    scene: &Scene,
    x: f64,
    y: f64,
) -> Option<nuraloumi_core::MenuAction> {
    use nuraloumi_core::model::CompactControl as Control;
    let id = scene.hit_test(x, y)?;
    for control in controls {
        match control {
            Control::Button {
                id: control_id,
                enabled: true,
                action,
                ..
            } if control_id == id => return Some(action.clone()),
            Control::Toolbar { buttons, .. } => {
                if let Some(b) = buttons.iter().find(|b| b.id == id && b.enabled) {
                    return Some(b.action.clone());
                }
            }
            Control::Tabs { tabs, .. } => {
                if let Some(t) = tabs.iter().find(|t| t.id == id && !t.selected) {
                    return Some(t.action.clone());
                }
            }
            Control::Slider {
                id: slider_id,
                min,
                max,
                ..
            } if slider_id == id => {
                let rect = scene.hits.iter().find(|h| h.item_id == id)?.rect;
                let ratio = ((x - rect.x) / rect.width).clamp(0.0, 1.0);
                let raw =
                    (f64::from(*min) + (i64::from(*max) - i64::from(*min)) as f64 * ratio).round();
                return control
                    .slider_action(raw.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32);
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod compact_geometry_tests {
    use super::*;
    use crate::render::DeterministicText;
    use nuraloumi_core::model::CompactControl;
    #[test]
    fn slider_touch_extremes_and_dpi_independent_targets() {
        let controls = vec![CompactControl::Slider {
            id: "volume".into(),
            label: "Volume".into(),
            value_id: "volume.set".into(),
            min: 0,
            max: 94,
            step: 10,
            value: 40,
            enabled: true,
        }];
        for scale in [1.0, 1.5, 2.0] {
            let scene = layout_compact_controls(
                "test",
                &controls,
                &InteractionState::default(),
                Viewport::new(400.0, 100.0, scale),
                &Theme::default(),
                &DeterministicText,
            );
            let hit = &scene.hits[0];
            assert_eq!(hit.rect.height, 48.0);
            assert_eq!(
                compact_action_at(&controls, &scene, hit.rect.x, 24.0),
                Some(nuraloumi_core::MenuAction::SetValue {
                    id: "volume.set".into(),
                    value: 0
                })
            );
            assert_eq!(
                compact_action_at(&controls, &scene, hit.rect.right() - 0.001, 24.0),
                Some(nuraloumi_core::MenuAction::SetValue {
                    id: "volume.set".into(),
                    value: 94
                })
            );
        }
    }
    #[test]
    fn toolbar_wrap_preserves_minimum_hit_size_and_disabled_state() {
        use nuraloumi_core::model::CompactButton;
        let buttons = (0..4)
            .map(|i| CompactButton {
                id: format!("b{i}"),
                label: format!("B{i}"),
                icon: "+".into(),
                enabled: i != 0,
                action: nuraloumi_core::MenuAction::Activate {
                    id: format!("a{i}"),
                },
            })
            .collect();
        let controls = vec![CompactControl::Toolbar {
            id: "toolbar".into(),
            buttons,
        }];
        let scene = layout_compact_controls(
            "test",
            &controls,
            &InteractionState::default(),
            Viewport::new(100.0, 400.0, 1.0),
            &Theme::default(),
            &DeterministicText,
        );
        assert_eq!(scene.hits.len(), 4);
        assert!(scene
            .hits
            .iter()
            .all(|h| h.rect.width >= 48.0 && h.rect.height >= 48.0));
        assert_eq!(scene.hit_test(25.0, 24.0), None);
        assert_eq!(scene.hit_test(75.0, 24.0), Some("b1"));
        assert_eq!(scene.hit_test(25.0, 72.0), Some("b2"));
    }
}

#[cfg(test)]
mod compact_fixture_tests {
    use super::*;
    use crate::render::{CairoRenderer, DeterministicText};
    use nuraloumi_core::model::{CompactButton, CompactControl, CompactTab};

    /// Explicitly regenerate with: cargo test -p nuraloumi-render-cairo
    /// compact_fixture_png -- --ignored
    #[test]
    #[ignore = "regenerates a reviewable in-repository PNG fixture"]
    fn compact_fixture_png() {
        let controls = vec![
            CompactControl::Toolbar {
                id: "volume.actions".into(),
                buttons: vec![
                    CompactButton {
                        id: "mute".into(),
                        label: "Mute".into(),
                        icon: "M".into(),
                        enabled: true,
                        action: nuraloumi_core::MenuAction::Toggle {
                            id: "audio.mute".into(),
                        },
                    },
                    CompactButton {
                        id: "down".into(),
                        label: "Decrease".into(),
                        icon: "-".into(),
                        enabled: true,
                        action: nuraloumi_core::MenuAction::Adjust {
                            id: "audio.volume".into(),
                            delta: -5,
                        },
                    },
                    CompactButton {
                        id: "up".into(),
                        label: "Increase".into(),
                        icon: "+".into(),
                        enabled: true,
                        action: nuraloumi_core::MenuAction::Adjust {
                            id: "audio.volume".into(),
                            delta: 5,
                        },
                    },
                ],
            },
            CompactControl::Slider {
                id: "volume".into(),
                label: "Volume".into(),
                value_id: "audio.volume".into(),
                min: 0,
                max: 100,
                step: 1,
                value: 58,
                enabled: true,
            },
            CompactControl::Tabs {
                id: "settings.tabs".into(),
                tabs: vec![
                    CompactTab {
                        id: "sound".into(),
                        label: "Sound".into(),
                        selected: true,
                        action: nuraloumi_core::MenuAction::Navigate {
                            target: "sound".into(),
                        },
                    },
                    CompactTab {
                        id: "screen".into(),
                        label: "Screen".into(),
                        selected: false,
                        action: nuraloumi_core::MenuAction::Navigate {
                            target: "screen".into(),
                        },
                    },
                ],
            },
            CompactControl::Status {
                id: "status".into(),
                fields: vec!["58%".into(), "Ready".into()],
            },
        ];
        let scene = layout_compact_controls(
            "qol-fixture",
            &controls,
            &InteractionState::default(),
            Viewport::new(440.0, 192.0, 1.0),
            &Theme::default(),
            &DeterministicText,
        );
        let buffer = CairoRenderer::default().render_scene(&scene).unwrap();
        let path = "docs/fixtures/qol-widget-controls.png";
        std::fs::create_dir_all("docs/fixtures").unwrap();
        let mut file = std::fs::File::create(path).unwrap();
        buffer.surface().write_to_png(&mut file).unwrap();
    }
}
