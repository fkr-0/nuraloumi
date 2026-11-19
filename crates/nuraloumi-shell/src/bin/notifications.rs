//! Notification Center: a standalone Cairo/wl_shm surface.
//!
//! Provider mutations are deliberately isolated from scene construction.
//! Until the provider exposes persisted read flags and timestamps, read state
//! is session-local and history is grouped by application, not fabricated time.
use nuraloumi_core::{
    MenuAction, MenuItem, MenuModel, MenuState, SemanticInput, DARK_THEME, LIGHT_THEME,
};
use nuraloumi_providers::notifications::{
    NotificationAction, NotificationEntry, NotificationProvider, NotificationSnapshot,
};
use nuraloumi_providers::{ActionProvider, Health, Provider};
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
    collections::{BTreeMap, BTreeSet},
    env,
    path::PathBuf,
    process::ExitCode,
};

const MAX_ENTRIES: usize = 64;
const MAX_QUERY_CHARS: usize = 80;
const BODY_LINE_CHARS: usize = 46;
const BODY_VISIBLE_LINES: usize = 5;
const PRIMARY_BUTTON: u32 = 0x110; // Linux BTN_LEFT

#[derive(Default)]
struct Args {
    config: Option<PathBuf>,
    fixture: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args::default();
    let mut iter = env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--fixture" => args.fixture = true,
            "--live" => {}
            "--config" => args.config = Some(iter.next().ok_or("--config requires a path")?.into()),
            "--help" | "-h" => {
                println!("--config PATH  --fixture  --live");
                std::process::exit(0)
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }
    Ok(args)
}

fn cmd(kind: &str, payload: impl Into<String>) -> MenuAction {
    MenuAction::Custom {
        kind: kind.into(),
        payload: payload.into(),
    }
}

fn safe_visible_char(ch: char) -> bool {
    !ch.is_control() && !matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

fn clean(value: &str, limit: usize) -> String {
    let mut result = String::new();
    for ch in value.chars() {
        // Keep untrusted summaries from introducing fake rows or visual reordering.
        if result.chars().count() >= limit {
            result.push('…');
            break;
        }
        if !safe_visible_char(ch) {
            result.push(' ');
        } else {
            result.push(ch);
        }
    }
    result.trim().to_owned()
}

fn body_lines(body: &str) -> Vec<String> {
    let mut lines = vec![String::new()];
    for ch in body.chars().take(240) {
        if ch == '\n'
            || lines
                .last()
                .is_some_and(|v| v.chars().count() >= BODY_LINE_CHARS)
        {
            lines.push(String::new());
        }
        if ch != '\n' {
            lines
                .last_mut()
                .expect("body always has a line")
                .push(if !safe_visible_char(ch) { ' ' } else { ch });
        }
    }
    lines
}

#[derive(Default)]
struct UiState {
    query: String,
    detail: Option<u32>,
    body_offset: usize,
    read: BTreeSet<u32>,
    confirm_ids: Option<Vec<u32>>,
    error: Option<String>,
    notice: Option<String>,
    pointer_press: Option<String>,
    touch_press: Option<TouchPress>,
    active_touches: BTreeSet<i32>,
    touch_cancelled: bool,
}

struct TouchPress {
    id: i32,
    item: Option<String>,
    x: f64,
    y: f64,
    scroll_y: f64,
    scrolling: bool,
}

impl UiState {
    fn selected_count(&self, snapshot: &NotificationSnapshot) -> usize {
        snapshot
            .notifications
            .iter()
            .take(MAX_ENTRIES)
            .filter(|n| !self.read.contains(&n.id))
            .count()
    }

    fn view(&mut self, id: u32, snapshot: &NotificationSnapshot) {
        if snapshot.notifications.iter().any(|n| n.id == id) {
            self.detail = Some(id);
            self.read.insert(id);
            self.body_offset = 0;
            self.error = None;
        }
    }

    fn refresh_read_ids(&mut self, snapshot: &NotificationSnapshot) {
        self.read
            .retain(|id| snapshot.notifications.iter().any(|entry| entry.id == *id));
        if self
            .detail
            .is_some_and(|id| !snapshot.notifications.iter().any(|n| n.id == id))
        {
            self.detail = None;
        }
    }

    fn reset_gestures(&mut self) {
        self.pointer_press = None;
        self.touch_press = None;
    }

    fn pointer(
        &mut self,
        scene: Option<&Scene>,
        x: f64,
        y: f64,
        button: u32,
        pressed: bool,
    ) -> Option<String> {
        if button != PRIMARY_BUTTON && button != 1 {
            return None;
        }
        let current = scene.and_then(|s| s.hit_test(x, y)).map(str::to_owned);
        if pressed {
            self.pointer_press = current;
            None
        } else {
            let start = self.pointer_press.take();
            if start.is_some() && start == current {
                start
            } else {
                None
            }
        }
    }

    fn touch_down(&mut self, scene: Option<&Scene>, id: i32, x: f64, y: f64) {
        if !self.active_touches.insert(id) {
            self.touch_cancelled = true;
        }
        if self.active_touches.len() > 1 {
            self.touch_cancelled = true;
            self.touch_press = None;
            return;
        }
        if !self.touch_cancelled {
            self.touch_press = Some(TouchPress {
                id,
                item: scene.and_then(|s| s.hit_test(x, y)).map(str::to_owned),
                x,
                y,
                scroll_y: y,
                scrolling: false,
            });
        }
    }

    /// Emit one navigational scroll step per 28 logical pixels of movement.
    /// A swipe never activates the originally pressed notification.
    fn touch_move(&mut self, id: i32, x: f64, y: f64) -> Option<i32> {
        let press = self.touch_press.as_mut().filter(|p| p.id == id)?;
        press.x = x;
        press.y = y;
        let delta = y - press.scroll_y;
        if delta.abs() < 28.0 {
            return None;
        }
        press.scrolling = true;
        let steps = (delta.abs() / 28.0).floor().min(8.0) as i32;
        press.scroll_y += f64::from(steps) * 28.0 * delta.signum();
        Some(if delta < 0.0 { steps } else { -steps })
    }

    fn touch_up(&mut self, scene: Option<&Scene>, id: i32) -> Option<String> {
        if !self.active_touches.remove(&id) {
            return None;
        }
        let result = if self.touch_cancelled {
            None
        } else {
            self.touch_press
                .take()
                .filter(|p| p.id == id && !p.scrolling)
                .and_then(|p| {
                    let current = scene.and_then(|s| s.hit_test(p.x, p.y));
                    (Some(p.item.as_deref()?) == current).then_some(p.item?)
                })
        };
        if self.active_touches.is_empty() {
            self.touch_cancelled = false;
            self.touch_press = None;
        }
        result
    }

    fn touch_cancel(&mut self, ids: &[i32]) {
        self.touch_cancelled = true;
        self.touch_press = None;
        if ids.is_empty() {
            self.active_touches.clear();
            self.touch_cancelled = false;
        } else {
            for id in ids {
                self.active_touches.remove(id);
            }
            if self.active_touches.is_empty() {
                self.touch_cancelled = false;
            }
        }
    }

    fn scroll_body(&mut self, snapshot: &NotificationSnapshot, delta: isize) {
        let Some(entry) = self
            .detail
            .and_then(|id| snapshot.notifications.iter().find(|n| n.id == id))
        else {
            return;
        };
        let max = body_lines(&entry.body)
            .len()
            .saturating_sub(BODY_VISIBLE_LINES);
        self.body_offset = self.body_offset.saturating_add_signed(delta).min(max);
    }
}

fn fixture() -> NotificationSnapshot {
    let mut snapshot = NotificationSnapshot::unavailable(0, String::new());
    snapshot.issues.clear();
    snapshot.meta.health = Health::Healthy;
    snapshot.backend = "fixture".into();
    snapshot.notifications = (0..10)
        .map(|i| NotificationEntry {
            id: i + 1,
            app: if i % 2 == 0 { "Chat" } else { "System" }.into(),
            summary: format!("Demo notification {}", i + 1),
            body: if i == 0 {
                "Long body: ".to_owned()
                    + &"This text is readable in the paginated detail view. ".repeat(5)
            } else {
                format!("Sample message body {}", i + 1)
            },
            actions: if i % 2 == 0 {
                vec!["open".into(), "reply".into()]
            } else {
                vec![]
            },
            default_action: None,
        })
        .collect();
    snapshot
}

fn filtered<'a>(snapshot: &'a NotificationSnapshot, query: &str) -> Vec<&'a NotificationEntry> {
    let needle = query.to_lowercase();
    let mut seen = BTreeSet::new();
    snapshot
        .notifications
        .iter()
        .take(MAX_ENTRIES)
        .filter(|entry| seen.insert(entry.id))
        .filter(|entry| {
            needle.is_empty()
                || entry.app.to_lowercase().contains(&needle)
                || entry.summary.to_lowercase().contains(&needle)
                || entry.body.to_lowercase().contains(&needle)
        })
        .collect()
}

fn model(snapshot: &NotificationSnapshot, ui: &UiState) -> MenuModel {
    let count = snapshot.notifications.len().min(MAX_ENTRIES);
    let unread = ui.selected_count(snapshot);
    let mut items = vec![
        MenuItem::status("heading", format!("Notifications  ·  {count}"))
            .with_subtitle(format!("{unread} unread in this session · F5 refresh")),
    ];

    if let Some(error) = &ui.error {
        items.push(MenuItem::status(
            "error",
            format!("Unavailable: {}", clean(error, 100)),
        ));
        items.push(MenuItem::action("retry", "Retry", cmd("refresh", "")));
    } else if snapshot.meta.health == Health::Unavailable {
        items.push(MenuItem::status(
            "error",
            "Notification backend unavailable",
        ));
        items.push(MenuItem::action("retry", "Retry", cmd("refresh", "")));
    }
    if let Some(notice) = &ui.notice {
        items.push(MenuItem::status("notice", clean(notice, 100)));
    }
    if !snapshot.issues.is_empty() {
        items.push(MenuItem::status(
            "provider-issue",
            format!("Backend reports {} issue(s)", snapshot.issues.len()),
        ));
    }

    if let Some(ids) = &ui.confirm_ids {
        items.push(MenuItem::section(
            "confirm-section",
            "CONFIRM DESTRUCTIVE ACTION",
        ));
        items.push(MenuItem::status(
            "confirm-text",
            format!(
                "Delete {} history entries? This cannot be undone.",
                ids.len()
            ),
        ));
        items.push(MenuItem::action(
            "confirm-clear",
            "Yes, clear all",
            cmd("clear", ""),
        ));
        items.push(MenuItem::action(
            "cancel-clear",
            "Cancel · keep notifications",
            cmd("cancel", ""),
        ));
        return MenuModel::new("notifications-confirm", "Notifications", items);
    }

    if let Some(id) = ui.detail {
        if let Some(entry) = snapshot.notifications.iter().find(|n| n.id == id) {
            items.push(MenuItem::action("back", "← Back to inbox", cmd("back", "")));
            items.push(MenuItem::section("detail-app", clean(&entry.app, 90)));
            items.push(MenuItem::status("detail-title", clean(&entry.summary, 140)));
            items.push(MenuItem::section("detail-body-heading", "MESSAGE"));
            let lines = body_lines(&entry.body);
            if lines.iter().all(|v| v.is_empty()) {
                items.push(MenuItem::status(
                    "detail-empty",
                    "No additional message text",
                ));
            } else {
                for (index, line) in lines
                    .iter()
                    .enumerate()
                    .skip(ui.body_offset)
                    .take(BODY_VISIBLE_LINES)
                {
                    items.push(MenuItem::status(format!("body-{index}"), line));
                }
                if ui.body_offset > 0 {
                    items.push(MenuItem::action(
                        "body-previous",
                        "↑ Previous lines",
                        cmd("scroll-up", ""),
                    ));
                }
                if ui.body_offset + BODY_VISIBLE_LINES < lines.len() {
                    items.push(MenuItem::action(
                        "body-next",
                        "↓ More lines",
                        cmd("scroll-down", ""),
                    ));
                }
            }
            items.push(MenuItem::section("detail-actions", "ACTIONS"));
            items.push(MenuItem::action(
                "toggle-read",
                if ui.read.contains(&id) {
                    "Mark unread"
                } else {
                    "Mark read"
                },
                cmd("read", id.to_string()),
            ));
            let mut seen = BTreeSet::new();
            for action in entry.actions.iter().chain(entry.default_action.iter()) {
                if seen.insert(action.clone()) {
                    items.push(MenuItem::action(
                        format!("invoke-{id}-{}", seen.len()),
                        format!("Run: {}", clean(action, 80)),
                        cmd("invoke", format!("{id}:{action}")),
                    ));
                }
            }
            items.push(MenuItem::action(
                "dismiss-detail",
                "Dismiss notification",
                cmd("dismiss", id.to_string()),
            ));
            return MenuModel::new("notifications-detail", "Notification detail", items);
        }
    }

    items.push(MenuItem::status(
        "hint",
        format!(
            "Type to filter: {} · Esc close",
            if ui.query.is_empty() {
                "all"
            } else {
                &ui.query
            }
        ),
    ));
    if !snapshot.notifications.is_empty() {
        items.push(MenuItem::action(
            "clear-all",
            "Clear all…",
            cmd("ask-clear", ""),
        ));
    }
    let entries = filtered(snapshot, &ui.query);
    if entries.is_empty() {
        items.push(MenuItem::status(
            "empty",
            if !ui.query.is_empty() {
                "No matching notifications"
            } else if snapshot.meta.health == Health::Unavailable {
                "No history available"
            } else {
                "Your inbox is empty"
            },
        ));
    } else {
        // Dunst's present contract contains neither per-entry timestamps nor
        // persisted read markers. Preserve app grouping without inventing ages.
        let mut groups: BTreeMap<String, Vec<&NotificationEntry>> = BTreeMap::new();
        for entry in entries {
            groups.entry(entry.app.clone()).or_default().push(entry);
        }
        for (index, (app, group)) in groups.iter().enumerate() {
            let title = clean(app, 70);
            items.push(MenuItem::section(
                format!("app-{index}"),
                format!(
                    "{}  ·  {}",
                    if title.is_empty() {
                        "Unknown application"
                    } else {
                        &title
                    },
                    group.len()
                ),
            ));
            for entry in group {
                let label = format!(
                    "{} {}",
                    if ui.read.contains(&entry.id) {
                        "○"
                    } else {
                        "●"
                    },
                    clean(&entry.summary, 100)
                );
                let subtitle = clean(&entry.body, 90);
                items.push(
                    MenuItem::action(
                        format!("view-{}", entry.id),
                        label,
                        cmd("view", entry.id.to_string()),
                    )
                    .with_subtitle(if subtitle.is_empty() {
                        "Open details".into()
                    } else {
                        subtitle
                    }),
                );
            }
        }
    }
    items.push(MenuItem::status(
        "footer",
        "● unread · ○ read (session only)",
    ));
    MenuModel::new("notifications", "Notifications", items)
}

fn selected_action(model: &MenuModel, state: &MenuState) -> Option<MenuAction> {
    model
        .item_by_id(state.selected_id.as_deref()?)?
        .action
        .clone()
}

fn selected_id_action(model: &MenuModel, item: &str) -> Option<MenuAction> {
    model.item_by_id(item)?.action.clone()
}

fn refresh(
    provider: &mut NotificationProvider<nuraloumi_providers::command::SystemCommandRunner>,
    snapshot: &mut NotificationSnapshot,
    ui: &mut UiState,
) {
    match provider.snapshot() {
        Ok(next) if next.meta.health != Health::Unavailable => {
            *snapshot = next;
            ui.error = None;
            ui.refresh_read_ids(snapshot);
        }
        Ok(next) => {
            ui.error = Some(
                next.issues
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "backend unavailable".into()),
            );
            if snapshot.notifications.is_empty() {
                *snapshot = next;
            }
        }
        Err(error) => {
            ui.error = Some(format!("{error:?}"));
        }
    }
}

fn perform(
    action: MenuAction,
    ui: &mut UiState,
    snapshot: &mut NotificationSnapshot,
    provider: &mut NotificationProvider<nuraloumi_providers::command::SystemCommandRunner>,
    fixture_mode: bool,
) {
    let MenuAction::Custom { kind, payload } = action else {
        return;
    };
    ui.notice = None;
    match kind.as_str() {
        "refresh" => {
            if fixture_mode {
                *snapshot = fixture();
                ui.error = None;
            } else {
                refresh(provider, snapshot, ui);
            }
        }
        "back" => {
            ui.detail = None;
            ui.body_offset = 0;
        }
        "view" => {
            if let Ok(id) = payload.parse() {
                ui.view(id, snapshot);
            }
        }
        "read" => {
            if let Ok(id) = payload.parse::<u32>() {
                if snapshot.notifications.iter().any(|n| n.id == id) && !ui.read.insert(id) {
                    ui.read.remove(&id);
                }
            }
        }
        "scroll-up" => ui.scroll_body(snapshot, -(BODY_VISIBLE_LINES as isize)),
        "scroll-down" => ui.scroll_body(snapshot, BODY_VISIBLE_LINES as isize),
        "ask-clear" => {
            if !snapshot.notifications.is_empty() {
                ui.confirm_ids = Some(
                    snapshot
                        .notifications
                        .iter()
                        .take(MAX_ENTRIES)
                        .map(|n| n.id)
                        .collect(),
                );
                ui.reset_gestures();
            }
        }
        "cancel" => ui.confirm_ids = None,
        "clear" => {
            let Some(expected) = ui.confirm_ids.take() else {
                return;
            };
            let current = snapshot
                .notifications
                .iter()
                .take(MAX_ENTRIES)
                .map(|n| n.id)
                .collect::<Vec<_>>();
            if expected != current {
                ui.error = Some("History changed; reconfirm clear-all".into());
                return;
            }
            if fixture_mode {
                snapshot.notifications.clear();
                ui.detail = None;
            } else {
                // Guard the confirmation against new arrivals since the prompt.
                // Provider::execute is still the sole action authority.
                match provider.snapshot() {
                    Ok(live)
                        if live.meta.health != Health::Unavailable
                            && live
                                .notifications
                                .iter()
                                .take(MAX_ENTRIES)
                                .map(|n| n.id)
                                .collect::<Vec<_>>()
                                == expected => {}
                    _ => {
                        ui.error =
                            Some("History changed or is unavailable; reconfirm clear-all".into());
                        return;
                    }
                }
                match provider.execute(NotificationAction::Clear) {
                    Ok(_) => {
                        refresh(provider, snapshot, ui);
                        ui.detail = None;
                    }
                    Err(error) => ui.error = Some(format!("Clear failed: {error:?}")),
                }
            }
        }
        "dismiss" | "invoke" => {
            let (id, backend_action) = if kind == "dismiss" {
                let Ok(id) = payload.parse::<u32>() else {
                    return;
                };
                (id, NotificationAction::Remove { id })
            } else {
                let Some((raw_id, name)) = payload.split_once(':') else {
                    return;
                };
                let Ok(id) = raw_id.parse::<u32>() else {
                    return;
                };
                let allowed = snapshot
                    .notifications
                    .iter()
                    .find(|n| n.id == id)
                    .is_some_and(|n| {
                        n.actions.iter().any(|a| a == name)
                            || n.default_action.as_deref() == Some(name)
                    });
                if !allowed {
                    ui.error = Some("Action is no longer available; refresh".into());
                    return;
                }
                (
                    id,
                    NotificationAction::Invoke {
                        id,
                        action: name.into(),
                    },
                )
            };
            if !snapshot.notifications.iter().any(|n| n.id == id) {
                ui.error = Some("Notification is no longer available".into());
                return;
            }
            if fixture_mode {
                if kind == "dismiss" {
                    snapshot.notifications.retain(|n| n.id != id);
                } else {
                    ui.notice = Some("Fixture: no system action executed".into());
                }
            } else {
                match provider.execute(backend_action) {
                    Ok(_) => refresh(provider, snapshot, ui),
                    Err(error) => {
                        ui.error = Some(format!("Action failed: {error:?}"));
                        return;
                    }
                }
            }
            ui.read.remove(&id);
            if ui.detail == Some(id) {
                ui.detail = None;
                ui.body_offset = 0;
            }
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn draw(
    backend: &mut WaylandBackend,
    surface: SurfaceId,
    renderer: &CairoRenderer,
    config: &ShellConfig,
    menu: &MenuModel,
    state: &MenuState,
    width: u32,
    height: u32,
    scale: i32,
) -> Result<Scene, String> {
    let tokens = match config.theme {
        ShellTheme::Dark => &DARK_THEME,
        ShellTheme::Light => &LIGHT_THEME,
    };
    let (scene, mut buffer) = render_menu_with_glyphs(
        renderer,
        menu,
        state,
        Viewport::new(f64::from(width), f64::from(height), f64::from(scale.max(1))),
        tokens,
        RenderOptions::default(),
        None,
        None,
    )
    .map_err(|error| format!("render: {error}"))?;
    let info = buffer.info();
    let width = u32::try_from(info.width).map_err(|error| error.to_string())?;
    let height = u32::try_from(info.height).map_err(|error| error.to_string())?;
    let stride = usize::try_from(info.stride).map_err(|error| error.to_string())?;
    let result = buffer
        .with_argb32_bytes(|pixels, _| {
            backend.present(
                surface,
                Frame {
                    width,
                    height,
                    stride,
                    format: PixelFormat::Argb8888,
                    pixels,
                },
            )
        })
        .map_err(|error| error.to_string())?;
    match result {
        Ok(()) | Err(BackendError::WouldBlock) => Ok(scene),
        Err(error) => Err(error.to_string()),
    }
}

fn transparent(
    backend: &mut WaylandBackend,
    surface: SurfaceId,
    width: u32,
    height: u32,
    scale: i32,
) -> Result<(), String> {
    let scale = u32::try_from(scale.max(1)).map_err(|e| e.to_string())?;
    let pixels = width
        .checked_mul(scale)
        .and_then(|w| height.checked_mul(scale).and_then(|h| w.checked_mul(h)))
        .and_then(|count| count.checked_mul(4))
        .and_then(|size| usize::try_from(size).ok())
        .ok_or("backdrop size overflow")?;
    let blank = vec![0u8; pixels];
    match backend.present(
        surface,
        Frame::packed(width * scale, height * scale, PixelFormat::Argb8888, &blank),
    ) {
        Ok(()) | Err(BackendError::WouldBlock) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn close(
    backend: &mut WaylandBackend,
    surface: SurfaceId,
    backdrop: Option<SurfaceId>,
) -> Result<(), String> {
    if let Some(id) = backdrop {
        backend.destroy_surface(id).map_err(|e| e.to_string())?;
    }
    backend.destroy_surface(surface).map_err(|e| e.to_string())
}

fn run(config: ShellConfig, fixture_mode: bool) -> Result<(), String> {
    let mut provider = NotificationProvider::system();
    let mut snapshot = if fixture_mode {
        fixture()
    } else {
        provider.snapshot().map_err(|error| format!("{error:?}"))?
    };
    let mut ui = UiState::default();
    let mut backend = WaylandBackend::connect().map_err(|error| error.to_string())?;
    if !backend.capabilities().layer_shell {
        return Err("layer shell unavailable".into());
    }
    backend
        .blocking_dispatch()
        .map_err(|error| error.to_string())?;
    let output = backend.outputs().into_iter().next();
    let (top, right, bottom, left) =
        dismiss_backdrop_margins(config.panel_edge, config.panel_height);
    let backdrop = backend
        .create_dismiss_backdrop(DismissBackdropConfig {
            margin_top: top,
            margin_right: right,
            margin_bottom: bottom,
            margin_left: left,
            output: output.as_ref().map(|o| o.id),
            namespace: "nuraloumi-notifications-dismiss".into(),
        })
        .ok();
    let surface = backend
        .create_menu(WaylandMenuConfig {
            width: config.menu_width.min(520),
            height: 600,
            margin_top: config.panel_height as i32,
            margin_left: 0,
            output: output.as_ref().map(|o| o.id),
            namespace: "nuraloumi-notifications".into(),
        })
        .map_err(|error| error.to_string())?;
    let renderer = CairoRenderer::default();
    let mut menu = model(&snapshot, &ui);
    let mut state = MenuState::new(&menu);
    let mut geometry = None;
    let mut scene: Option<Scene> = None;
    loop {
        // Blocking event dispatch, not a busy polling/animation loop.
        backend
            .blocking_dispatch()
            .map_err(|error| error.to_string())?;
        for event in backend.drain_events().collect::<Vec<_>>() {
            if event.surface == backdrop {
                match event.event {
                    WaylandEvent::Configure {
                        width,
                        height,
                        scale,
                    } => transparent(
                        &mut backend,
                        backdrop.ok_or("backdrop missing")?,
                        width,
                        height,
                        scale,
                    )?,
                    WaylandEvent::PointerButton { pressed: true, .. }
                    | WaylandEvent::TouchDown { .. }
                    | WaylandEvent::Close => {
                        close(&mut backend, surface, backdrop)?;
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
            let mut requested: Option<MenuAction> = None;
            let mut should_close = false;
            match event.event {
                WaylandEvent::Configure {
                    width,
                    height,
                    scale,
                } => {
                    geometry = Some((width, height, scale));
                    redraw = true;
                }
                WaylandEvent::Close => should_close = true,
                WaylandEvent::PointerLeave => ui.pointer_press = None,
                WaylandEvent::PointerButton {
                    x,
                    y,
                    button,
                    pressed,
                } => {
                    if pressed {
                        redraw = true;
                    }
                    if let Some(id) = ui.pointer(scene.as_ref(), x, y, button, pressed) {
                        requested = selected_id_action(&menu, &id);
                    }
                }
                WaylandEvent::TouchDown { id, x, y } => {
                    ui.touch_down(scene.as_ref(), id, x, y);
                    redraw = true;
                }
                WaylandEvent::TouchMotion { id, x, y } => {
                    if let Some(step) = ui.touch_move(id, x, y) {
                        if ui.detail.is_some() {
                            ui.scroll_body(&snapshot, isize::try_from(step).unwrap_or(0));
                        } else if ui.confirm_ids.is_none() {
                            for _ in 0..step.unsigned_abs() {
                                state.handle(
                                    &menu,
                                    if step > 0 {
                                        SemanticInput::Down
                                    } else {
                                        SemanticInput::Up
                                    },
                                );
                            }
                        }
                        redraw = true;
                    }
                }
                WaylandEvent::TouchUp { id } => {
                    if let Some(hit) = ui.touch_up(scene.as_ref(), id) {
                        requested = selected_id_action(&menu, &hit);
                    }
                }
                WaylandEvent::TouchCancel { ids } => ui.touch_cancel(&ids),
                WaylandEvent::Key { key, pressed: true } => {
                    ui.reset_gestures();
                    match key {
                        Key::Escape => {
                            if ui.confirm_ids.take().is_some() {
                                redraw = true;
                            } else if ui.detail.take().is_some() {
                                redraw = true;
                                ui.body_offset = 0;
                            } else {
                                should_close = true;
                            }
                        }
                        Key::Up => {
                            if ui.detail.is_some() {
                                ui.scroll_body(&snapshot, -1);
                            } else {
                                state.handle(&menu, SemanticInput::Up);
                            }
                            redraw = true;
                        }
                        Key::Down => {
                            if ui.detail.is_some() {
                                ui.scroll_body(&snapshot, 1);
                            } else {
                                state.handle(&menu, SemanticInput::Down);
                            }
                            redraw = true;
                        }
                        Key::Tab => {
                            state.handle(&menu, SemanticInput::Down);
                            redraw = true;
                        }
                        Key::Enter | Key::Space => requested = selected_action(&menu, &state),
                        Key::Function(5) => requested = Some(cmd("refresh", "")),
                        Key::Raw(104) => {
                            ui.scroll_body(&snapshot, -(BODY_VISIBLE_LINES as isize));
                            redraw = true;
                        }
                        Key::Raw(109) => {
                            ui.scroll_body(&snapshot, BODY_VISIBLE_LINES as isize);
                            redraw = true;
                        }
                        Key::Backspace if ui.confirm_ids.is_none() && ui.detail.is_none() => {
                            ui.query.pop();
                            redraw = true;
                        }
                        Key::Text(text)
                            if ui.confirm_ids.is_none()
                                && ui.detail.is_none()
                                && text.chars().all(safe_visible_char) =>
                        {
                            for ch in text.chars() {
                                if ui.query.chars().count() >= MAX_QUERY_CHARS {
                                    break;
                                }
                                ui.query.push(ch);
                            }
                            redraw = true;
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            if should_close {
                close(&mut backend, surface, backdrop)?;
                return Ok(());
            }
            if let Some(action) = requested {
                perform(action, &mut ui, &mut snapshot, &mut provider, fixture_mode);
                redraw = true;
            }
            if redraw {
                let previous = state.selected_id.clone();
                menu = model(&snapshot, &ui);
                state = MenuState::new(&menu);
                if let Some(previous) = previous {
                    if menu
                        .item_by_id(&previous)
                        .is_some_and(|item| item.is_actionable())
                    {
                        state.selected_id = Some(previous);
                    }
                }
                if let Some((width, height, scale)) = geometry {
                    scene = Some(draw(
                        &mut backend,
                        surface,
                        &renderer,
                        &config,
                        &menu,
                        &state,
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
    let args = match parse_args() {
        Ok(args) => args,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let config = load_config_or_default(args.config.as_deref()).unwrap_or_default();
    match run(config, args.fixture) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("nuraloumi-notifications: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(snapshot: &mut NotificationSnapshot, ui: &mut UiState, action: MenuAction) {
        let mut provider = NotificationProvider::system();
        perform(action, ui, snapshot, &mut provider, true);
    }

    #[test]
    fn groups_by_app_filters_body_and_keeps_stable_ids() {
        let snapshot = fixture();
        let ui = UiState::default();
        let menu = model(&snapshot, &ui);
        assert!(menu.item_by_id("app-0").is_some());
        assert!(menu.item_by_id("app-1").is_some());
        assert!(menu.item_by_id("view-1").is_some());
        assert_eq!(filtered(&snapshot, "Sample message body 2").len(), 1);
        assert_eq!(filtered(&snapshot, "unmatched").len(), 0);
        assert_eq!(filtered(&snapshot, "").len(), 10);
    }

    #[test]
    fn read_state_session_only_and_detail_returns_to_inbox() {
        let mut snapshot = fixture();
        let mut ui = UiState::default();
        action(&mut snapshot, &mut ui, cmd("view", "1"));
        assert!(ui.read.contains(&1));
        assert_eq!(ui.detail, Some(1));
        assert!(model(&snapshot, &ui).item_by_id("body-next").is_some());
        action(&mut snapshot, &mut ui, cmd("scroll-down", ""));
        assert!(ui.body_offset > 0);
        action(&mut snapshot, &mut ui, cmd("read", "1"));
        assert!(!ui.read.contains(&1));
        action(&mut snapshot, &mut ui, cmd("back", ""));
        assert!(ui.detail.is_none());
    }

    #[test]
    fn confirmation_cancel_and_stale_clear_protection() {
        let mut snapshot = fixture();
        let mut ui = UiState::default();
        action(&mut snapshot, &mut ui, cmd("clear", ""));
        assert_eq!(snapshot.notifications.len(), 10);
        action(&mut snapshot, &mut ui, cmd("ask-clear", ""));
        assert!(model(&snapshot, &ui).item_by_id("confirm-clear").is_some());
        action(&mut snapshot, &mut ui, cmd("cancel", ""));
        assert_eq!(snapshot.notifications.len(), 10);
        action(&mut snapshot, &mut ui, cmd("ask-clear", ""));
        snapshot.notifications.pop();
        action(&mut snapshot, &mut ui, cmd("clear", ""));
        assert_eq!(snapshot.notifications.len(), 9);
        assert!(ui.error.is_some());
        action(&mut snapshot, &mut ui, cmd("ask-clear", ""));
        action(&mut snapshot, &mut ui, cmd("clear", ""));
        assert!(snapshot.notifications.is_empty());
    }

    #[test]
    fn action_identity_must_exist_in_current_history() {
        let mut snapshot = fixture();
        let mut ui = UiState::default();
        action(&mut snapshot, &mut ui, cmd("invoke", "1:forged"));
        assert!(ui.error.is_some());
        assert_eq!(snapshot.notifications.len(), 10);
        action(&mut snapshot, &mut ui, cmd("dismiss", "999"));
        assert_eq!(snapshot.notifications.len(), 10);
        action(&mut snapshot, &mut ui, cmd("dismiss", "1"));
        assert_eq!(snapshot.notifications.len(), 9);
    }

    #[test]
    fn untrusted_text_cannot_inject_controls_or_unbounded_query() {
        assert!(!clean("hello\nworld\u{202e}", 40).contains('\n'));
        assert!(!clean("hello\u{202e}", 40).contains('\u{202e}'));
        assert_eq!(clean(&"x".repeat(500), 32).chars().count(), 33);
        assert_eq!(
            body_lines(&"x".repeat(800))
                .iter()
                .map(|v| v.chars().count())
                .sum::<usize>(),
            240
        );
    }

    #[test]
    fn touch_swipe_scrolls_and_cannot_accidentally_activate() {
        let scene = test_scene();
        let (x, y) = hit_center(&scene, "clear-all");
        let mut ui = UiState::default();
        ui.touch_down(Some(&scene), 10, x, y);
        assert_eq!(ui.touch_move(10, x, y - 280.0), Some(8));
        assert_eq!(ui.touch_up(Some(&scene), 10), None);
        ui.touch_down(Some(&scene), 11, x, y);
        assert_eq!(ui.touch_move(11, x, y + 56.0), Some(-2));
        assert_eq!(ui.touch_up(Some(&scene), 11), None);
    }

    #[test]
    fn keyboard_and_touch_resolve_the_same_typed_action() {
        let snapshot = fixture();
        let menu = model(&snapshot, &UiState::default());
        let mut state = MenuState::new(&menu);
        state.handle(&menu, SemanticInput::Down);
        let id = state.selected_id.clone().expect("keyboard selected a row");
        assert_eq!(
            selected_action(&menu, &state),
            selected_id_action(&menu, &id)
        );
        state.handle(&menu, SemanticInput::Up);
        assert!(selected_action(&menu, &state).is_some());
    }

    #[test]
    fn scene_actionable_rows_are_touch_sized_and_error_has_retry() {
        let scene = test_scene();
        assert!(scene
            .hits
            .iter()
            .filter(|hit| hit.actionable && hit.enabled)
            .all(|hit| hit.rect.height >= 48.0));
        let mut unavailable = NotificationSnapshot::unavailable(0, "service missing".into());
        assert!(model(&unavailable, &UiState::default())
            .item_by_id("retry")
            .is_some());
        unavailable.notifications.clear();
        let ui = UiState {
            query: "nothing".into(),
            ..UiState::default()
        };
        assert!(model(&unavailable, &ui).item_by_id("empty").is_some());
    }

    #[test]
    fn pointer_releases_inside_and_not_on_press_or_outside() {
        let scene = test_scene();
        let (x, y) = hit_center(&scene, "clear-all");
        let mut ui = UiState::default();
        assert_eq!(ui.pointer(Some(&scene), x, y, PRIMARY_BUTTON, true), None);
        assert_eq!(
            ui.pointer(Some(&scene), -1.0, -1.0, PRIMARY_BUTTON, false),
            None
        );
        ui.pointer(Some(&scene), x, y, PRIMARY_BUTTON, true);
        assert_eq!(
            ui.pointer(Some(&scene), x, y, PRIMARY_BUTTON, false),
            Some("clear-all".into())
        );
    }

    #[test]
    fn multitouch_cancels_activation_until_all_fingers_lift() {
        let scene = test_scene();
        let (x, y) = hit_center(&scene, "clear-all");
        let mut ui = UiState::default();
        ui.touch_down(Some(&scene), 1, x, y);
        ui.touch_down(Some(&scene), 2, x, y);
        assert_eq!(ui.touch_up(Some(&scene), 1), None);
        assert_eq!(ui.touch_up(Some(&scene), 2), None);
        ui.touch_down(Some(&scene), 3, x, y);
        assert_eq!(ui.touch_move(3, x, y), None);
        assert_eq!(ui.touch_up(Some(&scene), 3), Some("clear-all".into()));
        ui.touch_down(Some(&scene), 4, x, y);
        ui.touch_cancel(&[4]);
        assert_eq!(ui.touch_up(Some(&scene), 4), None);
    }

    fn test_scene() -> Scene {
        let snapshot = fixture();
        let ui = UiState::default();
        let menu = model(&snapshot, &ui);
        let state = MenuState::new(&menu);
        let renderer = CairoRenderer::default();
        let (scene, _) = render_menu_with_glyphs(
            &renderer,
            &menu,
            &state,
            Viewport::new(448.0, 720.0, 1.0),
            &DARK_THEME,
            RenderOptions::default(),
            None,
            None,
        )
        .unwrap();
        scene
    }

    fn hit_center(scene: &Scene, id: &str) -> (f64, f64) {
        let hit = scene.hits.iter().find(|h| h.item_id == id).unwrap();
        assert!(hit.rect.height >= 48.0);
        (
            hit.rect.x + hit.rect.width / 2.0,
            hit.rect.y + hit.rect.height / 2.0,
        )
    }
}
