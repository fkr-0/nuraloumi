use nuraloumi_core::{
    MenuAction, MenuItem, MenuModel, MenuState, SemanticInput, DARK_THEME, LIGHT_THEME,
};
use nuraloumi_providers::notifications::{
    NotificationAction, NotificationEntry, NotificationProvider, NotificationSnapshot,
};
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

const MAX_TEXT_CHARS: usize = 240;
fn fixture() -> NotificationSnapshot {
    let mut s = NotificationSnapshot::unavailable(0, String::new());
    s.issues.clear();
    s.backend = "fixture".into();
    s.notifications = (0..10)
        .map(|i| NotificationEntry {
            id: i + 1,
            app: if i % 2 == 0 { "Chat" } else { "System" }.into(),
            summary: format!("Demo notification {}", i + 1),
            body: format!("Sample message body {}", i + 1),
            actions: if i % 2 == 0 {
                vec!["open".into(), "reply".into()]
            } else {
                vec![]
            },
            default_action: None,
        })
        .collect();
    s
}
fn short(s: &str) -> String {
    let flat = s.replace('\n', " ");
    if flat.chars().count() <= MAX_TEXT_CHARS {
        flat
    } else {
        format!(
            "{}…",
            flat.chars().take(MAX_TEXT_CHARS - 1).collect::<String>()
        )
    }
}
fn model(snapshot: &NotificationSnapshot, query: &str, confirm: bool) -> MenuModel {
    let count = snapshot.notifications.len();
    let mut items = vec![
        MenuItem::status("header", format!("Notifications ({count})"))
            .with_subtitle(format!("Filter: {query}")),
    ];
    if confirm {
        items.push(MenuItem::action(
            "confirm",
            format!("Clear all {count} notifications? Enter to confirm"),
            MenuAction::Custom {
                kind: "clear".into(),
                payload: String::new(),
            },
        ));
    } else {
        items.push(MenuItem::action(
            "clear",
            "Clear all…",
            MenuAction::Custom {
                kind: "ask".into(),
                payload: String::new(),
            },
        ));
    }
    let q = query.to_lowercase();
    let mut shown = 0;
    for n in snapshot.notifications.iter().take(64) {
        if !n.app.to_lowercase().contains(&q) && !n.summary.to_lowercase().contains(&q) {
            continue;
        }
        shown += 1;
        items.push(
            MenuItem::status(format!("n-{}", n.id), short(&n.summary)).with_subtitle(format!(
                "{} · {}",
                n.app,
                short(&n.body)
            )),
        );
        for action in &n.actions {
            items.push(MenuItem::action(
                format!("action-{}-{action}", n.id),
                format!("↳ {action}"),
                MenuAction::Custom {
                    kind: "invoke".into(),
                    payload: format!("{}:{action}", n.id),
                },
            ));
        }
        items.push(MenuItem::action(
            format!("dismiss-{}", n.id),
            "Dismiss",
            MenuAction::Custom {
                kind: "remove".into(),
                payload: n.id.to_string(),
            },
        ));
    }
    if shown == 0 {
        items.push(MenuItem::status(
            "empty",
            if query.is_empty() {
                "No notifications"
            } else {
                "No matching notifications"
            },
        ));
    }
    items.push(MenuItem::status(
        "backend",
        format!("{} · {} issues", snapshot.backend, snapshot.issues.len()),
    ));
    MenuModel::new("notifications", "Notifications", items)
}
fn act(
    action: MenuAction,
    snapshot: &mut NotificationSnapshot,
    provider: &mut NotificationProvider<nuraloumi_providers::command::SystemCommandRunner>,
    fixture_mode: bool,
    confirm: &mut bool,
) -> Result<(), String> {
    if let MenuAction::Custom { kind, payload } = action {
        if kind == "ask" {
            *confirm = !snapshot.notifications.is_empty();
            return Ok(());
        }
        let action = match kind.as_str() {
            "clear" if *confirm => NotificationAction::Clear,
            "remove" => NotificationAction::Remove {
                id: payload.parse().map_err(|e| format!("{e}"))?,
            },
            "invoke" => {
                let (id, name) = payload.split_once(':').ok_or("invalid action payload")?;
                NotificationAction::Invoke {
                    id: id.parse().map_err(|e| format!("{e}"))?,
                    action: name.into(),
                }
            }
            _ => return Ok(()),
        };
        if fixture_mode {
            match action {
                NotificationAction::Clear => snapshot.notifications.clear(),
                NotificationAction::Remove { id } => snapshot.notifications.retain(|n| n.id != id),
                _ => {}
            }
        } else {
            provider.execute(action).map_err(|e| format!("{e:?}"))?;
            *snapshot = provider.snapshot().map_err(|e| format!("{e:?}"))?;
        }
        *confirm = false;
    }
    Ok(())
}
fn run(config: ShellConfig, fixture_mode: bool) -> Result<(), String> {
    let mut provider = NotificationProvider::system();
    let mut snapshot = if fixture_mode {
        fixture()
    } else {
        provider.snapshot().map_err(|e| format!("{e:?}"))?
    };
    let mut query = String::new();
    let mut confirm = false;
    let mut backend = WaylandBackend::connect().map_err(|e| e.to_string())?;
    if !backend.capabilities().layer_shell {
        return Err("layer shell unavailable".into());
    }
    backend.blocking_dispatch().map_err(|e| e.to_string())?;
    let output = backend.outputs().into_iter().next();
    let dismiss = {
        let (t, r, b, l) = dismiss_backdrop_margins(config.panel_edge, config.panel_height);
        backend
            .create_dismiss_backdrop(DismissBackdropConfig {
                margin_top: t,
                margin_right: r,
                margin_bottom: b,
                margin_left: l,
                output: output.as_ref().map(|o| o.id),
                namespace: "nuraloumi-notifications-dismiss".into(),
            })
            .ok()
    };
    let surface = backend
        .create_menu(WaylandMenuConfig {
            width: config.menu_width.min(520),
            height: 560,
            margin_top: config.panel_height as i32,
            margin_left: 0,
            output: output.as_ref().map(|o| o.id),
            namespace: "nuraloumi-notifications".into(),
        })
        .map_err(|e| e.to_string())?;
    let renderer = CairoRenderer::default();
    let mut menu = model(&snapshot, &query, confirm);
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
                        dismiss.ok_or("dismiss missing")?,
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
            let mut remove_selected = false;
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
                        if confirm {
                            confirm = false;
                            redraw = true;
                        } else {
                            close(&mut backend, surface, dismiss)?;
                            return Ok(());
                        }
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
                    Key::Raw(111) => remove_selected = true,
                    Key::Modified { key, modifiers }
                        if modifiers.ctrl && matches!(*key,Key::Text(ref t) if t=="l") =>
                    {
                        requested = Some(MenuAction::Custom {
                            kind: "ask".into(),
                            payload: String::new(),
                        })
                    }
                    Key::Backspace => {
                        query.pop();
                        redraw = true;
                    }
                    Key::Text(t) if t == "q" && query.is_empty() => {
                        close(&mut backend, surface, dismiss)?;
                        return Ok(());
                    }
                    Key::Text(t) if t == "r" && query.is_empty() => refresh = true,
                    Key::Text(t) if t == "c" && query.is_empty() => {
                        requested = Some(MenuAction::Custom {
                            kind: "ask".into(),
                            payload: String::new(),
                        });
                    }
                    Key::Text(t) if t == "d" && query.is_empty() => remove_selected = true,
                    Key::Text(t) if !t.chars().any(char::is_control) => {
                        query.push_str(&t);
                        redraw = true;
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
            if remove_selected {
                if let Some(id) = state
                    .selected_id
                    .as_ref()
                    .and_then(|v| v.strip_prefix("dismiss-").or_else(|| v.strip_prefix("n-")))
                {
                    requested = Some(MenuAction::Custom {
                        kind: "remove".into(),
                        payload: id.into(),
                    });
                }
            }
            if let Some(a) = requested {
                act(a, &mut snapshot, &mut provider, fixture_mode, &mut confirm)?;
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
                menu = model(&snapshot, &query, confirm);
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
            eprintln!("nuraloumi-notifications: {e}");
            ExitCode::FAILURE
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_and_truncation() {
        let s = fixture();
        assert_eq!(s.notifications.len(), 10);
        assert!(model(&s, "Chat", false).item_by_id("dismiss-1").is_some());
        assert_eq!(short(&"x".repeat(500)).chars().count(), 240);
    }
}
