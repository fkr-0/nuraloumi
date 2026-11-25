//! Pure, touch-sized status-sheet geometry. No native commands or Wayland calls.
use crate::FixtureSnapshot;
use nuraloumi_core::MenuModel;
use nuraloumi_render_cairo::{
    Color, Fill, HitRegion, PaintNode, Point, Rect, RowKind, Scene, ScrollWindow, TextStyle, Theme,
    Viewport,
};

#[derive(Clone, Copy)]
enum TileIcon {
    Clock,
    Speaker,
    Wifi,
    Bluetooth,
}

const TILES: [(&str, &str, TileIcon); 4] = [
    ("power.toggle.always-on", "Always on", TileIcon::Clock),
    ("power.toggle.mute", "Mute", TileIcon::Speaker),
    ("power.toggle.wifi", "Wi-Fi", TileIcon::Wifi),
    ("power.toggle.bluetooth", "Bluetooth", TileIcon::Bluetooth),
];

fn text(nodes: &mut Vec<PaintNode>, x: f64, y: f64, label: &str, size: f64, color: Color) {
    nodes.push(PaintNode::Text {
        origin: Point::new(x, y),
        text: label.to_owned(),
        style: TextStyle { size, bold: false },
        color,
    });
}

fn line(nodes: &mut Vec<PaintNode>, x1: f64, y1: f64, x2: f64, y2: f64, color: Color) {
    nodes.push(PaintNode::Line {
        from: Point::new(x1, y1),
        to: Point::new(x2, y2),
        width: 1.9,
        color,
    });
}

fn tile_icon(nodes: &mut Vec<PaintNode>, icon: TileIcon, x: f64, y: f64, color: Color) {
    match icon {
        TileIcon::Clock => {
            nodes.push(PaintNode::RoundedRect {
                rect: Rect::new(x - 10.0, y - 10.0, 20.0, 20.0),
                radius: 10.0,
                fill: Fill::Solid(Color::rgba(0, 0, 0, 0)),
                stroke: Some((color, 1.8)),
            });
            line(nodes, x, y - 7.0, x, y, color);
            line(nodes, x, y, x + 5.0, y + 2.5, color);
        }
        TileIcon::Speaker => {
            for (a, b, c, d) in [
                (-12.0, -4.0, -7.0, -4.0),
                (-12.0, -4.0, -12.0, 4.0),
                (-12.0, 4.0, -7.0, 4.0),
                (-7.0, -4.0, -1.0, -9.0),
                (-7.0, 4.0, -1.0, 9.0),
                (-1.0, -9.0, -1.0, 9.0),
                (4.0, -5.0, 8.0, 0.0),
                (8.0, 0.0, 4.0, 5.0),
            ] {
                line(nodes, x + a, y + b, x + c, y + d, color);
            }
        }
        TileIcon::Wifi => {
            for (a, b, c, d) in [
                (-13.0, -4.0, 0.0, -10.0),
                (0.0, -10.0, 13.0, -4.0),
                (-9.0, 1.0, 0.0, -3.0),
                (0.0, -3.0, 9.0, 1.0),
                (-4.0, 6.0, 0.0, 4.0),
                (0.0, 4.0, 4.0, 6.0),
            ] {
                line(nodes, x + a, y + b, x + c, y + d, color);
            }
            nodes.push(PaintNode::FillRect {
                rect: Rect::new(x - 1.5, y + 9.0, 3.0, 3.0),
                color,
            });
        }
        TileIcon::Bluetooth => {
            for (a, b, c, d) in [
                (0.0, -12.0, 0.0, 12.0),
                (0.0, -12.0, 8.0, -5.0),
                (8.0, -5.0, -7.0, 7.0),
                (-7.0, -7.0, 8.0, 5.0),
                (8.0, 5.0, 0.0, 12.0),
            ] {
                line(nodes, x + a, y + b, x + c, y + d, color);
            }
        }
    }
}

fn model_hit(
    menu: &MenuModel,
    selected: Option<&str>,
    id: &str,
    rect: Rect,
    nodes: &mut Vec<PaintNode>,
    hits: &mut Vec<HitRegion>,
    theme: Theme,
) -> (bool, Option<bool>) {
    let item = menu.items.iter().find(|item| item.id == id);
    let enabled = item.is_some_and(|item| item.enabled && item.action.is_some());
    if selected == Some(id) && enabled {
        nodes.push(PaintNode::FillRect {
            rect,
            color: theme.selected,
        });
    }
    hits.push(HitRegion {
        item_id: id.to_owned(),
        rect,
        actionable: enabled,
        enabled,
        kind: item.map_or(RowKind::Status, RowKind::from),
    });
    (enabled, item.and_then(|item| item.checked))
}

fn percent(value: Option<&str>) -> Option<u8> {
    let digits: String = value?.chars().take_while(char::is_ascii_digit).collect();
    digits.parse::<u8>().ok().filter(|value| *value <= 100)
}

/// Only used for reasonably tall status popovers. Short outputs use the
/// ordinary scrollable menu model instead of clipping power controls.
pub fn scene(
    menu: &MenuModel,
    snapshot: &FixtureSnapshot,
    selected: Option<&str>,
    confirming: Option<&str>,
    viewport: Viewport,
    theme: Theme,
) -> Scene {
    let full = viewport.logical_rect();
    let mut paint = vec![PaintNode::FillRect {
        rect: full,
        color: theme.overlay,
    }];
    let mut hits = Vec::new();
    text(
        &mut paint,
        16.0,
        30.0,
        "Quick status",
        19.0,
        theme.primary_text,
    );
    if let Some(battery) = snapshot.battery.value.as_deref() {
        text(
            &mut paint,
            viewport.width - 98.0,
            28.0,
            battery,
            11.0,
            theme.secondary_text,
        );
    }
    paint.push(PaintNode::Line {
        from: Point::new(16.0, 41.0),
        to: Point::new(viewport.width - 16.0, 41.0),
        width: 1.0,
        color: theme.separator,
    });

    let gap = 8.0;
    let content_width = (viewport.width - 32.0).max(180.0);
    let tile_width = (content_width - gap) / 2.0;
    // Adding more tiles extends the grid without changing the renderer API.
    for (index, (id, label, icon)) in TILES.iter().enumerate() {
        let col = index % 2;
        let row = index / 2;
        let rect = Rect::new(
            16.0 + col as f64 * (tile_width + gap),
            52.0 + row as f64 * 78.0,
            tile_width,
            70.0,
        );
        paint.push(PaintNode::RoundedRect {
            rect,
            radius: 0.0,
            fill: Fill::Solid(theme.card),
            stroke: Some((theme.border, 1.0)),
        });
        let (enabled, active) = model_hit(menu, selected, id, rect, &mut paint, &mut hits, theme);
        let color = if enabled {
            theme.primary_text
        } else {
            theme.hint
        };
        tile_icon(&mut paint, *icon, rect.x + 24.0, rect.y + 26.0, color);
        text(&mut paint, rect.x + 49.0, rect.y + 31.0, label, 13.0, color);
        if *id == "power.toggle.always-on" {
            text(
                &mut paint,
                rect.x + 49.0,
                rect.y + 49.0,
                if active == Some(true) {
                    "Forever"
                } else {
                    "10 min"
                },
                11.0,
                theme.secondary_text,
            );
        }
        paint.push(PaintNode::FillRect {
            rect: Rect::new(rect.x + 2.0, rect.bottom() - 4.0, rect.width - 4.0, 3.0),
            color: if active == Some(true) && enabled {
                theme.accent
            } else {
                theme.separator
            },
        });
    }

    for (slider_index, (prefix, title, value, available)) in [
        (
            "power.brightness",
            "Brightness",
            percent(snapshot.brightness.value.as_deref()),
            snapshot.brightness.available() && snapshot.brightness_writable,
        ),
        (
            "power.volume",
            "Volume",
            percent(snapshot.audio.value.as_deref()),
            snapshot.audio.available(),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let y = 220.0 + slider_index as f64 * 70.0;
        text(&mut paint, 16.0, y + 15.0, title, 13.0, theme.primary_text);
        text(
            &mut paint,
            viewport.width - 57.0,
            y + 15.0,
            &value.map_or("--".to_owned(), |v| format!("{v}%")),
            12.0,
            theme.secondary_text,
        );
        let track = Rect::new(16.0, y + 23.0, content_width, 30.0);
        paint.push(PaintNode::FillRect {
            rect: track,
            color: theme.card,
        });
        if let Some(percent) = value {
            paint.push(PaintNode::FillRect {
                rect: Rect::new(
                    track.x,
                    track.y + 3.0,
                    track.width * f64::from(percent) / 100.0,
                    track.height - 6.0,
                ),
                color: if available {
                    theme.accent
                } else {
                    theme.separator
                },
            });
        }
        let step_width = track.width / 6.0;
        for step in 0..6 {
            let step_value = if prefix == "power.brightness" {
                [10, 25, 40, 60, 80, 100][step]
            } else {
                [0, 20, 40, 60, 80, 100][step]
            };
            let rect = Rect::new(
                track.x + step_width * step as f64,
                track.y,
                step_width,
                track.height,
            );
            let id = format!("{prefix}.{step_value}");
            model_hit(menu, selected, &id, rect, &mut paint, &mut hits, theme);
            if step > 0 {
                line(
                    &mut paint,
                    rect.x,
                    rect.y + 4.0,
                    rect.x,
                    rect.bottom() - 4.0,
                    theme.border,
                );
            }
        }
    }

    text(&mut paint, 16.0, 367.0, "Power", 13.0, theme.secondary_text);
    for (index, (id, label)) in [
        ("power.suspend", "Sleep"),
        ("power.restart", "Restart"),
        ("power.poweroff", "Shut down"),
        ("power.hibernate", "Hibernate"),
    ]
    .into_iter()
    .enumerate()
    {
        let rect = Rect::new(16.0, 380.0 + index as f64 * 47.0, content_width, 43.0);
        paint.push(PaintNode::FillRect {
            rect,
            color: theme.card,
        });
        let (enabled, _) = model_hit(menu, selected, id, rect, &mut paint, &mut hits, theme);
        let needs_confirmation = confirming == Some(id);
        text(
            &mut paint,
            rect.x + 14.0,
            rect.y + 27.0,
            if needs_confirmation {
                "Tap again to confirm"
            } else {
                label
            },
            14.0,
            if needs_confirmation {
                theme.warning
            } else if enabled {
                theme.primary_text
            } else {
                theme.hint
            },
        );
        if !enabled {
            text(
                &mut paint,
                rect.right() - 113.0,
                rect.y + 27.0,
                "Unavailable",
                10.0,
                theme.hint,
            );
        }
    }
    Scene {
        viewport,
        menu_id: menu.id.clone(),
        panel_rect: full,
        paint,
        hits,
        scroll: ScrollWindow {
            viewport: full,
            content_height: full.height,
            offset: 0.0,
            max_offset: 0.0,
            clipped: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_power_menu;

    #[test]
    fn sheet_has_four_lit_tiles_two_six_step_sliders_and_safe_power_rows() {
        let snapshot = FixtureSnapshot::default();
        let model = build_power_menu(&snapshot);
        let scene = scene(
            &model,
            &snapshot,
            None,
            None,
            Viewport::new(420.0, 600.0, 1.0),
            Theme::dark(),
        );
        assert_eq!(scene.menu_id, "power");
        assert_eq!(scene.hits.len(), 4 + 12 + 4);
        assert!(
            scene
                .hits
                .iter()
                .find(|hit| hit.item_id == "power.toggle.always-on")
                .unwrap()
                .rect
                .y
                < 100.0
        );
        let brightness = scene
            .hits
            .iter()
            .find(|hit| hit.item_id == "power.brightness.60")
            .unwrap();
        let volume = scene
            .hits
            .iter()
            .find(|hit| hit.item_id == "power.volume.60")
            .unwrap();
        assert!(brightness.rect.y < volume.rect.y);
        assert!(
            volume.rect.y
                < scene
                    .hits
                    .iter()
                    .find(|hit| hit.item_id == "power.suspend")
                    .unwrap()
                    .rect
                    .y
        );
        assert!(
            !scene
                .hits
                .iter()
                .find(|hit| hit.item_id == "power.hibernate")
                .unwrap()
                .actionable
        );
        assert!(scene
            .hits
            .iter()
            .filter(|hit| hit.actionable)
            .all(|hit| hit.rect.width >= 48.0));
        assert!(scene.paint.iter().any(|node| matches!(node, PaintNode::FillRect { color, rect } if *color == Theme::dark().accent && rect.height == 3.0)));
    }

    #[test]
    fn unavailable_controls_are_not_hittable() {
        let snapshot = FixtureSnapshot {
            wifi_radio_enabled: None,
            brightness_writable: false,
            ..FixtureSnapshot::default()
        };
        let menu = build_power_menu(&snapshot);
        let scene = scene(
            &menu,
            &snapshot,
            None,
            None,
            Viewport::new(420.0, 600.0, 1.0),
            Theme::dark(),
        );
        assert!(
            !scene
                .hits
                .iter()
                .find(|h| h.item_id == "power.toggle.wifi")
                .unwrap()
                .enabled
        );
        assert!(
            !scene
                .hits
                .iter()
                .find(|h| h.item_id == "power.brightness.80")
                .unwrap()
                .enabled
        );
        assert!(scene.hit_test(25.0, 254.0).is_none());
    }

    #[test]
    fn sheet_rasterizes_in_software_at_sl101_scale() {
        let snapshot = FixtureSnapshot::default();
        let model = build_power_menu(&snapshot);
        let scene = scene(
            &model,
            &snapshot,
            None,
            None,
            Viewport::new(420.0, 574.0, 1.0),
            Theme::dark(),
        );
        let buffer = nuraloumi_render_cairo::CairoRenderer::default()
            .render_scene(&scene)
            .expect("quick status Cairo render");
        assert_eq!(buffer.info().width, 420);
        assert_eq!(buffer.info().height, 574);
    }

    #[test]
    fn destructive_action_shows_second_tap_confirmation() {
        let snapshot = FixtureSnapshot::default();
        let menu = build_power_menu(&snapshot);
        let scene = scene(
            &menu,
            &snapshot,
            Some("power.poweroff"),
            Some("power.poweroff"),
            Viewport::new(420.0, 600.0, 1.0),
            Theme::dark(),
        );
        assert!(scene.paint.iter().any(|node| matches!(node,
            PaintNode::Text { text, color, .. }
                if text == "Tap again to confirm" && *color == Theme::dark().warning
        )));
    }
}
