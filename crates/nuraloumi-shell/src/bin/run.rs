use nuraloumi_core::{
    MenuAction, MenuItem, MenuModel, MenuState, SemanticInput, DARK_THEME, LIGHT_THEME,
};
use nuraloumi_render_cairo::{CairoRenderer, RenderOptions, Scene, Viewport};
use nuraloumi_shell::{
    load_config_or_default, render_menu_with_glyphs, ShellConfig, Theme as ShellTheme,
};
use nuraloumi_wayland::{
    BackendError, DismissBackdropConfig, Frame, Key, MenuConfig as WaylandMenuConfig, PixelFormat,
    PlatformEvent as WaylandEvent, SurfaceId, WaylandBackend,
};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

const HELP: &str = r#"nuraloumi-run — NuraLoumi command runner

USAGE:
    nuraloumi-run [OPTIONS]

OPTIONS:
    --live                     Open a native Wayland/Cairo software-rendered run dialog
    --config <path>            Load JSON/TOML shell geometry/theme config
    --prompt <text>            Custom prompt text (default: "Run:")
    --path-complete            Populate with PATH executables for completion
    --recent-files             Include recently modified files and directories
    -h, --help                 Show this help

DESCRIPTION:
    A dmenu/rofi-style execute dialog for issuing commands with the compositor's
    environment. Opens as a layer-shell overlay. Type a command and press Enter
    to execute it. Escape or outside press to dismiss.
"#;

#[derive(Debug, Default)]
struct Args {
    live: bool,
    config: Option<PathBuf>,
    prompt: Option<String>,
    path_complete: bool,
    recent_files: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args::default();
    let mut argv = env::args().skip(1);
    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                eprint!("{HELP}");
                std::process::exit(0);
            }
            "--live" => args.live = true,
            "--config" => {
                args.config = Some(argv.next().ok_or("--config requires a path")?.into());
            }
            "--prompt" => {
                args.prompt = Some(argv.next().ok_or("--prompt requires text")?);
            }
            "--path-complete" => args.path_complete = true,
            "--recent-files" => args.recent_files = true,
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(args)
}

fn collect_path_executables() -> Vec<String> {
    let path_var = match env::var("PATH") {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    let mut names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for dir in path_var.split(':') {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            if let Ok(metadata) = entry.metadata() {
                use std::os::unix::fs::PermissionsExt;
                if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 {
                    if let Some(name) = entry.file_name().to_str() {
                        names.insert(name.to_string());
                    }
                }
            }
        }
    }
    names.into_iter().collect()
}

struct RecentEntry {
    display: String,
    path: String,
    is_dir: bool,
}

fn collect_recent_files() -> Vec<RecentEntry> {
    let home = match env::var("HOME") {
        Ok(h) => PathBuf::from(h),
        Err(_) => return Vec::new(),
    };

    let scan_dirs: Vec<PathBuf> = vec![
        home.clone(),
        home.join("Documents"),
        home.join("Downloads"),
        home.join("Desktop"),
        home.join("Pictures"),
        home.join("Music"),
        home.join("Videos"),
        home.join("Projects"),
        home.join("code"),
    ];

    let mut entries: Vec<(std::time::SystemTime, RecentEntry)> = Vec::new();

    for dir in &scan_dirs {
        let read = match fs::read_dir(dir) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
            let path = entry.path();
            let path_str = path.to_string_lossy().into_owned();
            let display = shorten_path(&path_str, &home);
            entries.push((
                mtime,
                RecentEntry {
                    display,
                    path: path_str,
                    is_dir: meta.is_dir(),
                },
            ));
        }
    }

    entries.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    entries.truncate(100);
    entries.into_iter().map(|(_, e)| e).collect()
}

fn shorten_path(path: &str, home: &Path) -> String {
    let home_str = home.to_string_lossy();
    if let Some(rest) = path.strip_prefix(home_str.as_ref()) {
        format!("~{rest}")
    } else {
        path.to_string()
    }
}

fn build_run_model(
    prompt: &str,
    executables: &[String],
    recent: &[RecentEntry],
    query: &str,
) -> MenuModel {
    let mut items = Vec::new();

    items.push(
        MenuItem::status(
            "run-input",
            if query.is_empty() {
                prompt.to_string()
            } else {
                format!("{prompt} {query}")
            },
        )
        .with_subtitle("Type a command, Enter to execute".to_string()),
    );

    items.push(MenuItem::separator("sep-input"));

    if !query.is_empty() {
        items.push(MenuItem::action(
            "run-execute",
            format!("Execute: {query}"),
            MenuAction::Custom {
                kind: "execute".into(),
                payload: query.to_string(),
            },
        ));
        items.push(MenuItem::separator("sep-results"));
    }

    let lower_query = query.to_lowercase();

    if !recent.is_empty() {
        let mut file_count = 0;
        let mut has_header = false;
        for entry in recent {
            if !query.is_empty()
                && !entry.display.to_lowercase().contains(&lower_query)
                && !entry.path.to_lowercase().contains(&lower_query)
            {
                continue;
            }
            if file_count >= 15 {
                break;
            }
            if !has_header {
                items.push(MenuItem::status("recent-header", "Recent files"));
                items.push(MenuItem::separator("sep-recent"));
                has_header = true;
            }
            let kind = if entry.is_dir {
                "open-dir"
            } else {
                "open-file"
            };
            let subtitle = if entry.is_dir {
                "Directory".to_string()
            } else {
                String::new()
            };
            items.push(
                MenuItem::action(
                    format!("recent-{file_count}"),
                    entry.display.clone(),
                    MenuAction::Custom {
                        kind: kind.into(),
                        payload: entry.path.clone(),
                    },
                )
                .with_subtitle(subtitle),
            );
            file_count += 1;
        }
        if has_header && !executables.is_empty() {
            items.push(MenuItem::separator("sep-exec-header"));
            items.push(MenuItem::status("exec-header", "Commands"));
            items.push(MenuItem::separator("sep-exec"));
        }
    }

    let mut count = 0;
    for name in executables {
        if !query.is_empty() && !name.to_lowercase().contains(&lower_query) {
            continue;
        }
        if count >= 50 {
            break;
        }
        items.push(MenuItem::action(
            format!("exec-{name}"),
            name.clone(),
            MenuAction::Custom {
                kind: "execute".into(),
                payload: name.clone(),
            },
        ));
        count += 1;
    }

    MenuModel::new("run-dialog", "Run", items)
}

fn execute_command(command: &str) -> Result<(), String> {
    Command::new("/bin/sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to spawn: {e}"))?;
    Ok(())
}

fn open_path(path: &str) -> Result<(), String> {
    Command::new("xdg-open")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("xdg-open failed: {e}"))?;
    Ok(())
}

fn dispatch_action(kind: &str, payload: &str) {
    match kind {
        "execute" => {
            let _ = execute_command(payload);
        }
        "open-file" | "open-dir" => {
            let _ = open_path(payload);
        }
        _ => {}
    }
}

fn glyph_for_item_with_dirs(id: &str, recent: &[RecentEntry]) -> Option<char> {
    if id == "run-input" {
        return Some('>');
    }
    if id == "run-execute" {
        return Some('$');
    }
    if id.starts_with("recent-") {
        if let Some(idx) = id
            .strip_prefix("recent-")
            .and_then(|s| s.parse::<usize>().ok())
        {
            if let Some(entry) = recent.get(idx) {
                return if entry.is_dir { Some('/') } else { Some('~') };
            }
        }
        return Some('~');
    }
    if id.starts_with("exec-") {
        return Some('#');
    }
    None
}

fn run_live(config: ShellConfig, args: &Args) -> Result<(), String> {
    let mut backend =
        WaylandBackend::connect().map_err(|e| format!("Wayland connection failed: {e}"))?;
    let capabilities = backend.capabilities();
    if !capabilities.layer_shell {
        return Err("compositor does not advertise zwlr_layer_shell_v1".into());
    }

    let prompt = args.prompt.as_deref().unwrap_or("Run:");
    let executables = if args.path_complete {
        collect_path_executables()
    } else {
        Vec::new()
    };
    let recent = if args.recent_files {
        collect_recent_files()
    } else {
        Vec::new()
    };

    let mut query = String::new();
    let mut model = build_run_model(prompt, &executables, &recent, &query);
    let mut state = MenuState::new(&model);

    let output = backend.outputs().into_iter().next();
    let requested_width = output
        .as_ref()
        .and_then(|o| {
            let scale = o.scale.max(1) as u32;
            (o.mode_width > 0).then_some(o.mode_width as u32 / scale)
        })
        .map(|w| config.menu_width.min(w.saturating_sub(16).max(240)))
        .unwrap_or(config.menu_width);
    let requested_height = output
        .as_ref()
        .and_then(|o| {
            let scale = o.scale.max(1) as u32;
            (o.mode_height > 0).then_some(o.mode_height as u32 / scale)
        })
        .map(|h| {
            h.saturating_sub(config.panel_height.saturating_add(16))
                .clamp(200, 480)
        })
        .unwrap_or(400);

    let dismiss_surface = {
        let (mt, mr, mb, ml) =
            nuraloumi_shell::dismiss_backdrop_margins(config.panel_edge, config.panel_height);
        backend
            .create_dismiss_backdrop(DismissBackdropConfig {
                margin_top: mt,
                margin_right: mr,
                margin_bottom: mb,
                margin_left: ml,
                output: output.as_ref().map(|o| o.id),
                namespace: "nuraloumi-run-dismiss".into(),
            })
            .ok()
    };

    let surface = match backend.create_menu(WaylandMenuConfig {
        width: requested_width,
        height: requested_height,
        margin_top: config.panel_height as i32,
        margin_left: 0,
        output: output.as_ref().map(|o| o.id),
        namespace: "nuraloumi-run".into(),
    }) {
        Ok(s) => s,
        Err(e) => {
            if let Some(ds) = dismiss_surface {
                let _ = backend.destroy_surface(ds);
            }
            return Err(format!("failed to create run surface: {e}"));
        }
    };

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
            .map_err(|e| format!("Wayland dispatch failed: {e}"))?;

        let events: Vec<_> = backend.drain_events().collect();
        for event in events {
            if let Some(ds) = dismiss_surface {
                if event.surface == Some(ds) {
                    match event.event {
                        WaylandEvent::Configure {
                            width,
                            height,
                            scale,
                        } => {
                            present_transparent(&mut backend, ds, width, height, scale)?;
                        }
                        WaylandEvent::PointerButton { pressed: true, .. }
                        | WaylandEvent::TouchDown { .. } => {
                            close_surfaces(&mut backend, surface, dismiss_surface)?;
                            return Ok(());
                        }
                        WaylandEvent::Close => {
                            close_surfaces(&mut backend, surface, dismiss_surface)?;
                            return Ok(());
                        }
                        _ => {}
                    }
                    continue;
                }
            }
            if event.surface.is_some() && event.surface != Some(surface) {
                continue;
            }

            let mut redraw = false;

            match event.event {
                WaylandEvent::Configure {
                    width,
                    height,
                    scale,
                } if event.surface == Some(surface) => {
                    geometry = Some((width, height, scale));
                    redraw = true;
                }
                WaylandEvent::Key { key, pressed: true } => match key {
                    Key::Escape => {
                        close_surfaces(&mut backend, surface, dismiss_surface)?;
                        return Ok(());
                    }
                    Key::Enter => {
                        let action = if let Some(sel) = &state.selected_id {
                            if sel == "run-execute" {
                                Some(("execute".to_string(), query.clone()))
                            } else if let Some(item) = model.item_by_id(sel) {
                                if let Some(MenuAction::Custom { kind, payload }) = &item.action {
                                    Some((kind.clone(), payload.clone()))
                                } else {
                                    Some(("execute".to_string(), query.clone()))
                                }
                            } else {
                                Some(("execute".to_string(), query.clone()))
                            }
                        } else if !query.is_empty() {
                            Some(("execute".to_string(), query.clone()))
                        } else {
                            None
                        };
                        if let Some((kind, payload)) = action {
                            if !payload.is_empty() {
                                close_surfaces(&mut backend, surface, dismiss_surface)?;
                                dispatch_action(&kind, &payload);
                                return Ok(());
                            }
                        }
                    }
                    Key::Backspace => {
                        query.pop();
                        model = build_run_model(prompt, &executables, &recent, &query);
                        state = MenuState::new(&model);
                        redraw = true;
                    }
                    Key::Up => {
                        state.handle(&model, SemanticInput::Up);
                        redraw = true;
                    }
                    Key::Down => {
                        state.handle(&model, SemanticInput::Down);
                        redraw = true;
                    }
                    Key::Tab => {
                        if let Some(sel) = &state.selected_id {
                            if let Some(item) = model.item_by_id(sel) {
                                if let Some(MenuAction::Custom { kind, payload }) = &item.action {
                                    if kind == "execute" {
                                        query = payload.clone();
                                        model =
                                            build_run_model(prompt, &executables, &recent, &query);
                                        state = MenuState::new(&model);
                                        redraw = true;
                                    }
                                }
                            }
                        }
                    }
                    Key::Text(text)
                        if !text.is_empty() && text.chars().all(|c| !c.is_control()) =>
                    {
                        query.push_str(&text);
                        model = build_run_model(prompt, &executables, &recent, &query);
                        state = MenuState::new(&model);
                        redraw = true;
                    }
                    _ => {}
                },
                WaylandEvent::PointerButton {
                    x,
                    y,
                    pressed: true,
                    ..
                } => {
                    if let Some(ref s) = scene {
                        if let Some(id) = find_hit_region(s, x, y) {
                            if let Some(item) = model.item_by_id(&id) {
                                if let Some(MenuAction::Custom { kind, payload }) = &item.action {
                                    close_surfaces(&mut backend, surface, dismiss_surface)?;
                                    dispatch_action(kind, payload);
                                    return Ok(());
                                }
                            }
                        }
                    }
                }
                WaylandEvent::TouchDown { x, y, .. } => {
                    if let Some(ref s) = scene {
                        if let Some(id) = find_hit_region(s, x, y) {
                            if let Some(item) = model.item_by_id(&id) {
                                if let Some(MenuAction::Custom { kind, payload }) = &item.action {
                                    close_surfaces(&mut backend, surface, dismiss_surface)?;
                                    dispatch_action(kind, payload);
                                    return Ok(());
                                }
                            }
                        }
                    }
                }
                WaylandEvent::Close => {
                    close_surfaces(&mut backend, surface, dismiss_surface)?;
                    return Ok(());
                }
                _ => {}
            }

            if redraw {
                if let Some((w, h, s)) = geometry {
                    let resolver = |id: &str| glyph_for_item_with_dirs(id, &recent);
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
                        Some(&resolver),
                    )?);
                }
            }
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

fn find_hit_region(scene: &Scene, x: f64, y: f64) -> Option<String> {
    scene.hit_test(x, y).map(str::to_owned)
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

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("nuraloumi-run: {e}");
            return ExitCode::from(2);
        }
    };

    if !args.live {
        eprintln!("{HELP}");
        eprintln!("nuraloumi-run requires --live mode");
        return ExitCode::from(2);
    }

    let config = args
        .config
        .as_deref()
        .map(|path| load_config_or_default(Some(path)))
        .unwrap_or_else(|| load_config_or_default(None))
        .unwrap_or_default();

    if let Err(e) = run_live(config, &args) {
        eprintln!("nuraloumi-run: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
