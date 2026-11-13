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
use std::{
    collections::BTreeSet,
    env,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    path::PathBuf,
    process::ExitCode,
};
#[derive(Default)]
struct Args {
    config: Option<PathBuf>,
    fixture: bool,
}
/// A touch or pointer release may only activate the action originally pressed.
/// In particular, a destructive confirmation cannot fire on touch-down.
fn release_action(pressed: Option<MenuAction>, current: Option<MenuAction>) -> Option<MenuAction> {
    match (pressed, current) {
        (Some(armed), Some(released)) if armed == released => Some(armed),
        _ => None,
    }
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
use nuraloumi_providers::{ProcessEntry, ProcessProvider, ProcessSnapshot};
#[derive(Clone, Copy)]
enum SortMode {
    Cpu,
    Memory,
    Name,
    Pid,
}
impl SortMode {
    fn next(self) -> Self {
        match self {
            Self::Cpu => Self::Memory,
            Self::Memory => Self::Name,
            Self::Name => Self::Pid,
            Self::Pid => Self::Cpu,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Memory => "memory",
            Self::Name => "name",
            Self::Pid => "PID",
        }
    }
}
fn demo() -> ProcessSnapshot {
    let mut s = ProcessProvider::new("/nonexistent-nuraloumi-demo")
        .snapshot()
        .expect("fixture snapshot");
    s.processes = vec![
        ProcessEntry {
            id: "pid:100".into(),
            pid: 100,
            label: "labwc".into(),
            state: "Running".into(),
            uid: Some(1000),
            start_time_ticks: Some(100),
            cpu_ticks: Some(100),
            cpu_percent: Some(12),
            memory_mib: Some(42),
        },
        ProcessEntry {
            id: "pid:101".into(),
            pid: 101,
            label: "waybar".into(),
            state: "Sleeping".into(),
            uid: Some(1000),
            start_time_ticks: Some(101),
            cpu_ticks: Some(101),
            cpu_percent: Some(3),
            memory_mib: Some(24),
        },
        ProcessEntry {
            id: "pid:102".into(),
            pid: 102,
            label: "terminal".into(),
            state: "Running".into(),
            uid: Some(1000),
            start_time_ticks: Some(102),
            cpu_ticks: Some(102),
            cpu_percent: Some(25),
            memory_mib: Some(65),
        },
    ];
    s.issues.clear();
    s
}
fn load_data(provider: &mut ProcessProvider, fixture: bool) -> Result<ProcessSnapshot, String> {
    if fixture {
        Ok(demo())
    } else {
        provider.snapshot().map_err(|e| e.to_string())
    }
}
fn ordered(data: &ProcessSnapshot, query: &str, sort: SortMode) -> Vec<ProcessEntry> {
    let q = query.to_lowercase();
    let mut rows = data
        .processes
        .iter()
        .filter(|p| p.label.to_lowercase().contains(&q) || p.pid.to_string().contains(&q))
        .cloned()
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| {
        match sort {
            SortMode::Cpu => b.cpu_percent.unwrap_or(0).cmp(&a.cpu_percent.unwrap_or(0)),
            SortMode::Memory => b.memory_mib.unwrap_or(0).cmp(&a.memory_mib.unwrap_or(0)),
            SortMode::Name => a.label.to_lowercase().cmp(&b.label.to_lowercase()),
            SortMode::Pid => a.pid.cmp(&b.pid),
        }
        .then_with(|| a.pid.cmp(&b.pid))
    });
    rows
}
fn build_model(
    data: &ProcessSnapshot,
    query: &str,
    sort: SortMode,
    confirmation: Option<&ProcessEntry>,
    message: &str,
) -> MenuModel {
    let mut items = vec![
        MenuItem::status(
            "header",
            format!(
                "Tasks: {} processes | sort {}",
                data.processes.len(),
                sort.name()
            ),
        )
        .with_subtitle("s sort · k terminate · r refresh (CPU after 2 samples) · q quit"),
        MenuItem::separator("sep"),
    ];
    if !query.is_empty() {
        items.push(MenuItem::status("filter", format!("Filter: {query}")))
    }
    if !message.is_empty() {
        items.push(MenuItem::status("result", message.to_string()))
    }
    if let Some(target) = confirmation {
        items.push(MenuItem::action(
            "confirm",
            format!(
                "Send SIGTERM to {} (PID {})? Press Enter to confirm.",
                target.label, target.pid
            ),
            MenuAction::Custom {
                kind: "confirm".into(),
                payload: target.pid.to_string(),
            },
        ));
    }
    for p in ordered(data, query, sort) {
        items.push(
            MenuItem::action(
                format!("pid-{}", p.pid),
                format!("{} (PID {})", p.label, p.pid),
                MenuAction::Custom {
                    kind: "select".into(),
                    payload: p.pid.to_string(),
                },
            )
            .with_subtitle(format!(
                "{} · CPU {} · {} MiB",
                p.state,
                p.cpu_percent
                    .map(|n| format!("{n}%"))
                    .unwrap_or("n/a".into()),
                p.memory_mib.map(|n| n.to_string()).unwrap_or("n/a".into())
            )),
        );
    }
    if items.len() == 2 {
        items.push(MenuItem::status("empty", "No processes"))
    }
    MenuModel::new("tasks", "Tasks", items)
}
/// Disallow critical session processes even if run by the current user.
/// This is a defensive UI policy in addition to the immutable PID identity.
fn protected_process(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with("nuraloumi-")
        || name.starts_with("sl101-")
        || matches!(
            name.as_str(),
            "sway"
                | "swaybar"
                | "swaybg"
                | "labwc"
                | "wayvnc"
                | "waybar"
                | "seatd"
                | "systemd"
                | "init"
                | "sshd"
                | "dbus-daemon"
                | "pipewire"
                | "wireplumber"
        )
}

fn validate_identity(
    expected: &ProcessEntry,
    actual: &ProcessEntry,
    effective_uid: u32,
    self_pid: u32,
    parent_pid: u32,
) -> Result<(), String> {
    if expected.pid <= 1 || expected.pid == self_pid || expected.pid == parent_pid {
        return Err("refusing to terminate an essential/current process".into());
    }
    if protected_process(&expected.label) || protected_process(&actual.label) {
        return Err("protected compositor, panel, or session process".into());
    }
    if effective_uid == 0 {
        return Err("task manager refuses process termination as root".into());
    }
    let expected_uid = expected.uid.ok_or("unknown original process UID")?;
    let start = expected
        .start_time_ticks
        .filter(|ticks| *ticks > 0)
        .ok_or("unknown original process start time")?;
    if expected_uid != effective_uid
        || actual.uid != Some(expected_uid)
        || actual.pid != expected.pid
        || actual.start_time_ticks != Some(start)
        || actual.label != expected.label
    {
        return Err("process identity changed (UID/PID/starttime/name); refresh tasks".into());
    }
    Ok(())
}

/// PIDFD pins the kernel task identity after opening: a reused numeric PID
/// cannot redirect the subsequent signal. Fail closed on older kernels.
fn terminate(target: &ProcessEntry, fixture: bool) -> Result<(), String> {
    if fixture {
        return Ok(());
    }
    let uid = unsafe { libc::geteuid() };
    let parent = unsafe { libc::getppid() };
    validate_identity(
        target,
        target,
        uid,
        std::process::id(),
        u32::try_from(parent).unwrap_or_default(),
    )?;
    let pid = libc::pid_t::try_from(target.pid).map_err(|_| "PID out of range")?;
    // SAFETY: pidfd_open has two scalar arguments; the returned owned fd is
    // closed on every path, including failed checks. No signal is sent here.
    let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0u32) };
    if raw < 0 {
        return Err(format!(
            "pidfd_open unavailable; refusing unsafe fallback: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: pidfd_open returned a new descriptor owned by this call.
    let pidfd = unsafe { OwnedFd::from_raw_fd(raw as libc::c_int) };
    let observed = ProcessProvider::system()
        .inspect(target.pid)
        .map_err(|e| format!("recheck process: {e}"))?
        .ok_or("process disappeared before confirmation")?;
    validate_identity(
        target,
        &observed,
        uid,
        std::process::id(),
        u32::try_from(parent).unwrap_or_default(),
    )?;
    // SAFETY: the pidfd refers to the revalidated original task. A NULL
    // siginfo pointer and flags=0 are the documented syscall arguments.
    let result = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            pidfd.as_raw_fd(),
            libc::SIGTERM,
            std::ptr::null::<libc::siginfo_t>(),
            0u32,
        )
    };
    if result < 0 {
        return Err(format!(
            "pidfd_send_signal failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}
fn run_live(config: ShellConfig, args: Args) -> Result<(), String> {
    let mut backend = WaylandBackend::connect().map_err(|e| format!("Wayland: {e}"))?;
    if !backend.capabilities().layer_shell {
        return Err("layer-shell not supported".into());
    }
    let mut provider = ProcessProvider::system();
    let mut data = load_data(&mut provider, args.fixture)?;
    let mut sort = SortMode::Cpu;
    let mut query = String::new();
    let mut confirmation: Option<ProcessEntry> = None;
    let mut message = String::new();
    let mut model = build_model(&data, &query, sort, confirmation.as_ref(), &message);
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
            namespace: "nuraloumi-tasks-dismiss".into(),
        })
        .ok();
    let surface = backend
        .create_menu(WaylandMenuConfig {
            width,
            height,
            margin_top: config.panel_height as i32,
            margin_left: 0,
            output: output.as_ref().map(|o| o.id),
            namespace: "nuraloumi-tasks".into(),
        })
        .map_err(|e| format!("create menu: {e}"))?;
    let renderer = CairoRenderer::default();
    let tokens = match config.theme {
        ShellTheme::Dark => &DARK_THEME,
        ShellTheme::Light => &LIGHT_THEME,
    };
    let mut geometry: Option<(u32, u32, i32)> = None;
    let mut scene: Option<Scene> = None;
    let mut pointer_armed: Option<MenuAction> = None;
    let mut touch_armed: Option<(i32, MenuAction)> = None;
    let mut active_touches = BTreeSet::new();
    let mut multitouch_cancelled = false;
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
                    Key::Enter => {
                        action = selected_action(&model, &state);
                    }
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
                        confirmation = None;
                        redraw = true
                    }
                    Key::Text(ref t) if t == "r" && confirmation.is_none() => {
                        data = load_data(&mut provider, args.fixture)?;
                        redraw = true
                    }
                    Key::Text(ref t) if t == "s" && confirmation.is_none() => {
                        sort = sort.next();
                        redraw = true
                    }
                    Key::Raw(111) => {
                        if let Some(MenuAction::Custom { kind, payload }) =
                            selected_action(&model, &state)
                        {
                            if kind == "select" {
                                confirmation = payload.parse::<u32>().ok().and_then(|pid| {
                                    data.processes.iter().find(|p| p.pid == pid).cloned()
                                });
                            }
                            redraw = true
                        }
                    }
                    Key::Text(ref t) if t == "k" => {
                        if let Some(MenuAction::Custom { kind, payload }) =
                            selected_action(&model, &state)
                        {
                            if kind == "select" {
                                confirmation = payload.parse::<u32>().ok().and_then(|pid| {
                                    data.processes.iter().find(|p| p.pid == pid).cloned()
                                });
                            }
                            redraw = true
                        }
                    }

                    Key::Text(ref t) if !t.is_empty() && t.chars().all(|c| !c.is_control()) => {
                        // Bound query allocation and work on every key event.
                        for ch in t
                            .chars()
                            .take(128_usize.saturating_sub(query.chars().count()))
                        {
                            query.push(ch);
                        }
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
                } => {
                    pointer_armed = hit_action(&scene, &model, x, y);
                }
                WaylandEvent::PointerButton {
                    x,
                    y,
                    pressed: false,
                    ..
                } => {
                    action = release_action(pointer_armed.take(), hit_action(&scene, &model, x, y));
                }
                WaylandEvent::TouchDown { id, x, y } => {
                    active_touches.insert(id);
                    // Never re-arm an action while another finger remains down.
                    if active_touches.len() != 1 || multitouch_cancelled {
                        multitouch_cancelled = true;
                        touch_armed = None;
                    } else {
                        touch_armed = hit_action(&scene, &model, x, y).map(|a| (id, a));
                    }
                }
                WaylandEvent::TouchMotion { id, x, y } => {
                    if touch_armed.as_ref().is_some_and(|(active, pressed)| {
                        *active == id && hit_action(&scene, &model, x, y).as_ref() != Some(pressed)
                    }) {
                        touch_armed = None;
                    }
                }
                WaylandEvent::TouchUp { id } => {
                    if !multitouch_cancelled
                        && touch_armed
                            .as_ref()
                            .is_some_and(|(active, _)| *active == id)
                    {
                        action = touch_armed.take().map(|(_, action)| action);
                    }
                    active_touches.remove(&id);
                    if active_touches.is_empty() {
                        touch_armed = None;
                        multitouch_cancelled = false;
                    }
                }
                WaylandEvent::TouchCancel { .. } => {
                    touch_armed = None;
                    active_touches.clear();
                    multitouch_cancelled = false;
                }
                WaylandEvent::Close => {
                    close_surfaces(&mut backend, surface, dismiss)?;
                    return Ok(());
                }
                _ => {}
            }
            if let Some(a) = action {
                match a {
                    MenuAction::Custom { kind, payload } if kind == "confirm" => {
                        // Payload is presentation data, never authorization.
                        // A previously selected immutable process is required.
                        if let Some(target) = confirmation
                            .take()
                            .filter(|p| payload.is_empty() || payload == p.pid.to_string())
                        {
                            message = match terminate(&target, args.fixture) {
                                Ok(()) => format!("SIGTERM sent to {}", target.pid),
                                Err(e) => format!("SIGTERM failed: {e}"),
                            };
                            if args.fixture && message.starts_with("SIGTERM sent") {
                                data.processes.retain(|process| process.pid != target.pid);
                            } else {
                                data = load_data(&mut provider, args.fixture)?;
                            }
                        }
                        confirmation = None;
                    }
                    MenuAction::Custom { kind, payload } if kind == "select" => {
                        confirmation = payload
                            .parse::<u32>()
                            .ok()
                            .and_then(|pid| data.processes.iter().find(|p| p.pid == pid).cloned())
                    }
                    _ => {}
                };
                redraw = true;
            }
            if redraw {
                model = build_model(&data, &query, sort, confirmation.as_ref(), &message);
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
                // Confirmation must be explicitly focused, rather than
                // allowing Enter on an unrelated selected process row.
                if confirmation.is_some() {
                    state.selected_id = Some("confirm".to_owned());
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
    fn fixture_sort_and_filter() {
        let d = demo();
        let cpu = ordered(&d, "", SortMode::Cpu);
        assert_eq!(cpu[0].pid, 102);
        assert_eq!(ordered(&d, "", SortMode::Memory)[0].pid, 102);
        assert_eq!(ordered(&d, "waybar", SortMode::Name).len(), 1);
        assert_eq!(ordered(&d, "", SortMode::Pid)[0].pid, 100);
    }
    #[test]
    fn kill_is_confirmed_and_fixture_is_safe() {
        let d = demo();
        let m = build_model(&d, "", SortMode::Cpu, d.processes.get(1), "");
        assert!(m.item_by_id("confirm").is_some());
        assert!(terminate(&d.processes[1], true).is_ok());
        let mut after = d;
        after.processes.retain(|process| process.pid != 101);
        assert!(ordered(&after, "", SortMode::Cpu)
            .iter()
            .all(|process| process.pid != 101));
    }

    #[test]
    fn security_rejects_pid_reuse_owner_changes_unknown_identity_and_session_processes() {
        let mut original = demo().processes[2].clone();
        let mut observed = original.clone();
        assert!(validate_identity(&original, &observed, 1000, 999, 998).is_ok());
        observed.start_time_ticks = Some(9999);
        assert!(validate_identity(&original, &observed, 1000, 999, 998).is_err());
        observed = original.clone();
        observed.uid = Some(1001);
        assert!(validate_identity(&original, &observed, 1000, 999, 998).is_err());
        observed = original.clone();
        original.uid = None;
        assert!(validate_identity(&original, &observed, 1000, 999, 998).is_err());
        original = observed.clone();
        assert!(validate_identity(&original, &observed, 0, 999, 998).is_err());
        assert!(validate_identity(&original, &observed, 1000, 102, 998).is_err());
        let protected = demo().processes[0].clone();
        assert!(validate_identity(&protected, &protected, 1000, 999, 998).is_err());
        assert!(protected_process("sl101-sway"));
        assert!(protected_process("nuraloumi-pane"));
    }

    #[test]
    fn release_requires_same_armed_action_and_cancel_is_noop() {
        let select = MenuAction::Custom {
            kind: "select".into(),
            payload: "102".into(),
        };
        let confirm = MenuAction::Custom {
            kind: "confirm".into(),
            payload: "102".into(),
        };
        assert_eq!(
            release_action(Some(select.clone()), Some(select.clone())),
            Some(select.clone())
        );
        assert_eq!(release_action(Some(confirm.clone()), Some(select)), None);
        assert_eq!(release_action(None, Some(confirm.clone())), None);
        assert_eq!(release_action(Some(confirm), None), None);
    }
}
