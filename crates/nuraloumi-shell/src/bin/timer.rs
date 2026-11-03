//! Native monotonic stopwatch/countdown on Cairo/wl_shm. No daemon or alternate renderer.
#[path = "../timer.rs"]
mod timer;
use nuraloumi_core::{MenuAction, MenuItem, MenuModel, MenuState, DARK_THEME, LIGHT_THEME};
use nuraloumi_providers::command::{
    CommandLimits, CommandRunner, CommandSpec, SystemCommandRunner,
};
use nuraloumi_render_cairo::{CairoRenderer, RenderOptions, Scene, Viewport};
use nuraloumi_shell::{load_config_or_default, render_menu_follow_selection, ShellConfig, Theme};
use nuraloumi_wayland::{
    BackendError, Frame, Key, MenuConfig, PixelFormat, PlatformEvent, SurfaceId, WaylandBackend,
};
use std::{env, io, path::PathBuf, process::ExitCode, time::Duration};
use timer::{formatted, Clock, Kind, MonotonicClock, Timer, ACTIVE_TICK};

#[derive(Debug, Default)]
struct Options {
    config: Option<PathBuf>,
    fixture: bool,
    headless: bool,
    no_notify: bool,
    seconds: Option<u64>,
}
fn options(args: impl IntoIterator<Item = String>) -> Result<Option<Options>, String> {
    let mut o = Options::default();
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("nuraloumi-timer [--seconds 1..86400] [--fixture] [--headless] [--no-notify] [--config PATH]\nSpace/Enter: start/pause, R: reset, L: lap, Escape/Q: close.\nCountdown completion sends one optional bounded notify-send via provider CommandRunner. No idle repaint.");
                return Ok(None);
            }
            "--seconds" => {
                let secs = it
                    .next()
                    .ok_or("--seconds requires positive integer")?
                    .parse::<u64>()
                    .map_err(|_| "invalid --seconds")?;
                if !(1..=86400).contains(&secs) {
                    return Err("--seconds must be 1..86400".into());
                }
                o.seconds = Some(secs);
            }
            "--fixture" => o.fixture = true,
            "--headless" => o.headless = true,
            "--no-notify" => o.no_notify = true,
            "--config" => o.config = Some(it.next().ok_or("--config needs PATH")?.into()),
            _ => return Err(format!("unknown option: {arg}")),
        }
    }
    Ok(Some(o))
}
fn action(kind: &str) -> MenuAction {
    MenuAction::Custom {
        kind: kind.into(),
        payload: String::new(),
    }
}
struct Surface<C: Clock> {
    timer: Timer<C>,
    message: String,
    fixture: bool,
    notify: bool,
}
impl<C: Clock> Surface<C> {
    fn new(timer: Timer<C>, fixture: bool, notify: bool) -> Self {
        Self {
            timer,
            message: "Use Space to start or pause".into(),
            fixture,
            notify,
        }
    }
    fn model(&self) -> MenuModel {
        let snap = self.timer.snapshot();
        let display = match snap.remaining {
            Some(d) => formatted(d),
            None => formatted(snap.elapsed),
        };
        let mut items = vec![
            MenuItem::status("elapsed", display).with_subtitle(match self.timer.kind() {
                Kind::Stopwatch => "Stopwatch",
                Kind::Countdown(_) => "Countdown",
            }),
            MenuItem::status(
                "state",
                if snap.completed {
                    "Complete"
                } else if snap.running {
                    "Running"
                } else {
                    "Paused"
                },
            )
            .with_subtitle(self.message.clone()),
            MenuItem::action(
                "toggle",
                if snap.running {
                    "Pause [Space]"
                } else {
                    "Start [Space]"
                },
                action("toggle"),
            ),
            MenuItem::action("reset", "Reset [R]", action("reset")),
            MenuItem::action("lap", "Record lap [L]", action("lap")),
        ];
        for (i, d) in self
            .timer
            .laps
            .iter()
            .rev()
            .enumerate()
            .take(timer::MAX_LAPS)
        {
            items.push(MenuItem::status(
                format!("lap-{i}"),
                format!("Lap {} · {}", self.timer.laps.len() - i, formatted(*d)),
            ));
        }
        items.push(MenuItem::action("close", "Close [Esc]", action("close")));
        MenuModel::new("timer", "Timer & Stopwatch", items)
    }
    fn activate(&mut self, action: MenuAction) -> bool {
        let MenuAction::Custom { kind, .. } = action else {
            return false;
        };
        match kind.as_str() {
            "toggle" => {
                if self.timer.running() {
                    self.timer.pause();
                    self.message = "Paused".into();
                } else if self.timer.start() {
                    self.message = "Running".into();
                } else {
                    self.message = "Completed: reset before restarting".into();
                }
            }
            "reset" => {
                self.timer.reset();
                self.message = "Reset".into();
            }
            "lap" => {
                if self.timer.lap() {
                    self.message = "Lap saved".into();
                } else {
                    self.message = "Laps require running timer and <=32 entries".into();
                }
            }
            "close" => return true,
            _ => {}
        }
        false
    }
    fn tick<R: CommandRunner>(&mut self, runner: &mut R) -> Result<bool, String> {
        let event = self.timer.tick();
        if event.completed_now {
            self.message = "Countdown completed".into();
        }
        if self.timer.take_completion() && self.notify && !self.fixture {
            if let Err(e) = send_completion(runner) {
                self.message = format!("Completed (notification unavailable: {e})");
            }
        }
        Ok(event.redraw)
    }
}
fn send_completion<R: CommandRunner>(runner: &mut R) -> Result<(), String> {
    let result = runner
        .run(
            &CommandSpec::new("notify-send").args(["--app-name", "NuraLoumi", "Timer finished"]),
            CommandLimits::new(Duration::from_secs(2), 1024),
        )
        .map_err(|e| e.to_string())?;
    if result.status != 0 {
        return Err(format!("notify-send exited {}", result.status));
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
fn draw(
    backend: Option<(&mut WaylandBackend, SurfaceId)>,
    renderer: &CairoRenderer,
    config: &ShellConfig,
    model: &MenuModel,
    state: &MenuState,
    w: u32,
    h: u32,
    s: i32,
) -> Result<Scene, String> {
    if !(240..=2048).contains(&w)
        || !(160..=2048).contains(&h)
        || !(1..=3).contains(&s)
        || u64::from(w) * u64::from(h) * (s as u64).pow(2) * 4 > 12 * 1024 * 1024
    {
        return Err("viewport too large or invalid".into());
    }
    let theme = match config.theme {
        Theme::Dark => &DARK_THEME,
        Theme::Light => &LIGHT_THEME,
    };
    let (scene, mut buffer) = render_menu_follow_selection(
        renderer,
        model,
        state,
        Viewport::new(f64::from(w), f64::from(h), f64::from(s)),
        theme,
        RenderOptions::default(),
        None,
    )?;
    if let Some((backend, id)) = backend {
        let info = buffer.info();
        let result = buffer
            .with_argb32_bytes(|pixels, _| {
                backend.present(
                    id,
                    Frame {
                        width: info.width as u32,
                        height: info.height as u32,
                        stride: info.stride as usize,
                        format: PixelFormat::Argb8888,
                        pixels,
                    },
                )
            })
            .map_err(|e| e.to_string())?;
        match result {
            Ok(()) | Err(BackendError::WouldBlock) => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(scene)
}
#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}
unsafe extern "C" {
    fn poll(fds: *mut PollFd, nfds: usize, timeout: i32) -> i32;
}
fn dispatch(backend: &mut WaylandBackend, active: bool) -> Result<(), String> {
    if !active {
        return backend
            .blocking_dispatch()
            .map(|_| ())
            .map_err(|e| e.to_string());
    }
    // SAFETY: poll receives a valid stack-allocated pollfd, nfds=1, and its
    // descriptor is owned for this call by the live Wayland backend.
    let mut slot = PollFd {
        fd: backend.poll_fd(),
        events: 1,
        revents: 0,
    };
    backend.flush().map_err(|e| e.to_string())?;
    let rc = unsafe { poll(&mut slot as *mut PollFd, 1, ACTIVE_TICK.as_millis() as i32) };
    if rc < 0 {
        let e = io::Error::last_os_error();
        if e.kind() == io::ErrorKind::Interrupted {
            return Ok(());
        }
        return Err(format!("Wayland poll: {e}"));
    }
    if rc > 0 {
        backend.blocking_dispatch().map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn hit(scene: &Option<Scene>, menu: &MenuModel, x: f64, y: f64) -> Option<MenuAction> {
    menu.item_by_id(scene.as_ref()?.hit_test(x, y)?)?
        .action
        .clone()
}
fn live(config: ShellConfig, mut ui: Surface<MonotonicClock>) -> Result<(), String> {
    let mut backend = WaylandBackend::connect().map_err(|e| e.to_string())?;
    if !backend.capabilities().layer_shell {
        return Err("layer-shell not advertised".into());
    }
    let output = backend.outputs().into_iter().next();
    let id = backend
        .create_menu(MenuConfig {
            width: config.menu_width.clamp(320, 720),
            height: 540,
            margin_top: config.panel_height as i32,
            margin_left: 0,
            output: output.as_ref().map(|o| o.id),
            namespace: "nuraloumi-timer".into(),
        })
        .map_err(|e| e.to_string())?;
    let renderer = CairoRenderer::default();
    let mut geom = None;
    let mut menu = ui.model();
    let mut scene = None;
    let mut touch: Option<(i32, MenuAction)> = None;
    let mut pointer: Option<MenuAction> = None;
    let mut runner = SystemCommandRunner;
    loop {
        dispatch(&mut backend, ui.timer.running())?;
        let mut redraw = ui.tick(&mut runner)?;
        for event in backend.drain_events().collect::<Vec<_>>() {
            if event.surface.is_some() && event.surface != Some(id) {
                continue;
            }
            let mut requested = None;
            match event.event {
                PlatformEvent::Configure {
                    width,
                    height,
                    scale,
                } => {
                    geom = Some((width, height, scale));
                    redraw = true;
                }
                PlatformEvent::Close
                | PlatformEvent::Key {
                    key: Key::Escape,
                    pressed: true,
                } => {
                    let _ = backend.destroy_surface(id);
                    return Ok(());
                }
                PlatformEvent::Key { pressed: true, key } => match key {
                    Key::Enter | Key::Space => requested = Some(action("toggle")),
                    Key::Text(t) if t == "r" || t == "R" => requested = Some(action("reset")),
                    Key::Text(t) if t == "l" || t == "L" => requested = Some(action("lap")),
                    Key::Text(t) if t == "q" => {
                        let _ = backend.destroy_surface(id);
                        return Ok(());
                    }
                    _ => {}
                },
                PlatformEvent::PointerButton {
                    pressed: true,
                    x,
                    y,
                    button: 0x110,
                } => pointer = hit(&scene, &menu, x, y),
                PlatformEvent::PointerButton {
                    pressed: false,
                    x,
                    y,
                    button: 0x110,
                } => {
                    if pointer
                        .as_ref()
                        .is_some_and(|p| hit(&scene, &menu, x, y).as_ref() == Some(p))
                    {
                        requested = pointer.take();
                    } else {
                        pointer = None;
                    }
                }
                PlatformEvent::TouchDown { id: tid, x, y } => {
                    if touch.is_none() {
                        touch = hit(&scene, &menu, x, y).map(|a| (tid, a));
                    }
                }
                PlatformEvent::TouchMotion { id: tid, x, y } => {
                    if touch.as_ref().is_some_and(|(id, a)| {
                        *id == tid && hit(&scene, &menu, x, y).as_ref() != Some(a)
                    }) {
                        touch = None;
                    }
                }
                PlatformEvent::TouchUp { id: tid } => {
                    if touch.as_ref().is_some_and(|(id, _)| *id == tid) {
                        requested = touch.take().map(|(_, a)| a);
                    }
                }
                PlatformEvent::TouchCancel { .. } => touch = None,
                _ => {}
            }
            if let Some(a) = requested {
                if ui.activate(a) {
                    let _ = backend.destroy_surface(id);
                    return Ok(());
                }
                redraw = true;
            }
        }
        if redraw {
            menu = ui.model();
            let state = MenuState::new(&menu);
            if let Some((w, h, s)) = geom {
                scene = Some(draw(
                    Some((&mut backend, id)),
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
fn run(opts: Options) -> Result<(), String> {
    let config = load_config_or_default(opts.config.as_deref())?;
    let kind = opts
        .seconds
        .map_or(Kind::Stopwatch, |n| Kind::Countdown(Duration::from_secs(n)));
    let ui = Surface::new(
        Timer::new(MonotonicClock::default(), kind)?,
        opts.fixture,
        !opts.no_notify,
    );
    if opts.headless {
        let menu = ui.model();
        menu.validate().map_err(|e| e.to_string())?;
        let scene = draw(
            None,
            &CairoRenderer::default(),
            &config,
            &menu,
            &MenuState::new(&menu),
            480,
            540,
            1,
        )?;
        println!(
            "Monotonic Cairo timer fixture: {} interactive targets",
            scene.hits.iter().filter(|h| h.actionable).count()
        );
        Ok(())
    } else {
        live(config, ui)
    }
}
fn main() -> ExitCode {
    match options(env::args().skip(1)).and_then(|o| o.map(run).transpose()) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("nuraloumi-timer: {e}");
            ExitCode::FAILURE
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use nuraloumi_providers::command::{CommandOutput, FixtureCommandRunner};
    #[test]
    fn fixture_cairo_has_48px_controls() {
        let ui = Surface::new(
            Timer::new(MonotonicClock::default(), Kind::Stopwatch).unwrap(),
            true,
            false,
        );
        let menu = ui.model();
        let state = MenuState::new(&menu);
        let scene = draw(
            None,
            &CairoRenderer::default(),
            &ShellConfig::default(),
            &menu,
            &state,
            480,
            540,
            1,
        )
        .unwrap();
        assert!(scene.hits.iter().any(|h| h.item_id == "toggle"));
        for h in scene.hits.iter().filter(|h| h.actionable) {
            assert!(h.rect.height >= 48.0, "{}: {}", h.item_id, h.rect.height);
        }
    }
    #[test]
    fn completion_notification_argv_is_exact() {
        let mut runner = FixtureCommandRunner::default();
        runner.insert(
            CommandSpec::new("notify-send").args(["--app-name", "NuraLoumi", "Timer finished"]),
            CommandOutput {
                status: 0,
                stdout: String::new(),
                stderr: String::new(),
            },
        );
        send_completion(&mut runner).unwrap();
    }
    #[test]
    fn fixture_cli_and_no_autostart() {
        let opts = options(vec![
            "--fixture".into(),
            "--headless".into(),
            "--seconds".into(),
            "90".into(),
        ])
        .unwrap()
        .unwrap();
        run(opts).unwrap();
        assert!(options(vec!["--seconds".into(), "86401".into()]).is_err());
    }
}
