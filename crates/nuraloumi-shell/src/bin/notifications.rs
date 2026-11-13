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
use std::{collections::BTreeSet, env, path::PathBuf, process::ExitCode, sync::mpsc, thread};
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

const PRIMARY_POINTER_BUTTON: u32 = 0x110;

#[derive(Default)]
struct SafePress {
    pointer: Option<MenuAction>,
    touches: BTreeSet<i32>,
    touch: Option<(i32, MenuAction)>,
}

impl SafePress {
    fn pointer_button(
        &mut self,
        button: u32,
        pressed: bool,
        hovered: Option<MenuAction>,
    ) -> Option<MenuAction> {
        if button != PRIMARY_POINTER_BUTTON {
            return None;
        }
        if pressed {
            self.pointer = hovered;
            None
        } else {
            self.pointer
                .take()
                .filter(|armed| Some(armed) == hovered.as_ref())
        }
    }

    fn pointer_move(&mut self, hovered: Option<MenuAction>) {
        if self.pointer.as_ref() != hovered.as_ref() {
            self.pointer = None;
        }
    }

    fn pointer_leave(&mut self) {
        self.pointer = None;
    }

    fn touch_down(&mut self, id: i32, hovered: Option<MenuAction>) {
        self.touches.insert(id);
        if self.touches.len() == 1 {
            self.touch = hovered.map(|action| (id, action));
        } else {
            self.touch = None; // Multitouch cannot activate destructive actions.
        }
    }

    fn touch_move(&mut self, id: i32, hovered: Option<MenuAction>) {
        if self
            .touch
            .as_ref()
            .is_some_and(|(armed_id, action)| *armed_id == id && Some(action) != hovered.as_ref())
        {
            self.touch = None;
        }
    }

    fn touch_up(&mut self, id: i32) -> Option<MenuAction> {
        let was_active = self.touches.remove(&id);
        if was_active
            && self.touches.is_empty()
            && self.touch.as_ref().is_some_and(|(active, _)| *active == id)
        {
            self.touch.take().map(|(_, action)| action)
        } else {
            if self.touch.as_ref().is_some_and(|(active, _)| *active == id) {
                self.touch = None;
            }
            None
        }
    }

    fn touch_cancel(&mut self, ids: &[i32]) {
        for id in ids {
            self.touches.remove(id);
        }
        self.touch = None;
    }

    fn reset(&mut self) {
        self.pointer = None;
        self.touches.clear();
        self.touch = None;
    }
}

/// At most one bounded clear task can be active, and the UI is not blocked
/// while Dunst performs subprocess operations. The result channel is bounded.
fn start_clear(ids: Vec<u32>) -> mpsc::Receiver<(Result<String, String>, NotificationSnapshot)> {
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut provider = NotificationProvider::system();
        let result = provider
            .execute(NotificationAction::ClearKnown { ids })
            .map(|result| result.message)
            .map_err(|error| format!("{error:?}"));
        let snapshot = provider
            .snapshot()
            .unwrap_or_else(|error| NotificationSnapshot::unavailable(0, format!("{error:?}")));
        let _ = sender.send((result, snapshot));
    });
    receiver
}

/// Only periodically wake during an in-flight clear. Otherwise use blocking
/// Wayland dispatch and remain idle without polling or worker threads.
fn dispatch_while_clearing(backend: &mut WaylandBackend) -> Result<(), String> {
    backend.flush().map_err(|error| error.to_string())?;
    let mut fd = libc::pollfd {
        fd: backend.poll_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let ready = unsafe { libc::poll(&mut fd, 1, 40) };
    if ready < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(format!("poll Wayland while clearing: {error}"));
        }
    } else if ready > 0 {
        if fd.revents & libc::POLLIN != 0 {
            backend
                .blocking_dispatch()
                .map_err(|error| error.to_string())?;
        } else if fd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err("Wayland disconnected while clearing notifications".to_owned());
        }
    } else {
        backend
            .dispatch_pending()
            .map_err(|error| error.to_string())?;
    }
    Ok(())
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
fn model(
    snapshot: &NotificationSnapshot,
    query: &str,
    confirmed_ids: Option<&[u32]>,
    notice: Option<&str>,
    clearing: bool,
) -> MenuModel {
    let count = snapshot.notifications.len();
    let mut items = vec![
        MenuItem::status("header", format!("Notifications ({count})"))
            .with_subtitle(format!("Filter: {query}")),
    ];
    if clearing {
        items.push(MenuItem::status("clearing", "Clearing confirmed history…"));
    } else if let Some(ids) = confirmed_ids {
        items.push(MenuItem::action(
            "confirm",
            format!(
                "Clear {} confirmed notifications? Enter to confirm",
                ids.len()
            ),
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
    if let Some(notice) = notice {
        items.push(MenuItem::status("feedback", short(notice)));
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
        // Dunst cannot atomically target a historical notification action
        // by ID. Never render dangerous "Invoke" buttons that fail closed.
        if !clearing {
            items.push(MenuItem::action(
                format!("dismiss-{}", n.id),
                "Dismiss",
                MenuAction::Custom {
                    kind: "remove".into(),
                    payload: n.id.to_string(),
                },
            ));
        }
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
    if snapshot
        .notifications
        .iter()
        .any(|entry| !entry.actions.is_empty())
    {
        items.push(MenuItem::status(
            "action-safety",
            "Actions disabled: Dunst cannot target history actions safely",
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
    confirmed_ids: &mut Option<Vec<u32>>,
) -> Result<(), String> {
    if let MenuAction::Custom { kind, payload } = action {
        if kind == "ask" {
            // Freeze the exact IDs when requesting confirmation. A later
            // refresh or sender arrival must not widen the destructive action.
            *confirmed_ids = (!snapshot.notifications.is_empty()).then(|| {
                snapshot
                    .notifications
                    .iter()
                    .map(|entry| entry.id)
                    .collect()
            });
            return Ok(());
        }
        let action = match kind.as_str() {
            // Defense in depth: never route an unsafe Dunst history action,
            // even if another caller forges a custom menu activation.
            "invoke" => {
                return Err("Dunst history actions cannot safely target a notification ID".into())
            }
            "clear" => match confirmed_ids.as_ref() {
                Some(ids) => NotificationAction::ClearKnown { ids: ids.clone() },
                None => return Ok(()),
            },
            "remove" => NotificationAction::Remove {
                id: payload.parse().map_err(|e| format!("{e}"))?,
            },
            _ => return Ok(()),
        };
        if fixture_mode {
            match action {
                NotificationAction::ClearKnown { ids } => {
                    snapshot
                        .notifications
                        .retain(|entry| !ids.contains(&entry.id));
                }
                NotificationAction::Remove { id } => snapshot.notifications.retain(|n| n.id != id),
                _ => {}
            }
        } else {
            provider.execute(action).map_err(|e| format!("{e:?}"))?;
            *snapshot = provider.snapshot().map_err(|e| format!("{e:?}"))?;
        }
        *confirmed_ids = None;
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
    let mut confirmed_ids: Option<Vec<u32>> = None;
    let mut notice: Option<String> = None;
    let mut pending_clear: Option<mpsc::Receiver<(Result<String, String>, NotificationSnapshot)>> =
        None;
    let mut press = SafePress::default();
    let mut dismiss_press = SafePress::default();
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
    let mut menu = model(
        &snapshot,
        &query,
        confirmed_ids.as_deref(),
        notice.as_deref(),
        false,
    );
    let mut state = MenuState::new(&menu);
    let mut geometry = None;
    let mut scene = None;
    loop {
        if pending_clear.is_some() {
            dispatch_while_clearing(&mut backend)?;
        } else {
            backend.blocking_dispatch().map_err(|e| e.to_string())?;
        }
        for ev in backend.drain_events().collect::<Vec<_>>() {
            if dismiss.is_some() && ev.surface == dismiss {
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
                    WaylandEvent::PointerButton {
                        button, pressed, ..
                    } => {
                        if dismiss_press
                            .pointer_button(button, pressed, Some(MenuAction::Close))
                            .is_some()
                        {
                            close(&mut backend, surface, dismiss)?;
                            return Ok(());
                        }
                    }
                    WaylandEvent::TouchDown { id, .. } => {
                        dismiss_press.touch_down(id, Some(MenuAction::Close));
                    }
                    WaylandEvent::TouchUp { id } => {
                        if dismiss_press.touch_up(id).is_some() {
                            close(&mut backend, surface, dismiss)?;
                            return Ok(());
                        }
                    }
                    WaylandEvent::TouchCancel { ids } => dismiss_press.touch_cancel(&ids),
                    WaylandEvent::PointerLeave => dismiss_press.pointer_leave(),
                    WaylandEvent::Close => {
                        close(&mut backend, surface, dismiss)?;
                        return Ok(());
                    }
                    _ => {}
                }
                continue;
            }
            if ev.surface.is_some() && ev.surface != Some(surface) {
                press.reset();
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
                        if confirmed_ids.is_some() {
                            confirmed_ids = None;
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
                    pressed,
                    button,
                } => {
                    requested = press.pointer_button(button, pressed, hit(&scene, &menu, x, y));
                }
                WaylandEvent::PointerMove { x, y } | WaylandEvent::PointerEnter { x, y } => {
                    press.pointer_move(hit(&scene, &menu, x, y));
                }
                WaylandEvent::PointerLeave => press.pointer_leave(),
                WaylandEvent::TouchDown { id, x, y } => {
                    press.touch_down(id, hit(&scene, &menu, x, y));
                }
                WaylandEvent::TouchMotion { id, x, y } => {
                    press.touch_move(id, hit(&scene, &menu, x, y));
                }
                WaylandEvent::TouchUp { id } => requested = press.touch_up(id),
                WaylandEvent::TouchCancel { ids } => press.touch_cancel(&ids),
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
                if pending_clear.is_none() {
                    if !fixture_mode
                        && matches!(a, MenuAction::Custom { ref kind, .. } if kind == "clear")
                    {
                        if let Some(ids) = confirmed_ids.take() {
                            pending_clear = Some(start_clear(ids));
                            notice = None;
                        }
                    } else {
                        match act(
                            a,
                            &mut snapshot,
                            &mut provider,
                            fixture_mode,
                            &mut confirmed_ids,
                        ) {
                            Ok(()) => notice = None,
                            Err(error) => {
                                confirmed_ids = None;
                                notice = Some(error);
                            }
                        }
                    }
                }
                press.reset();
                redraw = true;
            }
            if refresh && pending_clear.is_none() {
                if fixture_mode {
                    snapshot = fixture();
                    notice = None;
                } else {
                    match provider.snapshot() {
                        Ok(fresh) => {
                            snapshot = fresh;
                            notice = None;
                        }
                        Err(error) => notice = Some(format!("{error:?}")),
                    }
                }
                redraw = true;
            }
            if redraw {
                let selected = state.selected_id.clone();
                menu = model(
                    &snapshot,
                    &query,
                    confirmed_ids.as_deref(),
                    notice.as_deref(),
                    pending_clear.is_some(),
                );
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
        // Apply a background clear result even with no incoming Wayland event.
        // Pending work runs on a bounded thread, and the menu stays responsive.
        if let Some(receiver) = pending_clear.as_ref() {
            let completed = match receiver.try_recv() {
                Ok(value) => Some(value),
                Err(mpsc::TryRecvError::Disconnected) => Some((
                    Err("notification clear worker disconnected".into()),
                    NotificationSnapshot::unavailable(0, "clear worker disconnected".into()),
                )),
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some((result, fresh)) = completed {
                pending_clear = None;
                snapshot = fresh; // Reflect partial success after any failure.
                notice = Some(match result {
                    Ok(message) => message,
                    Err(error) => format!("Clear incomplete: {error}"),
                });
                menu = model(
                    &snapshot,
                    &query,
                    confirmed_ids.as_deref(),
                    notice.as_deref(),
                    false,
                );
                state = MenuState::new(&menu);
                press.reset();
                if let Some((w, h, scale)) = geometry {
                    scene = Some(draw(
                        &mut backend,
                        surface,
                        &renderer,
                        &config,
                        &menu,
                        &state,
                        w,
                        h,
                        scale,
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
    fn ui_action(kind: &str) -> MenuAction {
        MenuAction::Custom {
            kind: kind.into(),
            payload: String::new(),
        }
    }

    #[test]
    fn dangerous_history_action_is_not_clickable_and_cannot_be_forged() {
        let snapshot = fixture();
        let menu = model(&snapshot, "", None, None, false);
        assert!(menu.item_by_id("action-1-open").is_none());
        assert!(menu.item_by_id("action-safety").is_some());
        assert!(menu.item_by_id("dismiss-1").is_some());
        let mut provider = NotificationProvider::system();
        let mut copy = snapshot;
        let mut confirmed = None;
        let rejected = act(
            MenuAction::Custom {
                kind: "invoke".into(),
                payload: "1:reply".into(),
            },
            &mut copy,
            &mut provider,
            true,
            &mut confirmed,
        );
        assert!(rejected.is_err());
    }

    #[test]
    fn release_activation_requires_same_primary_pointer_target() {
        let mut p = SafePress::default();
        let first = ui_action("ask");
        let other = ui_action("clear");
        assert!(p
            .pointer_button(PRIMARY_POINTER_BUTTON, true, Some(first.clone()))
            .is_none());
        assert!(p
            .pointer_button(0x111, false, Some(first.clone()))
            .is_none());
        assert!(p
            .pointer_button(PRIMARY_POINTER_BUTTON, false, Some(other))
            .is_none());
        assert!(p
            .pointer_button(PRIMARY_POINTER_BUTTON, false, Some(first.clone()))
            .is_none());
        assert!(p
            .pointer_button(PRIMARY_POINTER_BUTTON, true, Some(first.clone()))
            .is_none());
        assert_eq!(
            p.pointer_button(PRIMARY_POINTER_BUTTON, false, Some(first.clone())),
            Some(first.clone())
        );
        assert!(p
            .pointer_button(PRIMARY_POINTER_BUTTON, true, Some(first.clone()))
            .is_none());
        p.pointer_leave();
        assert!(p
            .pointer_button(PRIMARY_POINTER_BUTTON, false, Some(first.clone()))
            .is_none());
        assert!(p
            .pointer_button(PRIMARY_POINTER_BUTTON, true, Some(first.clone()))
            .is_none());
        p.pointer_move(None);
        assert!(p
            .pointer_button(PRIMARY_POINTER_BUTTON, false, Some(first))
            .is_none());
    }

    #[test]
    fn touch_requires_same_finger_target_and_release_without_multitouch() {
        let mut p = SafePress::default();
        let first = ui_action("ask");
        let other = ui_action("clear");
        p.touch_down(7, Some(first.clone()));
        assert!(p.touch_up(8).is_none());
        p.touch_move(7, Some(other));
        assert!(p.touch_up(7).is_none());
        p.touch_down(7, Some(first.clone()));
        assert_eq!(p.touch_up(7), Some(first.clone()));
        p.touch_down(7, Some(first.clone()));
        p.touch_down(8, Some(first.clone()));
        assert!(p.touch_up(7).is_none());
        assert!(p.touch_up(8).is_none());
        p.touch_down(11, Some(first.clone()));
        p.touch_cancel(&[11]);
        assert!(p.touch_up(11).is_none());
        p.touch_down(12, Some(first));
        p.reset();
        assert!(p.touch_up(12).is_none());
    }

    #[test]
    fn background_clear_mode_does_not_expose_destructive_buttons() {
        let snapshot = fixture();
        let menu = model(&snapshot, "", None, Some("pending"), true);
        assert!(menu.item_by_id("clearing").is_some());
        assert!(menu.item_by_id("feedback").is_some());
        assert!(menu.item_by_id("dismiss-1").is_none());
        assert!(menu.item_by_id("clear").is_none());
    }

    #[test]
    fn confirmed_clear_preserves_notification_arriving_after_ask() {
        let mut snapshot = fixture();
        let mut provider = NotificationProvider::system();
        let mut confirmed_ids = None;

        act(
            ui_action("ask"),
            &mut snapshot,
            &mut provider,
            true,
            &mut confirmed_ids,
        )
        .unwrap();
        assert_eq!(confirmed_ids.as_ref().unwrap().len(), 10);
        let mut incoming = snapshot.notifications[0].clone();
        incoming.id = 101;
        incoming.summary = "Arrived after confirmation".into();
        snapshot.notifications.push(incoming);

        // The confirmation text reflects the frozen set, not the refreshed
        // history count. New arrivals cannot be silently added to Clear All.
        let menu = model(&snapshot, "", confirmed_ids.as_deref(), None, false);
        assert!(menu
            .item_by_id("confirm")
            .unwrap()
            .label
            .contains("10 confirmed"));

        act(
            ui_action("clear"),
            &mut snapshot,
            &mut provider,
            true,
            &mut confirmed_ids,
        )
        .unwrap();
        assert_eq!(
            snapshot
                .notifications
                .iter()
                .map(|item| item.id)
                .collect::<Vec<_>>(),
            vec![101],
        );
        assert!(confirmed_ids.is_none());
    }

    #[test]
    fn clear_requires_confirmation_and_rearming_captures_new_set() {
        let mut snapshot = fixture();
        let mut provider = NotificationProvider::system();
        let mut confirmed_ids = None;

        // Direct clear without an earlier Ask is inert.
        act(
            ui_action("clear"),
            &mut snapshot,
            &mut provider,
            true,
            &mut confirmed_ids,
        )
        .unwrap();
        assert_eq!(snapshot.notifications.len(), 10);

        act(
            ui_action("ask"),
            &mut snapshot,
            &mut provider,
            true,
            &mut confirmed_ids,
        )
        .unwrap();
        let mut incoming = snapshot.notifications[0].clone();
        incoming.id = 102;
        snapshot.notifications.push(incoming);
        assert_eq!(confirmed_ids.as_ref().unwrap().len(), 10);

        // Explicitly asking again refreshes the proposed destructive set.
        act(
            ui_action("ask"),
            &mut snapshot,
            &mut provider,
            true,
            &mut confirmed_ids,
        )
        .unwrap();
        assert!(confirmed_ids.as_ref().unwrap().contains(&102));
        act(
            ui_action("clear"),
            &mut snapshot,
            &mut provider,
            true,
            &mut confirmed_ids,
        )
        .unwrap();
        assert!(snapshot.notifications.is_empty());
        assert!(confirmed_ids.is_none());

        // No confirmation is armed for an empty history.
        act(
            ui_action("ask"),
            &mut snapshot,
            &mut provider,
            true,
            &mut confirmed_ids,
        )
        .unwrap();
        assert!(confirmed_ids.is_none());
    }

    // A real D-Bus/daemon integration test must never touch the user's live
    // notification daemon. This runs only when explicitly requested against
    // a fresh, ephemeral dbus-run-session and dedicated Dunst/Xvfb instance.
    #[test]
    #[ignore = "requires isolated dbus-run-session + Dunst/Xvfb"]
    fn isolated_dbus_ui_confirmation_keeps_new_arrival() {
        assert_eq!(
            std::env::var("NURALOUMI_PRIVATE_DUNST_E2E").as_deref(),
            Ok("isolated"),
            "explicit isolated-bus opt-in required"
        );
        let bus = std::env::var("DBUS_SESSION_BUS_ADDRESS").expect("private D-Bus address");
        assert!(
            bus.starts_with("unix:path=/tmp/dbus-"),
            "refusing to mutate any non-ephemeral D-Bus session"
        );
        fn send(summary: &str) -> u32 {
            let output = std::process::Command::new("notify-send")
                .args([
                    "-p",
                    "-a",
                    "nuraloumi-private-e2e",
                    "-t",
                    "700",
                    summary,
                    "one private notification",
                ])
                .output()
                .expect("notify-send is installed");
            assert!(output.status.success(), "{output:?}");
            String::from_utf8(output.stdout)
                .unwrap()
                .trim()
                .parse()
                .expect("notification ID")
        }
        let first = send("before-confirmation");
        std::thread::sleep(std::time::Duration::from_millis(1350));
        let mut provider = NotificationProvider::system();
        let mut snapshot = provider.snapshot().expect("private Dunst history");
        assert_eq!(snapshot.notifications.len(), 1);
        assert_eq!(snapshot.notifications[0].id, first);

        let mut confirmed_ids = None;
        act(
            ui_action("ask"),
            &mut snapshot,
            &mut provider,
            false,
            &mut confirmed_ids,
        )
        .unwrap();
        assert_eq!(confirmed_ids.as_deref(), Some(&[first][..]));

        let second = send("after-confirmation");
        std::thread::sleep(std::time::Duration::from_millis(1350));
        act(
            ui_action("clear"),
            &mut snapshot,
            &mut provider,
            false,
            &mut confirmed_ids,
        )
        .unwrap();
        assert!(confirmed_ids.is_none());
        assert!(snapshot.notifications.iter().all(|n| n.id != first));
        assert!(snapshot.notifications.iter().any(|n| n.id == second));
        assert_eq!(snapshot.notifications.len(), 1);

        let third = send("async-clear-later");
        std::thread::sleep(std::time::Duration::from_millis(1350));
        let rx = start_clear(vec![third]);
        let (result, fresh) = rx.recv_timeout(std::time::Duration::from_secs(7)).unwrap();
        assert!(result.is_ok(), "{result:?}");
        assert!(fresh.notifications.iter().all(|entry| entry.id != third));
        assert!(fresh.notifications.iter().any(|entry| entry.id == second));
    }

    #[test]
    fn model_and_truncation() {
        let s = fixture();
        assert_eq!(s.notifications.len(), 10);
        assert!(model(&s, "Chat", None, None, false)
            .item_by_id("dismiss-1")
            .is_some());
        assert_eq!(short(&"x".repeat(500)).chars().count(), 240);
    }
}
