#[cfg(feature = "packaged-font")]
use std::{error::Error, path::PathBuf};

#[cfg(feature = "packaged-font")]
use nuraloumi_render_cairo::{
    CairoRenderer, InteractionState, MenuItemView, MenuView, PackagedFontSource, PackagedFontText,
    RenderOptions, Theme, Viewport,
};

#[cfg(feature = "packaged-font")]
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let font = args
        .next()
        .map(PathBuf::from)
        .ok_or("usage: render_strict_font FONT_FILE OUTPUT_PNG")?;
    let output = args
        .next()
        .map(PathBuf::from)
        .ok_or("usage: render_strict_font FONT_FILE OUTPUT_PNG")?;
    if args.next().is_some() {
        return Err("usage: render_strict_font FONT_FILE OUTPUT_PNG".into());
    }

    let backend = PackagedFontText::new(PackagedFontSource::new(&font))?;
    let source = backend.regular_source();
    let mixed = "abc سلام 42";
    let runs = backend.bidi_runs(mixed)?;
    for run in &runs {
        println!(
            "BIDI_RUN visual={} level={} rtl={} bytes={}..{} text={:?}",
            run.visual_index,
            run.embedding_level,
            run.rtl,
            run.byte_start,
            run.byte_end,
            &mixed[run.byte_start..run.byte_end],
        );
    }

    let renderer = CairoRenderer::with_text_backend(backend);
    let menu = MenuView {
        id: "strict-font-evidence".into(),
        title: "NuraLoumi strict font".into(),
        items: vec![
            MenuItemView::action("mixed", mixed),
            MenuItemView::action("arabic", "سلام"),
            MenuItemView::action("latin", "Exact packaged font file"),
        ],
    };
    let (scene, buffer) = renderer.render_strict(
        &menu,
        &InteractionState {
            selected_id: Some("mixed".into()),
            ..Default::default()
        },
        Viewport::new(480.0, 720.0, 1.0),
        &Theme::dark(),
        RenderOptions::default(),
    )?;
    buffer.write_png(&output)?;

    println!(
        "STRICT_FONT_EVIDENCE=PASS font={} output={} paint_nodes={} bidi_runs={}",
        source.path.display(),
        output.display(),
        scene.paint.len(),
        runs.len(),
    );
    Ok(())
}

#[cfg(not(feature = "packaged-font"))]
fn main() {
    eprintln!("render_strict_font requires --features packaged-font");
    std::process::exit(2);
}
