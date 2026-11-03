use nuraloumi_render_cairo::{
    CairoRenderer, Color, DeterministicText, InteractionState, MenuItemView, MenuView, PaintNode,
    RenderOptions, RowKind, TextMeasurer, Theme, Viewport,
};

fn menu(count: usize) -> MenuView {
    let mut items = Vec::with_capacity(count + 2);
    items.push(MenuItemView {
        id: "heading".into(),
        label: "Applications".into(),
        subtitle: None,
        shortcut: None,
        glyph: None,
        kind: RowKind::Section,
        enabled: false,
    });
    for i in 0..count {
        let mut item = MenuItemView::action(
            format!("action-{i}"),
            format!("This is a deliberately long application title number {i}"),
        );
        item.subtitle = (i % 2 == 0).then(|| format!("A detailed secondary label {i}"));
        item.shortcut = (i % 3 == 0).then(|| "Ctrl+Shift+Return".into());
        item.glyph = (i % 5 == 0).then_some('◇');
        items.push(item);
    }
    MenuView {
        id: "accessibility".into(),
        title: "Applications and a long workspace name".into(),
        items,
    }
}

fn luminance(c: Color) -> f64 {
    fn linear(byte: u8) -> f64 {
        let c = f64::from(byte) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * linear(c.r) + 0.7152 * linear(c.g) + 0.0722 * linear(c.b)
}

fn contrast(a: Color, b: Color) -> f64 {
    let (top, bottom) = (luminance(a), luminance(b));
    (top.max(bottom) + 0.05) / (top.min(bottom) + 0.05)
}

#[test]
fn default_scene_is_exactly_the_explicit_one_x_scene() {
    let renderer = CairoRenderer::default();
    let menu = menu(5);
    let state = InteractionState::default();
    let viewport = Viewport::new(1024.0, 600.0, 1.0);
    for theme in [Theme::dark(), Theme::light()] {
        let legacy =
            renderer.build_scene(&menu, &state, viewport, &theme, RenderOptions::default());
        let scaled = renderer.build_scene_with_text_scale(
            &menu,
            &state,
            viewport,
            &theme,
            RenderOptions::default(),
            1.0,
        );
        assert_eq!(legacy, scaled);
    }
}

#[test]
fn text_scaling_is_finite_clamped_and_independent_of_pixel_density() {
    let renderer = CairoRenderer::default();
    let menu = menu(2);
    let state = InteractionState::default();
    let options = RenderOptions::default();
    let viewport = Viewport::new(1024.0, 600.0, 1.0);
    let baseline =
        renderer.build_scene_with_text_scale(&menu, &state, viewport, &Theme::dark(), options, 1.0);
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.5] {
        let scene = renderer.build_scene_with_text_scale(
            &menu,
            &state,
            viewport,
            &Theme::dark(),
            options,
            invalid,
        );
        assert_eq!(scene, baseline, "invalid scale {invalid:?} changed scene");
    }
    let max =
        renderer.build_scene_with_text_scale(&menu, &state, viewport, &Theme::dark(), options, 2.0);
    let clamped = renderer.build_scene_with_text_scale(
        &menu,
        &state,
        viewport,
        &Theme::dark(),
        options,
        1000.0,
    );
    assert_eq!(max, clamped);
    assert!(max.hits[1].rect.height > baseline.hits[1].rect.height);
    let retina = renderer.build_scene_with_text_scale(
        &menu,
        &state,
        Viewport::new(1024.0, 600.0, 2.0),
        &Theme::dark(),
        options,
        2.0,
    );
    assert_eq!(max.hits, retina.hits);
    assert_eq!(retina.viewport.device_size(), (2048, 1200));
}

#[test]
fn long_scaled_menu_reveals_selected_keyboard_row_and_preserves_touch_targets() {
    let renderer = CairoRenderer::default();
    let menu = menu(40);
    let state = InteractionState {
        selected_id: Some("action-39".into()),
        scroll_offset: 0.0,
        ..Default::default()
    };
    for scale in [1.0, 1.25, 1.5, 2.0] {
        for theme in [Theme::dark(), Theme::light()] {
            let scene = renderer.build_scene_with_text_scale(
                &menu,
                &state,
                Viewport::new(1024.0, 600.0, 1.0),
                &theme,
                RenderOptions::default(),
                scale,
            );
            assert!(scene.scroll.clipped);
            assert!(scene.scroll.offset > 0.0);
            assert!(scene.scroll.offset <= scene.scroll.max_offset);
            assert!(scene.scroll.viewport.bottom() <= 600.0);
            let selected = scene
                .hits
                .iter()
                .find(|hit| hit.item_id == "action-39")
                .expect("keyboard-selected row is entirely visible");
            assert!(selected.rect.height >= 48.0);
            assert!(selected.rect.y >= scene.scroll.viewport.y);
            assert!(selected.rect.bottom() <= scene.scroll.viewport.bottom() + 1e-9);
            let (x, y) = (
                selected.rect.x + selected.rect.width / 2.0,
                selected.rect.y + selected.rect.height / 2.0,
            );
            assert_eq!(scene.hit_test(x, y), Some("action-39"));
            assert!(scene
                .hits
                .iter()
                .filter(|hit| hit.actionable)
                .all(|hit| hit.rect.height >= 48.0));
            assert!(
                scene.paint.iter().any(|node| matches!(node,
                    PaintNode::RoundedRect { stroke: Some((color, width)), .. }
                        if *color == theme.accent && *width >= 2.0
                )),
                "selected focus outline missing"
            );
            assert!(
                scene.paint.iter().any(|node| matches!(node,
                    PaintNode::FillRect { rect, color }
                        if *color == theme.accent && rect.width == 2.0
                )),
                "scroll indicator missing"
            );
        }
    }
}

#[test]
fn scaled_text_has_measured_horizontal_bounds_and_actual_cairo_raster() {
    let renderer = CairoRenderer::default();
    let menu = menu(8);
    for scale in [1.0, 1.5, 2.0] {
        let (scene, mut raster) = renderer
            .render_with_text_scale(
                &menu,
                &InteractionState::default(),
                Viewport::new(1024.0, 600.0, 1.0),
                &Theme::light(),
                RenderOptions::default(),
                scale,
            )
            .expect("software Cairo render");
        assert_eq!(raster.info().width, 1024);
        assert_eq!(raster.info().height, 600);
        let info = raster.info();
        assert_eq!(
            raster.copy_argb32_bytes().unwrap().len(),
            info.stride as usize * info.height as usize
        );
        assert!(info.stride >= 1024 * 4);
        for node in &scene.paint {
            if let PaintNode::Text {
                origin,
                text,
                style,
                ..
            } = node
            {
                let width = DeterministicText.measure(text, *style).width;
                assert!(origin.x >= scene.panel_rect.x - 0.001);
                assert!(
                    origin.x + width <= scene.panel_rect.right() + 0.001,
                    "text overflows panel at {scale}: {text:?}"
                );
            }
        }
    }
}

#[test]
fn light_and_dark_text_and_focus_contrast_meet_readability_thresholds() {
    for theme in [Theme::dark(), Theme::light()] {
        for foreground in [theme.primary_text, theme.secondary_text, theme.hint] {
            assert!(
                contrast(foreground, theme.card) >= 4.5,
                "text contrast {:.2} < 4.5",
                contrast(foreground, theme.card)
            );
        }
        assert!(contrast(theme.primary_text, theme.selected) >= 4.5);
        assert!(
            contrast(theme.accent, theme.selected) >= 3.0,
            "focus contrast {:.2} < 3",
            contrast(theme.accent, theme.selected)
        );
    }
}

#[test]
fn partially_clipped_rows_cannot_expose_undersized_touch_targets() {
    let renderer = CairoRenderer::default();
    let menu = menu(12);
    for scale in [1.0, 1.5, 2.0] {
        let scene = renderer.build_scene_with_text_scale(
            &menu,
            &InteractionState {
                scroll_offset: 37.0,
                ..Default::default()
            },
            Viewport::new(400.0, 260.0, 1.0),
            &Theme::dark(),
            RenderOptions::default(),
            scale,
        );
        assert!(scene.scroll.clipped);
        assert!(scene
            .hits
            .iter()
            .filter(|hit| hit.actionable)
            .all(|hit| hit.rect.height >= 48.0));
        assert!(scene.hits.iter().all(|hit| {
            hit.rect.y >= scene.scroll.viewport.y
                && hit.rect.bottom() <= scene.scroll.viewport.bottom()
        }));
    }
}
