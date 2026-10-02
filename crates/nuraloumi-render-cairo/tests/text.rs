use cairo::{Context, Format, ImageSurface};
use nuraloumi_render_cairo::{
    DeterministicText, Point, TextBackend, TextMeasurer, TextStyle, ToyText,
};

fn style(size: f64, bold: bool) -> TextStyle {
    TextStyle { size, bold }
}

#[test]
fn metric_width_is_the_sum_of_the_shared_advance_function() {
    let text = "Wi-Fi 100%!";
    let style = style(16.0, false);
    let expected: f64 = text
        .chars()
        .map(|ch| DeterministicText::advance_for_char(ch, style))
        .sum();
    assert_eq!(DeterministicText.measure(text, style).width, expected);
}

#[test]
fn punctuation_ascii_classes_and_bold_have_stable_relative_widths() {
    let normal = style(20.0, false);
    let bold = style(20.0, true);
    assert!(
        DeterministicText::advance_for_char('.', normal)
            < DeterministicText::advance_for_char('a', normal)
    );
    assert!(
        DeterministicText::advance_for_char('a', normal)
            < DeterministicText::advance_for_char('A', normal)
    );
    let normal_width = DeterministicText.measure("Menu", normal).width;
    let bold_width = DeterministicText.measure("Menu", bold).width;
    assert_eq!(bold_width, normal_width * 1.04);
}

#[test]
fn combining_marks_and_format_selectors_do_not_change_advance() {
    let style = style(18.0, false);
    let plain = DeterministicText.measure("e", style);
    let combining = DeterministicText.measure("e\u{0301}", style);
    let variation = DeterministicText.measure("e\u{fe0f}", style);
    assert_eq!(plain.width, combining.width);
    assert_eq!(plain.width, variation.width);
}

#[test]
fn wide_unicode_uses_a_full_em_cell() {
    let style = style(17.0, false);
    assert_eq!(DeterministicText::advance_for_char('界', style), style.size);
    assert_eq!(DeterministicText::advance_for_char('🙂', style), style.size);
}

#[test]
fn cairo_draw_endpoint_matches_measured_width_even_under_context_scale() {
    let surface = ImageSurface::create(Format::ARgb32, 640, 200).unwrap();
    let context = Context::new(&surface).unwrap();
    context.scale(2.0, 2.0);
    let text = DeterministicText;
    let style = style(16.0, false);
    let origin = Point::new(12.0, 32.0);
    let run = "Cairo e\u{0301} 界!";

    let end = text.draw_to_cairo(&context, origin, run, style).unwrap();
    let measured = text.measure(run, style);
    assert_eq!(end, origin.x + measured.width);
}

#[test]
fn legacy_toy_text_name_delegates_to_the_same_metrics() {
    let style = style(14.0, true);
    assert_eq!(
        ToyText.measure("NuraLoumi", style),
        DeterministicText.measure("NuraLoumi", style)
    );
}

#[test]
fn deterministic_ellipsize_never_exceeds_the_requested_width() {
    let text = DeterministicText;
    let style = style(16.0, false);
    let max_width = 72.0;
    let fitted = text.ellipsize(
        "A deliberately long menu label with punctuation!",
        max_width,
        style,
    );
    assert!(fitted.ends_with('…'));
    assert!(text.measure(&fitted, style).width <= max_width);
}

#[test]
fn deterministic_backend_reports_lightweight_capabilities() {
    let capabilities = DeterministicText.capabilities();
    assert!(!capabilities.shaping);
    assert!(capabilities.deterministic_metrics);
    assert!(!capabilities.explicit_font_family);
    assert!(!capabilities.exact_font_file);
}

#[test]
fn ellipsize_keeps_emoji_modifier_and_zwj_clusters_together() {
    let text = DeterministicText;
    let style = style(16.0, false);
    let family = "👩🏽‍💻 menu entry";
    let cluster_width = text.measure("👩🏽‍💻…", style).width;
    let fitted = text.ellipsize(family, cluster_width, style);
    assert_eq!(fitted, "👩🏽‍💻…");
}

#[cfg(feature = "pangocairo")]
#[test]
fn pangocairo_backend_shapes_and_preserves_baseline_contract() {
    use nuraloumi_render_cairo::{
        CairoRenderer, InteractionState, MenuItemView, MenuView, PangoCairoText, RenderOptions,
        Theme, Viewport,
    };

    let backend = PangoCairoText::new("DejaVu Sans").unwrap();
    let capabilities = backend.capabilities();
    assert!(capabilities.shaping);
    assert!(!capabilities.deterministic_metrics);
    assert!(capabilities.explicit_font_family);
    assert!(!capabilities.exact_font_file);
    assert_eq!(backend.family().to_bytes(), b"DejaVu Sans");

    let style = style(18.0, false);
    let run = "سلام — नमस्ते — office ffi";
    let measured = backend.measure(run, style);
    assert!(measured.width > 0.0);
    assert!(measured.height > 0.0);
    assert!(measured.ascent > 0.0);

    let surface = ImageSurface::create(Format::ARgb32, 640, 200).unwrap();
    let context = Context::new(&surface).unwrap();
    context.scale(2.0, 2.0);
    let origin = Point::new(12.0, 40.0);
    let end = backend.draw(&context, origin, run, style).unwrap();
    assert_eq!(end, origin.x + measured.width);

    let renderer = CairoRenderer::with_text_backend(backend);
    assert!(renderer.text_backend().capabilities().shaping);

    let menu = MenuView {
        id: "shaped".into(),
        title: "النظام".into(),
        items: vec![MenuItemView::action("hello", "नमस्ते — سلام")],
    };
    let (_scene, mut buffer) = renderer
        .render(
            &menu,
            &InteractionState::default(),
            Viewport::new(360.0, 220.0, 2.0),
            &Theme::dark(),
            RenderOptions::default(),
        )
        .unwrap();
    let bytes = buffer.copy_argb32_bytes().unwrap();
    assert!(bytes.iter().any(|byte| *byte != 0));
}

#[cfg(all(feature = "packaged-font", unix))]
fn dejavu_source(name: &str) -> nuraloumi_render_cairo::PackagedFontSource {
    use std::path::Path;

    for directory in [
        "/usr/share/fonts/TTF",
        "/usr/share/fonts/truetype/dejavu",
        "/usr/share/fonts/dejavu",
    ] {
        let path = Path::new(directory).join(name);
        if path.is_file() {
            return nuraloumi_render_cairo::PackagedFontSource::new(path);
        }
    }
    panic!("DejaVu test font not found")
}

#[cfg(all(feature = "packaged-font", unix))]
#[test]
fn packaged_font_backend_binds_exact_files_shapes_and_fails_closed() {
    use std::fs;

    use nuraloumi_render_cairo::{
        CairoRenderer, InteractionState, MenuItemView, MenuView, PackagedFontError,
        PackagedFontText, RenderOptions, Theme, Viewport,
    };

    let regular = dejavu_source("DejaVuSans.ttf");
    let bold = dejavu_source("DejaVuSans-Bold.ttf");
    let expected_regular = fs::canonicalize(&regular.path).unwrap();
    let expected_bold = fs::canonicalize(&bold.path).unwrap();
    let backend = PackagedFontText::with_bold(regular, bold).unwrap();
    assert_eq!(backend.regular_source().path, expected_regular);
    assert_eq!(backend.bold_source().path, expected_bold);
    assert!(backend.capabilities().shaping);
    assert!(backend.capabilities().exact_font_file);
    assert!(!backend.capabilities().explicit_font_family);

    let normal = style(18.0, false);
    let arabic = "سلام";
    let measured = backend.try_measure(arabic, normal).unwrap();
    assert!(measured.width > 0.0);

    let surface = ImageSurface::create(Format::ARgb32, 640, 200).unwrap();
    let context = Context::new(&surface).unwrap();
    context.scale(2.0, 2.0);
    let origin = Point::new(12.0, 40.0);
    let end = backend.try_draw(&context, origin, arabic, normal).unwrap();
    assert_eq!(end, origin.x + measured.width);

    let mixed = "abc سلام 42";
    let runs = backend.bidi_runs(mixed).unwrap();
    assert_eq!(runs.len(), 3);
    assert_eq!(
        runs.iter()
            .map(|run| (
                &mixed[run.byte_start..run.byte_end],
                run.embedding_level,
                run.rtl,
                run.visual_index,
            ))
            .collect::<Vec<_>>(),
        vec![
            ("abc ", 0, false, 0),
            ("42", 2, false, 1),
            ("سلام ", 1, true, 2),
        ]
    );

    let mixed_measured = backend.try_measure(mixed, normal).unwrap();
    let mixed_end = backend.try_draw(&context, origin, mixed, normal).unwrap();
    assert_eq!(mixed_end, origin.x + mixed_measured.width);

    assert!(matches!(
        backend.try_measure("नमस्ते", normal),
        Err(PackagedFontError::MissingGlyph { .. })
    ));

    let renderer = CairoRenderer::with_text_backend(backend);
    let menu = MenuView {
        id: "strict-font".into(),
        title: "System".into(),
        items: vec![MenuItemView::action("mixed", mixed)],
    };
    let (_scene, mut buffer) = renderer
        .render_strict(
            &menu,
            &InteractionState::default(),
            Viewport::new(360.0, 220.0, 2.0),
            &Theme::dark(),
            RenderOptions::default(),
        )
        .unwrap();
    assert!(buffer
        .copy_argb32_bytes()
        .unwrap()
        .iter()
        .any(|byte| *byte != 0));

    let unsupported = MenuView {
        id: "strict-missing".into(),
        title: "System".into(),
        items: vec![MenuItemView::action("missing", "नमस्ते")],
    };
    assert!(matches!(
        renderer.render_strict(
            &unsupported,
            &InteractionState::default(),
            Viewport::new(360.0, 220.0, 1.0),
            &Theme::dark(),
            RenderOptions::default(),
        ),
        Err(nuraloumi_render_cairo::StrictRenderError::Font(
            PackagedFontError::MissingGlyph { .. }
        ))
    ));
    assert!(matches!(
        renderer.render(
            &unsupported,
            &InteractionState::default(),
            Viewport::new(360.0, 220.0, 1.0),
            &Theme::dark(),
            RenderOptions::default(),
        ),
        Err(cairo::Error::InvalidString)
    ));
}
