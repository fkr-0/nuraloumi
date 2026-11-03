use nuraloumi_core::{
    MenuAction, MenuItem, MenuModel, MenuState, SemanticInput, DARK_THEME, LIGHT_THEME,
};
use nuraloumi_providers::Provider;
use nuraloumi_render_cairo::{CairoRenderer, RenderOptions, Scene, Viewport};
use nuraloumi_shell::{
    dismiss_backdrop_margins, load_config_or_default, render_menu_with_glyphs, ShellConfig,
    Theme as ShellTheme,
};
use nuraloumi_wayland::{
    BackendError, DismissBackdropConfig, Frame, Key, MenuConfig as WaylandMenuConfig, PixelFormat,
    PlatformEvent as WaylandEvent, SurfaceId, WaylandBackend,
};
use std::{env, path::PathBuf, process::ExitCode};
#[derive(Default)]
struct Args {
    config: Option<PathBuf>,
    fixture: bool,
}
fn parse_args() -> Result<Args, String> {
    let mut args = Args::default();
    let mut argv = env::args().skip(1);
    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("--config PATH --fixture --live (Wayland required)");
                std::process::exit(0);
            }
            "--config" => args.config = Some(argv.next().ok_or("--config requires PATH")?.into()),
            "--fixture" => args.fixture = true,
            "--live" => {}
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(args)
}
fn selected_action(model: &MenuModel, state: &MenuState) -> Option<MenuAction> {
    model
        .item_by_id(state.selected_id.as_ref()?)
        .and_then(|i| i.action.clone())
}
fn hit_action(scene: &Option<Scene>, model: &MenuModel, x: f64, y: f64) -> Option<MenuAction> {
    model
        .item_by_id(scene.as_ref()?.hit_test(x, y)?)
        .and_then(|i| i.action.clone())
}
use nuraloumi_providers::{
    CpuUsage, DiskIoStats, FilesystemStats, Health, MemoryStats, NetworkIoStats, ResourceProcess,
    ResourceProvider, ResourceSnapshot, SnapshotMeta, TemperatureReading,
};
#[derive(Clone, Copy)]
enum SortMode {
    Cpu,
}
fn demo() -> ResourceSnapshot {
    let mut s = ResourceSnapshot {
        meta: SnapshotMeta::new(0, Health::Healthy, false, "fixture"),
        cpu_percent: None,
        cpu_cores: Vec::new(),
        memory: None,
        filesystem: None,
        top_processes: Vec::new(),
        network: None,
        disk_io: None,
        temperatures: Vec::new(),
        issues: Vec::new(),
    };
    s.cpu_percent = Some(42);
    s.cpu_cores = vec![
        CpuUsage {
            id: "cpu0".into(),
            percent: Some(52),
        },
        CpuUsage {
            id: "cpu1".into(),
            percent: Some(32),
        },
    ];
    s.memory = Some(MemoryStats {
        total_kib: 1_048_576,
        available_kib: 524_288,
        used_kib: 524_288,
        used_percent: 50,
        cached_kib: 131_072,
        buffers_kib: 16_384,
        swap_total_kib: 262_144,
        swap_free_kib: 196_608,
    });
    s.filesystem = Some(FilesystemStats {
        mount: "/".into(),
        total_kib: 16_777_216,
        used_kib: 8_388_608,
        available_kib: 8_388_608,
        used_percent: 50,
    });
    s.network = Some(NetworkIoStats {
        received_bytes_total: 0,
        transmitted_bytes_total: 0,
        received_bytes_per_sec: Some(125_000),
        transmitted_bytes_per_sec: Some(64_000),
    });
    s.disk_io = Some(DiskIoStats {
        read_bytes_total: 0,
        written_bytes_total: 0,
        read_bytes_per_sec: Some(16_384),
        written_bytes_per_sec: Some(8192),
    });
    s.temperatures = vec![TemperatureReading {
        label: "CPU".into(),
        milli_celsius: 47_500,
    }];
    s.top_processes = vec![
        ResourceProcess {
            pid: 101,
            label: "labwc".into(),
            cpu_percent: Some(12),
            memory_mib: Some(43),
        },
        ResourceProcess {
            pid: 102,
            label: "terminal".into(),
            cpu_percent: Some(7),
            memory_mib: Some(65),
        },
    ];
    s.issues.clear();
    s
}
fn load_data(provider: &mut ResourceProvider, fixture: bool) -> Result<ResourceSnapshot, String> {
    if fixture {
        Ok(demo())
    } else {
        provider.snapshot().map_err(|e| e.to_string())
    }
}
fn bar(percent: Option<u8>) -> String {
    match percent {
        Some(v) => {
            let n = usize::from(v.min(100)) / 10;
            format!("[{}{}] {v}%", "█".repeat(n), "░".repeat(10 - n))
        }
        None => "[----------] n/a".into(),
    }
}
fn rate(n: Option<u64>) -> String {
    match n {
        Some(v) if v >= 1_000_000 => format!("{:.1} MB/s", v as f64 / 1_000_000.0),
        Some(v) => format!("{:.1} KB/s", v as f64 / 1000.0),
        None => "n/a".into(),
    }
}
fn build_model(
    data: &ResourceSnapshot,
    query: &str,
    _sort: SortMode,
    _confirmation: Option<u32>,
    _message: &str,
) -> MenuModel {
    let mut items = Vec::new();
    let mut add = |id: String, label: String, subtitle: String| {
        if query.is_empty()
            || label.to_lowercase().contains(&query.to_lowercase())
            || subtitle.to_lowercase().contains(&query.to_lowercase())
        {
            items.push(
                MenuItem::action(
                    id,
                    label,
                    MenuAction::Custom {
                        kind: "noop".into(),
                        payload: String::new(),
                    },
                )
                .with_subtitle(subtitle),
            );
        }
    };
    add(
        "section-cpu".into(),
        "CPU aggregate".into(),
        bar(data.cpu_percent),
    );
    for c in &data.cpu_cores {
        add(
            format!("core-{}", c.id),
            format!("core {}", c.id.strip_prefix("cpu").unwrap_or(&c.id)),
            bar(c.percent),
        );
    }
    if let Some(m) = &data.memory {
        add(
            "memory".into(),
            "Memory".into(),
            format!(
                "used {} / total {} MiB · available {} MiB · {}",
                m.used_kib / 1024,
                m.total_kib / 1024,
                m.available_kib / 1024,
                bar(Some(m.used_percent))
            ),
        );
        if m.swap_total_kib > 0 {
            add(
                "swap".into(),
                "Swap".into(),
                format!(
                    "used {} / total {} MiB",
                    m.swap_total_kib.saturating_sub(m.swap_free_kib) / 1024,
                    m.swap_total_kib / 1024
                ),
            );
        }
    }
    if let Some(d) = &data.filesystem {
        add(
            "disk".into(),
            format!("Disk {}", d.mount),
            format!(
                "{} · free {} MiB",
                bar(Some(d.used_percent)),
                d.available_kib / 1024
            ),
        );
    }
    for (i, t) in data.temperatures.iter().enumerate() {
        add(
            format!("thermal-{i}"),
            format!("Thermal {}", t.label),
            format!("{:.1} °C", f64::from(t.milli_celsius) / 1000.0),
        );
    }
    if let Some(n) = &data.network {
        add(
            "network".into(),
            "Network I/O".into(),
            format!(
                "RX {} · TX {}",
                rate(n.received_bytes_per_sec),
                rate(n.transmitted_bytes_per_sec)
            ),
        );
    }
    if let Some(d) = &data.disk_io {
        add(
            "disk-io".into(),
            "Disk I/O".into(),
            format!(
                "read {} · write {}",
                rate(d.read_bytes_per_sec),
                rate(d.written_bytes_per_sec)
            ),
        );
    }
    for p in data.top_processes.iter().take(10) {
        add(
            format!("pid-{}", p.pid),
            format!("Process {} (PID {})", p.label, p.pid),
            format!(
                "CPU {}% · memory {} MiB",
                p.cpu_percent.map_or("n/a".into(), |v| v.to_string()),
                p.memory_mib.map_or("n/a".into(), |v| v.to_string())
            ),
        );
    }
    if items.is_empty() {
        items.push(MenuItem::status("empty", "No readings match filter"));
    }
    items.insert(
        0,
        MenuItem::status("header", "Resources · r refresh · q quit"),
    );
    MenuModel::new("resources", "Resources", items)
}
fn run_live(config: ShellConfig, args: Args) -> Result<(), String> {
    let mut backend = WaylandBackend::connect().map_err(|e| format!("Wayland: {e}"))?;
    if !backend.capabilities().layer_shell {
        return Err("layer-shell not supported".into());
    }
    let mut provider = ResourceProvider::system();
    let mut data = load_data(&mut provider, args.fixture)?;
    let sort = SortMode::Cpu;
    let mut query = String::new();
    let mut confirmation: Option<u32> = None;
    let message = String::new();
    let mut model = build_model(&data, &query, sort, confirmation, &message);
    let mut state = MenuState::new(&model);
    let output = backend.outputs().into_iter().next();
    let width = output
        .as_ref()
        .and_then(|o| (o.mode_width > 0).then_some(o.mode_width as u32 / o.scale.max(1) as u32))
        .map(|w| config.menu_width.min(w.saturating_sub(16).max(240)))
        .unwrap_or(config.menu_width);
    let height = output
        .as_ref()
        .and_then(|o| (o.mode_height > 0).then_some(o.mode_height as u32 / o.scale.max(1) as u32))
        .map(|h| h.saturating_sub(config.panel_height + 16).clamp(300, 720))
        .unwrap_or(640);
    let (mt, mr, mb, ml) = dismiss_backdrop_margins(config.panel_edge, config.panel_height);
    let dismiss = backend
        .create_dismiss_backdrop(DismissBackdropConfig {
            margin_top: mt,
            margin_right: mr,
            margin_bottom: mb,
            margin_left: ml,
            output: output.as_ref().map(|o| o.id),
            namespace: "nuraloumi-resources-dismiss".into(),
        })
        .ok();
    let surface = backend
        .create_menu(WaylandMenuConfig {
            width,
            height,
            margin_top: config.panel_height as i32,
            margin_left: 0,
            output: output.as_ref().map(|o| o.id),
            namespace: "nuraloumi-resources".into(),
        })
        .map_err(|e| format!("create menu: {e}"))?;
    let renderer = CairoRenderer::default();
    let tokens = match config.theme {
        ShellTheme::Dark => &DARK_THEME,
        ShellTheme::Light => &LIGHT_THEME,
    };
    let mut geometry: Option<(u32, u32, i32)> = None;
    let mut scene: Option<Scene> = None;
    loop {
        backend
            .blocking_dispatch()
            .map_err(|e| format!("Wayland dispatch: {e}"))?;
        for event in backend.drain_events().collect::<Vec<_>>() {
            if let Some(ds) = dismiss.filter(|ds| event.surface == Some(*ds)) {
                match event.event {
                    WaylandEvent::Configure {
                        width,
                        height,
                        scale,
                    } => present_transparent(&mut backend, ds, width, height, scale)?,
                    WaylandEvent::PointerButton { pressed: true, .. }
                    | WaylandEvent::TouchDown { .. }
                    | WaylandEvent::Close => {
                        close_surfaces(&mut backend, surface, dismiss)?;
                        return Ok(());
                    }
                    _ => {}
                }
                continue;
            }
            if event.surface.is_some() && event.surface != Some(surface) {
                continue;
            }
            let mut redraw = false;
            let mut action = None;
            match event.event {
                WaylandEvent::Configure {
                    width,
                    height,
                    scale,
                } if event.surface == Some(surface) => {
                    geometry = Some((width, height, scale));
                    redraw = true
                }
                WaylandEvent::Key { key, pressed: true } => match key {
                    Key::Escape => {
                        if confirmation.take().is_some() {
                            redraw = true
                        } else {
                            close_surfaces(&mut backend, surface, dismiss)?;
                            return Ok(());
                        }
                    }
                    Key::Up => {
                        state.handle(&model, SemanticInput::Up);
                        redraw = true
                    }
                    Key::Down | Key::Tab => {
                        state.handle(&model, SemanticInput::Down);
                        redraw = true
                    }
                    Key::Enter => action = selected_action(&model, &state),
                    Key::Backspace => {
                        query.pop();
                        confirmation = None;
                        redraw = true
                    }
                    Key::Text(ref t) if t == "q" && query.is_empty() && confirmation.is_none() => {
                        close_surfaces(&mut backend, surface, dismiss)?;
                        return Ok(());
                    }
                    Key::Function(5) => {
                        data = load_data(&mut provider, args.fixture)?;
                        redraw = true
                    }
                    Key::Text(ref t) if t == "r" && confirmation.is_none() => {
                        data = load_data(&mut provider, args.fixture)?;
                        redraw = true
                    }

                    Key::Text(ref t) if !t.is_empty() && t.chars().all(|c| !c.is_control()) => {
                        query.push_str(t);
                        confirmation = None;
                        redraw = true
                    }
                    _ => {}
                },
                WaylandEvent::PointerButton {
                    x,
                    y,
                    pressed: true,
                    ..
                }
                | WaylandEvent::TouchDown { x, y, .. } => action = hit_action(&scene, &model, x, y),
                WaylandEvent::Close => {
                    close_surfaces(&mut backend, surface, dismiss)?;
                    return Ok(());
                }
                _ => {}
            }
            if action.is_some() {
                redraw = true;
            }
            if redraw {
                model = build_model(&data, &query, sort, confirmation, &message);
                let previous = state.selected_id.clone();
                state = MenuState::new(&model);
                if let Some(id) = previous {
                    if model
                        .item_by_id(&id)
                        .is_some_and(|item| item.action.is_some())
                    {
                        state.selected_id = Some(id);
                    }
                }
                if let Some((w, h, s)) = geometry {
                    scene = Some(render_and_present(
                        &mut backend,
                        surface,
                        &renderer,
                        tokens,
                        &model,
                        &state,
                        w,
                        h,
                        s,
                        None,
                    )?);
                }
            }
        }
    }
}
fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let config = load_config_or_default(args.config.as_deref()).unwrap_or_default();
    match run_live(config, args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn render_and_present<'a>(
    backend: &mut WaylandBackend,
    surface: SurfaceId,
    renderer: &CairoRenderer,
    tokens: &nuraloumi_core::ThemeTokens,
    model: &'a MenuModel,
    state: &MenuState,
    width: u32,
    height: u32,
    scale: i32,
    glyph_resolver: Option<&'a dyn Fn(&str) -> Option<char>>,
) -> Result<Scene, String> {
    let viewport = Viewport::new(f64::from(width), f64::from(height), f64::from(scale.max(1)));
    let (scene, mut buffer) = render_menu_with_glyphs(
        renderer,
        model,
        state,
        viewport,
        tokens,
        RenderOptions::default(),
        None,
        glyph_resolver,
    )
    .map_err(|e| format!("Cairo render failed: {e}"))?;

    let info = buffer.info();
    let bw = u32::try_from(info.width).map_err(|_| "negative width".to_owned())?;
    let bh = u32::try_from(info.height).map_err(|_| "negative height".to_owned())?;
    let stride = usize::try_from(info.stride).map_err(|_| "negative stride".to_owned())?;
    let present = buffer
        .with_argb32_bytes(|pixels, _| {
            backend.present(
                surface,
                Frame {
                    width: bw,
                    height: bh,
                    stride,
                    format: PixelFormat::Argb8888,
                    pixels,
                },
            )
        })
        .map_err(|e| format!("pixel borrow failed: {e}"))?;
    match present {
        Ok(()) | Err(BackendError::WouldBlock) => Ok(scene),
        Err(e) => Err(format!("present failed: {e}")),
    }
}

fn present_transparent(
    backend: &mut WaylandBackend,
    surface: SurfaceId,
    width: u32,
    height: u32,
    scale: i32,
) -> Result<(), String> {
    let scale = u32::try_from(scale.max(1)).map_err(|_| "invalid scale".to_owned())?;
    let pw = width.checked_mul(scale).ok_or("width overflow")?;
    let ph = height.checked_mul(scale).ok_or("height overflow")?;
    let byte_len = usize::try_from(pw)
        .ok()
        .and_then(|w| usize::try_from(ph).ok().and_then(|h| w.checked_mul(h)))
        .and_then(|n| n.checked_mul(4))
        .ok_or("buffer too large")?;
    let pixels = vec![0_u8; byte_len];
    match backend.present(
        surface,
        Frame::packed(pw, ph, PixelFormat::Argb8888, &pixels),
    ) {
        Ok(()) | Err(BackendError::WouldBlock) => Ok(()),
        Err(e) => Err(format!("present dismiss: {e}")),
    }
}

fn close_surfaces(
    backend: &mut WaylandBackend,
    surface: SurfaceId,
    dismiss: Option<SurfaceId>,
) -> Result<(), String> {
    if let Some(ds) = dismiss {
        backend
            .destroy_surface(ds)
            .map_err(|e| format!("destroy dismiss: {e}"))?;
    }
    backend
        .destroy_surface(surface)
        .map_err(|e| format!("destroy surface: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixture_needs_no_procfs() {
        let d = demo();
        assert_eq!(d.cpu_percent, Some(42));
        assert_eq!(d.cpu_cores.len(), 2);
        assert!(build_model(&d, "thermal", SortMode::Cpu, None, "")
            .item_by_id("thermal-0")
            .is_some());
    }
    #[test]
    fn bars_and_rates_are_bounded() {
        assert!(bar(Some(100)).contains("100%"));
        assert!(bar(None).contains("n/a"));
        assert_eq!(rate(Some(2_000_000)), "2.0 MB/s");
    }
}
