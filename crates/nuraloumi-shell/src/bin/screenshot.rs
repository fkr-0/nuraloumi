//! NuraLoumi screenshot preview surface. Capture, save and copy are explicit.
//! Transitional local-module include keeps this independent of the dirty
//! provider lib.rs. Provider owner will export it; lane 08 owns shared Cargo.
pub use nuraloumi_providers::{ActionProvider, ActionResult, Provider, ProviderError};
#[path = "../../../nuraloumi-providers/src/screenshot.rs"]
mod screenshot;

use nuraloumi_core::{
    MenuAction, MenuItem, MenuModel, MenuState, SemanticInput, DARK_THEME, LIGHT_THEME,
};
use nuraloumi_render_cairo::{CairoRenderer, Rect, RenderOptions, Scene, Viewport};
use nuraloumi_shell::{load_config_or_default, render_menu_with_glyphs, Theme};
use nuraloumi_wayland::{
    BackendError, Frame, Key, MenuConfig, PixelFormat, PlatformEvent, SurfaceId, WaylandBackend,
};
use screenshot::{
    CaptureSelection, ScreenshotAction, ScreenshotPreview, ScreenshotProvider, ScreenshotSnapshot,
    ScreenshotSource,
};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
struct Options {
    selection: CaptureSelection,
    fixture: bool,
    config: Option<PathBuf>,
    help: bool,
}
fn parse_options(arguments: impl IntoIterator<Item = String>) -> Result<Options, String> {
    let mut args = arguments.into_iter();
    let mut selection = None;
    let mut config = None;
    let mut fixture = false;
    let mut help = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--fixture" => fixture = true,
            "--all" => {
                if selection.replace(CaptureSelection::All).is_some() {
                    return Err("choose only one selection".into());
                }
            }
            "--output" => {
                let value = args
                    .next()
                    .ok_or("--output requires a compositor output name")?;
                if selection.replace(CaptureSelection::Output(value)).is_some() {
                    return Err("choose only one selection".into());
                }
            }
            "--geometry" => {
                let value = args.next().ok_or("--geometry requires 'X,Y WxH'")?;
                if selection
                    .replace(CaptureSelection::Geometry(value))
                    .is_some()
                {
                    return Err("choose only one selection".into());
                }
            }
            "--config" => {
                config = Some(PathBuf::from(
                    args.next().ok_or("--config requires a path")?,
                ))
            }
            "-h" | "--help" => help = true,
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }
    if !help && selection.is_none() && !fixture {
        return Err(
            "explicit screenshot selection required: --all, --output NAME or --geometry 'X,Y WxH'"
                .into(),
        );
    }
    Ok(Options {
        selection: selection.unwrap_or(CaptureSelection::All),
        fixture,
        config,
        help,
    })
}
fn fixture_ppm() -> Vec<u8> {
    let (w, h) = (320u32, 200u32);
    let mut image = format!("P6\n{w} {h}\n255\n").into_bytes();
    for y in 0..h {
        for x in 0..w {
            image.push(20 + (x * 100 / w) as u8);
            image.push(31 + (y * 105 / h) as u8);
            image.push(46 + ((x + y) * 100 / (w + h)) as u8);
        }
    }
    image
}
fn model(snapshot: &ScreenshotSnapshot, info: &str) -> MenuModel {
    let captured = snapshot.preview.is_some();
    let mut items = vec![MenuItem::status(
        "target",
        match &snapshot.selection {
            CaptureSelection::All => "Target: all displays".to_owned(),
            CaptureSelection::Output(s) => format!("Target output: {s}"),
            CaptureSelection::Geometry(s) => format!("Target rectangle: {s}"),
        },
    )];
    if captured {
        // Re-capturing while this layer-shell surface is open would capture the
        // screenshot tool itself. Close and reopen for another clean desktop shot.
        items.push(MenuItem::status(
            "captured",
            "Captured before overlay; close and reopen to retake",
        ));
        items.push(
            MenuItem::action(
                "save",
                "Save PNG to ~/Pictures",
                MenuAction::Custom {
                    kind: "save".into(),
                    payload: String::new(),
                },
            )
            .with_subtitle("New file only; never overwrite"),
        );
        items.push(
            MenuItem::action(
                "copy",
                "Copy PNG to clipboard",
                MenuAction::Custom {
                    kind: "copy".into(),
                    payload: String::new(),
                },
            )
            .with_subtitle("Clipboard changes only on selection"),
        );
    } else {
        items.push(
            MenuItem::action(
                "capture",
                "Capture screen",
                MenuAction::Custom {
                    kind: "capture".into(),
                    payload: String::new(),
                },
            )
            .with_subtitle("grim PPM; bounded to 4 s / 8 MiB"),
        );
    }
    items.push(MenuItem::separator("preview-separator"));
    items.push(MenuItem::status(
        "preview-heading",
        match (snapshot.image_width, snapshot.image_height) {
            (Some(w), Some(h)) => format!("Preview: {w} x {h}"),
            _ => "Preview: capture first".to_owned(),
        },
    ));
    items.push(MenuItem::status("status", info.to_owned()));
    if !captured {
        items.push(MenuItem::status("empty", "No screenshot retained yet"));
    }
    MenuModel::new("screenshot", "Screenshot", items)
}
fn perform(
    provider: &mut ScreenshotProvider,
    action: &MenuAction,
    fixture: bool,
) -> Result<String, String> {
    let MenuAction::Custom { kind, .. } = action else {
        return Ok(String::new());
    };
    let operation = match kind.as_str() {
        "capture" => ScreenshotAction::Capture,
        "copy" => ScreenshotAction::Copy,
        "save" => {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .ok_or("HOME not configured")?;
            if !home.is_absolute() {
                return Err("HOME must be absolute".into());
            }
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_millis();
            ScreenshotAction::Save {
                pictures_dir: home.join("Pictures"),
                stem: format!("nuraloumi-{now}"),
            }
        }
        _ => return Err("unsupported screenshot action".into()),
    };
    // Fixture mode never touches the clipboard even when Copy is selected.
    let result = provider.execute(operation).map_err(|e| e.to_string())?;
    Ok(if fixture && result.dry_run {
        "Fixture: clipboard unchanged".into()
    } else {
        result.message
    })
}
#[allow(clippy::too_many_arguments)]
fn draw(
    backend: &mut WaylandBackend,
    surface: SurfaceId,
    renderer: &CairoRenderer,
    menu: &MenuModel,
    state: &MenuState,
    preview: Option<&ScreenshotPreview>,
    dark: bool,
    width: u32,
    height: u32,
    scale: i32,
) -> Result<Scene, String> {
    let viewport = Viewport::new(f64::from(width), f64::from(height), f64::from(scale.max(1)));
    let tokens = if dark { &DARK_THEME } else { &LIGHT_THEME };
    let (scene, mut pixels) = render_menu_with_glyphs(
        renderer,
        menu,
        state,
        viewport,
        tokens,
        RenderOptions::default(),
        None,
        None,
    )?;
    if let Some(preview) = preview {
        // Dedicated lower canvas; the menu occupies the upper actionable rows.
        // Never allocate a full-resolution display image for preview.
        let max_width = f64::from(width.saturating_sub(36));
        let max_height = f64::from(height.saturating_sub(396)).min(220.0);
        if max_width > 16.0 && max_height > 16.0 {
            let aspect = f64::from(preview.width) / f64::from(preview.height);
            let pw = max_width.min(max_height * aspect);
            let ph = (pw / aspect).min(max_height);
            let region = Rect {
                x: (f64::from(width) - pw) / 2.0,
                y: 374.0,
                width: pw,
                height: ph,
            };
            pixels
                .paint_argb32_preview(
                    region,
                    f64::from(scale.max(1)),
                    preview.width,
                    preview.height,
                    &preview.argb32,
                )
                .map_err(|e| e.to_string())?;
        }
    }
    let info = pixels.info();
    let frame = pixels
        .with_argb32_bytes(|bytes, _| {
            backend.present(
                surface,
                Frame {
                    width: info.width as u32,
                    height: info.height as u32,
                    stride: info.stride as usize,
                    format: PixelFormat::Argb8888,
                    pixels: bytes,
                },
            )
        })
        .map_err(|e| e.to_string())?;
    match frame {
        Ok(()) | Err(BackendError::WouldBlock) => Ok(scene),
        Err(e) => Err(e.to_string()),
    }
}
fn run(options: Options) -> Result<(), String> {
    let source = if options.fixture {
        ScreenshotSource::Fixture(fixture_ppm())
    } else {
        ScreenshotSource::Grim
    };
    let mut provider =
        ScreenshotProvider::new(source, options.selection).map_err(|e| e.to_string())?;
    // Capture before creating our Wayland overlay so the first screenshot
    // reflects the underlying desktop rather than our own preview window.
    let mut info = match provider.execute(ScreenshotAction::Capture) {
        Ok(result) if options.fixture => format!("Fixture preview · {}", result.message),
        Ok(result) => result.message,
        Err(error) => format!("Capture unavailable: {error}"),
    };
    let config = load_config_or_default(options.config.as_deref()).unwrap_or_default();
    let mut backend = WaylandBackend::connect().map_err(|e| e.to_string())?;
    if !backend.capabilities().layer_shell {
        return Err("Wayland layer shell not available".into());
    }
    backend.blocking_dispatch().map_err(|e| e.to_string())?;
    let surface = backend
        .create_menu(MenuConfig {
            width: config.menu_width.min(520),
            height: 620,
            margin_top: config.panel_height as i32,
            margin_left: 0,
            output: backend.outputs().first().map(|o| o.id),
            namespace: "nuraloumi-screenshot".into(),
        })
        .map_err(|e| e.to_string())?;
    let renderer = CairoRenderer::default();
    let mut snapshot = provider.snapshot().map_err(|e| e.to_string())?;
    let mut menu = model(&snapshot, &info);
    let mut state = MenuState::new(&menu);
    let mut scene = None;
    let mut geometry = None;
    let mut pressed = None::<String>;
    loop {
        backend.blocking_dispatch().map_err(|e| e.to_string())?;
        for event in backend.drain_events().collect::<Vec<_>>() {
            if event.surface != Some(surface) {
                continue;
            }
            let mut redraw = false;
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
                    action = state
                        .selected_id
                        .as_ref()
                        .and_then(|id| menu.item_by_id(id))
                        .and_then(|item| item.action.clone());
                }
                PlatformEvent::Key {
                    key: Key::Text(ref v),
                    pressed: true,
                } if matches!(v.as_str(), "c" | "s" | "p") => {
                    action = menu
                        .item_by_id(match v.as_str() {
                            "c" => "capture",
                            "s" => "save",
                            _ => "copy",
                        })
                        .and_then(|item| item.action.clone());
                }
                PlatformEvent::PointerButton {
                    x,
                    y,
                    pressed: true,
                    ..
                }
                | PlatformEvent::TouchDown { x, y, .. } => {
                    pressed = scene
                        .as_ref()
                        .and_then(|s: &Scene| s.hit_test(x, y))
                        .map(str::to_owned);
                }
                PlatformEvent::PointerButton {
                    x,
                    y,
                    pressed: false,
                    ..
                } => {
                    // Pointer activates only when released in its original row.
                    let release = scene.as_ref().and_then(|s: &Scene| s.hit_test(x, y));
                    if release.is_some() && release == pressed.as_deref() {
                        action = release
                            .and_then(|id| menu.item_by_id(id))
                            .and_then(|item| item.action.clone());
                    }
                    pressed = None;
                }
                PlatformEvent::TouchMotion { x, y, .. } => {
                    // A touch dragged outside its initial row cannot activate it.
                    if scene.as_ref().and_then(|s: &Scene| s.hit_test(x, y)) != pressed.as_deref() {
                        pressed = None;
                    }
                }
                PlatformEvent::TouchUp { .. } => {
                    // TouchUp has no coordinates; touch cancellation clears selection.
                    action = pressed
                        .take()
                        .as_deref()
                        .and_then(|id| menu.item_by_id(id))
                        .and_then(|item| item.action.clone());
                }
                PlatformEvent::TouchCancel { .. } => pressed = None,
                _ => {}
            }
            if let Some(action) = action {
                info = match perform(&mut provider, &action, options.fixture) {
                    Ok(value) => value,
                    Err(error) => format!("Action failed: {error}"),
                };
                snapshot = provider.snapshot().map_err(|e| e.to_string())?;
                menu = model(&snapshot, &info);
                state = MenuState::new(&menu);
                redraw = true;
            }
            if redraw {
                if let Some((width, height, scale)) = geometry {
                    scene = Some(draw(
                        &mut backend,
                        surface,
                        &renderer,
                        &menu,
                        &state,
                        snapshot.preview.as_ref(),
                        config.theme == Theme::Dark,
                        width,
                        height,
                        scale,
                    )?);
                }
            }
        }
    }
}
fn main() -> ExitCode {
    let opts = match parse_options(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("nuraloumi-screenshot: {error}");
            return ExitCode::from(2);
        }
    };
    if opts.help {
        println!("nuraloumi-screenshot [--all | --output NAME | --geometry 'X,Y WxH'] [--fixture] [--config PATH]\nKeyboard: c capture, s save PNG, p copy PNG, Escape exit. Clipboard is never modified automatically.");
        return ExitCode::SUCCESS;
    }
    match run(opts) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("nuraloumi-screenshot: {error}");
            ExitCode::FAILURE
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_is_explicit_and_unambiguous() {
        assert!(parse_options(Vec::<String>::new()).is_err());
        assert!(parse_options(["--all", "--output", "eDP-1"].map(str::to_owned)).is_err());
        let fixture = parse_options(["--fixture"].map(str::to_owned)).unwrap();
        assert!(fixture.fixture);
        assert!(matches!(fixture.selection, CaptureSelection::All));
    }
    #[test]
    fn fixture_actions_never_implicitly_copy() {
        let mut provider = ScreenshotProvider::new(
            ScreenshotSource::Fixture(fixture_ppm()),
            CaptureSelection::All,
        )
        .unwrap();
        let snapshot = provider.snapshot().unwrap();
        let initial = model(&snapshot, "test");
        assert_eq!(
            initial.items.iter().filter(|i| i.action.is_some()).count(),
            1
        );
        assert!(initial.item_by_id("copy").is_none());
        let action = initial
            .item_by_id("capture")
            .unwrap()
            .action
            .clone()
            .unwrap();
        assert!(perform(&mut provider, &action, true)
            .unwrap()
            .contains("captured"));
        let captured = model(&provider.snapshot().unwrap(), "captured");
        assert!(captured.item_by_id("capture").is_none());
        assert_eq!(
            captured.items.iter().filter(|i| i.action.is_some()).count(),
            2
        );
        let copy = captured.item_by_id("copy").unwrap().action.clone().unwrap();
        assert_eq!(
            perform(&mut provider, &copy, true).unwrap(),
            "Fixture: clipboard unchanged"
        );
    }
}
