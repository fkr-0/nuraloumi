//! Safe quick browser for Home and removable-media roots, using Cairo/wl_shm.
#[path = "../file_manager.rs"]
mod file_manager;

use file_manager::{application_roots, valid_name, Browser, Capabilities};
use nuraloumi_core::{
    MenuAction, MenuItem, MenuModel, MenuState, SemanticInput, DARK_THEME, LIGHT_THEME,
};
use nuraloumi_providers::command::SystemCommandRunner;
use nuraloumi_render_cairo::{CairoRenderer, RenderOptions, Scene, Viewport};
use nuraloumi_shell::{load_config_or_default, render_menu_follow_selection, ShellConfig, Theme};
use nuraloumi_wayland::{
    BackendError, Frame, Key, MenuConfig, PixelFormat, PlatformEvent, SurfaceId, WaylandBackend,
};
use std::{env, path::PathBuf, process::ExitCode};

const HELP: &str = "nuraloumi-files [--live] [--config PATH] [--allow-rename] [--allow-delete] [--open-with DESKTOP_ID] [ROOT]\n\
Default read-only. Select files/dirs, Enter opens; R rename, D delete (requires capability and two confirmations); O open-with discovered desktop ID; Esc cancel/exit. No recursive deletion.\n";

#[derive(Debug)]
struct Options {
    live: bool,
    config: Option<PathBuf>,
    root: PathBuf,
    capabilities: Capabilities,
    open_with: Option<String>,
}
fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Option<Options>, String> {
    let mut out = Options {
        live: false,
        config: None,
        root: env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/")),
        capabilities: Capabilities::default(),
        open_with: None,
    };
    let mut iter = args.into_iter();
    let mut root_seen = false;
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(None);
            }
            "--live" => out.live = true,
            "--config" => out.config = Some(iter.next().ok_or("--config needs PATH")?.into()),
            "--allow-rename" => out.capabilities.rename = true,
            "--allow-delete" => out.capabilities.delete = true,
            "--open-with" => out.open_with = Some(iter.next().ok_or("--open-with needs ID")?),
            other if !other.starts_with('-') && !root_seen => {
                out.root = PathBuf::from(other);
                root_seen = true;
            }
            _ => return Err(format!("unknown or repeated argument {arg}")),
        }
    }
    if let Some(id) = &out.open_with {
        if !file_manager::valid_desktop_id(id) {
            return Err("unsafe .desktop ID".into());
        }
    }
    Ok(Some(out))
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Mode {
    Normal,
    RenameInput {
        index: usize,
        old: String,
        name: String,
    },
    RenameConfirm {
        index: usize,
        old: String,
        name: String,
    },
    DeleteConfirm {
        index: usize,
        old: String,
    },
}
struct Ui {
    browser: Browser,
    selected: Option<usize>,
    mode: Mode,
    app: Option<String>,
}
fn action(kind: &str, payload: impl Into<String>) -> MenuAction {
    MenuAction::Custom {
        kind: kind.into(),
        payload: payload.into(),
    }
}
impl Ui {
    fn new(options: &Options) -> Result<Self, String> {
        let browser = Browser::new(&options.root, options.capabilities)?;
        Ok(Self {
            selected: (!browser.entries.is_empty()).then_some(0),
            browser,
            mode: Mode::Normal,
            app: options.open_with.clone(),
        })
    }
    fn selected_name(&self) -> Option<String> {
        self.selected
            .and_then(|i| self.browser.entries.get(i))
            .map(|e| e.name.clone())
    }
    fn menu(&self) -> MenuModel {
        let mut items = vec![
            MenuItem::status("path", self.browser.current.display().to_string())
                .with_subtitle(self.browser.message.clone()),
            MenuItem::action("parent", ".. / parent directory", action("parent", "")),
        ];
        for (i, e) in self.browser.entries.iter().enumerate() {
            let suffix = if e.directory { "/" } else { "" };
            items.push(
                MenuItem::action(
                    format!("entry-{i}"),
                    format!("{}{suffix}", e.name),
                    action("select", i.to_string()),
                )
                .with_subtitle(if e.directory {
                    "Directory".into()
                } else {
                    format!("{} bytes", e.len)
                }),
            );
        }
        if self.browser.entries.is_empty() {
            items.push(MenuItem::status("empty", "No visible files"));
        }
        let label = self.selected_name().unwrap_or_else(|| "(none)".into());
        items.push(MenuItem::status("focus", format!("Selected: {label}")));
        items.push(MenuItem::action(
            "open",
            "Enter / Open selected",
            action("open", ""),
        ));
        items.push(MenuItem::action(
            "rename",
            "R / Rename selected",
            action("rename", ""),
        ));
        items.push(MenuItem::action(
            "delete",
            "D / Delete selected",
            action("delete", ""),
        ));
        if let Some(id) = &self.app {
            items.push(MenuItem::action(
                "open-with",
                format!("O / Open with {id}"),
                action("open-with", ""),
            ));
        }
        match &self.mode {
            Mode::Normal => {}
            Mode::RenameInput { old, name, .. } => {
                items.push(MenuItem::status(
                    "input",
                    format!("Rename {old} to: {name}_"),
                ));
                items.push(MenuItem::action(
                    "rename-next",
                    "Confirm typed name",
                    action("rename-next", ""),
                ));
                items.push(MenuItem::action(
                    "cancel",
                    "Cancel rename",
                    action("cancel", ""),
                ));
            }
            Mode::RenameConfirm { old, name, .. } => {
                items.push(MenuItem::status(
                    "warning",
                    format!("Rename {old} → {name}?"),
                ));
                items.push(MenuItem::action(
                    "confirm",
                    "Confirm rename (second Enter)",
                    action("confirm", ""),
                ));
                items.push(MenuItem::action(
                    "cancel",
                    "Cancel rename",
                    action("cancel", ""),
                ));
            }
            Mode::DeleteConfirm { old, .. } => {
                items.push(MenuItem::status(
                    "warning",
                    format!("Delete {old}? This cannot be undone."),
                ));
                items.push(MenuItem::action(
                    "confirm",
                    "Confirm deletion (second Enter)",
                    action("confirm", ""),
                ));
                items.push(MenuItem::action(
                    "cancel",
                    "Cancel deletion",
                    action("cancel", ""),
                ));
            }
        }
        MenuModel::new("files", "File browser", items)
    }
    fn activate(&mut self, requested: MenuAction) -> Result<(), String> {
        let MenuAction::Custom { kind, payload } = requested else {
            return Ok(());
        };
        match kind.as_str() {
            "cancel" => self.mode = Mode::Normal,
            "select" => {
                let Ok(index) = payload.parse::<usize>() else {
                    return Ok(());
                };
                if index < self.browser.entries.len() {
                    self.selected = Some(index);
                    if self.mode == Mode::Normal {
                        self.enter_selected()?;
                    }
                }
            }
            "parent" if self.mode == Mode::Normal => {
                self.browser.parent()?;
                self.selected = (!self.browser.entries.is_empty()).then_some(0);
            }
            "open" if self.mode == Mode::Normal => self.enter_selected()?,
            "rename" if self.mode == Mode::Normal => self.begin_rename()?,
            "delete" if self.mode == Mode::Normal => self.begin_delete()?,
            "open-with" if self.mode == Mode::Normal => self.open_with()?,
            "rename-next" => self.confirm_name()?,
            "confirm" => self.confirm()?,
            _ => {}
        }
        Ok(())
    }
    fn enter_selected(&mut self) -> Result<(), String> {
        let Some(index) = self.selected else {
            return Ok(());
        };
        if self.browser.entries.get(index).is_none() {
            return Ok(());
        }
        if self.browser.enter(index)? {
            self.selected = (!self.browser.entries.is_empty()).then_some(0);
        } else {
            self.open_with()?;
        }
        Ok(())
    }
    fn open_with(&mut self) -> Result<(), String> {
        let Some(index) = self.selected else {
            return Ok(());
        };
        let Some(id) = self.app.as_deref() else {
            self.browser.message = "Use --open-with validated DESKTOP_ID to open a file".into();
            return Ok(());
        };
        self.browser
            .open_with(index, id, application_roots(), &mut SystemCommandRunner)?;
        self.browser.message = format!("Requested {id} for selected file");
        Ok(())
    }
    fn begin_rename(&mut self) -> Result<(), String> {
        if !self.browser.capabilities.rename {
            return Err("rename disabled; use --allow-rename".into());
        }
        let Some(index) = self.selected else {
            return Ok(());
        };
        let Some(old) = self.selected_name() else {
            return Ok(());
        };
        self.mode = Mode::RenameInput {
            index,
            old,
            name: String::new(),
        };
        Ok(())
    }
    fn begin_delete(&mut self) -> Result<(), String> {
        if !self.browser.capabilities.delete {
            return Err("delete disabled; use --allow-delete".into());
        }
        let Some(index) = self.selected else {
            return Ok(());
        };
        let Some(old) = self.selected_name() else {
            return Ok(());
        };
        self.mode = Mode::DeleteConfirm { index, old };
        Ok(())
    }
    fn confirm_name(&mut self) -> Result<(), String> {
        if let Mode::RenameInput { index, old, name } = &self.mode {
            if !valid_name(name) {
                return Err("invalid new filename".into());
            }
            self.mode = Mode::RenameConfirm {
                index: *index,
                old: old.clone(),
                name: name.clone(),
            };
        }
        Ok(())
    }
    fn confirm(&mut self) -> Result<(), String> {
        match self.mode.clone() {
            Mode::RenameConfirm { index, old, name } => {
                if self
                    .browser
                    .entries
                    .get(index)
                    .is_none_or(|e| e.name != old)
                {
                    return Err("rename selection changed".into());
                }
                self.browser.rename(index, &name, true)?;
                self.selected = (!self.browser.entries.is_empty())
                    .then_some(index.min(self.browser.entries.len() - 1));
            }
            Mode::DeleteConfirm { index, old } => {
                self.browser.delete(index, &old, true)?;
                self.selected = (!self.browser.entries.is_empty())
                    .then_some(index.min(self.browser.entries.len() - 1));
            }
            _ => return Ok(()),
        }
        self.mode = Mode::Normal;
        Ok(())
    }
    fn text(&mut self, t: &str) {
        if let Mode::RenameInput { name, .. } = &mut self.mode {
            if !t.chars().any(char::is_control)
                && name.len() + t.len() <= file_manager::MAX_FILENAME_BYTES
            {
                name.push_str(t);
            }
        }
    }
    fn backspace(&mut self) {
        if let Mode::RenameInput { name, .. } = &mut self.mode {
            name.pop();
        }
    }
    fn state(&self, menu: &MenuModel, selected_id: Option<String>) -> MenuState {
        let mut state = MenuState::new(menu);
        let prefer = selected_id.or_else(|| self.selected.map(|i| format!("entry-{i}")));
        if let Some(id) = prefer {
            if menu.item_by_id(&id).is_some_and(|m| m.action.is_some()) {
                state.selected_id = Some(id);
            }
        }
        state
    }
}
#[allow(clippy::too_many_arguments)]
fn render(
    backend: &mut WaylandBackend,
    id: SurfaceId,
    renderer: &CairoRenderer,
    config: &ShellConfig,
    menu: &MenuModel,
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
        return Err("unsafe viewport dimensions".into());
    }
    let tokens = match config.theme {
        Theme::Dark => &DARK_THEME,
        Theme::Light => &LIGHT_THEME,
    };
    let (scene, mut buffer) = render_menu_follow_selection(
        renderer,
        menu,
        state,
        Viewport::new(f64::from(w), f64::from(h), f64::from(s)),
        tokens,
        RenderOptions::default(),
        None,
    )?;
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
        Ok(()) | Err(BackendError::WouldBlock) => Ok(scene),
        Err(e) => Err(e.to_string()),
    }
}
fn hit(scene: &Option<Scene>, menu: &MenuModel, x: f64, y: f64) -> Option<MenuAction> {
    menu.item_by_id(scene.as_ref()?.hit_test(x, y)?)?
        .action
        .clone()
}
fn run(opts: Options) -> Result<(), String> {
    let config = load_config_or_default(opts.config.as_deref())?;
    let mut ui = Ui::new(&opts)?;
    if !opts.live {
        println!("{}{}", HELP, ui.browser.current.display());
        return Ok(());
    }
    let mut backend = WaylandBackend::connect().map_err(|e| e.to_string())?;
    if !backend.capabilities().layer_shell {
        return Err("layer-shell unavailable".into());
    }
    let output = backend.outputs().into_iter().next();
    let id = backend
        .create_menu(MenuConfig {
            width: config.menu_width.clamp(320, 720),
            height: 640,
            margin_top: config.panel_height as i32,
            margin_left: 0,
            output: output.as_ref().map(|o| o.id),
            namespace: "nuraloumi-files".into(),
        })
        .map_err(|e| e.to_string())?;
    let renderer = CairoRenderer::default();
    let mut menu = ui.menu();
    let mut state = ui.state(&menu, None);
    let mut geometry = None;
    let mut scene = None;
    let mut touch: Option<(i32, MenuAction)> = None;
    let mut pointer: Option<MenuAction> = None;
    loop {
        backend.blocking_dispatch().map_err(|e| e.to_string())?;
        for incoming in backend.drain_events().collect::<Vec<_>>() {
            if incoming.surface.is_some() && incoming.surface != Some(id) {
                continue;
            }
            let mut redraw = false;
            let mut requested = None;
            match incoming.event {
                PlatformEvent::Configure {
                    width,
                    height,
                    scale,
                } => {
                    geometry = Some((width, height, scale));
                    redraw = true;
                }
                PlatformEvent::Close => {
                    let _ = backend.destroy_surface(id);
                    return Ok(());
                }
                PlatformEvent::Key { pressed: true, key } => match key {
                    Key::Escape if ui.mode != Mode::Normal => {
                        ui.mode = Mode::Normal;
                        redraw = true;
                    }
                    Key::Escape => {
                        let _ = backend.destroy_surface(id);
                        return Ok(());
                    }
                    Key::Enter if matches!(ui.mode, Mode::RenameInput { .. }) => {
                        requested = Some(action("rename-next", ""));
                    }
                    Key::Enter if ui.mode != Mode::Normal => {
                        requested = Some(action("confirm", ""));
                    }
                    Key::Backspace if matches!(ui.mode, Mode::RenameInput { .. }) => {
                        ui.backspace();
                        redraw = true;
                    }
                    Key::Text(t) if matches!(ui.mode, Mode::RenameInput { .. }) => {
                        ui.text(&t);
                        redraw = true;
                    }
                    Key::Up | Key::Down if ui.mode == Mode::Normal => {
                        state.handle(
                            &menu,
                            if matches!(key, Key::Up) {
                                SemanticInput::Up
                            } else {
                                SemanticInput::Down
                            },
                        );
                        if let Some(index) = state
                            .selected_id
                            .as_deref()
                            .and_then(|id| id.strip_prefix("entry-"))
                            .and_then(|i| i.parse::<usize>().ok())
                        {
                            ui.selected = Some(index);
                        }
                        redraw = true;
                    }
                    Key::Enter => {
                        requested = state
                            .selected_id
                            .as_deref()
                            .and_then(|id| menu.item_by_id(id))
                            .and_then(|i| i.action.clone())
                    }
                    Key::Backspace => requested = Some(action("parent", "")),
                    Key::Text(t) if t == "r" || t == "R" => requested = Some(action("rename", "")),
                    Key::Text(t) if t == "d" || t == "D" => requested = Some(action("delete", "")),
                    Key::Text(t) if t == "o" || t == "O" => {
                        requested = Some(action("open-with", ""))
                    }
                    _ => {}
                },
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
                        .is_some_and(|a| hit(&scene, &menu, x, y).as_ref() == Some(a))
                    {
                        requested = pointer.take();
                    } else {
                        pointer = None;
                    }
                }
                _ => {}
            }
            if let Some(a) = requested {
                if let Err(error) = ui.activate(a) {
                    ui.browser.message = format!("Action refused: {error}");
                }
                redraw = true;
            }
            if redraw {
                let selected = state.selected_id.clone();
                menu = ui.menu();
                state = ui.state(&menu, selected);
                if let Some((w, h, s)) = geometry {
                    scene = Some(render(
                        &mut backend,
                        id,
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
    match parse(env::args().skip(1)).and_then(|v| v.map(run).transpose()) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("nuraloumi-files: {e}");
            ExitCode::FAILURE
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capabilities_default_safe() {
        assert!(
            !parse(vec!["--live".into()])
                .unwrap()
                .unwrap()
                .capabilities
                .delete
        );
        assert!(parse(vec!["--open-with".into(), "../../malicious.desktop".into()]).is_err());
    }
}
