use nuraloumi_render_cairo::{
    layout_menu, InteractionState, MenuItemView, MenuView, Rect, RenderOptions, RowKind,
    TextMeasurer, TextMetrics, TextStyle, Theme, ToyText, Viewport,
};

fn item(id: &str, kind: RowKind, enabled: bool) -> MenuItemView {
    MenuItemView {
        id: id.into(),
        label: id.into(),
        subtitle: None,
        shortcut: None,
        glyph: None,
        kind,
        enabled,
    }
}

fn mixed_menu() -> MenuView {
    MenuView {
        id: "mixed".into(),
        title: "Mixed".into(),
        items: vec![
            item("section", RowKind::Section, false),
            item("action", RowKind::Action, true),
            item("submenu", RowKind::Submenu, true),
            item("check", RowKind::Checkable { checked: true }, true),
            item("status", RowKind::Status, true),
            item("separator", RowKind::Separator, false),
            item("disabled", RowKind::Action, false),
        ],
    }
}

#[test]
fn actionable_touch_targets_are_at_least_48_logical_pixels() {
    let scene = layout_menu(
        &mixed_menu(),
        &InteractionState::default(),
        Viewport::new(800.0, 600.0, 1.0),
        &Theme::dark(),
        &ToyText,
        false,
    );
    let actionable: Vec<_> = scene.hits.iter().filter(|hit| hit.actionable).collect();
    assert!(!actionable.is_empty());
    for hit in actionable {
        assert!(
            hit.rect.height >= 48.0,
            "{} is only {} px",
            hit.item_id,
            hit.rect.height
        );
    }
}

#[test]
fn non_action_and_disabled_rows_never_activate() {
    let scene = layout_menu(
        &mixed_menu(),
        &InteractionState::default(),
        Viewport::new(800.0, 600.0, 1.0),
        &Theme::dark(),
        &ToyText,
        false,
    );
    for id in ["section", "status", "separator", "disabled"] {
        let hit = scene.hits.iter().find(|hit| hit.item_id == id).unwrap();
        let x = hit.rect.x + hit.rect.width / 2.0;
        let y = hit.rect.y + hit.rect.height / 2.0;
        assert_eq!(scene.hit_test(x, y), None, "{id} unexpectedly activated");
    }
    for id in ["action", "submenu", "check"] {
        let hit = scene.hits.iter().find(|hit| hit.item_id == id).unwrap();
        let x = hit.rect.x + hit.rect.width / 2.0;
        let y = hit.rect.y + hit.rect.height / 2.0;
        assert_eq!(scene.hit_test(x, y), Some(id));
    }
}

#[test]
fn transitioned_raster_preserves_settled_semantic_geometry() {
    let renderer = nuraloumi_render_cairo::CairoRenderer::default();
    let viewport = Viewport::new(800.0, 600.0, 1.0);
    let scene = renderer.build_scene(
        &mixed_menu(),
        &InteractionState::default(),
        viewport,
        &Theme::dark(),
        RenderOptions::default(),
    );
    let buffer = renderer
        .render_scene_transition(
            &scene,
            nuraloumi_core::Transition {
                opacity: 0.5,
                translate_y: 8.0,
                scale: 1.0,
            },
        )
        .expect("transitioned raster");

    assert_eq!(buffer.info().width, 800);
    assert_eq!(buffer.info().height, 600);
    let hit = scene
        .hits
        .iter()
        .find(|hit| hit.item_id == "action")
        .expect("action hit");
    assert_eq!(
        scene.hit_test(
            hit.rect.x + hit.rect.width / 2.0,
            hit.rect.y + hit.rect.height / 2.0
        ),
        Some("action")
    );
}

#[test]
fn argb_preview_overlay_changes_pixels_without_scene_mutation() {
    let renderer = nuraloumi_render_cairo::CairoRenderer::default();
    let scene = renderer.build_scene(
        &mixed_menu(),
        &InteractionState::default(),
        Viewport::new(320.0, 240.0, 1.0),
        &Theme::dark(),
        RenderOptions::default(),
    );
    let original_hits = scene.hits.clone();
    let mut buffer = renderer.render_scene(&scene).expect("base raster");
    let marker = [0x12, 0x34, 0x56, 0x78];
    buffer
        .paint_argb32_preview(
            Rect {
                x: 8.0,
                y: 8.0,
                width: 12.0,
                height: 12.0,
            },
            1.0,
            1,
            1,
            &marker,
        )
        .expect("preview overlay");
    let pixels = buffer.copy_argb32_bytes().expect("read raster");
    assert!(pixels.chunks_exact(4).any(|pixel| pixel == marker));
    assert_eq!(scene.hits, original_hits);
}

#[test]
fn viewport_width_is_clamped_below_target_range() {
    let scene = layout_menu(
        &mixed_menu(),
        &InteractionState::default(),
        Viewport::new(360.0, 640.0, 1.0),
        &Theme::dark(),
        &ToyText,
        true,
    );
    assert_eq!(scene.panel_rect.x, 8.0);
    assert_eq!(scene.panel_rect.width, 344.0);
    assert!(scene.panel_rect.right() <= 360.0);
}

#[test]
fn normal_desktop_viewport_uses_compact_448_logical_menu_width() {
    let scene = layout_menu(
        &mixed_menu(),
        &InteractionState::default(),
        Viewport::new(800.0, 600.0, 1.0),
        &Theme::dark(),
        &ToyText,
        false,
    );
    assert_eq!(scene.panel_rect.width, 448.0);
}

#[test]
fn scale_changes_device_pixels_not_logical_hit_geometry() {
    let renderer = nuraloumi_render_cairo::CairoRenderer::default();
    let menu = mixed_menu();
    let one = renderer.build_scene(
        &menu,
        &InteractionState::default(),
        Viewport::new(800.0, 600.0, 1.0),
        &Theme::dark(),
        RenderOptions::default(),
    );
    let two = renderer.build_scene(
        &menu,
        &InteractionState::default(),
        Viewport::new(800.0, 600.0, 2.0),
        &Theme::dark(),
        RenderOptions::default(),
    );
    assert_eq!(one.panel_rect, two.panel_rect);
    assert_eq!(one.hits, two.hits);
    assert_eq!(one.viewport.device_size(), (800, 600));
    assert_eq!(two.viewport.device_size(), (1600, 1200));
}

#[test]
fn overflow_reports_bounded_scroll_window() {
    let mut menu = mixed_menu();
    for index in 0..30 {
        menu.items
            .push(item(&format!("row-{index}"), RowKind::Action, true));
    }
    let scene = layout_menu(
        &menu,
        &InteractionState {
            scroll_offset: 10_000.0,
            ..Default::default()
        },
        Viewport::new(480.0, 320.0, 1.0),
        &Theme::dark(),
        &ToyText,
        false,
    );
    assert!(scene.scroll.clipped);
    assert!(scene.scroll.max_offset > 0.0);
    assert_eq!(scene.scroll.offset, scene.scroll.max_offset);
    assert!(scene.hits.iter().all(|hit| {
        hit.rect.y >= scene.scroll.viewport.y && hit.rect.bottom() <= scene.scroll.viewport.bottom()
    }));
}

#[test]
fn toy_text_metrics_are_deterministic() {
    let measurer = ToyText;
    let style = TextStyle {
        size: 16.0,
        bold: false,
    };
    let a: TextMetrics = measurer.measure("Terminal", style);
    let b: TextMetrics = measurer.measure("Terminal", style);
    assert_eq!(a, b);
    assert!(a.width > 0.0);
    assert!(a.ascent > 0.0);
}

#[test]
fn semantic_core_adapter_respects_path_query_check_state_and_theme() {
    use nuraloumi_render_cairo::semantic_core::{
        MenuAction, MenuItem, MenuModel, MenuState, DARK_THEME,
    };

    let checked = MenuItem::checkable("wifi", "Wi-Fi", "wifi:toggle", true);
    let submenu = MenuItem::submenu(
        "system",
        "System",
        vec![
            checked,
            MenuItem::action(
                "volume",
                "Volume",
                MenuAction::Adjust {
                    id: "volume".into(),
                    delta: 1,
                },
            ),
        ],
    );
    let model = MenuModel::new("demo", "Demo", vec![submenu]);
    let state = MenuState {
        selected_id: Some("wifi".into()),
        path: vec!["system".into()],
        query: "wi".into(),
    };
    let renderer = nuraloumi_render_cairo::CairoRenderer::default();
    let scene = renderer.build_core_scene(
        &model,
        &state,
        Viewport::new(800.0, 600.0, 1.0),
        &DARK_THEME,
        RenderOptions::default(),
    );

    assert_eq!(scene.hits.len(), 1);
    assert_eq!(scene.hits[0].item_id, "wifi");
    assert_eq!(scene.hits[0].kind, RowKind::Checkable { checked: true });
}

#[test]
fn rendered_buffer_is_cpu_readable_argb32_with_scaled_dimensions() {
    let renderer = nuraloumi_render_cairo::CairoRenderer::default();
    let menu = MenuView {
        id: "buffer".into(),
        title: "Buffer".into(),
        items: vec![item("action", RowKind::Action, true)],
    };
    let (_scene, mut buffer) = renderer
        .render(
            &menu,
            &InteractionState::default(),
            Viewport::new(320.0, 200.0, 2.0),
            &Theme::dark(),
            RenderOptions::default(),
        )
        .unwrap();

    let info = buffer.info();
    assert_eq!((info.width, info.height), (640, 400));
    assert!(info.stride >= info.width * 4);
    let bytes = buffer.copy_argb32_bytes().unwrap();
    assert_eq!(bytes.len(), info.stride as usize * info.height as usize);
    assert!(bytes.iter().any(|byte| *byte != 0));
}

#[test]
fn long_action_text_is_ellipsized_before_the_shortcut_region() {
    use nuraloumi_render_cairo::PaintNode;

    let menu = MenuView {
        id: "long-text".into(),
        title: "A very long launcher title that should not escape the panel".into(),
        items: vec![MenuItemView {
            id: "long-action".into(),
            label: "A very long action label that would otherwise collide with its shortcut".into(),
            subtitle: Some("A secondary description that also needs deterministic clipping".into()),
            shortcut: Some("Ctrl+Super+Enter".into()),
            glyph: None,
            kind: RowKind::Action,
            enabled: true,
        }],
    };
    let scene = layout_menu(
        &menu,
        &InteractionState::default(),
        Viewport::new(360.0, 240.0, 1.0),
        &Theme::dark(),
        &ToyText,
        false,
    );

    let strings: Vec<_> = scene
        .paint
        .iter()
        .filter_map(|node| match node {
            PaintNode::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(strings.iter().any(|text| text.ends_with('…')));
    assert!(!strings.iter().any(|text| {
        *text == "A very long action label that would otherwise collide with its shortcut"
    }));
}

#[test]
fn core_status_rows_use_status_text_colors_even_though_they_are_non_actionable() {
    use nuraloumi_render_cairo::{
        semantic_core::{MenuItem, MenuModel, MenuState, DARK_THEME},
        Color, PaintNode,
    };

    let model = MenuModel::new(
        "status-menu",
        "Status",
        vec![MenuItem::status("battery", "Battery").with_subtitle("83%")],
    );
    let scene = nuraloumi_render_cairo::CairoRenderer::default().build_core_scene(
        &model,
        &MenuState::new(&model),
        Viewport::new(480.0, 240.0, 1.0),
        &DARK_THEME,
        RenderOptions::default(),
    );
    let label_color = scene.paint.iter().find_map(|node| match node {
        PaintNode::Text { text, color, .. } if text == "Battery" => Some(*color),
        _ => None,
    });
    assert_eq!(label_color, Some(Color::from(DARK_THEME.text.secondary)));
}
