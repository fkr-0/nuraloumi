use nuraloumi_core::{MenuAction, MenuItem, MenuModel, MenuState, SemanticInput, DARK_THEME, LIGHT_THEME};
use nuraloumi_render_cairo::{CairoRenderer, RenderOptions, Scene, Viewport};
use nuraloumi_shell::{
    load_config_or_default, render_menu_with_glyphs, dismiss_backdrop_margins,
    ShellConfig, Theme as ShellTheme,
};
use nuraloumi_wayland::{
    BackendError, DismissBackdropConfig, Frame, Key, MenuConfig as WaylandMenuConfig,
    PixelFormat, PlatformEvent as WaylandEvent, SurfaceId, ToplevelId, ToplevelInfo,
    WaylandBackend,
};
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

const HELP: &str = r#"nuraloumi-windows — NuraLoumi window switcher

USAGE:
    nuraloumi-windows [OPTIONS]

OPTIONS:
    --live                     Open a native Wayland/Cairo software-rendered window list
    --config <path>            Load JSON/TOML shell geometry/theme config
    -h, --help                 Show this help

DESCRIPTION:
    An Alt+Tab / Super-style window switcher. Lists all open windows using the
    compositor's foreign-toplevel protocol and allows switching focus, closing, or
    fullscreening windows. Touch and keyboard friendly.
"#;

#[derive(Debug, Default)]
struct Args {
    live: bool,
    config: Option<PathBuf>,
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
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(args)
}

fn build_window_model(toplevels: &[ToplevelInfo], query: &str) -> MenuModel {
    let mut items = Vec::new();

    let label = if query.is_empty() {
        "Windows".to_string()
    } else {
        format!("Windows: {query}")
    };
    items.push(
        MenuItem::status("win-header", label)
            .with_subtitle("Enter to focus, Ctrl to close".to_string()),
    );
    items.push(MenuItem::separator("sep-header"));

    if toplevels.is_empty() {
        items.push(MenuItem::status("win-empty", "No open windows".to_string()));
        return MenuModel::new("windows", "Windows", items);
    }

    let lower_query = query.to_lowercase();
    let mut count = 0;
    for tl in toplevels {
        let title = if tl.title.is_empty() {
            tl.app_id.as_deref().unwrap_or("Unknown").to_string()
        } else {
            tl.title.clone()
        };
        let app_id = tl.app_id.as_deref().unwrap_or("");

        if !query.is_empty()
            && !title.to_lowercase().contains(&lower_query)
            && !app_id.to_lowercase().contains(&lower_query)
        {
            continue;
        }

        let state_indicator = if tl.state.activated {
            " [focused]"
        } else if tl.state.minimized {
            " [minimized]"
        } else if tl.state.fullscreen {
            " [fullscreen]"
        } else {
            ""
        };

        let subtitle = format!("{}{}", app_id, state_indicator);
        let id_str = tl.id.to_string();

        items.push(
            MenuItem::action(
                format!("tl-{id_str}"),
                title,
                MenuAction::Custom {
                    kind: "activate".into(),
                    payload: id_str,
                },
            )
            .with_subtitle(subtitle),
        );
        count += 1;
        if count >= 30 {
            break;
        }
    }

    if count == 0 && !query.is_empty() {
        items.push(MenuItem::status("win-no-match", "No matching windows"));
    }

    MenuModel::new("windows", "Windows", items)
}

fn run_live(config: ShellConfig) -> Result<(), String> {
    let mut backend =
        WaylandBackend::connect().map_err(|e| format!("Wayland connection failed: {e}"))?;
    let capabilities = backend.capabilities();
    if !capabilities.layer_shell {
        return Err("compositor does not advertise zwlr_layer_shell_v1".into());
    }
    if !capabilities.toplevel.list {
        return Err("compositor does not advertise foreign-toplevel protocol".into());
    }

    backend
        .blocking_dispatch()
        .map_err(|e| format!("initial dispatch failed: {e}"))?;

    let toplevels = backend.toplevels();
    let mut query = String::new();
    let mut model = build_window_model(&toplevels, &query);
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
            dismiss_backdrop_margins(config.panel_edge, config.panel_height);
        backend
            .create_dismiss_backdrop(DismissBackdropConfig {
                margin_top: mt,
                margin_right: mr,
                margin_bottom: mb,
                margin_left: ml,
                output: output.as_ref().map(|o| o.id),
                namespace: "nuraloumi-windows-dismiss".into(),
            })
            .ok()
    };

    let surface = match backend.create_menu(WaylandMenuConfig {
        width: requested_width,
        height: requested_height,
        margin_top: config.panel_height as i32,
        margin_left: 0,
        output: output.as_ref().map(|o| o.id),
        namespace: "nuraloumi-windows".into(),
    }) {
        Ok(s) => s,
        Err(e) => {
            if let Some(ds) = dismiss_surface {
                let _ = backend.destroy_surface(ds);
            }
            return Err(format!("failed to create window-list surface: {e}"));
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

        let tl_events: Vec<_> = backend.drain_toplevel_events().collect();
        let toplevel_changed = !tl_events.is_empty();

        let events: Vec<_> = backend.drain_events().collect();
        for event in events {
            if let Some(ds) = dismiss_surface {
                if event.surface == Some(ds) {
                    match event.event {
                        WaylandEvent::Configure { width, height, scale } => {
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
                WaylandEvent::Configure { width, height, scale }
                    if event.surface == Some(surface) =>
                {
                    geometry = Some((width, height, scale));
                    redraw = true;
                }
                WaylandEvent::Key { key, pressed: true } => match key {
                    Key::Escape => {
                        close_surfaces(&mut backend, surface, dismiss_surface)?;
                        return Ok(());
                    }
                    Key::Enter => {
                        if let Some(action) = selected_action(&model, &state) {
                            match handle_window_action(
                                &action,
                                &mut backend,
                                surface,
                                dismiss_surface,
                            )? {
                                ActionResult::Close => return Ok(()),
                                ActionResult::Refresh => {
                                    let tls = backend.toplevels();
                                    model = build_window_model(&tls, &query);
                                    state = MenuState::new(&model);
                                    redraw = true;
                                }
                                ActionResult::Noop => {}
                            }
                        }
                    }
                    Key::Backspace => {
                        query.pop();
                        let tls = backend.toplevels();
                        model = build_window_model(&tls, &query);
                        state = MenuState::new(&model);
                        redraw = true;
                    }
                    Key::Up => {
                        state.handle(&model, SemanticInput::Up);
                        redraw = true;
                    }
                    Key::Down | Key::Tab => {
                        state.handle(&model, SemanticInput::Down);
                        redraw = true;
                    }
                    Key::Modified { ref key, ref modifiers } if matches!(**key, Key::Tab) && modifiers.shift => {
                        state.handle(&model, SemanticInput::Up);
                        redraw = true;
                    }
                    Key::Text(ref text) => {
                        if !text.is_empty() && text.chars().all(|c| !c.is_control()) {
                            query.push_str(text);
                            let tls = backend.toplevels();
                            model = build_window_model(&tls, &query);
                            state = MenuState::new(&model);
                            redraw = true;
                        }
                    }
                    _ => {}
                },
                WaylandEvent::Key { key, pressed: false } if is_alt_key(&key) => {
                    if let Some(action) = selected_action(&model, &state) {
                        match handle_window_action(
                            &action,
                            &mut backend,
                            surface,
                            dismiss_surface,
                        )? {
                            ActionResult::Close => return Ok(()),
                            ActionResult::Refresh => {}
                            ActionResult::Noop => {}
                        }
                    }
                    close_surfaces(&mut backend, surface, dismiss_surface)?;
                    return Ok(());
                }
                WaylandEvent::PointerButton {
                    x,
                    y,
                    pressed: true,
                    ..
                } => {
                    if let Some(action) = hit_action(&scene, &model, x, y) {
                        match handle_window_action(
                            &action,
                            &mut backend,
                            surface,
                            dismiss_surface,
                        )? {
                            ActionResult::Close => return Ok(()),
                            ActionResult::Refresh => {
                                let tls = backend.toplevels();
                                model = build_window_model(&tls, &query);
                                state = MenuState::new(&model);
                                redraw = true;
                            }
                            ActionResult::Noop => {}
                        }
                    }
                }
                WaylandEvent::TouchDown { x, y, .. } => {
                    if let Some(action) = hit_action(&scene, &model, x, y) {
                        match handle_window_action(
                            &action,
                            &mut backend,
                            surface,
                            dismiss_surface,
                        )? {
                            ActionResult::Close => return Ok(()),
                            ActionResult::Refresh => {
                                let tls = backend.toplevels();
                                model = build_window_model(&tls, &query);
                                state = MenuState::new(&model);
                                redraw = true;
                            }
                            ActionResult::Noop => {}
                        }
                    }
                }
                WaylandEvent::Close => {
                    close_surfaces(&mut backend, surface, dismiss_surface)?;
                    return Ok(());
                }
                _ => {}
            }

            if toplevel_changed && !redraw {
                let tls = backend.toplevels();
                model = build_window_model(&tls, &query);
                state = MenuState::new(&model);
                redraw = true;
            }

            if redraw {
                if let Some((w, h, s)) = geometry {
                    let resolver = |id: &str| window_glyph(id);
                    scene = Some(render_and_present(
                        &mut backend, surface, &renderer, tokens, &model, &state, w, h, s,
                        Some(&resolver),
                    )?);
                }
            }
        }
    }
}

enum ActionResult {
    Noop,
    Close,
    Refresh,
}

fn selected_action(model: &MenuModel, state: &MenuState) -> Option<MenuAction> {
    let sel = state.selected_id.as_ref()?;
    let item = model.item_by_id(sel)?;
    item.action.clone()
}

fn hit_action(scene: &Option<Scene>, model: &MenuModel, x: f64, y: f64) -> Option<MenuAction> {
    let s = scene.as_ref()?;
    let id = s.hit_test(x, y)?;
    let item = model.item_by_id(id)?;
    item.action.clone()
}

fn handle_window_action(
    action: &MenuAction,
    backend: &mut WaylandBackend,
    surface: SurfaceId,
    dismiss: Option<SurfaceId>,
) -> Result<ActionResult, String> {
    match action {
        MenuAction::Custom { kind, payload } if kind == "activate" => {
            let id: ToplevelId = payload
                .parse()
                .map_err(|e: &str| format!("bad toplevel id: {e}"))?;
            let _ = backend.activate_toplevel(id);
            close_surfaces(backend, surface, dismiss)?;
            Ok(ActionResult::Close)
        }
        MenuAction::Custom { kind, payload } if kind == "close" => {
            let id: ToplevelId = payload
                .parse()
                .map_err(|e: &str| format!("bad toplevel id: {e}"))?;
            let _ = backend.close_toplevel(id);
            Ok(ActionResult::Refresh)
        }
        _ => Ok(ActionResult::Noop),
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

const ALT_LEFT: u32 = 56;
const ALT_RIGHT: u32 = 100;

fn is_alt_key(key: &Key) -> bool {
    matches!(key, Key::Raw(ALT_LEFT) | Key::Raw(ALT_RIGHT))
}

fn window_glyph(id: &str) -> Option<char> {
    if id == "win-header" {
        return Some('>');
    }
    if id == "win-empty" || id == "win-no-match" {
        return None;
    }
    if id.starts_with("tl-") {
        return Some('=');
    }
    None
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
            eprintln!("nuraloumi-windows: {e}");
            return ExitCode::from(2);
        }
    };

    if !args.live {
        eprintln!("{HELP}");
        eprintln!("nuraloumi-windows requires --live mode");
        return ExitCode::from(2);
    }

    let config = args
        .config
        .as_deref()
        .map(|path| load_config_or_default(Some(path)))
        .unwrap_or_else(|| load_config_or_default(None))
        .unwrap_or_default();

    if let Err(e) = run_live(config) {
        eprintln!("nuraloumi-windows: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
