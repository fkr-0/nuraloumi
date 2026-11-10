use nuraloumi_core::{
    MenuAction, MenuItem, MenuModel, MenuState, SemanticInput, DARK_THEME, LIGHT_THEME,
};
use nuraloumi_providers::file_open::{self, DesktopHandler, OpenOutcome};
use nuraloumi_render_cairo::{CairoRenderer, RenderOptions, Scene, Viewport};
use nuraloumi_shell::{
    load_config_or_default, render_menu_with_glyphs, ShellConfig, Theme as ShellTheme,
};
use nuraloumi_wayland::{
    BackendError, DismissBackdropConfig, Frame, Key, MenuConfig as WaylandMenuConfig, PixelFormat,
    PlatformEvent as WaylandEvent, SurfaceId, WaylandBackend,
};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

// Keep chooser actions visible on a 400px SL101 popup, including Cancel.
const OPEN_WITH_PAGE_ROWS: usize = 2;
const MAX_EXECUTABLE_SCAN_PER_DIR: usize = 2048;
const MAX_EXECUTABLE_NAMES: usize = 4096;
const MAX_RECENT_SCAN_PER_DIR: usize = 512;

const HELP: &str = r#"nuraloumi-run — NuraLoumi command runner

USAGE:
    nuraloumi-run [OPTIONS]

OPTIONS:
    --live                     Open a native Wayland/Cairo software-rendered run dialog
    --config <path>            Load JSON/TOML shell geometry/theme config
    --prompt <text>            Custom prompt text (default: "Run:")
    --path-complete            Populate with PATH executables for completion
    --recent-files             Include recently modified files and directories
    -h, --help                 Show this help

DESCRIPTION:
    A dmenu/rofi-style execute dialog for issuing commands with the compositor's
    environment. Opens as a layer-shell overlay. Type a command and press Enter
    to execute it. Escape or outside press to dismiss.
"#;

#[derive(Debug, Default)]
struct Args {
    live: bool,
    config: Option<PathBuf>,
    prompt: Option<String>,
    path_complete: bool,
    recent_files: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args::default();
    let mut argv = env::args().skip(1);
    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                eprint!("{HELP}");
                std::process::exit(0);
            }
            "--live" => args.live = true,
            "--config" => {
                args.config = Some(argv.next().ok_or("--config requires a path")?.into());
            }
            "--prompt" => {
                args.prompt = Some(argv.next().ok_or("--prompt requires text")?);
            }
            "--path-complete" => args.path_complete = true,
            "--recent-files" => args.recent_files = true,
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(args)
}

fn collect_path_executables() -> Vec<String> {
    let path_var = match env::var("PATH") {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    let mut names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    'dirs: for dir in path_var.split(':') {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.take(MAX_EXECUTABLE_SCAN_PER_DIR).flatten() {
            if names.len() >= MAX_EXECUTABLE_NAMES {
                break 'dirs;
            }
            if let Ok(metadata) = entry.metadata() {
                use std::os::unix::fs::PermissionsExt;
                if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 {
                    if let Some(name) = entry.file_name().to_str() {
                        names.insert(name.to_string());
                    }
                }
            }
        }
    }
    names.into_iter().collect()
}

#[derive(Clone)]
struct RecentEntry {
    display: String,
    path: PathBuf,
    is_dir: bool,
}

fn collect_recent_files() -> Vec<RecentEntry> {
    let home = match env::var("HOME") {
        Ok(h) => PathBuf::from(h),
        Err(_) => return Vec::new(),
    };

    let scan_dirs: Vec<PathBuf> = vec![
        home.clone(),
        home.join("Documents"),
        home.join("Downloads"),
        home.join("Desktop"),
        home.join("Pictures"),
        home.join("Music"),
        home.join("Videos"),
        home.join("Projects"),
        home.join("code"),
    ];

    let mut entries: Vec<(std::time::SystemTime, RecentEntry)> = Vec::new();

    for dir in &scan_dirs {
        let read = match fs::read_dir(dir) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for entry in read.take(MAX_RECENT_SCAN_PER_DIR).flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
            let path = entry.path();
            let display = shorten_path(&path.to_string_lossy(), &home);
            entries.push((
                mtime,
                RecentEntry {
                    display,
                    path,
                    is_dir: meta.is_dir(),
                },
            ));
        }
    }

    entries.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    entries.truncate(100);
    entries.into_iter().map(|(_, e)| e).collect()
}

fn shorten_path(path: &str, home: &Path) -> String {
    let home_str = home.to_string_lossy();
    if let Some(rest) = path.strip_prefix(home_str.as_ref()) {
        format!("~{rest}")
    } else {
        path.to_string()
    }
}

fn build_run_model(
    prompt: &str,
    executables: &[String],
    recent: &[RecentEntry],
    query: &str,
) -> MenuModel {
    let mut items = Vec::new();

    items.push(
        MenuItem::status(
            "run-input",
            if query.is_empty() {
                prompt.to_string()
            } else {
                format!("{prompt} {query}")
            },
        )
        .with_subtitle("Type a command, Enter to execute".to_string()),
    );

    items.push(MenuItem::separator("sep-input"));

    if !query.is_empty() {
        items.push(MenuItem::action(
            "run-execute",
            format!("Execute: {query}"),
            MenuAction::Custom {
                kind: "execute".into(),
                payload: query.to_string(),
            },
        ));
        items.push(MenuItem::separator("sep-results"));
    }

    let lower_query = query.to_lowercase();

    if !recent.is_empty() {
        let mut file_count = 0;
        let mut has_header = false;
        for (index, entry) in recent.iter().enumerate() {
            if !query.is_empty()
                && !entry.display.to_lowercase().contains(&lower_query)
                && !entry
                    .path
                    .to_string_lossy()
                    .to_lowercase()
                    .contains(&lower_query)
            {
                continue;
            }
            if file_count >= 15 {
                break;
            }
            if !has_header {
                items.push(MenuItem::status("recent-header", "Recent files"));
                items.push(MenuItem::separator("sep-recent"));
                has_header = true;
            }
            let kind = "open-recent";
            let subtitle = if entry.is_dir {
                "Directory".to_string()
            } else {
                String::new()
            };
            items.push(
                MenuItem::action(
                    format!("recent-{index}"),
                    entry.display.clone(),
                    MenuAction::Custom {
                        kind: kind.into(),
                        payload: index.to_string(),
                    },
                )
                .with_subtitle(subtitle),
            );
            file_count += 1;
        }
        if has_header && !executables.is_empty() {
            items.push(MenuItem::separator("sep-exec-header"));
            items.push(MenuItem::status("exec-header", "Commands"));
            items.push(MenuItem::separator("sep-exec"));
        }
    }

    let mut count = 0;
    for name in executables {
        if !query.is_empty() && !name.to_lowercase().contains(&lower_query) {
            continue;
        }
        if count >= 50 {
            break;
        }
        items.push(MenuItem::action(
            format!("exec-{name}"),
            name.clone(),
            MenuAction::Custom {
                kind: "execute".into(),
                payload: name.clone(),
            },
        ));
        count += 1;
    }

    MenuModel::new("run-dialog", "Run", items)
}

fn execute_command(command: &str) -> Result<(), String> {
    Command::new("/bin/sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to spawn: {e}"))?;
    Ok(())
}

#[derive(Debug)]
enum DialogState {
    Run,
    Busy,
    OpenWith {
        path: PathBuf,
        reason: String,
        handlers: Vec<DesktopHandler>,
        page: usize,
    },
    Diagnostic {
        path: Option<PathBuf>,
        reason: String,
        description: String,
    },
}
fn safe_label(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' => '�',
            c if c.is_control() => ' ',
            c => c,
        })
        .take(160)
        .collect()
}
fn diagnostic(path: Option<PathBuf>, reason: String) -> DialogState {
    let description = path
        .as_deref()
        .map(|p| file_open::describe_file(p).unwrap_or_else(|e| format!("file -- PATH: {e}")))
        .unwrap_or_else(|| "No file was opened".into());
    DialogState::Diagnostic {
        path,
        reason,
        description,
    }
}
fn build_dialog_model(
    prompt: &str,
    commands: &[String],
    recent: &[RecentEntry],
    query: &str,
    dialog: &DialogState,
) -> MenuModel {
    match dialog {
        DialogState::Run => build_run_model(prompt, commands, recent, query),
        DialogState::Busy => MenuModel::new(
            "open-busy",
            "Opening file",
            vec![MenuItem::status(
                "open-progress",
                "Resolving MIME, handler or file description…",
            )
            .with_subtitle("Bounded background request; keyboard and touch remain responsive")],
        ),
        DialogState::OpenWith {
            path,
            reason,
            handlers,
            page,
        } => {
            let mut items = vec![
                MenuItem::status(
                    "open-with-header",
                    format!("Open With: {}", safe_label(&path.to_string_lossy())),
                )
                .with_subtitle(safe_label(reason)),
                MenuItem::separator("sep-open-with"),
            ];
            for (index, app) in handlers
                .iter()
                .enumerate()
                .skip(page * OPEN_WITH_PAGE_ROWS)
                .take(OPEN_WITH_PAGE_ROWS)
            {
                items.push(
                    MenuItem::action(
                        format!("handler-{index}"),
                        app.name.clone(),
                        MenuAction::Custom {
                            kind: "choose-handler".into(),
                            payload: index.to_string(),
                        },
                    )
                    .with_subtitle(app.id.clone()),
                );
            }
            if *page > 0 {
                items.push(MenuItem::action(
                    "handler-prev",
                    "Previous handlers",
                    MenuAction::Custom {
                        kind: "handler-prev".into(),
                        payload: String::new(),
                    },
                ));
            }
            if (*page + 1) * OPEN_WITH_PAGE_ROWS < handlers.len() {
                items.push(MenuItem::action(
                    "handler-next",
                    "More handlers",
                    MenuAction::Custom {
                        kind: "handler-next".into(),
                        payload: String::new(),
                    },
                ));
            }
            items.push(MenuItem::action(
                "describe-file",
                "Describe file",
                MenuAction::Custom {
                    kind: "describe-file".into(),
                    payload: String::new(),
                },
            ));
            items.push(MenuItem::action(
                "cancel-open-with",
                "Cancel",
                MenuAction::Custom {
                    kind: "cancel-open".into(),
                    payload: String::new(),
                },
            ));
            MenuModel::new("open-with-dialog", "Open With", items)
        }
        DialogState::Diagnostic {
            path,
            reason,
            description,
        } => {
            let mut items = vec![
                MenuItem::status("open-error", safe_label(reason)),
                MenuItem::status("file-description", safe_label(description)),
                MenuItem::separator("sep-diagnostic"),
            ];
            if path.is_some() {
                items.push(MenuItem::action(
                    "retry-open-with",
                    "Open With...",
                    MenuAction::Custom {
                        kind: "retry-open-with".into(),
                        payload: String::new(),
                    },
                ));
            }
            items.push(MenuItem::action(
                "cancel-open",
                "Back to Run",
                MenuAction::Custom {
                    kind: "cancel-open".into(),
                    payload: String::new(),
                },
            ));
            MenuModel::new("file-diagnostic", "File diagnostic", items)
        }
    }
}
/// Returns true only when an explicit launch request was accepted.
fn activate_action(
    dialog: &mut DialogState,
    recent: &[RecentEntry],
    kind: &str,
    payload: &str,
) -> bool {
    match kind {
        "execute" => match execute_command(payload) {
            Ok(()) => true,
            Err(e) => {
                *dialog = diagnostic(None, format!("command failed: {e}"));
                false
            }
        },
        "open-recent" => {
            let Some(entry) = payload.parse::<usize>().ok().and_then(|i| recent.get(i)) else {
                *dialog = diagnostic(None, "recent file selection is no longer valid".into());
                return false;
            };
            match file_open::open_default(&entry.path) {
                OpenOutcome::Opened { .. } => true,
                OpenOutcome::NeedsOpenWith { reason, handlers } => {
                    *dialog = DialogState::OpenWith {
                        path: entry.path.clone(),
                        reason,
                        handlers,
                        page: 0,
                    };
                    false
                }
                OpenOutcome::NeedsFallback { reason } => {
                    *dialog = diagnostic(Some(entry.path.clone()), reason);
                    false
                }
            }
        }
        "choose-handler" => {
            let DialogState::OpenWith { path, handlers, .. } = dialog else {
                return false;
            };
            let Some(app) = payload.parse::<usize>().ok().and_then(|i| handlers.get(i)) else {
                return false;
            };
            let path = path.clone();
            let id = app.id.clone();
            match file_open::open_with(&path, &id) {
                Ok(_) => true,
                Err(e) => {
                    *dialog = diagnostic(Some(path), format!("Open With {id} failed: {e}"));
                    false
                }
            }
        }
        "handler-next" => {
            if let DialogState::OpenWith { page, handlers, .. } = dialog {
                if (*page + 1) * OPEN_WITH_PAGE_ROWS < handlers.len() {
                    *page += 1;
                }
            }
            false
        }
        "handler-prev" => {
            if let DialogState::OpenWith { page, .. } = dialog {
                *page = page.saturating_sub(1);
            }
            false
        }
        "describe-file" => {
            if let DialogState::OpenWith { path, reason, .. } = dialog {
                *dialog = diagnostic(Some(path.clone()), reason.clone());
            }
            false
        }
        "retry-open-with" => {
            if let DialogState::Diagnostic {
                path: Some(path),
                reason,
                ..
            } = dialog
            {
                let handlers = file_open::handlers_for_path(path);
                if !handlers.is_empty() {
                    *dialog = DialogState::OpenWith {
                        path: path.clone(),
                        reason: reason.clone(),
                        handlers,
                        page: 0,
                    };
                }
            }
            false
        }
        "cancel-open" => {
            *dialog = DialogState::Run;
            false
        }
        _ => false,
    }
}

type OpenAction = (String, String);

fn action_at(scene: Option<&Scene>, model: &MenuModel, x: f64, y: f64) -> Option<OpenAction> {
    let id = find_hit_region(scene?, x, y)?;
    let MenuAction::Custom { kind, payload } = model.item_by_id(&id)?.action.as_ref()? else {
        return None;
    };
    Some((kind.clone(), payload.clone()))
}

// A press is never enough to launch a file. Pointer release must hit the
// same semantic action; touch release must retain its original contact id.
fn release_action(
    pressed: &mut Option<OpenAction>,
    release: Option<OpenAction>,
) -> Option<OpenAction> {
    if pressed.as_ref() == release.as_ref() {
        pressed.take()
    } else {
        *pressed = None;
        None
    }
}

type FileJobResult = (DialogState, bool);
fn dispatch_or_queue(
    dialog: &mut DialogState,
    recent: &[RecentEntry],
    kind: &str,
    payload: &str,
    pending: &mut Option<Receiver<FileJobResult>>,
) -> bool {
    // Preserve user-entered commands as a distinct, explicit action.
    if !matches!(
        kind,
        "open-recent" | "choose-handler" | "describe-file" | "retry-open-with"
    ) {
        return activate_action(dialog, recent, kind, payload);
    }
    if pending.is_some() {
        return false;
    }
    let previous = std::mem::replace(dialog, DialogState::Busy);
    let files = recent.to_vec();
    let kind = kind.to_owned();
    let payload = payload.to_owned();
    let (sender, receiver) = mpsc::sync_channel(1);
    match thread::Builder::new()
        .name("nuraloumi-file-open".to_owned())
        .spawn(move || {
            let mut current = previous;
            let close = activate_action(&mut current, &files, &kind, &payload);
            let _ = sender.send((current, close));
        }) {
        Ok(_) => *pending = Some(receiver),
        Err(error) => *dialog = diagnostic(None, format!("cannot start file opener: {error}")),
    }
    false
}

// Poll the compositor FD briefly only while an opener worker is active.
// Idle still uses blocking Wayland dispatch and consumes no timer CPU.
#[cfg(unix)]
fn dispatch_while_opening(backend: &mut WaylandBackend) -> Result<(), String> {
    use std::os::raw::{c_int, c_short};
    #[repr(C)]
    struct PollFd {
        fd: c_int,
        events: c_short,
        revents: c_short,
    }
    unsafe extern "C" {
        fn poll(fds: *mut PollFd, nfds: usize, timeout: c_int) -> c_int;
    }
    const POLLIN: c_short = 1;
    backend.flush().map_err(|e| format!("Wayland flush: {e}"))?;
    let mut fd = PollFd {
        fd: backend.poll_fd(),
        events: POLLIN,
        revents: 0,
    };
    let result = unsafe { poll(&mut fd, 1, 40) };
    if result < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(format!("Wayland fd poll: {error}"));
        }
    } else if result > 0 && fd.revents != 0 {
        backend
            .blocking_dispatch()
            .map_err(|e| format!("Wayland dispatch: {e}"))?;
    } else {
        backend
            .dispatch_pending()
            .map_err(|e| format!("Wayland dispatch pending: {e}"))?;
    }
    Ok(())
}

fn glyph_for_item_with_dirs(id: &str, recent: &[RecentEntry]) -> Option<char> {
    if id == "run-input" {
        return Some('>');
    }
    if id == "run-execute" {
        return Some('$');
    }
    if id.starts_with("recent-") {
        if let Some(idx) = id
            .strip_prefix("recent-")
            .and_then(|s| s.parse::<usize>().ok())
        {
            if let Some(entry) = recent.get(idx) {
                return if entry.is_dir { Some('/') } else { Some('~') };
            }
        }
        return Some('~');
    }
    if id.starts_with("exec-") {
        return Some('#');
    }
    None
}

fn run_live(config: ShellConfig, args: &Args) -> Result<(), String> {
    let mut backend =
        WaylandBackend::connect().map_err(|e| format!("Wayland connection failed: {e}"))?;
    let capabilities = backend.capabilities();
    if !capabilities.layer_shell {
        return Err("compositor does not advertise zwlr_layer_shell_v1".into());
    }

    let prompt = args.prompt.as_deref().unwrap_or("Run:");
    let executables = if args.path_complete {
        collect_path_executables()
    } else {
        Vec::new()
    };
    let recent = if args.recent_files {
        collect_recent_files()
    } else {
        Vec::new()
    };

    let mut query = String::new();
    let mut dialog = DialogState::Run;
    let mut model = build_dialog_model(prompt, &executables, &recent, &query, &dialog);
    let mut state = MenuState::new(&model);

    let output = backend.outputs().into_iter().next();
    let requested_width = output
        .as_ref()
        .and_then(|o| {
            let scale = o.scale.max(1) as u32;
            (o.mode_width > 0).then_some(o.mode_width as u32 / scale)
        })
        .map(|w| config.menu_width.min(w.saturating_sub(16).max(240)))
        .unwrap_or(config.menu_width);
    let requested_height = output
        .as_ref()
        .and_then(|o| {
            let scale = o.scale.max(1) as u32;
            (o.mode_height > 0).then_some(o.mode_height as u32 / scale)
        })
        .map(|h| {
            h.saturating_sub(config.panel_height.saturating_add(16))
                .clamp(200, 480)
        })
        .unwrap_or(400);

    let dismiss_surface = {
        let (mt, mr, mb, ml) =
            nuraloumi_shell::dismiss_backdrop_margins(config.panel_edge, config.panel_height);
        backend
            .create_dismiss_backdrop(DismissBackdropConfig {
                margin_top: mt,
                margin_right: mr,
                margin_bottom: mb,
                margin_left: ml,
                output: output.as_ref().map(|o| o.id),
                namespace: "nuraloumi-run-dismiss".into(),
            })
            .ok()
    };

    let surface = match backend.create_menu(WaylandMenuConfig {
        width: requested_width,
        height: requested_height,
        margin_top: config.panel_height as i32,
        margin_left: 0,
        output: output.as_ref().map(|o| o.id),
        namespace: "nuraloumi-run".into(),
    }) {
        Ok(s) => s,
        Err(e) => {
            if let Some(ds) = dismiss_surface {
                let _ = backend.destroy_surface(ds);
            }
            return Err(format!("failed to create run surface: {e}"));
        }
    };

    let renderer = CairoRenderer::default();
    let tokens = match config.theme {
        ShellTheme::Dark => &DARK_THEME,
        ShellTheme::Light => &LIGHT_THEME,
    };
    let mut geometry: Option<(u32, u32, i32)> = None;
    let mut scene: Option<Scene> = None;
    let mut pointer_press: Option<OpenAction> = None;
    let mut touch_press: Option<(i32, OpenAction)> = None;
    let mut pending: Option<Receiver<FileJobResult>> = None;

    loop {
        if pending.is_some() {
            dispatch_while_opening(&mut backend)?;
        } else {
            backend
                .blocking_dispatch()
                .map_err(|e| format!("Wayland dispatch failed: {e}"))?;
        }

        // Worker completion repaints even if Wayland sends no further input.
        if let Some(result) = pending.as_ref().map(Receiver::try_recv) {
            let finished = match result {
                Ok(value) => Some(value),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some((
                    diagnostic(None, "file opener worker terminated unexpectedly".into()),
                    false,
                )),
            };
            if let Some((next_dialog, close)) = finished {
                pending = None;
                if close {
                    close_surfaces(&mut backend, surface, dismiss_surface)?;
                    return Ok(());
                }
                dialog = next_dialog;
                model = build_dialog_model(prompt, &executables, &recent, &query, &dialog);
                state = MenuState::new(&model);
                pointer_press = None;
                touch_press = None;
                if let Some((w, h, s)) = geometry {
                    let resolver = |id: &str| glyph_for_item_with_dirs(id, &recent);
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
                        Some(&resolver),
                    )?);
                }
            }
        }

        let events: Vec<_> = backend.drain_events().collect();
        for event in events {
            if let Some(ds) = dismiss_surface {
                if event.surface == Some(ds) {
                    match event.event {
                        WaylandEvent::Configure {
                            width,
                            height,
                            scale,
                        } => {
                            present_transparent(&mut backend, ds, width, height, scale)?;
                        }
                        WaylandEvent::PointerButton { pressed: true, .. }
                        | WaylandEvent::TouchDown { .. } => {
                            close_surfaces(&mut backend, surface, dismiss_surface)?;
                            return Ok(());
                        }
                        WaylandEvent::Close => {
                            close_surfaces(&mut backend, surface, dismiss_surface)?;
                            return Ok(());
                        }
                        _ => {}
                    }
                    continue;
                }
            }
            if event.surface.is_some() && event.surface != Some(surface) {
                continue;
            }

            let mut redraw = false;
            let mut requested: Option<OpenAction> = None;

            match event.event {
                WaylandEvent::Configure {
                    width,
                    height,
                    scale,
                } if event.surface == Some(surface) => {
                    geometry = Some((width, height, scale));
                    redraw = true;
                }
                WaylandEvent::Key { key, pressed: true } => match key {
                    Key::Escape => {
                        if pending.is_some() {
                            continue;
                        }
                        if matches!(dialog, DialogState::Run) {
                            close_surfaces(&mut backend, surface, dismiss_surface)?;
                            return Ok(());
                        }
                        dialog = DialogState::Run;
                        model = build_dialog_model(prompt, &executables, &recent, &query, &dialog);
                        state = MenuState::new(&model);
                        redraw = true;
                    }
                    Key::Enter => {
                        let action = if let Some(sel) = &state.selected_id {
                            if sel == "run-execute" {
                                Some(("execute".to_string(), query.clone()))
                            } else if let Some(item) = model.item_by_id(sel) {
                                if let Some(MenuAction::Custom { kind, payload }) = &item.action {
                                    Some((kind.clone(), payload.clone()))
                                } else if matches!(dialog, DialogState::Run) {
                                    Some(("execute".to_string(), query.clone()))
                                } else {
                                    None
                                }
                            } else if matches!(dialog, DialogState::Run) {
                                Some(("execute".to_string(), query.clone()))
                            } else {
                                None
                            }
                        } else if !query.is_empty() && matches!(dialog, DialogState::Run) {
                            Some(("execute".to_string(), query.clone()))
                        } else {
                            None
                        };
                        if let Some((kind, payload)) = action {
                            if kind != "execute" || !payload.is_empty() {
                                if dispatch_or_queue(
                                    &mut dialog,
                                    &recent,
                                    &kind,
                                    &payload,
                                    &mut pending,
                                ) {
                                    close_surfaces(&mut backend, surface, dismiss_surface)?;
                                    return Ok(());
                                }
                                model = build_dialog_model(
                                    prompt,
                                    &executables,
                                    &recent,
                                    &query,
                                    &dialog,
                                );
                                state = MenuState::new(&model);
                                redraw = true;
                            }
                        }
                    }
                    Key::Backspace if matches!(dialog, DialogState::Run) => {
                        query.pop();
                        model = build_dialog_model(prompt, &executables, &recent, &query, &dialog);
                        state = MenuState::new(&model);
                        redraw = true;
                    }
                    Key::Up => {
                        state.handle(&model, SemanticInput::Up);
                        redraw = true;
                    }
                    Key::Down => {
                        state.handle(&model, SemanticInput::Down);
                        redraw = true;
                    }
                    Key::Tab if matches!(dialog, DialogState::Run) => {
                        if let Some(sel) = &state.selected_id {
                            if let Some(item) = model.item_by_id(sel) {
                                if let Some(MenuAction::Custom { kind, payload }) = &item.action {
                                    if kind == "execute" {
                                        query = payload.clone();
                                        model = build_dialog_model(
                                            prompt,
                                            &executables,
                                            &recent,
                                            &query,
                                            &dialog,
                                        );
                                        state = MenuState::new(&model);
                                        redraw = true;
                                    }
                                }
                            }
                        }
                    }
                    Key::Text(text)
                        if matches!(dialog, DialogState::Run)
                            && !text.is_empty()
                            && text.chars().all(|c| !c.is_control()) =>
                    {
                        query.push_str(&text);
                        model = build_dialog_model(prompt, &executables, &recent, &query, &dialog);
                        state = MenuState::new(&model);
                        redraw = true;
                    }
                    _ => {}
                },
                WaylandEvent::PointerButton {
                    x,
                    y,
                    pressed: true,
                    button: 0x110,
                } => pointer_press = action_at(scene.as_ref(), &model, x, y),
                WaylandEvent::PointerButton {
                    x,
                    y,
                    pressed: false,
                    button: 0x110,
                } => {
                    requested =
                        release_action(&mut pointer_press, action_at(scene.as_ref(), &model, x, y));
                }
                WaylandEvent::TouchDown { id, x, y } if touch_press.is_none() => {
                    touch_press = action_at(scene.as_ref(), &model, x, y).map(|a| (id, a));
                }
                WaylandEvent::TouchMotion { id, x, y } => {
                    if touch_press.as_ref().is_some_and(|(tid, action)| {
                        *tid == id
                            && action_at(scene.as_ref(), &model, x, y).as_ref() != Some(action)
                    }) {
                        touch_press = None;
                    }
                }
                WaylandEvent::TouchUp { id } => {
                    if touch_press.as_ref().is_some_and(|(tid, _)| *tid == id) {
                        requested = touch_press.take().map(|(_, action)| action);
                    }
                }
                WaylandEvent::TouchCancel { ids } => {
                    if touch_press.as_ref().is_some_and(|(id, _)| ids.contains(id)) {
                        touch_press = None;
                    }
                }
                WaylandEvent::Close => {
                    close_surfaces(&mut backend, surface, dismiss_surface)?;
                    return Ok(());
                }
                _ => {}
            }

            if let Some((kind, payload)) = requested {
                if kind != "execute" || !payload.is_empty() {
                    if dispatch_or_queue(&mut dialog, &recent, &kind, &payload, &mut pending) {
                        close_surfaces(&mut backend, surface, dismiss_surface)?;
                        return Ok(());
                    }
                    model = build_dialog_model(prompt, &executables, &recent, &query, &dialog);
                    state = MenuState::new(&model);
                    redraw = true;
                }
            }
            if redraw {
                pointer_press = None;
                touch_press = None;
                if let Some((w, h, s)) = geometry {
                    let resolver = |id: &str| glyph_for_item_with_dirs(id, &recent);
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
                        Some(&resolver),
                    )?);
                }
            }
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

fn find_hit_region(scene: &Scene, x: f64, y: f64) -> Option<String> {
    scene.hit_test(x, y).map(str::to_owned)
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

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("nuraloumi-run: {e}");
            return ExitCode::from(2);
        }
    };

    if !args.live {
        eprintln!("{HELP}");
        eprintln!("nuraloumi-run requires --live mode");
        return ExitCode::from(2);
    }

    let config = args
        .config
        .as_deref()
        .map(|path| load_config_or_default(Some(path)))
        .unwrap_or_else(|| load_config_or_default(None))
        .unwrap_or_default();

    if let Err(e) = run_live(config, &args) {
        eprintln!("nuraloumi-run: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opener_uses_bounded_background_reply_and_diagnostic_on_invalid_index() {
        use std::time::Duration;
        let mut dialog = DialogState::Run;
        let mut pending = None;
        assert!(!dispatch_or_queue(
            &mut dialog,
            &[],
            "open-recent",
            "999",
            &mut pending,
        ));
        assert!(matches!(dialog, DialogState::Busy));
        let (result, close) = pending
            .take()
            .unwrap()
            .recv_timeout(Duration::from_secs(2))
            .expect("background worker must complete");
        assert!(!close);
        assert!(matches!(result, DialogState::Diagnostic { .. }));
    }

    #[test]
    fn diagnostic_labels_strip_controls_and_bidi_spoofing() {
        assert_eq!(safe_label("abc\u{202e}ok\n"), "abc�ok ");
        assert_eq!(safe_label(&"a".repeat(200)).len(), 160);
    }

    #[test]
    fn pointer_release_must_match_pressed_action() {
        let action = ("open-recent".to_owned(), "1".to_owned());
        let other = ("open-recent".to_owned(), "2".to_owned());
        let mut held = Some(action.clone());
        assert_eq!(release_action(&mut held, Some(other)), None);
        assert!(held.is_none());
        held = Some(action.clone());
        assert_eq!(release_action(&mut held, None), None);
        held = Some(action.clone());
        assert_eq!(
            release_action(&mut held, Some(action.clone())),
            Some(action)
        );
        assert!(held.is_none());
    }

    #[test]
    fn chooser_cancel_and_page_controls_remain_hittable_on_sl101() {
        let handlers = (0..9)
            .map(|index| DesktopHandler {
                id: format!("app{index}.desktop"),
                name: format!("Application {index}"),
                desktop_file: format!("/usr/share/applications/app{index}.desktop").into(),
            })
            .collect();
        let dialog = DialogState::OpenWith {
            path: "/tmp/test.jpg".into(),
            reason: "no MIME default".into(),
            handlers,
            page: 1,
        };
        let model = build_dialog_model("Run:", &[], &[], "", &dialog);
        let state = MenuState::new(&model);
        let viewport = Viewport::new(480.0, 400.0, 1.0);
        let (scene, _) = render_menu_with_glyphs(
            &CairoRenderer::default(),
            &model,
            &state,
            viewport,
            &DARK_THEME,
            RenderOptions::default(),
            None,
            None,
        )
        .unwrap();
        for id in [
            "handler-2",
            "handler-3",
            "handler-prev",
            "handler-next",
            "describe-file",
            "cancel-open-with",
        ] {
            let region = scene
                .hits
                .iter()
                .find(|hit| hit.item_id == id)
                .unwrap_or_else(|| panic!("missing touch action {id}"));
            assert!(region.rect.height >= 48.0, "too small: {id}");
            assert!(
                region.rect.y >= 0.0 && region.rect.bottom() <= 400.0,
                "offscreen action: {id} {:?}",
                region.rect
            );
        }
    }

    #[test]
    fn filtered_recent_rows_keep_original_path_index() {
        let recent = vec![
            RecentEntry {
                display: "first.txt".into(),
                path: "/tmp/first.txt".into(),
                is_dir: false,
            },
            RecentEntry {
                display: "target folder".into(),
                path: "/tmp/target folder".into(),
                is_dir: true,
            },
        ];
        let menu = build_run_model("Run:", &[], &recent, "target");
        let action = menu
            .item_by_id("recent-1")
            .unwrap()
            .action
            .as_ref()
            .unwrap();
        assert!(
            matches!(action, MenuAction::Custom {kind,payload} if kind=="open-recent" && payload=="1")
        );
        assert_eq!(glyph_for_item_with_dirs("recent-1", &recent), Some('/'));
    }

    #[test]
    fn chooser_is_paginated_keyboard_and_touch_action_model() {
        let handlers = (0..9)
            .map(|i| DesktopHandler {
                id: format!("app{i}.desktop"),
                name: format!("Application {i}"),
                desktop_file: format!("/usr/share/applications/app{i}.desktop").into(),
            })
            .collect();
        let mut dialog = DialogState::OpenWith {
            path: "/tmp/a b".into(),
            reason: "no default".into(),
            handlers,
            page: 0,
        };
        let model = build_dialog_model("Run:", &[], &[], "", &dialog);
        assert!(model.item_by_id("handler-next").is_some());
        assert!(model.item_by_id("handler-0").is_some());
        assert!(!activate_action(&mut dialog, &[], "handler-next", ""));
        let model = build_dialog_model("Run:", &[], &[], "", &dialog);
        assert!(model.item_by_id("handler-2").is_some());
        assert!(model.item_by_id("handler-prev").is_some());
        assert!(!activate_action(&mut dialog, &[], "cancel-open", ""));
        assert!(matches!(dialog, DialogState::Run));
    }

    #[test]
    fn diagnostic_surfaces_command_error_and_cancel_works() {
        let mut dialog = diagnostic(None, "helper could not start".into());
        let model = build_dialog_model("Run:", &[], &[], "", &dialog);
        assert!(model.item_by_id("open-error").is_some());
        assert!(model.item_by_id("cancel-open").is_some());
        assert!(!activate_action(&mut dialog, &[], "cancel-open", ""));
        assert!(matches!(dialog, DialogState::Run));
    }
}
