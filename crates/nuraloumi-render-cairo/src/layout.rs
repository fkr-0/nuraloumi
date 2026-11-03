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
    let width = panel_width(viewport, metrics);
    let x = ((viewport.width - width) / 2.0).max(0.0);
    let max_panel_height =
        (viewport.height - metrics.outer_margin * 2.0).max(metrics.header_height);

    let content_height: f64 = (0..menu.len())
        .map(|index| {
            let item = menu.item(index);
            metrics.row_height(item.kind, item.subtitle.is_some(), scale)
        })
        .sum();
    let natural_height = metrics.header_height + content_height;
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
        panel.y + metrics.header_height,
        panel.width,
        (panel.height - metrics.header_height).max(0.0),
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
        let mut top = 0.0;
        for index in 0..menu.len() {
            let item = menu.item(index);
            let height = metrics.row_height(item.kind, item.subtitle.is_some(), scale);
            if item.id == selected_id && item.kind.actionable() {
                if top < offset || height > body.height {
                    offset = top;
                } else if top + height > offset + body.height {
                    offset = top + height - body.height;
                }
                offset = offset.clamp(0.0, max_offset);
                break;
            }
            top += height;
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

    paint.push(PaintNode::ClipPush(body));
    let mut hits = Vec::with_capacity(menu.len());
    let mut content_y = body.y - offset;

    for index in 0..menu.len() {
        let item = menu.item(index);
        let row_height = metrics.row_height(item.kind, item.subtitle.is_some(), scale);
        let row = Rect::new(panel.x, content_y, panel.width, row_height);
        content_y += row_height;

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
