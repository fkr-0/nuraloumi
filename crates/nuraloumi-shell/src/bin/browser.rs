//! Native NuraLoumi browser controls — no GTK, EGL, JS injection or TCP.
//!
//! The actual WebKit rendering surface belongs to the separate WPEPlatform
//! engine bundled in components/nura-browser-engine. This software-rendered
//! companion surface uses the existing NuraLoumi Wayland/Cairo contracts.

use nuraloumi_core::{MenuAction, MenuModel, MenuState, SemanticInput};
use nuraloumi_render_cairo::{CairoRenderer, RenderOptions, Scene, Viewport};
use nuraloumi_shell::browser::{
    Bookmark, BrowserRpc, BrowserUi, NativeBrowser, PageStatus, Tab, MAX_BOOKMARK_PAGE, MAX_TABS,
};
use nuraloumi_shell::{load_config_or_default, render_menu_with_glyphs, ShellConfig, Theme};
use nuraloumi_wayland::{
    BackendError, Frame, Key, MenuConfig, PixelFormat, PlatformEvent, SurfaceId, WaylandBackend,
};
use serde_json::{json, Value};
use std::{env, path::PathBuf, process::ExitCode};

#[derive(Default)]
struct Options {
    config: Option<PathBuf>,
    fixture: bool,
    headless: bool,
}

fn parse_options(args: impl IntoIterator<Item = String>) -> Result<Option<Options>, String> {
    let mut options = Options::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!(
                    "nuraloumi-browser [--fixture] [--headless] [--config PATH]\n\
                         Chrome: touch address to edit, Enter to navigate, Esc to dismiss. \
                         Keyboard: Ctrl+L address, Ctrl+T new tab, Ctrl+W close tab."
                );
                return Ok(None);
            }
            "--config" => {
                options.config = Some(args.next().ok_or("--config requires a path")?.into())
            }
            "--fixture" => options.fixture = true,
            "--headless" => options.headless = true,
            _ => return Err(format!("Unknown browser option: {arg}")),
        }
    }
    Ok(Some(options))
}

enum Connection {
    Native(NativeBrowser),
    Fixture(FixtureRpc),
}

impl BrowserRpc for Connection {
    fn request(&mut self, command: &str, argument: Option<&str>) -> Result<Value, String> {
        match self {
            Self::Native(browser) => browser.request(command, argument),
            Self::Fixture(browser) => browser.request(command, argument),
        }
    }
}

struct FixtureRpc {
    tabs: Vec<Tab>,
    active: usize,
    bookmarks: Vec<Bookmark>,
}

impl FixtureRpc {
    fn from_ui(ui: &BrowserUi) -> Self {
        Self {
            tabs: ui.tabs.tabs.clone(),
            active: ui.tabs.active,
            bookmarks: ui.bookmarks.entries.clone(),
        }
    }
}

impl BrowserRpc for FixtureRpc {
    fn request(&mut self, command: &str, argument: Option<&str>) -> Result<Value, String> {
        let tab = &self.tabs[self.active];
        match command {
            "uri" => Ok(json!(PageStatus {
                uri: tab.uri.clone(),
                title: tab.title.clone(),
                can_go_back: true,
                can_go_forward: false,
                is_loading: tab.loading,
                bookmarked: self.bookmarks.iter().any(|item| item.url == tab.uri),
            })),
            "tabs" => Ok(json!({"tabs": self.tabs, "active": self.active})),
            "bookmarks" => {
                let page = argument.unwrap_or("0").parse::<usize>().unwrap_or(0).min(7);
                let offset = page * MAX_BOOKMARK_PAGE;
                let entries: Vec<_> = self
                    .bookmarks
                    .iter()
                    .skip(offset)
                    .take(MAX_BOOKMARK_PAGE)
                    .cloned()
                    .collect();
                Ok(json!({"total":self.bookmarks.len(),"offset":offset,"entries":entries}))
            }
            "open" => {
                let url = argument.ok_or("Fixture: missing address")?;
                self.tabs[self.active].uri = url.into();
                self.tabs[self.active].title = "Fixture page".into();
                Ok(Value::Null)
            }
            "tab-new" => {
                if self.tabs.len() >= MAX_TABS {
                    return Err("Maximum six tabs".into());
                }
                self.tabs.push(Tab::default());
                self.active = self.tabs.len() - 1;
                Ok(Value::Null)
            }
            "tab-select" => {
                let index = argument
                    .unwrap_or("")
                    .parse::<usize>()
                    .map_err(|_| "Invalid fixture tab")?;
                if index >= self.tabs.len() {
                    return Err("Unknown fixture tab".into());
                }
                self.active = index;
                Ok(Value::Null)
            }
            "tab-close" => {
                let index = argument
                    .unwrap_or("")
                    .parse::<usize>()
                    .map_err(|_| "Invalid fixture tab")?;
                if self.tabs.len() < 2 || index >= self.tabs.len() {
                    return Err("Cannot close fixture tab".into());
                }
                self.tabs.remove(index);
                self.active = self.active.min(self.tabs.len() - 1);
                Ok(Value::Null)
            }
            "bookmark-toggle" => {
                let uri = self.tabs[self.active].uri.clone();
                if let Some(index) = self.bookmarks.iter().position(|item| item.url == uri) {
                    self.bookmarks.remove(index);
                } else {
                    self.bookmarks.push(Bookmark {
                        url: uri,
                        title: self.tabs[self.active].title.clone(),
                    });
                }
                Ok(Value::Null)
            }
            "back" | "forward" | "reload" | "stop" => Ok(Value::Null),
            _ => Err("Fixture: unsupported browser action".into()),
        }
    }
}

fn draw(
    backend: &mut WaylandBackend,
    surface: SurfaceId,
    renderer: &CairoRenderer,
    config: &ShellConfig,
    menu: &MenuModel,
    state: &MenuState,
    geometry: (u32, u32, i32),
) -> Result<Scene, String> {
    let (w, h, scale) = geometry;
    let tokens = config
        .palette
        .map(|palette| palette.tokens())
        .unwrap_or(match config.theme {
            Theme::Dark => &nuraloumi_core::DARK_THEME,
            Theme::Light => &nuraloumi_core::LIGHT_THEME,
        });
    let (scene, mut buffer) = render_menu_with_glyphs(
        renderer,
        menu,
        state,
        Viewport::new(f64::from(w), f64::from(h), f64::from(scale.max(1))),
        tokens,
        RenderOptions::default(),
        None,
        None,
    )?;
    let info = buffer.info();
    let result = buffer
        .with_argb32_bytes(|pixels, _| {
            backend.present(
                surface,
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
        Ok(()) | Err(BackendError::WouldBlock) => Ok(scene),
        Err(err) => Err(err.to_string()),
    }
}

fn hit(scene: &Option<Scene>, menu: &MenuModel, x: f64, y: f64) -> Option<MenuAction> {
    let item = menu.item_by_id(scene.as_ref()?.hit_test(x, y)?)?;
    item.is_actionable().then(|| item.action.clone()).flatten()
}

fn selected(menu: &MenuModel, state: &MenuState) -> Option<MenuAction> {
    let item = menu.item_by_id(state.selected_id.as_deref()?)?;
    item.is_actionable().then(|| item.action.clone()).flatten()
}

/// NuraLoumi's global interaction contract: a press is never an activation.
/// Releasing must match the same action; a moved/cancelled touch or a
/// refreshed browser model invalidates the gesture. This prevents a finger
/// dragging across the tab strip from opening/closing the wrong tab.
#[derive(Default)]
struct BrowserPress {
    pointer: Option<MenuAction>,
    touch: Option<(i32, MenuAction)>,
}

impl BrowserPress {
    fn pointer_down(&mut self, action: Option<MenuAction>) {
        self.pointer = action;
    }

    fn pointer_up(&mut self, released: Option<MenuAction>) -> Option<MenuAction> {
        self.pointer
            .take()
            .filter(|pressed| Some(pressed) == released.as_ref())
    }

    fn touch_down(&mut self, id: i32, action: Option<MenuAction>) {
        if self.touch.is_none() {
            self.touch = action.map(|pressed| (id, pressed));
        }
    }

    fn touch_motion(&mut self, id: i32, current: Option<MenuAction>) {
        if self
            .touch
            .as_ref()
            .is_some_and(|(active, pressed)| *active == id && Some(pressed) != current.as_ref())
        {
            self.touch = None;
        }
    }

    fn touch_up(&mut self, id: i32) -> Option<MenuAction> {
        if self.touch.as_ref().is_some_and(|(active, _)| *active == id) {
            return self.touch.take().map(|(_, pressed)| pressed);
        }
        None
    }

    fn touch_cancel(&mut self, ids: &[i32]) {
        if self
            .touch
            .as_ref()
            .is_some_and(|(active, _)| ids.contains(active))
        {
            self.touch = None;
        }
    }

    fn reset(&mut self) {
        self.pointer = None;
        self.touch = None;
    }
}

/// Only poll while WebKit reports an outstanding load; otherwise block on
/// Wayland to avoid idle CPU/wakeups on the Tegra20.
fn dispatch_until_event(backend: &mut WaylandBackend, loading: bool) -> Result<bool, String> {
    if !loading {
        backend.blocking_dispatch().map_err(|e| e.to_string())?;
        return Ok(false);
    }
    backend.flush().map_err(|e| e.to_string())?;
    let mut fd = libc::pollfd {
        fd: backend.poll_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let result = unsafe { libc::poll(&mut fd, 1, 350) };
    if result < 0 {
        let err = std::io::Error::last_os_error();
        if err.kind() != std::io::ErrorKind::Interrupted {
            return Err(format!("Wayland wait failed: {err}"));
        }
    } else if result > 0 {
        if fd.revents & libc::POLLIN != 0 {
            backend.blocking_dispatch().map_err(|e| e.to_string())?;
        } else if fd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err("Wayland session disconnected".into());
        }
    } else {
        backend.dispatch_pending().map_err(|e| e.to_string())?;
    }
    Ok(result == 0)
}

fn run(
    mut ui: BrowserUi,
    mut rpc: Connection,
    config: ShellConfig,
    headless: bool,
) -> Result<(), String> {
    if headless {
        println!(
            "NuraLoumi native browser controls: {} tabs, {} bookmarks",
            ui.tabs.tabs.len(),
            ui.bookmarks.total
        );
        nuraloumi_core::validate_menu(&ui.menu()).map_err(|e| e.to_string())?;
        return Ok(());
    }
    let mut backend = WaylandBackend::connect().map_err(|e| e.to_string())?;
    if !backend.capabilities().layer_shell {
        return Err("Wayland layer-shell is required for browser controls".into());
    }
    backend.blocking_dispatch().map_err(|e| e.to_string())?;
    let output = backend.outputs().into_iter().next();
    let width = config.menu_width.clamp(720, 820);
    let surface = backend
        .create_menu(MenuConfig {
            width,
            height: 312,
            margin_top: config.panel_height as i32,
            margin_left: 0,
            output: output.as_ref().map(|o| o.id),
            namespace: "nuraloumi-browser".into(),
        })
        .map_err(|e| e.to_string())?;
    let renderer = CairoRenderer::default();
    let mut menu = ui.menu();
    let mut state = MenuState::new(&menu);
    let mut scene = None;
    let mut geometry = None;
    let mut press = BrowserPress::default();
    loop {
        let tick = dispatch_until_event(&mut backend, ui.page.is_loading)?;
        let mut redraw = tick;
        if tick {
            if let Err(message) = ui.refresh(&mut rpc) {
                ui.status = message;
            }
        }
        for event in backend.drain_events().collect::<Vec<_>>() {
            if event.surface.is_some() && event.surface != Some(surface) {
                continue;
            }
            let mut action = None;
            match event.event {
                PlatformEvent::Configure {
                    width,
                    height,
                    scale,
                } => {
                    geometry = Some((width, height, scale));
                    redraw = true;
                }
                PlatformEvent::Close => return Ok(()),
                PlatformEvent::Key { key, pressed: true } => {
                    match key {
                        Key::Escape if ui.editing_address => ui.cancel_address(),
                        Key::Escape => return Ok(()),
                        Key::Backspace => ui.backspace(),
                        Key::Enter if ui.editing_address => {
                            if let Err(message) = ui.submit_address(&mut rpc) {
                                ui.status = message;
                            }
                        }
                        Key::Enter => action = selected(&menu, &state),
                        Key::Up => {
                            state.handle(&menu, SemanticInput::Up);
                        }
                        Key::Down | Key::Tab => {
                            state.handle(&menu, SemanticInput::Down);
                        }
                        Key::Modified { key, modifiers } if modifiers.ctrl => match *key {
                            Key::Text(ref text) if text.eq_ignore_ascii_case("l") => {
                                ui.focus_address()
                            }
                            Key::Text(ref text) if text.eq_ignore_ascii_case("t") => {
                                action = menu
                                    .item_by_id("browser.tab.new")
                                    .and_then(|item| item.action.clone());
                            }
                            Key::Text(ref text) if text.eq_ignore_ascii_case("w") => {
                                action = menu
                                    .item_by_id("browser.tab.close")
                                    .and_then(|item| item.action.clone());
                            }
                            _ => {}
                        },
                        Key::Text(text) if ui.editing_address => ui.text_input(&text),
                        Key::Text(text) if text == "l" => ui.focus_address(),
                        Key::Text(text) if text == "q" => return Ok(()),
                        _ => {}
                    }
                    redraw = true;
                }
                PlatformEvent::PointerButton {
                    x,
                    y,
                    pressed: true,
                    button: 0x110,
                } => press.pointer_down(hit(&scene, &menu, x, y)),
                PlatformEvent::PointerButton {
                    x,
                    y,
                    pressed: false,
                    button: 0x110,
                } => action = press.pointer_up(hit(&scene, &menu, x, y)),
                PlatformEvent::PointerLeave => press.pointer = None,
                PlatformEvent::TouchDown { id, x, y } => {
                    press.touch_down(id, hit(&scene, &menu, x, y));
                }
                PlatformEvent::TouchMotion { id, x, y } => {
                    press.touch_motion(id, hit(&scene, &menu, x, y));
                }
                PlatformEvent::TouchUp { id } => action = press.touch_up(id),
                PlatformEvent::TouchCancel { ids } => press.touch_cancel(&ids),
                _ => {}
            }
            if let Some(action) = action {
                if let Err(message) = ui.activate(action, &mut rpc) {
                    ui.status = message;
                }
                redraw = true;
            }
        }
        if redraw {
            // A changed menu/layout can recycle an item ID while a touch is
            // held; never apply the old gesture to that new snapshot.
            press.reset();
            let keep = state.selected_id.clone();
            menu = ui.menu();
            state = MenuState::new(&menu);
            if let Some(id) = keep {
                if menu
                    .item_by_id(&id)
                    .is_some_and(|item| item.enabled && item.action.is_some())
                {
                    state.selected_id = Some(id);
                }
            }
            if let Some(geometry) = geometry {
                scene = Some(draw(
                    &mut backend,
                    surface,
                    &renderer,
                    &config,
                    &menu,
                    &state,
                    geometry,
                )?);
            }
        }
    }
}

fn main() -> ExitCode {
    let args = match parse_options(env::args().skip(1)) {
        Ok(Some(args)) => args,
        Ok(None) => return ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("nuraloumi-browser: {error}");
            return ExitCode::from(2);
        }
    };
    let config = load_config_or_default(args.config.as_deref()).unwrap_or_default();
    let result = if args.fixture {
        let ui = BrowserUi::fixture();
        let rpc = Connection::Fixture(FixtureRpc::from_ui(&ui));
        run(ui, rpc, config, args.headless)
    } else {
        NativeBrowser::from_environment().and_then(|mut client| {
            let mut ui = BrowserUi::default();
            ui.refresh(&mut client)?;
            run(ui, Connection::Native(client), config, args.headless)
        })
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("nuraloumi-browser: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab_action(index: u8) -> MenuAction {
        MenuAction::Custom {
            kind: "browser.tab.select".into(),
            payload: index.to_string(),
        }
    }

    #[test]
    fn browser_pointer_activates_only_on_matching_release() {
        let first = tab_action(0);
        let second = tab_action(1);
        let mut press = BrowserPress::default();
        press.pointer_down(Some(first.clone()));
        assert_eq!(press.pointer_up(Some(second)), None);
        press.pointer_down(Some(first.clone()));
        assert_eq!(press.pointer_up(None), None);
        press.pointer_down(Some(first.clone()));
        assert_eq!(press.pointer_up(Some(first.clone())), Some(first));
        assert_eq!(press.pointer_up(None), None);
    }

    #[test]
    fn browser_touch_must_release_same_finger_without_leaving_action() {
        let first = tab_action(0);
        let second = tab_action(1);
        let mut press = BrowserPress::default();
        press.touch_down(7, Some(first.clone()));
        assert_eq!(press.touch_up(8), None);
        press.touch_motion(7, Some(second));
        assert_eq!(press.touch_up(7), None);

        press.touch_down(7, Some(first.clone()));
        press.touch_cancel(&[8]);
        assert_eq!(press.touch_up(7), Some(first.clone()));
        press.touch_down(7, Some(first.clone()));
        press.touch_cancel(&[7]);
        assert_eq!(press.touch_up(7), None);
        press.touch_down(7, Some(first.clone()));
        press.reset();
        assert_eq!(press.touch_up(7), None);
    }

    #[test]
    fn fixture_allows_actual_tab_switch_and_bookmark_mutation() {
        let mut ui = BrowserUi::fixture();
        let mut rpc = FixtureRpc::from_ui(&ui);
        ui.activate(
            MenuAction::Custom {
                kind: "browser.tab.select".into(),
                payload: "1".into(),
            },
            &mut rpc,
        )
        .unwrap();
        assert_eq!(ui.tabs.active, 1);
        ui.activate(
            MenuAction::Custom {
                kind: "browser.bookmark.toggle".into(),
                payload: String::new(),
            },
            &mut rpc,
        )
        .unwrap();
        assert!(!ui.page.bookmarked);
        ui.activate(
            MenuAction::Custom {
                kind: "browser.tab.new".into(),
                payload: String::new(),
            },
            &mut rpc,
        )
        .unwrap();
        assert_eq!(ui.tabs.tabs.len(), 3);
        ui.activate(
            MenuAction::Custom {
                kind: "browser.tab.close".into(),
                payload: "2".into(),
            },
            &mut rpc,
        )
        .unwrap();
        assert_eq!(ui.tabs.tabs.len(), 2);
    }
}
