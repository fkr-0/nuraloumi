use nuraloumi_core::{
    MenuAction, MenuItem, MenuModel, MenuState, SemanticInput, DARK_THEME, LIGHT_THEME,
};
use nuraloumi_providers::media::{MediaAction, MediaPlayer, MediaProvider, MediaSnapshot};
use nuraloumi_providers::{ActionProvider, Provider};
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
    let mut a = Args::default();
    let mut iter = env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--fixture" => a.fixture = true,
            "--live" => {}
            "--config" => a.config = Some(iter.next().ok_or("--config requires a path")?.into()),
            "--help" | "-h" => {
                println!("--config PATH  --fixture  --live");
                std::process::exit(0)
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }
    Ok(a)
}
fn selection(model: &MenuModel, state: &MenuState) -> Option<MenuAction> {
    model
        .item_by_id(state.selected_id.as_deref()?)?
        .action
        .clone()
}
fn hit(scene: &Option<Scene>, model: &MenuModel, x: f64, y: f64) -> Option<MenuAction> {
    model
        .item_by_id(scene.as_ref()?.hit_test(x, y)?)?
        .action
        .clone()
}
#[allow(clippy::too_many_arguments)]
fn draw(
    backend: &mut WaylandBackend,
    surface: SurfaceId,
    renderer: &CairoRenderer,
    config: &ShellConfig,
    model: &MenuModel,
    state: &MenuState,
    width: u32,
    height: u32,
    scale: i32,
) -> Result<Scene, String> {
    let tokens = match config.theme {
        ShellTheme::Dark => &DARK_THEME,
        ShellTheme::Light => &LIGHT_THEME,
    };
    let viewport = Viewport::new(f64::from(width), f64::from(height), f64::from(scale.max(1)));
    let (scene, mut buffer) = render_menu_with_glyphs(
        renderer,
        model,
        state,
        viewport,
        tokens,
        RenderOptions::default(),
        None,
        None,
    )
    .map_err(|e| format!("render: {e}"))?;
    let info = buffer.info();
    let bw = u32::try_from(info.width).map_err(|e| e.to_string())?;
    let bh = u32::try_from(info.height).map_err(|e| e.to_string())?;
    let stride = usize::try_from(info.stride).map_err(|e| e.to_string())?;
    let result = buffer
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
        .map_err(|e| e.to_string())?;
    match result {
        Ok(()) | Err(BackendError::WouldBlock) => Ok(scene),
        Err(e) => Err(e.to_string()),
    }
}
fn transparent(
    backend: &mut WaylandBackend,
    surface: SurfaceId,
    w: u32,
    h: u32,
    s: i32,
) -> Result<(), String> {
    let scale = u32::try_from(s.max(1)).map_err(|e| e.to_string())?;
    let pw = w.checked_mul(scale).ok_or("overflow")?;
    let ph = h.checked_mul(scale).ok_or("overflow")?;
    let count = usize::try_from(pw)
        .ok()
        .and_then(|v| usize::try_from(ph).ok().and_then(|n| v.checked_mul(n)))
        .and_then(|n| n.checked_mul(4))
        .ok_or("overflow")?;
    let blank = vec![0u8; count];
    match backend.present(
        surface,
        Frame::packed(pw, ph, PixelFormat::Argb8888, &blank),
    ) {
        Ok(()) | Err(BackendError::WouldBlock) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
fn close(
    backend: &mut WaylandBackend,
    surface: SurfaceId,
    dismiss: Option<SurfaceId>,
) -> Result<(), String> {
    if let Some(s) = dismiss {
        backend.destroy_surface(s).map_err(|e| e.to_string())?;
    }
    backend.destroy_surface(surface).map_err(|e| e.to_string())
}

fn fixture() -> MediaSnapshot {
    let mut s = MediaSnapshot::unavailable(0, "fixture", String::new());
    s.issues.clear();
    s.players = vec![
        MediaPlayer {
            id: "mpv".into(),
            status: "Playing".into(),
            artist: Some("Demo Artist".into()),
            title: Some("Demo Track".into()),
        },
        MediaPlayer {
            id: "vlc".into(),
            status: "Paused".into(),
            artist: Some("Another Artist".into()),
            title: Some("Second Track".into()),
        },
        MediaPlayer {
            id: "browser".into(),
            status: "Stopped".into(),
            artist: None,
            title: None,
        },
    ];
    s.current_player = Some("mpv".into());
    s
}
fn model(snapshot: &MediaSnapshot, target: &str) -> MenuModel {
    let mut items = vec![MenuItem::status("header", "Now playing")];
    if let Some(p) = snapshot
        .players
        .iter()
        .find(|p| p.id == target)
        .or_else(|| snapshot.current())
    {
        items.push(
            MenuItem::status(
                "track",
                p.title.clone().unwrap_or_else(|| "Unknown track".into()),
            )
            .with_subtitle(format!(
                "{} · {}",
                p.artist.as_deref().unwrap_or("Unknown artist"),
                p.status
            )),
        );
    } else {
        items.push(MenuItem::status("empty", "No media player available"));
    }
    for (id, label) in [
        ("previous", "⏮ Previous"),
        ("toggle", "⏯ Play / Pause"),
        ("next", "⏭ Next"),
    ] {
        items.push(MenuItem::action(
            id,
            label,
            MenuAction::Custom {
                kind: id.into(),
                payload: target.into(),
            },
        ));
    }
    if snapshot.players.len() > 1 {
        items.push(MenuItem::separator("players-sep"));
        items.push(MenuItem::status("players-head", "Players"));
        for p in &snapshot.players {
            items.push(
                MenuItem::action(
                    format!("player-{}", p.id),
                    format!("{} {}", if p.id == target { "●" } else { "○" }, p.id),
                    MenuAction::Custom {
                        kind: "select".into(),
                        payload: p.id.clone(),
                    },
                )
                .with_subtitle(p.status.clone()),
            );
        }
    }
    items.push(MenuItem::status(
        "backend",
        format!("{} · {} issues", snapshot.backend, snapshot.issues.len()),
    ));
    MenuModel::new("music", "Music", items)
}
fn act(
    action: MenuAction,
    target: &mut String,
    snapshot: &mut MediaSnapshot,
    provider: &mut MediaProvider<nuraloumi_providers::command::SystemCommandRunner>,
    fixture_mode: bool,
) -> Result<(), String> {
    if let MenuAction::Custom { kind, payload } = action {
        if kind == "select" {
            *target = payload;
            return Ok(());
        }
        let media = match kind.as_str() {
            "toggle" => MediaAction::PlayPause {
                player: payload.clone(),
            },
            "previous" => MediaAction::Previous {
                player: payload.clone(),
            },
            "next" => MediaAction::Next {
                player: payload.clone(),
            },
            _ => return Ok(()),
        };
        if !snapshot.players.iter().any(|p| p.id == payload) {
            return Ok(());
        }
        if fixture_mode {
            if let Some(p) = snapshot.players.iter_mut().find(|p| p.id == payload) {
                if kind == "toggle" {
                    p.status = if p.status == "Playing" {
                        "Paused"
                    } else {
                        "Playing"
                    }
                    .into();
                }
                if kind == "next" || kind == "previous" {
                    p.title = Some(format!("Demo {} track", kind));
                }
            }
        } else {
            provider.execute(media).map_err(|e| format!("{e:?}"))?;
            *snapshot = provider.snapshot().map_err(|e| format!("{e:?}"))?;
        }
    }
    Ok(())
}
fn run(config: ShellConfig, fixture_mode: bool) -> Result<(), String> {
    let mut provider = MediaProvider::system();
    let mut snapshot = if fixture_mode {
        fixture()
    } else {
        provider.snapshot().map_err(|e| format!("{e:?}"))?
    };
    let mut target = snapshot.current_player.clone().unwrap_or_default();
    let mut backend = WaylandBackend::connect().map_err(|e| e.to_string())?;
    if !backend.capabilities().layer_shell {
        return Err("layer shell unavailable".into());
    }
    backend.blocking_dispatch().map_err(|e| e.to_string())?;
    let output = backend.outputs().into_iter().next();
    let width = config.menu_width.min(520);
    let height = 480;
    let dismiss = {
        let (t, r, b, l) = dismiss_backdrop_margins(config.panel_edge, config.panel_height);
        backend
            .create_dismiss_backdrop(DismissBackdropConfig {
                margin_top: t,
                margin_right: r,
                margin_bottom: b,
                margin_left: l,
                output: output.as_ref().map(|o| o.id),
                namespace: "nuraloumi-music-dismiss".into(),
            })
            .ok()
    };
    let surface = backend
        .create_menu(WaylandMenuConfig {
            width,
            height,
            margin_top: config.panel_height as i32,
            margin_left: 0,
            output: output.as_ref().map(|o| o.id),
            namespace: "nuraloumi-music".into(),
        })
        .map_err(|e| e.to_string())?;
    let renderer = CairoRenderer::default();
    let mut menu = model(&snapshot, &target);
    let mut state = MenuState::new(&menu);
    let mut geometry = None;
    let mut scene = None;
    loop {
        backend.blocking_dispatch().map_err(|e| e.to_string())?;
        for ev in backend.drain_events().collect::<Vec<_>>() {
            if ev.surface == dismiss {
                match ev.event {
                    WaylandEvent::Configure {
                        width,
                        height,
                        scale,
                    } => transparent(
                        &mut backend,
                        dismiss.ok_or("dismiss surface missing")?,
                        width,
                        height,
                        scale,
                    )?,
                    WaylandEvent::PointerButton { pressed: true, .. }
                    | WaylandEvent::TouchDown { .. }
                    | WaylandEvent::Close => {
                        close(&mut backend, surface, dismiss)?;
                        return Ok(());
                    }
                    _ => {}
                }
                continue;
            }
            if ev.surface.is_some() && ev.surface != Some(surface) {
                continue;
            }
            let mut redraw = false;
            let mut requested = None;
            let mut refresh = false;
            match ev.event {
                WaylandEvent::Configure {
                    width,
                    height,
                    scale,
                } => {
                    geometry = Some((width, height, scale));
                    redraw = true;
                }
                WaylandEvent::Close => {
                    close(&mut backend, surface, dismiss)?;
                    return Ok(());
                }
                WaylandEvent::Key { key, pressed: true } => match key {
                    Key::Escape => {
                        close(&mut backend, surface, dismiss)?;
                        return Ok(());
                    }
                    Key::Up => {
                        state.handle(&menu, SemanticInput::Up);
                        redraw = true;
                    }
                    Key::Down | Key::Tab => {
                        state.handle(&menu, SemanticInput::Down);
                        redraw = true;
                    }
                    Key::Enter => requested = selection(&menu, &state),
                    Key::Function(5) => refresh = true,
                    Key::Space => {
                        requested = Some(MenuAction::Custom {
                            kind: "toggle".into(),
                            payload: target.clone(),
                        })
                    }
                    Key::Text(t) if t == "q" => {
                        close(&mut backend, surface, dismiss)?;
                        return Ok(());
                    }
                    Key::Text(t) if t == "r" => refresh = true,
                    Key::Text(t) if t == " " => {
                        requested = Some(MenuAction::Custom {
                            kind: "toggle".into(),
                            payload: target.clone(),
                        })
                    }
                    _ => {}
                },
                WaylandEvent::PointerButton {
                    x,
                    y,
                    pressed: true,
                    ..
                }
                | WaylandEvent::TouchDown { x, y, .. } => requested = hit(&scene, &menu, x, y),
                _ => {}
            }
            if let Some(a) = requested {
                act(a, &mut target, &mut snapshot, &mut provider, fixture_mode)?;
                redraw = true;
            }
            if refresh {
                snapshot = if fixture_mode {
                    fixture()
                } else {
                    provider.snapshot().map_err(|e| format!("{e:?}"))?
                };
                redraw = true;
            }
            if redraw {
                let selected = state.selected_id.clone();
                menu = model(&snapshot, &target);
                state = MenuState::new(&menu);
                if let Some(id) = selected {
                    if menu
                        .item_by_id(&id)
                        .and_then(|i| i.action.as_ref())
                        .is_some()
                    {
                        state.selected_id = Some(id);
                    }
                }
                if let Some((w, h, s)) = geometry {
                    scene = Some(draw(
                        &mut backend,
                        surface,
                        &renderer,
                        &config,
                        &menu,
                        &state,
                        w,
                        h,
                        s,
                    )?);
                }
            }
        }
    }
}
fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let config = load_config_or_default(args.config.as_deref()).unwrap_or_default();
    match run(config, args.fixture) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("nuraloumi-music: {e}");
            ExitCode::FAILURE
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixture_players_and_controls() {
        let s = fixture();
        let m = model(&s, "mpv");
        assert_eq!(s.players.len(), 3);
        assert!(m.item_by_id("toggle").is_some());
        assert!(m.item_by_id("player-vlc").is_some());
    }
}
