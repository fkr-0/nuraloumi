//! Native Wayland/Cairo offline feed-reader. No online fetching.
#[path = "../feeds.rs"]
mod feeds;
use feeds::provider::{OpenInBrowser, ReadState, MAX_FEED_BYTES, MAX_OPML_BYTES};
use feeds::{action, open_browser, read_bounded, Effect, FeedView};
use nuraloumi_core::{MenuAction, MenuModel, MenuState, DARK_THEME, LIGHT_THEME};
use nuraloumi_providers::command::SystemCommandRunner;
use nuraloumi_render_cairo::{CairoRenderer, RenderOptions, Scene, Viewport};
use nuraloumi_shell::{load_config_or_default, render_menu_follow_selection, ShellConfig, Theme};
use nuraloumi_wayland::{
    BackendError, Frame, Key, MenuConfig, PixelFormat, PlatformEvent, SurfaceId, WaylandBackend,
};
use std::{env, path::PathBuf, process::ExitCode};

#[derive(Default)]
struct Options {
    fixture: bool,
    headless: bool,
    config: Option<PathBuf>,
    state: Option<PathBuf>,
    opml: Option<PathBuf>,
    sources: Vec<(String, PathBuf)>,
}
fn options(args: impl IntoIterator<Item = String>) -> Result<Option<Options>, String> {
    let mut o = Options::default();
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("nuraloumi-feeds [--fixture] [--headless] [--feed URL=XML_PATH]... [--opml PATH] [--state PATH] [--config PATH]\nArrows: select article; Enter/R: mark read; O: open browser; Left/Right: page; Esc: quit.");
                return Ok(None);
            }
            "--fixture" => o.fixture = true,
            "--headless" => o.headless = true,
            "--config" => o.config = Some(it.next().ok_or("--config requires path")?.into()),
            "--state" => o.state = Some(it.next().ok_or("--state requires path")?.into()),
            "--opml" => o.opml = Some(it.next().ok_or("--opml requires path")?.into()),
            "--feed" => {
                let value = it.next().ok_or("--feed requires URL=XML_PATH")?;
                let (url, path) = value
                    .split_once('=')
                    .ok_or("--feed requires URL=XML_PATH")?;
                OpenInBrowser::new(url).map_err(|e| e.to_string())?;
                if path.is_empty() {
                    return Err("empty feed path".into());
                }
                o.sources.push((url.into(), path.into()));
                if o.sources.len() > 128 {
                    return Err("too many --feed inputs".into());
                }
            }
            _ => return Err(format!("unknown option: {arg}")),
        }
    }
    Ok(Some(o))
}
fn default_state_path() -> Result<PathBuf, String> {
    if let Some(dir) = env::var_os("XDG_STATE_HOME").filter(|v| !v.is_empty()) {
        let root = PathBuf::from(dir);
        if !root.is_absolute() {
            return Err("XDG_STATE_HOME must be absolute".into());
        }
        return Ok(root.join("nuraloumi/feeds.read"));
    }
    let home = env::var_os("HOME").ok_or("HOME required for read-state")?;
    Ok(PathBuf::from(home).join(".local/state/nuraloumi/feeds.read"))
}
fn load(o: &Options) -> Result<FeedView, String> {
    if o.fixture && o.sources.is_empty() && o.opml.is_none() {
        return FeedView::fixture();
    }
    let mut docs = Vec::new();
    for (url, path) in &o.sources {
        docs.push((url.clone(), read_bounded(path, MAX_FEED_BYTES)?));
    }
    let opml = o
        .opml
        .as_ref()
        .map(|p| read_bounded(p, MAX_OPML_BYTES))
        .transpose()?;
    let state_path = if o.fixture {
        o.state.clone()
    } else {
        Some(o.state.clone().map(Ok).unwrap_or_else(default_state_path)?)
    };
    let reads = match state_path.as_deref() {
        Some(p) if p.exists() => ReadState::load(p).map_err(|e| e.to_string())?,
        _ => ReadState::new(),
    };
    FeedView::new(docs, opml.as_deref(), reads, state_path)
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
        return Err("invalid or oversized viewport".into());
    }
    let tokens = match config.theme {
        Theme::Dark => &DARK_THEME,
        Theme::Light => &LIGHT_THEME,
    };
    let (scene, mut buffer) = render_menu_follow_selection(
        renderer,
        model,
        state,
        Viewport::new(f64::from(w), f64::from(h), f64::from(s)),
        tokens,
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
fn hit(scene: &Option<Scene>, menu: &MenuModel, x: f64, y: f64) -> Option<MenuAction> {
    menu.item_by_id(scene.as_ref()?.hit_test(x, y)?)?
        .action
        .clone()
}
fn handle(effect: Effect, fixture: bool, view: &mut FeedView) -> Result<bool, String> {
    match effect {
        Effect::None => Ok(false),
        Effect::Redraw => Ok(true),
        Effect::Browser(req) if fixture => {
            view.message = format!("Fixture browser blocked: {}", req.url());
            Ok(true)
        }
        Effect::Browser(req) => {
            open_browser(&mut SystemCommandRunner, &req)?;
            view.message = "Browser opened".into();
            Ok(true)
        }
    }
}
fn live(config: ShellConfig, fixture: bool, mut view: FeedView) -> Result<(), String> {
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
            namespace: "nuraloumi-feeds".into(),
        })
        .map_err(|e| e.to_string())?;
    let renderer = CairoRenderer::default();
    let mut geom = None;
    let mut scene = None;
    let mut menu = view.menu();
    let mut pressed: Option<(i32, MenuAction)> = None;
    loop {
        backend.blocking_dispatch().map_err(|e| e.to_string())?;
        for ev in backend.drain_events().collect::<Vec<_>>() {
            if ev.surface.is_some() && ev.surface != Some(id) {
                continue;
            }
            let mut redraw = false;
            let mut request = None;
            match ev.event {
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
                    Key::Up => redraw = view.select_delta(-1),
                    Key::Down => redraw = view.select_delta(1),
                    Key::Left => request = Some(action("prev", "")),
                    Key::Right => request = Some(action("next", "")),
                    Key::Enter => request = Some(action("mark", "")),
                    Key::Text(s) if s == "r" || s == "R" => request = Some(action("mark", "")),
                    Key::Text(s) if s == "o" || s == "O" => request = Some(action("open", "")),
                    Key::Text(s) if s == "q" => {
                        let _ = backend.destroy_surface(id);
                        return Ok(());
                    }
                    _ => {}
                },
                PlatformEvent::PointerButton {
                    x,
                    y,
                    pressed: true,
                    ..
                } => request = hit(&scene, &menu, x, y),
                PlatformEvent::TouchDown { id: tid, x, y } => {
                    if pressed.is_none() {
                        pressed = hit(&scene, &menu, x, y).map(|a| (tid, a));
                    }
                }
                PlatformEvent::TouchMotion { id: tid, x, y } => {
                    if pressed.as_ref().is_some_and(|(active, old)| {
                        *active == tid && hit(&scene, &menu, x, y).as_ref() != Some(old)
                    }) {
                        pressed = None;
                    }
                }
                PlatformEvent::TouchUp { id: tid } => {
                    if pressed.as_ref().is_some_and(|(active, _)| *active == tid) {
                        request = pressed.take().map(|(_, a)| a);
                    }
                }
                PlatformEvent::TouchCancel { .. } => pressed = None,
                _ => {}
            }
            if let Some(a) = request {
                redraw |= handle(view.activate(a)?, fixture, &mut view)?;
            }
            if redraw {
                menu = view.menu();
                let state = view.menu_state(&menu);
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
}
fn run(o: Options) -> Result<(), String> {
    let config = load_config_or_default(o.config.as_deref())?;
    let view = load(&o)?;
    if o.headless {
        let menu = view.menu();
        menu.validate().map_err(|e| e.to_string())?;
        draw(
            None,
            &CairoRenderer::default(),
            &config,
            &menu,
            &view.menu_state(&menu),
            512,
            640,
            1,
        )?;
        println!(
            "Offline Cairo fixture: {} articles, {} subscriptions",
            view.articles.len(),
            view.subscriptions.len()
        );
        Ok(())
    } else {
        live(config, o.fixture, view)
    }
}
fn main() -> ExitCode {
    match options(env::args().skip(1)).and_then(|o| o.map(run).transpose()) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("nuraloumi-feeds: {e}");
            ExitCode::FAILURE
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixture_headless() {
        let o = options(vec!["--fixture".into(), "--headless".into()])
            .unwrap()
            .unwrap();
        assert_eq!(load(&o).unwrap().articles.len(), 2);
        run(o).unwrap();
    }
    #[test]
    fn malicious_feed_arg_rejected() {
        assert!(options(vec!["--feed".into(), "file:///tmp/a=/tmp/a.xml".into()]).is_err());
    }

    #[test]
    fn cairo_touch_targets_and_hit_testing() {
        let config = ShellConfig::default();
        let view = FeedView::fixture().unwrap();
        let model = view.menu();
        let scene = draw(
            None,
            &CairoRenderer::default(),
            &config,
            &model,
            &view.menu_state(&model),
            512,
            640,
            1,
        )
        .unwrap();
        for hit in scene.hits.iter().filter(|hit| hit.actionable) {
            assert!(
                hit.rect.height >= 48.0,
                "{} has a {}px hit target",
                hit.item_id,
                hit.rect.height
            );
            let x = hit.rect.x + hit.rect.width / 2.0;
            let y = hit.rect.y + hit.rect.height / 2.0;
            assert_eq!(scene.hit_test(x, y), Some(hit.item_id.as_str()));
        }
        assert!(scene.hits.iter().any(|hit| hit.item_id == "article-0"));
    }
}
