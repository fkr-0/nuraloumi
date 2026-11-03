use nuraloumi_core::{
    MenuAction, MenuItem, MenuModel, MenuState, SemanticInput, DARK_THEME, LIGHT_THEME,
};
use nuraloumi_providers::{CommandLimits, CommandRunner, CommandSpec, SystemCommandRunner};
use nuraloumi_render_cairo::{CairoRenderer, RenderOptions, Scene, Viewport};
use nuraloumi_shell::{load_config_or_default, render_menu_with_glyphs, Theme};
use nuraloumi_wayland::{
    BackendError, Frame, Key, MenuConfig, PixelFormat, PlatformEvent, SurfaceId, WaylandBackend,
};
use std::{
    env,
    io::Write,
    path::PathBuf,
    process::{Command, ExitCode, Stdio},
    time::{Duration, Instant},
};

const LIMITS: CommandLimits = CommandLimits::new(Duration::from_secs(2), 64 * 1024);
#[derive(Clone)]
struct Entry {
    id: String,
    preview: String,
    data: String,
    history: bool,
}
fn fetch(runner: &mut impl CommandRunner) -> Result<Vec<Entry>, String> {
    let types = runner
        .run(&CommandSpec::new("wl-paste").arg("--list-types"), LIMITS)
        .map_err(|e| format!("wl-clipboard not installed or unavailable: {e}"))?;
    if types.status != 0 {
        return Ok(Vec::new());
    }
    if !types
        .stdout
        .lines()
        .any(|line| line.starts_with("text/") || line == "UTF8_STRING" || line == "STRING")
    {
        return Ok(Vec::new());
    }
    if let Ok(history) = runner.run(&CommandSpec::new("cliphist").arg("list"), LIMITS) {
        if history.status == 0 {
            let entries = history
                .stdout
                .lines()
                .take(128)
                .filter_map(|line| {
                    let (id, text) = line.split_once('\t')?;
                    if !id.chars().all(|c| c.is_ascii_digit()) {
                        return None;
                    }
                    Some(Entry {
                        id: id.into(),
                        preview: preview(text),
                        data: line.into(),
                        history: true,
                    })
                })
                .collect::<Vec<_>>();
            if !entries.is_empty() {
                return Ok(entries);
            }
        }
    }
    let current = runner
        .run(&CommandSpec::new("wl-paste").arg("--no-newline"), LIMITS)
        .map_err(|e| format!("wl-paste failed: {e}"))?;
    if current.status != 0 || current.stdout.is_empty() {
        return Ok(Vec::new());
    }
    Ok(vec![Entry {
        id: "current".into(),
        preview: preview(&current.stdout),
        data: current.stdout,
        history: false,
    }])
}
fn preview(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(80)
        .collect()
}
fn model(entries: &[Entry], query: &str, error: Option<&str>) -> MenuModel {
    let mut items = vec![MenuItem::status(
        "heading",
        format!("Clipboard History ({})", entries.len()),
    )];
    if let Some(err) = error {
        items.push(MenuItem::status("error", err.to_string()));
    }
    for (i, entry) in entries.iter().enumerate() {
        if !entry.preview.to_lowercase().contains(&query.to_lowercase()) {
            continue;
        }
        items.push(MenuItem::action(
            format!("clip-{i}"),
            entry.preview.clone(),
            MenuAction::Custom {
                kind: "paste".into(),
                payload: i.to_string(),
            },
        ));
    }
    if items.len() == 1 || entries.is_empty() {
        items.push(MenuItem::status("empty", "Clipboard empty"));
    }
    MenuModel::new("clipboard", "Clipboard History", items)
}
fn write_to(program: &str, args: &[&str], data: &[u8]) -> Result<(), String> {
    if data.len() > LIMITS.max_output_bytes {
        return Err("clipboard entry too large".into());
    }
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    if let Some(mut input) = child.stdin.take() {
        input.write_all(data).map_err(|e| e.to_string())?;
    }
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(format!("{program} exited with {status}"))
                }
            }
            Ok(None) if start.elapsed() < LIMITS.timeout => {
                std::thread::sleep(Duration::from_millis(5))
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{program} timed out"));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e.to_string());
            }
        }
    }
}
fn select(entry: &Entry, delete: bool) -> Result<(), String> {
    if entry.history {
        if delete {
            let mut runner = SystemCommandRunner;
            let result = runner
                .run(
                    &CommandSpec::new("cliphist").arg("delete").arg(&entry.id),
                    LIMITS,
                )
                .map_err(|e| e.to_string())?;
            if result.status == 0 {
                Ok(())
            } else {
                Err("cliphist delete failed".into())
            }
        } else {
            // cliphist decode produces bytes; a shell pipeline would be unsafe.
            // Decode with bounded CommandRunner, then copy the exact text bytes.
            let mut runner = SystemCommandRunner;
            let output = runner
                .run(
                    &CommandSpec::new("cliphist").arg("decode").arg(&entry.id),
                    LIMITS,
                )
                .map_err(|e| e.to_string())?;
            if output.status != 0 {
                return Err("cliphist decode failed".into());
            }
            write_to("wl-copy", &[], output.stdout.as_bytes())
        }
    } else if delete {
        Err("clipboard history manager unavailable".into())
    } else {
        write_to("wl-copy", &[], entry.data.as_bytes())
    }
}
#[allow(clippy::too_many_arguments)]
fn present(
    backend: &mut WaylandBackend,
    surface: SurfaceId,
    renderer: &CairoRenderer,
    tokens: &nuraloumi_core::ThemeTokens,
    model: &MenuModel,
    state: &MenuState,
    w: u32,
    h: u32,
    scale: i32,
) -> Result<Scene, String> {
    let viewport = Viewport::new(f64::from(w), f64::from(h), f64::from(scale.max(1)));
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
    .map_err(|e| e.to_string())?;
    let info = buffer.info();
    let pw = u32::try_from(info.width).map_err(|e| e.to_string())?;
    let ph = u32::try_from(info.height).map_err(|e| e.to_string())?;
    let stride = usize::try_from(info.stride).map_err(|e| e.to_string())?;
    let result = buffer
        .with_argb32_bytes(|pixels, _| {
            backend.present(
                surface,
                Frame {
                    width: pw,
                    height: ph,
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
fn run() -> Result<(), String> {
    let mut config = None;
    let mut fixture = false;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                config = Some(PathBuf::from(args.next().ok_or("missing --config value")?))
            }
            "--fixture" => fixture = true,
            "-h" | "--help" => {
                println!("nuraloumi-clipboard [--config PATH] [--fixture]");
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }
    let mut runner = SystemCommandRunner;
    let mut error = None;
    let mut entries = if fixture {
        vec![
            Entry {
                id: "1".into(),
                preview: "https://example.org".into(),
                data: "https://example.org".into(),
                history: false,
            },
            Entry {
                id: "2".into(),
                preview: "A sample clipboard note".into(),
                data: "A sample clipboard note".into(),
                history: false,
            },
        ]
    } else {
        match fetch(&mut runner) {
            Ok(v) => v,
            Err(e) => {
                error = Some(e);
                Vec::new()
            }
        }
    };
    let cfg = load_config_or_default(config.as_deref()).unwrap_or_default();
    let mut backend = WaylandBackend::connect().map_err(|e| e.to_string())?;
    if !backend.capabilities().layer_shell {
        return Err("layer shell unavailable".into());
    }
    backend.blocking_dispatch().map_err(|e| e.to_string())?;
    let surface = backend
        .create_menu(MenuConfig {
            width: cfg.menu_width,
            height: 440,
            margin_top: cfg.panel_height as i32,
            margin_left: 0,
            output: backend.outputs().first().map(|o| o.id),
            namespace: "nuraloumi-clipboard".into(),
        })
        .map_err(|e| e.to_string())?;
    let renderer = CairoRenderer::default();
    let tokens = if cfg.theme == Theme::Dark {
        &DARK_THEME
    } else {
        &LIGHT_THEME
    };
    let mut query = String::new();
    let mut menu = model(&entries, &query, error.as_deref());
    let mut state = MenuState::new(&menu);
    let mut geometry = None;
    let mut scene: Option<Scene> = None;
    loop {
        backend.blocking_dispatch().map_err(|e| e.to_string())?;
        for event in backend.drain_events().collect::<Vec<_>>() {
            if event.surface.is_some() && event.surface != Some(surface) {
                continue;
            }
            let mut redraw = false;
            let mut act = None;
            match event.event {
                PlatformEvent::Configure {
                    width,
                    height,
                    scale,
                } if event.surface == Some(surface) => {
                    geometry = Some((width, height, scale));
                    redraw = true;
                }
                PlatformEvent::Close
                | PlatformEvent::Key {
                    key: Key::Escape,
                    pressed: true,
                } => return Ok(()),
                PlatformEvent::Key {
                    key: Key::Up,
                    pressed: true,
                } => {
                    state.handle(&menu, SemanticInput::Up);
                    redraw = true;
                }
                PlatformEvent::Key {
                    key: Key::Down | Key::Tab,
                    pressed: true,
                } => {
                    state.handle(&menu, SemanticInput::Down);
                    redraw = true;
                }
                PlatformEvent::Key {
                    key: Key::Enter,
                    pressed: true,
                } => {
                    act = state
                        .selected_id
                        .as_ref()
                        .and_then(|id| menu.item_by_id(id))
                        .and_then(|item| item.action.clone());
                }
                PlatformEvent::Key {
                    key: Key::Backspace,
                    pressed: true,
                } => {
                    query.pop();
                    redraw = true;
                }
                PlatformEvent::Key {
                    key: Key::Text(ref s),
                    pressed: true,
                } => {
                    if s == "q" && query.is_empty() {
                        return Ok(());
                    }
                    if s == "d" && query.is_empty() {
                        act = state
                            .selected_id
                            .as_ref()
                            .and_then(|id| id.strip_prefix("clip-"))
                            .map(|idx| MenuAction::Custom {
                                kind: "delete".into(),
                                payload: idx.into(),
                            });
                    } else if !s.chars().any(char::is_control) {
                        query.push_str(s);
                        redraw = true;
                    }
                }
                PlatformEvent::PointerButton {
                    x,
                    y,
                    pressed: true,
                    ..
                }
                | PlatformEvent::TouchDown { x, y, .. } => {
                    act = scene
                        .as_ref()
                        .and_then(|s| s.hit_test(x, y))
                        .and_then(|id| menu.item_by_id(id))
                        .and_then(|item| item.action.clone());
                }
                _ => {}
            }
            if let Some(MenuAction::Custom { kind, payload }) = act {
                if let Ok(index) = payload.parse::<usize>() {
                    if let Some(entry) = entries.get(index) {
                        match select(entry, kind == "delete") {
                            Ok(()) if kind == "paste" => return Ok(()),
                            Ok(()) => {
                                entries.remove(index);
                                error = None;
                            }
                            Err(e) => error = Some(e),
                        }
                        redraw = true;
                    }
                }
            }
            if redraw {
                menu = model(&entries, &query, error.as_deref());
                state = MenuState::new(&menu);
                if let Some((w, h, s)) = geometry {
                    scene = Some(present(
                        &mut backend,
                        surface,
                        &renderer,
                        tokens,
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
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("nuraloumi-clipboard: {e}");
            ExitCode::FAILURE
        }
    }
}
#[cfg(test)]
mod tests {
    use super::preview;
    #[test]
    fn compact_preview() {
        assert_eq!(preview("a\nb"), "a b");
        assert_eq!(preview(&"x".repeat(100)).len(), 80);
    }
}
