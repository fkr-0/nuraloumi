use nuraloumi_core::{
    MenuAction, MenuItem, MenuModel, MenuState, NavigationOutcome, SemanticInput, DARK_THEME,
    LIGHT_THEME,
};
use nuraloumi_render_cairo::{CairoRenderer, RenderOptions, Scene, Viewport};
use nuraloumi_shell::{load_config_or_default, render_menu_with_glyphs, ShellConfig, Theme as ShellTheme};
use nuraloumi_wayland::{
    BackendError, Frame, Key, MenuConfig as WaylandMenuConfig, PixelFormat,
    PlatformEvent as WaylandEvent, SurfaceId, WaylandBackend,
};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

const HELP: &str = r#"nuraloumi-files — NuraLoumi file manager

USAGE:
    nuraloumi-files [OPTIONS] [PATH]

OPTIONS:
    --live                     Open a native Wayland/Cairo software-rendered file browser
    --config <path>            Load JSON/TOML shell geometry/theme config
    -h, --help                 Show this help

DESCRIPTION:
    A touch-friendly file manager built on the NuraLoumi menu model. Directories
    navigate in-place, files open with xdg-open. Supports keyboard navigation,
    search filter, and the standard 48px touch targets.
"#;

#[derive(Debug)]
struct Args {
    live: bool,
    config: Option<PathBuf>,
    start_path: PathBuf,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            live: false,
            config: None,
            start_path: env::var("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/")),
        }
    }
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
            other if !other.starts_with('-') => {
                args.start_path = PathBuf::from(other);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(args)
}

fn human_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn build_directory_model(dir: &Path) -> MenuModel {
    let title = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("/")
        .to_string();

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            return MenuModel::new(
                "files-error",
                &title,
                vec![MenuItem::status("err", format!("Cannot read: {e}"))],
            );
        }
    };

    let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    sorted.sort_by(|a, b| {
        let a_dir = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let b_dir = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
        b_dir.cmp(&a_dir).then_with(|| {
            a.file_name()
                .to_string_lossy()
                .to_lowercase()
                .cmp(&b.file_name().to_string_lossy().to_lowercase())
        })
    });

    let mut items = Vec::new();

    items.push(MenuItem::status(
        "path-display",
        dir.to_string_lossy().into_owned(),
    ));
    items.push(MenuItem::separator("sep-path"));

    if let Some(parent) = dir.parent() {
        items.push(
            MenuItem::action(
                "nav-parent",
                "..",
                MenuAction::Navigate {
                    target: parent.to_string_lossy().into_owned(),
                },
            )
            .with_subtitle("Parent directory".to_string()),
        );
    }

    let mut has_dirs = false;
    let mut has_files = false;

    for entry in &sorted {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let path_str = path.to_string_lossy().into_owned();
        let id = format!("f-{}", path_str.replace('/', "-"));

        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            has_dirs = true;
            items.push(
                MenuItem::action(
                    id,
                    format!("{name}/"),
                    MenuAction::Navigate {
                        target: path_str,
                    },
                )
                .with_subtitle("Directory".to_string()),
            );
        } else {
            if !has_files && has_dirs {
                items.push(MenuItem::separator("sep-files"));
            }
            has_files = true;
            let subtitle = entry
                .metadata()
                .ok()
                .map(|m| human_size(m.len()))
                .unwrap_or_default();
            items.push(
                MenuItem::action(
                    id,
                    name,
                    MenuAction::Custom {
                        kind: "open".into(),
                        payload: path_str,
                    },
                )
                .with_subtitle(subtitle),
            );
        }
    }

    if items.len() <= 2 {
        items.push(MenuItem::status("empty", "Empty directory"));
    }

    MenuModel::new("files", &title, items)
}

fn open_file(path: &str) -> Result<(), String> {
    Command::new("xdg-open")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("xdg-open failed: {e}"))?;
    Ok(())
}

fn handle_action(action: &MenuAction, current_dir: &mut PathBuf, model: &mut MenuModel, state: &mut MenuState) -> HandleResult {
    match action {
        MenuAction::Navigate { target } => {
            let path = PathBuf::from(target);
            if path.is_dir() {
                *current_dir = path;
                *model = build_directory_model(current_dir);
                *state = MenuState::new(model);
                HandleResult::Redraw
            } else {
                HandleResult::Noop
            }
        }
        MenuAction::Custom { kind, payload } if kind == "open" => {
            let _ = open_file(payload);
            HandleResult::Close
        }
        _ => HandleResult::Noop,
    }
}

enum HandleResult {
    Noop,
    Redraw,
    Close,
}

fn run_live(config: ShellConfig, args: &Args) -> Result<(), String> {
    let mut backend =
        WaylandBackend::connect().map_err(|e| format!("Wayland connection failed: {e}"))?;
    let capabilities = backend.capabilities();
    if !capabilities.layer_shell {
        return Err("compositor does not advertise zwlr_layer_shell_v1".into());
    }

    let mut current_dir = args.start_path.clone();
    let mut model = build_directory_model(&current_dir);
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
                .clamp(300, 720)
        })
        .unwrap_or(640);

    let surface = match backend.create_menu(WaylandMenuConfig {
        width: requested_width,
        height: requested_height,
        margin_top: config.panel_height as i32,
        margin_left: 0,
        output: output.as_ref().map(|o| o.id),
        namespace: "nuraloumi-files".into(),
    }) {
        Ok(s) => s,
        Err(e) => return Err(format!("failed to create files surface: {e}")),
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
            if event.surface.is_some() && event.surface != Some(surface) {
                continue;
            }

            let mut redraw = false;

            match event.event {
                WaylandEvent::Configure { width, height, scale }
                    if event.surface == Some(surface) =>
                {
                    geometry = Some((width, height, scale));
                    redraw = true;
                }
                WaylandEvent::Key { key, pressed: true } => {
                    let input = match key {
                        Key::Up => Some(SemanticInput::Up),
                        Key::Down => Some(SemanticInput::Down),
                        Key::Left => Some(SemanticInput::Left),
                        Key::Right => Some(SemanticInput::Right),
                        Key::Enter => Some(SemanticInput::Activate),
                        Key::Escape => Some(SemanticInput::Escape),
                        Key::Backspace => Some(SemanticInput::Backspace),
                        Key::Text(ref text) => {
                            if !text.is_empty() && text.chars().all(|c| !c.is_control()) {
                                Some(SemanticInput::Text(text.clone()))
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };
                    if let Some(input) = input {
                        let outcome = state.handle(&model, input);
                        match outcome {
                            NavigationOutcome::CloseRequested => {
                                let _ = backend.destroy_surface(surface);
                                return Ok(());
                            }
                            NavigationOutcome::ActionRequested { action, .. } => {
                                match handle_action(&action, &mut current_dir, &mut model, &mut state) {
                                    HandleResult::Close => {
                                        let _ = backend.destroy_surface(surface);
                                        return Ok(());
                                    }
                                    HandleResult::Redraw => {}
                                    HandleResult::Noop => {}
                                }
                            }
                            _ => {}
                        }
                        redraw = true;
                    }
                }
                WaylandEvent::PointerButton { x, y, pressed: true, .. } => {
                    if let Some(ref s) = scene {
                        if let Some(id) = find_hit_region(s, x, y) {
                            let action = model.item_by_id(&id).and_then(|i| i.action.clone());
                            if let Some(action) = action {
                                match handle_action(&action, &mut current_dir, &mut model, &mut state) {
                                    HandleResult::Close => {
                                        let _ = backend.destroy_surface(surface);
                                        return Ok(());
                                    }
                                    HandleResult::Redraw => redraw = true,
                                    HandleResult::Noop => {}
                                }
                            }
                        }
                    }
                }
                WaylandEvent::TouchDown { x, y, .. } => {
                    if let Some(ref s) = scene {
                        if let Some(id) = find_hit_region(s, x, y) {
                            let action = model.item_by_id(&id).and_then(|i| i.action.clone());
                            if let Some(action) = action {
                                match handle_action(&action, &mut current_dir, &mut model, &mut state) {
                                    HandleResult::Close => {
                                        let _ = backend.destroy_surface(surface);
                                        return Ok(());
                                    }
                                    HandleResult::Redraw => redraw = true,
                                    HandleResult::Noop => {}
                                }
                            }
                        }
                    }
                }
                WaylandEvent::Close => {
                    let _ = backend.destroy_surface(surface);
                    return Ok(());
                }
                _ => {}
            }

            if redraw {
                if let Some((w, h, s)) = geometry {
                    let resolver = |id: &str| file_glyph(id, &model);
                    scene = Some(render_and_present(
                        &mut backend, surface, &renderer, tokens, &model, &state, w, h, s,
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
    let viewport = Viewport::new(
        f64::from(width),
        f64::from(height),
        f64::from(scale.max(1)),
    );
    let (scene, mut buffer) = render_menu_with_glyphs(
        renderer, model, state, viewport, tokens, RenderOptions::default(), None,
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

fn file_glyph(id: &str, model: &MenuModel) -> Option<char> {
    if id == "path-display" {
        return Some('/');
    }
    if id == "nav-parent" {
        return Some('<');
    }
    let item = model.item_by_id(id)?;
    match &item.action {
        Some(MenuAction::Navigate { .. }) => Some('/'),
        Some(MenuAction::Custom { kind, payload }) if kind == "open" => {
            let ext = std::path::Path::new(payload.as_str())
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            match ext.as_str() {
                "jpg" | "jpeg" | "png" | "gif" | "bmp" | "svg" | "webp" => Some('*'),
                "mp3" | "flac" | "ogg" | "wav" | "opus" | "m4a" => Some('~'),
                "mp4" | "mkv" | "avi" | "webm" | "mov" => Some('~'),
                "pdf" => Some('@'),
                "zip" | "tar" | "gz" | "xz" | "bz2" | "7z" => Some('%'),
                "rs" | "py" | "sh" | "c" | "h" | "js" | "ts" | "toml" | "json" | "yaml" | "yml" => {
                    Some('#')
                }
                _ => Some('-'),
            }
        }
        _ => None,
    }
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("nuraloumi-files: {e}");
            return ExitCode::from(2);
        }
    };

    if !args.live {
        eprintln!("{HELP}");
        eprintln!("nuraloumi-files requires --live mode");
        return ExitCode::from(2);
    }

    let config = args
        .config
        .as_deref()
        .map(|path| load_config_or_default(Some(path)))
        .unwrap_or_else(|| load_config_or_default(None))
        .unwrap_or_default();

    if let Err(e) = run_live(config, &args) {
        eprintln!("nuraloumi-files: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
