//! Host-only bounded CPU/RSS probe; not SL101 device qualification.
use nuraloumi_render_cairo::{
    CairoRenderer, InteractionState, MenuItemView, MenuView, RenderOptions, Theme, Viewport,
};
use std::{hint::black_box, time::Instant};

fn peak_rss_kib() -> Option<usize> {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        let line = status.lines().find(|line| line.starts_with("VmHWM:"))?;
        line.split_whitespace().nth(1)?.parse().ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let items = (0..48)
        .map(|i| {
            let mut item = MenuItemView::action(
                format!("row-{i}"),
                format!("Accessible action number {i}, with a longer label"),
            );
            item.subtitle = Some(format!("A compact explanatory description {i}"));
            item
        })
        .collect();
    let menu = MenuView {
        id: "bench".into(),
        title: "Accessibility probe".into(),
        items,
    };
    let renderer = CairoRenderer::default();
    let viewport = Viewport::new(1024.0, 600.0, 1.0);
    let state = InteractionState {
        selected_id: Some("row-47".into()),
        ..Default::default()
    };
    const FRAMES: u32 = 120;
    let started = Instant::now();
    for _ in 0..FRAMES {
        let (scene, buffer) = renderer.render_with_text_scale(
            &menu,
            &state,
            viewport,
            &Theme::dark(),
            RenderOptions::default(),
            1.5,
        )?;
        black_box((scene.paint.len(), buffer.info()));
    }
    println!(
        "host-only cairo 1024x600 1.5x: frames={FRAMES} mean_cpu_wall_ms={:.3} peak_rss_kib={:?} framebuffer_bytes={}",
        started.elapsed().as_secs_f64() * 1000.0 / f64::from(FRAMES),
        peak_rss_kib(), 1024 * 600 * 4
    );
    Ok(())
}
