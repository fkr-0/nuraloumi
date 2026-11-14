//! First-class NuraLoumi media UI. Cairo/wl_shm works without GStreamer/EGL.
//! Hardware VDE/Grate video is an optional, in-process playerd engine.
#![allow(dead_code)]
#[path = "../player_engine.rs"]
mod player_engine;
use nuraloumi_core::{
    MenuAction, MenuItem, MenuModel, MenuState, SemanticInput, DARK_THEME, LIGHT_THEME,
};
use nuraloumi_render_cairo::{CairoRenderer, RenderOptions, Scene, Viewport};
use nuraloumi_shell::{
    dismiss_backdrop_margins, load_config_or_default, render_menu_with_glyphs, ShellConfig,
    Theme as ShellTheme,
};
use nuraloumi_wayland::{
    BackendError, DismissBackdropConfig, Frame, Key, MenuConfig as WaylandMenuConfig, PixelFormat,
    PlatformEvent as WaylandEvent, SurfaceId, WaylandBackend,
};
use player_engine::{Command, Playlist, Reply, Snapshot};
use std::{
    collections::HashSet,
    env, fs,
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::{Command as ProcCommand, ExitCode, Stdio},
    time::{Duration, Instant},
};

#[derive(Default)]
struct Args {
    open: Option<String>,
    status: bool,
    config: Option<PathBuf>,
    fixture: bool,
}

/// Configure the sole playback daemon for the SHA-pinned three-pass renderer,
/// when the private engine was bundled by the ARM release. Never launch an
/// alternate player or change EGL requirements of the Cairo menu itself.
fn configure_single_player_daemon(daemon: &mut ProcCommand) {
    // A versioned /opt/nuraloumi/releases/<commit>/bin/player uses the
    // sibling release library. This ensures one atomic 'current' switch
    // updates ALL tools and the exact Grate/GStreamer backend together.
    // Ordinary APK installations use /usr/lib instead.
    let private = env::current_exe()
        .ok()
        .and_then(|executable| executable.parent()?.parent().map(Path::to_path_buf))
        .map(|release| release.join("lib/nuraloumi/video"));
    let system = PathBuf::from("/usr/lib/nuraloumi/video");
    let root = private
        .filter(|path| path.join("lib/libnuraloumi-video.so").is_file())
        .unwrap_or(system);
    let lib = root.join("lib");
    let gst = root.join("gst");
    if !lib.join("libnuraloumi-video.so").is_file()
        || !lib.join("libgallium-25.0.7.so").is_file()
        || !gst.join("libgstv4l2codecs.so").is_file()
    {
        return;
    }
    let system_drm = Path::new("/usr/local/lib/sl101-libdrm-import-lists-5b06215a2876fec6");
    let serializer = Path::new("/usr/local/lib/gr3d_serialize.so");
    // This strictly qualified private stack must not be replaced with
    // unpinned system Mesa or a CPU video fallback.
    if !system_drm.is_dir() || !serializer.is_file() {
        return;
    }
    let old = std::env::var_os("LD_LIBRARY_PATH").unwrap_or_default();
    let mut ld = std::ffi::OsString::from(system_drm.as_os_str());
    ld.push(":");
    ld.push(&lib);
    if !old.is_empty() {
        ld.push(":");
        ld.push(old);
    }
    daemon.env("LD_LIBRARY_PATH", ld);
    daemon.env("LD_PRELOAD", serializer);
    daemon.env("LIBGL_DRIVERS_PATH", &lib);
    daemon.env("GBM_BACKENDS_PATH", lib.join("gbm"));
    daemon.env("MESA_LOADER_DRIVER_OVERRIDE", "grate");
    // Opt in to the exact-TGSI I420 Y+U specialization. The pinned Grate
    // library checks both options and rejects any unsupported shader; no
    // silent external player or CPU YUV conversion may be selected here.
    daemon.env("GRATE_I420_YU_PAIR", "1");
    daemon.env("GRATE_I420_YU_FP_SCHEDULE", "1");
    daemon.env("GST_PLUGIN_PATH_1_0", &gst);
    daemon.env("SL101_PRIVATE_GST_DIR", &gst);
    // No system installation, no KMS/DRM-master access, no software upload.
}
fn args() -> Result<Args, String> {
    let mut out = Args::default();
    let mut a = env::args().skip(1);
    while let Some(arg) = a.next() {
        match arg.as_str() {
            "--open" => out.open = Some(a.next().ok_or("--open needs a path")?),
            "--status" => out.status = true,
            "--fixture" => out.fixture = true,
            "--config" => out.config = Some(a.next().ok_or("--config needs a path")?.into()),
            "--help" | "-h" => {
                println!("nuraloumi-player [--open FILE] [--status] [--config FILE] [--fixture]");
                std::process::exit(0);
            }
            _ if !arg.starts_with('-') && out.open.is_none() => out.open = Some(arg),
            _ => return Err(format!("unknown option: {arg}")),
        }
    }
    Ok(out)
}
fn sockpath() -> Result<PathBuf, String> {
    let p = env::var_os("XDG_RUNTIME_DIR").ok_or("XDG_RUNTIME_DIR required")?;
    let path = PathBuf::from(p).join("nuraloumi-player.sock");
    if !path.is_absolute() || path.as_os_str().len() > 100 {
        return Err("invalid runtime socket path".into());
    }
    Ok(path)
}
// A read timeout is per syscall, not a total deadline. Bound the whole
// response so a slow or malfunctioning daemon cannot freeze the menu.
fn read_response(mut connection: &UnixStream) -> Result<Reply, String> {
    const MAX_RESPONSE: usize = 256 * 1024;
    let deadline = Instant::now() + Duration::from_millis(600);
    let mut response = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("playback response deadline exceeded".into());
        }
        connection
            .set_read_timeout(Some(remaining))
            .map_err(|e| e.to_string())?;
        let mut chunk = [0u8; 1024];
        let count = connection.read(&mut chunk).map_err(|e| e.to_string())?;
        if count == 0 {
            return Err("unterminated playback response".into());
        }
        let delimiter = chunk[..count].iter().position(|byte| *byte == b'\n');
        let consumed = delimiter.map_or(count, |index| index + 1);
        if response.len() + consumed > MAX_RESPONSE {
            return Err("oversized playback response".into());
        }
        response.extend_from_slice(&chunk[..consumed]);
        if delimiter.is_some() {
            return serde_json::from_slice(&response)
                .map_err(|e| format!("invalid playback response: {e}"));
        }
    }
}
fn send(cmd: &Command, spawn_daemon: bool) -> Result<Reply, String> {
    let path = sockpath()?;
    let mut connection = UnixStream::connect(&path);
    if connection.is_err() && spawn_daemon {
        let sibling = env::current_exe()
            .map_err(|e| e.to_string())?
            .with_file_name("nuraloumi-playerd");
        let mut daemon = ProcCommand::new(sibling);
        configure_single_player_daemon(&mut daemon);
        daemon
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("start playback service: {e}"))?;
        for _ in 0..12 {
            std::thread::sleep(Duration::from_millis(100));
            connection = UnixStream::connect(&path);
            if connection.is_ok() {
                break;
            }
        }
    }
    let mut connection = connection.map_err(|e| format!("playback service unavailable: {e}"))?;
    connection
        .set_write_timeout(Some(Duration::from_millis(600)))
        .map_err(|e| e.to_string())?;
    let mut request = serde_json::to_vec(cmd).map_err(|e| e.to_string())?;
    if request.len() > 4095 {
        return Err("command too long".into());
    }
    request.push(b'\n');
    connection.write_all(&request).map_err(|e| e.to_string())?;
    read_response(&connection)
}
fn offline(error: Option<String>) -> Snapshot {
    Snapshot {
        playlist: Playlist::default(),
        status: "Unavailable".into(),
        position_ms: 0,
        duration_ms: 0,
        error,
        audio_backend: "GStreamer service offline".into(),
        gpu_status: "VDE/Grate source qualified; playerd unavailable".into(),
    }
}
fn fixture() -> Snapshot {
    Snapshot {
        playlist: Playlist {
            tracks: vec![
                "/music/Example one.ogg".into(),
                "/music/Example two.mp3".into(),
            ],
            current: Some(0),
            volume: 0.7,
            muted: false,
        },
        status: "Paused".into(),
        position_ms: 38_200,
        duration_ms: 224_000,
        error: None,
        audio_backend: "fixture (NO audio output)".into(),
        gpu_status: "UNSUPPORTED_GPU: fixture does not render video".into(),
    }
}
fn basename(path: &str) -> String {
    let label = Path::new(path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let mut chars = label.chars();
    let shortened: String = chars.by_ref().take(43).collect();
    if chars.next().is_some() {
        format!("{shortened}…")
    } else {
        shortened
    }
}
fn time_string(ms: u64) -> String {
    format!("{}:{:02}", ms / 60_000, (ms / 1000) % 60)
}
// Activate a touch target only after its *own* release, with every other
// contact lifted. A second finger cancels the gesture rather than changing
// the selected target (or activating a row when an unrelated finger lifts).
#[derive(Default)]
struct TouchActivation {
    contacts: HashSet<i32>,
    primary: Option<(i32, Option<String>, f64, f64)>,
}
impl TouchActivation {
    fn down(&mut self, id: i32, at: Option<String>, x: f64, y: f64) {
        self.contacts.insert(id);
        if self.contacts.len() == 1 {
            self.primary = Some((id, at, x, y));
        } else {
            self.primary = None;
        }
    }
    fn motion(&mut self, id: i32, x: f64, y: f64) {
        if let Some((active, _, current_x, current_y)) = &mut self.primary {
            if *active == id {
                *current_x = x;
                *current_y = y;
            }
        }
    }
    fn up(&mut self, id: i32) -> Option<(String, f64, f64)> {
        if !self.contacts.remove(&id) || !self.contacts.is_empty() {
            self.primary = None;
            return None;
        }
        self.primary
            .take()
            .and_then(|(down_id, at, x, y)| (down_id == id).then_some((at?, x, y)))
    }
    fn cancel(&mut self, ids: &[i32]) {
        for id in ids {
            self.contacts.remove(id);
        }
        self.primary = None;
    }
}
#[derive(Clone)]
enum Page {
    Now,
    Queue { start: usize, selected: usize },
    Files { dir: PathBuf, start: usize },
}
struct FileEntry {
    path: PathBuf,
    directory: bool,
}
fn media_extension(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "mp3"
                | "aac"
                | "m4a"
                | "ogg"
                | "oga"
                | "opus"
                | "flac"
                | "wav"
                | "wma"
                | "mp4"
                | "m4v"
                | "mov"
                | "mkv"
                | "webm"
                | "avi"
                | "ts"
                | "mpeg"
                | "mpg"
                | "3gp"
        )
    })
}
fn browse_entries(dir: &Path) -> Vec<FileEntry> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<_> = read
        .take(4096)
        .filter_map(Result::ok)
        .filter_map(|e| {
            let path = e.path();
            if e.file_name().to_string_lossy().starts_with('.') {
                return None;
            }
            let directory = path.is_dir();
            if directory || (path.is_file() && media_extension(&path)) {
                Some(FileEntry { path, directory })
            } else {
                None
            }
        })
        .collect();
    entries.sort_by(|a, b| {
        b.directory
            .cmp(&a.directory)
            .then_with(|| a.path.file_name().cmp(&b.path.file_name()))
    });
    entries
}
fn action(id: &str, label: impl Into<String>, kind: &str, payload: impl Into<String>) -> MenuItem {
    MenuItem::action(
        id,
        label,
        MenuAction::Custom {
            kind: kind.into(),
            payload: payload.into(),
        },
    )
}
fn model(s: &Snapshot, page: &Page, files: &[FileEntry]) -> MenuModel {
    let mut items = Vec::new();
    match page {
        Page::Now => {
            items.push(MenuItem::status("now-title", "Now playing"));
            let title = s
                .playlist
                .current
                .and_then(|i| s.playlist.tracks.get(i))
                .map(|p| basename(p))
                .unwrap_or_else(|| "No media selected".into());
            items.push(MenuItem::status("track", title).with_subtitle(format!(
                "{} · {} / {}",
                s.status,
                time_string(s.position_ms),
                time_string(s.duration_ms)
            )));
            if let Some(error) = &s.error {
                items.push(MenuItem::status(
                    "error",
                    format!("Error: {}", error.chars().take(74).collect::<String>()),
                ));
            }
            items.push(action("previous", "⏮ Previous", "previous", ""));
            items.push(action(
                "toggle",
                if s.status == "Playing" {
                    "⏸ Pause"
                } else {
                    "▶ Play"
                },
                "toggle",
                "",
            ));
            items.push(action("stop", "■ Stop", "stop", ""));
            items.push(action("next", "⏭ Next", "next", ""));
            items.push(action("seek-back", "↶ Seek −10s", "seek_back", ""));
            items.push(action("seek-forward", "↷ Seek +10s", "seek_forward", ""));
            items.push(action("volume-down", "− Volume", "volume_down", ""));
            items.push(action(
                "volume-up",
                format!("+ Volume ({}%)", (s.playlist.volume * 100.0).round()),
                "volume_up",
                "",
            ));
            items.push(action(
                "mute",
                if s.playlist.muted { "Unmute" } else { "Mute" },
                "mute",
                "",
            ));
            items.push(action(
                "queue",
                format!("Playlist ({} tracks)", s.playlist.tracks.len()),
                "queue",
                "",
            ));
            items.push(action("open", "Open media file…", "files", ""));
            items.push(
                MenuItem::status("gpu", "Video: experimental VDE/GPU")
                    .with_subtitle(s.gpu_status.clone()),
            );
        }
        Page::Queue { start, selected } => {
            items.push(MenuItem::status("queue-head", "Ordered playlist"));
            items.push(action("back", "← Player", "home", ""));
            let end = (start + 7).min(s.playlist.tracks.len());
            for i in *start..end {
                let marker = if Some(i) == s.playlist.current {
                    "▶"
                } else if i == *selected {
                    "●"
                } else {
                    "○"
                };
                items.push(action(
                    &format!("track-{i}"),
                    format!("{marker} {}", basename(&s.playlist.tracks[i])),
                    "queue_select",
                    i.to_string(),
                ));
            }
            items.push(action(
                "q-prevpage",
                "↑ Previous page",
                "queue_prevpage",
                "",
            ));
            items.push(action("q-nextpage", "↓ Next page", "queue_nextpage", ""));
            items.push(action("q-play", "▶ Play selected", "queue_play", ""));
            items.push(action("q-up", "↑ Move selected earlier", "queue_up", ""));
            items.push(action("q-down", "↓ Move selected later", "queue_down", ""));
            items.push(action("q-remove", "✕ Remove selected", "queue_remove", ""));
            items.push(action("q-open", "+ Add media file…", "files", ""));
        }
        Page::Files { dir, start } => {
            items.push(MenuItem::status("files-head", "Open media file"));
            items.push(MenuItem::status("directory", {
                let p = dir.to_string_lossy();
                if p.chars().count() > 54 {
                    format!(
                        "…{}",
                        p.chars()
                            .rev()
                            .take(53)
                            .collect::<String>()
                            .chars()
                            .rev()
                            .collect::<String>()
                    )
                } else {
                    p.into_owned()
                }
            }));
            items.push(action("f-back", "← Player", "home", ""));
            items.push(action("f-updir", "↑ Parent directory", "parent_dir", ""));
            let end = (start + 8).min(files.len());
            for (i, entry) in files.iter().enumerate().take(end).skip(*start) {
                let name = entry.path.file_name().unwrap_or_default().to_string_lossy();
                items.push(action(
                    &format!("file-{i}"),
                    format!(
                        "{} {}",
                        if entry.directory { "▸" } else { "♫" },
                        name.chars().take(49).collect::<String>()
                    ),
                    "entry",
                    i.to_string(),
                ));
            }
            items.push(action("f-prev", "↑ Previous files", "files_prevpage", ""));
            items.push(action("f-next", "↓ More files", "files_nextpage", ""));
        }
    };
    MenuModel::new("player", "Media Player", items)
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
    let viewport = Viewport::new(width as f64, height as f64, f64::from(scale.max(1)));
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
    let (w, h, stride) = (info.width as u32, info.height as u32, info.stride as usize);
    let result = buffer
        .with_argb32_bytes(|pixels, _| {
            backend.present(
                surface,
                Frame {
                    width: w,
                    height: h,
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
    scale: i32,
) -> Result<(), String> {
    let s = u32::try_from(scale.max(1)).map_err(|e| e.to_string())?;
    let width = w.checked_mul(s).ok_or("backdrop width overflow")?;
    let height = h.checked_mul(s).ok_or("backdrop height overflow")?;
    let len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|v| v.checked_mul(4))
        .filter(|v| *v <= 16 * 1024 * 1024)
        .ok_or("backdrop allocation limit")?;
    let data = vec![0; len];
    match backend.present(
        surface,
        Frame::packed(width, height, PixelFormat::Argb8888, &data),
    ) {
        Ok(()) | Err(BackendError::WouldBlock) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
fn send_and_update(command: Command, s: &mut Snapshot, fixture_mode: bool) {
    if fixture_mode {
        s.error = Some("fixture mode: controls cannot output sound".into());
    } else {
        match send(&command, false) {
            Ok(reply) => {
                *s = reply.state;
                if !reply.ok {
                    s.error = reply.error;
                }
            }
            Err(e) => s.error = Some(e),
        }
    }
}
fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| PathBuf::from("/"))
}
fn handle_action(
    action: MenuAction,
    page: &mut Page,
    files: &[FileEntry],
    snapshot: &mut Snapshot,
    fixture_mode: bool,
) {
    let MenuAction::Custom { kind, payload } = action else {
        return;
    };
    let command = match kind.as_str() {
        "home" => {
            *page = Page::Now;
            return;
        }
        "files" => {
            *page = Page::Files {
                dir: home_dir(),
                start: 0,
            };
            return;
        }
        "queue" => {
            *page = Page::Queue {
                start: 0,
                selected: snapshot.playlist.current.unwrap_or(0),
            };
            return;
        }
        "parent_dir" => {
            if let Page::Files { dir, start } = page {
                if let Some(parent) = dir.parent() {
                    *dir = parent.to_path_buf();
                    *start = 0;
                }
            }
            return;
        }
        "files_prevpage" => {
            if let Page::Files { start, .. } = page {
                *start = start.saturating_sub(8);
            }
            return;
        }
        "files_nextpage" => {
            if let Page::Files { start, .. } = page {
                if *start + 8 < files.len() {
                    *start += 8;
                }
            }
            return;
        }
        "entry" => {
            let Ok(index) = payload.parse::<usize>() else {
                return;
            };
            let Some(entry) = files.get(index) else {
                return;
            };
            if entry.directory {
                *page = Page::Files {
                    dir: entry.path.clone(),
                    start: 0,
                };
                return;
            }
            let path = entry.path.to_string_lossy().to_string();
            *page = Page::Now;
            Some(Command::Add { path })
        }
        "queue_select" => {
            if let (Page::Queue { selected, .. }, Ok(i)) = (page, payload.parse::<usize>()) {
                if i < snapshot.playlist.tracks.len() {
                    *selected = i;
                }
            }
            return;
        }
        "queue_prevpage" => {
            if let Page::Queue { start, .. } = page {
                *start = start.saturating_sub(7);
            }
            return;
        }
        "queue_nextpage" => {
            if let Page::Queue { start, .. } = page {
                if *start + 7 < snapshot.playlist.tracks.len() {
                    *start += 7;
                }
            }
            return;
        }
        "queue_play" => {
            if let Page::Queue { selected, .. } = page {
                Some(Command::Select { index: *selected })
            } else {
                None
            }
        }
        "queue_remove" => {
            if let Page::Queue { selected, .. } = page {
                let index = *selected;
                *selected = selected.saturating_sub(1);
                Some(Command::Remove { index })
            } else {
                None
            }
        }
        "queue_up" => {
            if let Page::Queue { selected, .. } = page {
                let from = *selected;
                *selected = selected.saturating_sub(1);
                Some(Command::Move {
                    from,
                    to: *selected,
                })
            } else {
                None
            }
        }
        "queue_down" => {
            if let Page::Queue { selected, .. } = page {
                let from = *selected;
                *selected = (*selected + 1).min(snapshot.playlist.tracks.len().saturating_sub(1));
                Some(Command::Move {
                    from,
                    to: *selected,
                })
            } else {
                None
            }
        }
        "previous" => Some(Command::Previous),
        "toggle" => Some(Command::Toggle),
        "stop" => Some(Command::Stop),
        "next" => Some(Command::Next),
        "seek_back" => Some(Command::Seek {
            position_ms: snapshot.position_ms.saturating_sub(10_000),
        }),
        "seek_forward" => Some(Command::Seek {
            position_ms: (snapshot.position_ms + 10_000).min(if snapshot.duration_ms > 0 {
                snapshot.duration_ms
            } else {
                snapshot.position_ms + 10_000
            }),
        }),
        "volume_down" => Some(Command::Volume {
            level: ((snapshot.playlist.volume * 10.0).round() - 1.0).max(0.0) / 10.0,
        }),
        "volume_up" => Some(Command::Volume {
            level: ((snapshot.playlist.volume * 10.0).round() + 1.0).min(10.0) / 10.0,
        }),
        "mute" => Some(Command::Mute {
            muted: !snapshot.playlist.muted,
        }),
        _ => None,
    };
    if let Some(command) = command {
        send_and_update(command, snapshot, fixture_mode);
    }
}
fn run(config: ShellConfig, mut snap: Snapshot, fixture_mode: bool) -> Result<(), String> {
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
                namespace: "nuraloumi-player-dismiss".into(),
            })
            .ok()
    };
    let surface = backend
        .create_menu(WaylandMenuConfig {
            width: config.menu_width.min(550),
            height: 690,
            margin_top: config.panel_height as i32,
            margin_left: 0,
            output: output.as_ref().map(|o| o.id),
            namespace: "nuraloumi-player".into(),
        })
        .map_err(|e| e.to_string())?;
    let renderer = CairoRenderer::default();
    let mut page = Page::Now;
    let mut geometry = None;
    let mut menu = model(&snap, &page, &[]);
    let mut state = MenuState::new(&menu);
    let mut scene: Option<Scene> = None;
    let mut pointer_down: Option<String> = None;
    let mut touch = TouchActivation::default();
    let mut dirty = true;
    let mut ticks = 0u64;
    loop {
        backend.flush().map_err(|e| e.to_string())?;
        let mut pollfd = libc::pollfd {
            fd: backend.poll_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let polled = unsafe { libc::poll(&mut pollfd, 1, 250) };
        if polled < 0 {
            return Err("Wayland poll failed".into());
        }
        if polled > 0 {
            backend.blocking_dispatch().map_err(|e| e.to_string())?;
        } else {
            backend.dispatch_pending().map_err(|e| e.to_string())?;
        }
        for ev in backend.drain_events().collect::<Vec<_>>() {
            if ev.surface == dismiss {
                match ev.event {
                    WaylandEvent::Configure {
                        width,
                        height,
                        scale,
                    } => transparent(
                        &mut backend,
                        dismiss.ok_or("backdrop missing")?,
                        width,
                        height,
                        scale,
                    )?,
                    WaylandEvent::PointerButton { pressed: true, .. }
                    | WaylandEvent::TouchDown { .. }
                    | WaylandEvent::Close => return Ok(()),
                    _ => {}
                }
                continue;
            }
            if ev.surface.is_some() && ev.surface != Some(surface) {
                continue;
            }
            let mut requested = None;
            match ev.event {
                WaylandEvent::Configure {
                    width,
                    height,
                    scale,
                } => {
                    geometry = Some((width, height, scale));
                    dirty = true;
                }
                WaylandEvent::Close => return Ok(()),
                WaylandEvent::Key { pressed: true, key } => match key {
                    Key::Escape => {
                        if matches!(page, Page::Now) {
                            return Ok(());
                        }
                        page = Page::Now;
                        dirty = true;
                    }
                    Key::Up => {
                        state.handle(&menu, SemanticInput::Up);
                        dirty = true;
                    }
                    Key::Down | Key::Tab => {
                        state.handle(&menu, SemanticInput::Down);
                        dirty = true;
                    }
                    Key::Enter => {
                        requested = state
                            .selected_id
                            .as_deref()
                            .and_then(|id| menu.item_by_id(id))
                            .and_then(|item| item.action.clone());
                    }
                    Key::Space => {
                        requested = Some(MenuAction::Custom {
                            kind: "toggle".into(),
                            payload: String::new(),
                        })
                    }
                    Key::Text(t) if t == "q" => return Ok(()),
                    Key::Text(t) if t == " " => {
                        requested = Some(MenuAction::Custom {
                            kind: "toggle".into(),
                            payload: String::new(),
                        })
                    }
                    Key::Text(t) if t == "o" => {
                        page = Page::Files {
                            dir: home_dir(),
                            start: 0,
                        };
                        dirty = true;
                    }
                    Key::Text(t) if t == "+" => {
                        requested = Some(MenuAction::Custom {
                            kind: "volume_up".into(),
                            payload: String::new(),
                        })
                    }
                    Key::Text(t) if t == "-" => {
                        requested = Some(MenuAction::Custom {
                            kind: "volume_down".into(),
                            payload: String::new(),
                        })
                    }
                    _ => {}
                },
                WaylandEvent::PointerButton {
                    x,
                    y,
                    button: 0x110,
                    pressed,
                } => {
                    let at = scene
                        .as_ref()
                        .and_then(|s| s.hit_test(x, y))
                        .map(str::to_owned);
                    if pressed {
                        pointer_down = at;
                    } else {
                        if pointer_down.is_some() && pointer_down == at {
                            requested = at
                                .as_deref()
                                .and_then(|key| menu.item_by_id(key))
                                .and_then(|item| item.action.clone());
                        }
                        pointer_down = None;
                    }
                }
                WaylandEvent::TouchDown { id, x, y } => {
                    let at = scene
                        .as_ref()
                        .and_then(|s| s.hit_test(x, y))
                        .map(str::to_owned);
                    touch.down(id, at, x, y);
                }
                WaylandEvent::TouchMotion { id, x, y } => {
                    touch.motion(id, x, y);
                }
                WaylandEvent::TouchUp { id } => {
                    if let Some((selected, x, y)) = touch.up(id) {
                        let released = scene.as_ref().and_then(|s| s.hit_test(x, y));
                        if Some(selected.as_str()) == released {
                            requested = released
                                .and_then(|key| menu.item_by_id(key))
                                .and_then(|item| item.action.clone());
                        }
                    }
                }
                WaylandEvent::TouchCancel { ids } => touch.cancel(&ids),
                _ => {}
            }
            if let Some(action) = requested {
                let files = match &page {
                    Page::Files { dir, .. } => browse_entries(dir),
                    _ => Vec::new(),
                };
                handle_action(action, &mut page, &files, &mut snap, fixture_mode);
                dirty = true;
            }
        }
        ticks += 1;
        if ticks.is_multiple_of(2) && !fixture_mode {
            if let Ok(reply) = send(&Command::Status, false) {
                let changed = reply.state.position_ms / 1000 != snap.position_ms / 1000
                    || reply.state.status != snap.status
                    || reply.state.playlist != snap.playlist
                    || reply.state.error != snap.error;
                snap = reply.state;
                dirty |= changed;
            }
        }
        if dirty {
            let chosen = state.selected_id.clone();
            let files = match &page {
                Page::Files { dir, .. } => browse_entries(dir),
                _ => Vec::new(),
            };
            menu = model(&snap, &page, &files);
            state = MenuState::new(&menu);
            if let Some(id) = chosen {
                if menu
                    .item_by_id(&id)
                    .and_then(|item| item.action.as_ref())
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
            dirty = false;
        }
    }
}
fn main() -> ExitCode {
    let args = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let mut snapshot = if args.fixture {
        fixture()
    } else {
        match send(&Command::Status, true) {
            Ok(reply) => reply.state,
            Err(e) => offline(Some(e)),
        }
    };
    let open_requested = args.open.is_some();
    if let Some(path) = args.open {
        if args.fixture {
            snapshot.error = Some("fixture mode: cannot open media".into());
        } else {
            send_and_update(Command::Add { path }, &mut snapshot, false);
        }
    }
    if args.status {
        match serde_json::to_string_pretty(&snapshot) {
            Ok(s) => {
                println!("{s}");
                return if open_requested && snapshot.error.is_some() {
                    ExitCode::FAILURE
                } else {
                    ExitCode::SUCCESS
                };
            }
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
        }
    }
    let cfg = load_config_or_default(args.config.as_deref()).unwrap_or_default();
    match run(cfg, snapshot, args.fixture) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("nuraloumi-player: {e}");
            ExitCode::FAILURE
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browser_filters_extension_and_unknown_files() {
        assert!(media_extension(Path::new("/music/demo.opus")));
        assert!(media_extension(Path::new("/music/video.MKV")));
        assert!(!media_extension(Path::new("/music/executable.sh")));
    }
    #[test]
    fn view_is_playable_without_audio_engine() {
        let s = offline(Some("missing GStreamer".into()));
        let m = model(&s, &Page::Now, &[]);
        assert!(m.item_by_id("open").is_some());
        assert!(m.item_by_id("gpu").is_some());
        assert!(m.item_by_id("stop").is_some());
    }
    #[test]
    fn touch_activation_requires_matching_single_contact_release() {
        let mut touch = TouchActivation::default();
        touch.down(1, Some("play".into()), 10.0, 11.0);
        touch.motion(1, 12.0, 13.0);
        assert_eq!(touch.up(1), Some(("play".into(), 12.0, 13.0)));
        touch.down(1, Some("play".into()), 10.0, 11.0);
        touch.down(2, Some("stop".into()), 20.0, 21.0);
        assert_eq!(touch.up(1), None);
        assert_eq!(touch.up(2), None);
        touch.down(3, Some("stop".into()), 20.0, 21.0);
        touch.cancel(&[3]);
        assert_eq!(touch.up(3), None);
        touch.down(4, Some("mute".into()), 1.0, 2.0);
        assert_eq!(touch.up(4), Some(("mute".into(), 1.0, 2.0)));
    }
    #[test]
    fn slow_response_does_not_extend_client_deadline() {
        let (mut writer, reader) = UnixStream::pair().unwrap();
        let sender = std::thread::spawn(move || {
            let _ = writer.write_all(b"{");
            std::thread::sleep(Duration::from_millis(350));
            let _ = writer.write_all(b"\"");
            std::thread::sleep(Duration::from_millis(350));
            let _ = writer.write_all(b"ok");
        });
        let start = Instant::now();
        assert!(read_response(&reader).is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
        sender.join().unwrap();
    }
}
