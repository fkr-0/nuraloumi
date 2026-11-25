//! First-class NuraLoumi media UI. Cairo/wl_shm works without GStreamer/EGL.
//! Hardware VDE/Grate video is an optional, in-process playerd engine.
#![allow(dead_code)]
#[path = "../player_engine.rs"]
mod player_engine;
#[path = "../player_frames.rs"]
mod player_frames;
use nuraloumi_core::{MenuAction, MenuItem, MenuModel, MenuState, SemanticInput};
use nuraloumi_render_cairo::{
    CairoRenderer, Color, Fill, HitRegion, PaintNode, Point, Rect, RowKind, Scene, TextStyle,
    Viewport,
};
use nuraloumi_shell::{load_config_or_default, ShellConfig};
use nuraloumi_wayland::{
    BackendError, Frame, Key, PixelFormat, PlatformEvent as WaylandEvent, SurfaceId, WaylandBackend,
};
use player_engine::{Command, LoopMode, Playlist, Reply, Snapshot};
use std::{
    collections::HashSet,
    env, fs,
    io::{self, Read, Write},
    os::fd::{AsFd, AsRawFd},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::{Command as ProcCommand, ExitCode, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

#[derive(Default)]
struct Args {
    open: Option<String>,
    status: bool,
    config: Option<PathBuf>,
    fixture: bool,
}
// Compact player geometry is independent of the menu-row renderer. Hit
// rectangles are the full 48px touch targets, not the smaller icon glyphs.
fn compact_scene(viewport: Viewport, snapshot: &Snapshot, focused: Option<&str>) -> Scene {
    let width = viewport.width;
    let panel = Rect::new(0.0, 0.0, width, viewport.height);
    let background = Color::rgb(24, 28, 35);
    let raised = Color::rgb(42, 48, 60);
    let accent = Color::rgb(108, 173, 235);
    let foreground = Color::rgb(234, 240, 249);
    let muted = Color::rgb(167, 178, 193);
    let mut scene = Scene {
        viewport,
        menu_id: "player".into(),
        panel_rect: panel,
        paint: vec![PaintNode::FillRect {
            rect: panel,
            color: background,
        }],
        hits: vec![],
        scroll: nuraloumi_render_cairo::ScrollWindow {
            viewport: panel,
            content_height: viewport.height,
            offset: 0.0,
            max_offset: 0.0,
            clipped: false,
        },
    };
    let label = |scene: &mut Scene, x: f64, y: f64, text: String, size: f64, color| {
        scene.paint.push(PaintNode::Text {
            origin: Point::new(x, y),
            text,
            style: TextStyle { size, bold: false },
            color,
        });
    };
    // The title and playback details fit 480px height without scrolling.
    label(
        &mut scene,
        76.0,
        32.0,
        "Media player".into(),
        20.0,
        foreground,
    );
    let close = Rect::new(18.0, 4.0, 48.0, 48.0);
    scene.paint.push(PaintNode::RoundedRect {
        rect: close,
        radius: 6.0,
        fill: Fill::Solid(raised),
        stroke: None,
    });
    scene.paint.push(PaintNode::Text {
        origin: Point::new(25.0, 34.0),
        text: "Back".into(),
        style: TextStyle {
            size: 14.0,
            bold: false,
        },
        color: foreground,
    });
    scene.hits.push(HitRegion {
        item_id: "dismiss".into(),
        rect: close,
        actionable: true,
        enabled: true,
        kind: RowKind::Action,
    });
    let track = snapshot
        .playlist
        .current
        .and_then(|i| snapshot.playlist.tracks.get(i))
        .map(|path| basename(path))
        .unwrap_or_else(|| "No media selected".into());
    label(&mut scene, 18.0, 94.0, track, 18.0, foreground);
    label(
        &mut scene,
        18.0,
        122.0,
        snapshot.status.clone(),
        14.0,
        muted,
    );
    let gap = 4.0;
    let button_width = ((width - 36.0 - 5.0 * gap) / 6.0).max(48.0);
    let controls = [
        ("previous", "Prev", "Previous track"),
        ("seek-back", "-15", "Seek back 15 seconds"),
        ("stop", "Stop", "Stop"),
        (
            "toggle",
            if snapshot.status == "Playing" {
                "Pause"
            } else {
                "Play"
            },
            "Play or pause",
        ),
        ("seek-forward", "+15", "Seek forward 15 seconds"),
        ("next", "Next", "Next track"),
    ];
    for (index, (id, symbol, _hint)) in controls.iter().enumerate() {
        let rect = Rect::new(
            18.0 + index as f64 * (button_width + gap),
            148.0,
            button_width,
            52.0,
        );
        scene.paint.push(PaintNode::RoundedRect {
            rect,
            radius: 7.0,
            fill: Fill::Solid(raised),
            stroke: None,
        });
        scene.paint.push(PaintNode::Text {
            origin: Point::new(rect.x + 7.0, rect.y + 32.0),
            text: (*symbol).into(),
            style: TextStyle {
                size: 14.0,
                bold: false,
            },
            color: foreground,
        });
        scene.hits.push(HitRegion {
            item_id: (*id).into(),
            rect,
            actionable: true,
            enabled: true,
            kind: RowKind::Action,
        });
    }
    let start = 18.0;
    let bar_width = (width - 36.0).max(48.0);
    let progress = if snapshot.duration_ms == 0 {
        0.0
    } else {
        snapshot.position_ms.min(snapshot.duration_ms) as f64 / snapshot.duration_ms as f64
    };
    scene.paint.push(PaintNode::RoundedRect {
        rect: Rect::new(start, 228.0, bar_width, 12.0),
        radius: 6.0,
        fill: Fill::Solid(raised),
        stroke: None,
    });
    scene.paint.push(PaintNode::RoundedRect {
        rect: Rect::new(start, 228.0, bar_width * progress, 12.0),
        radius: 6.0,
        fill: Fill::Solid(accent),
        stroke: None,
    });
    scene.hits.push(HitRegion {
        item_id: "scrub".into(),
        rect: Rect::new(start, 210.0, bar_width, 48.0),
        actionable: snapshot.duration_ms > 0,
        enabled: snapshot.duration_ms > 0,
        kind: RowKind::Action,
    });
    scene.paint.push(PaintNode::Text {
        origin: Point::new(start, 282.0),
        text: format!(
            "{} / {}",
            time_string(snapshot.position_ms),
            if snapshot.duration_ms == 0 {
                "—".into()
            } else {
                time_string(snapshot.duration_ms)
            }
        ),
        style: TextStyle {
            size: 15.0,
            bold: false,
        },
        color: muted,
    });
    let loop_label = match snapshot.playlist.loop_mode {
        LoopMode::Off => "Repeat: off",
        LoopMode::One => "Repeat: one",
        LoopMode::All => "Repeat: all",
    };
    for (i, (id, text)) in [
        ("loop", loop_label),
        (
            "mute",
            if snapshot.playlist.muted {
                "Unmute"
            } else {
                "Mute"
            },
        ),
        ("queue", "Playlist"),
        ("open", "Open file"),
    ]
    .iter()
    .enumerate()
    {
        let cols = 2.0;
        let bw = ((width - 44.0) / cols).max(48.0);
        let rect = Rect::new(
            18.0 + (i % 2) as f64 * (bw + 8.0),
            306.0 + (i / 2) as f64 * 56.0,
            bw,
            48.0,
        );
        scene.paint.push(PaintNode::RoundedRect {
            rect,
            radius: 6.0,
            fill: Fill::Solid(raised),
            stroke: None,
        });
        scene.paint.push(PaintNode::Text {
            origin: Point::new(rect.x + 12.0, rect.y + 30.0),
            text: (*text).into(),
            style: TextStyle {
                size: 15.0,
                bold: false,
            },
            color: foreground,
        });
        scene.hits.push(HitRegion {
            item_id: (*id).into(),
            rect,
            actionable: true,
            enabled: true,
            kind: RowKind::Action,
        });
    }
    label(
        &mut scene,
        18.0,
        418.0,
        "Hardware video: 854x480 at 24 fps".into(),
        13.0,
        muted,
    );
    if let Some(error) = &snapshot.error {
        scene.paint.push(PaintNode::Text {
            origin: Point::new(18.0, 431.0),
            text: format!("Error: {}", error.chars().take(48).collect::<String>()),
            style: TextStyle {
                size: 13.0,
                bold: false,
            },
            color: Color::rgb(246, 150, 145),
        });
    }
    if let Some(focused) = focused {
        if let Some(hit) = scene.hits.iter().find(|hit| hit.item_id == focused) {
            scene.paint.push(PaintNode::RoundedRect {
                rect: hit.rect,
                radius: 7.0,
                fill: Fill::Solid(Color::rgba(0, 0, 0, 0)),
                stroke: Some((accent, 2.0)),
            });
        }
    }
    scene
}

// A dedicated list layout keeps entries apart from actions on a 440px sheet.
// Both queue and browser retain their original semantic menu command IDs.
fn list_scene(
    viewport: Viewport,
    snapshot: &Snapshot,
    page: &Page,
    files: &[FileEntry],
    focused: Option<&str>,
) -> Scene {
    let mut scene = compact_scene(viewport, snapshot, None);
    scene.paint.truncate(1);
    scene.hits.clear();
    let fg = Color::rgb(234, 240, 249);
    let bg = Color::rgb(42, 48, 60);
    let accent = Color::rgb(108, 173, 235);
    let mut add = |id: &str, label: String, rect: Rect| {
        scene.paint.push(PaintNode::RoundedRect {
            rect,
            radius: 5.0,
            fill: Fill::Solid(bg),
            stroke: None,
        });
        scene.paint.push(PaintNode::Text {
            origin: Point::new(rect.x + 8.0, rect.y + 30.0),
            text: label,
            style: TextStyle {
                size: 14.0,
                bold: false,
            },
            color: fg,
        });
        scene.hits.push(HitRegion {
            item_id: id.into(),
            rect,
            actionable: true,
            enabled: true,
            kind: RowKind::Action,
        });
    };
    let width = viewport.width;
    let full = width - 36.0;
    match page {
        Page::Queue { start, selected } => {
            add(
                "back",
                "Back to Player".into(),
                Rect::new(18.0, 8.0, full, 48.0),
            );
            for (row, i) in (*start..(*start + 4).min(snapshot.playlist.tracks.len())).enumerate() {
                let marker = if Some(i) == snapshot.playlist.current {
                    "[Now]"
                } else if i == *selected {
                    "[*]"
                } else {
                    "-"
                };
                add(
                    &format!("track-{i}"),
                    format!("{marker} {}", basename(&snapshot.playlist.tracks[i])),
                    Rect::new(18.0, 64.0 + row as f64 * 50.0, full, 48.0),
                );
            }
            let controls = [
                ("q-prevpage", "Prev page"),
                ("q-nextpage", "Next page"),
                ("q-play", "Play"),
                ("q-up", "Move up"),
                ("q-down", "Move down"),
                ("q-remove", "Remove"),
                ("q-open", "+ Add"),
            ];
            let cell = (width - 54.0) / 3.0;
            for (i, (id, label)) in controls.iter().enumerate() {
                add(
                    id,
                    (*label).into(),
                    Rect::new(
                        18.0 + (i % 3) as f64 * (cell + 9.0),
                        268.0 + (i / 3) as f64 * 52.0,
                        cell,
                        48.0,
                    ),
                );
            }
        }
        Page::Files { start, .. } => {
            add(
                "f-back",
                "Back to Player".into(),
                Rect::new(18.0, 8.0, full, 48.0),
            );
            add(
                "f-updir",
                "Parent directory".into(),
                Rect::new(18.0, 60.0, full, 48.0),
            );
            for (row, i) in (*start..(*start + 4).min(files.len())).enumerate() {
                let file = &files[i];
                add(
                    &format!("file-{i}"),
                    format!(
                        "{} {}",
                        if file.directory { "Dir:" } else { "File:" },
                        basename(&file.path.to_string_lossy())
                    ),
                    Rect::new(18.0, 116.0 + row as f64 * 51.0, full, 48.0),
                );
            }
            let cell = (width - 44.0) / 2.0;
            add(
                "f-prev",
                "Previous".into(),
                Rect::new(18.0, 330.0, cell, 48.0),
            );
            add(
                "f-next",
                "Next".into(),
                Rect::new(26.0 + cell, 330.0, cell, 48.0),
            );
        }
        Page::Now => {}
    }
    if let Some(id) = focused {
        if let Some(hit) = scene.hits.iter().find(|hit| hit.item_id == id) {
            scene.paint.push(PaintNode::RoundedRect {
                rect: hit.rect,
                radius: 5.0,
                fill: Fill::Solid(Color::rgba(0, 0, 0, 0)),
                stroke: Some((accent, 2.0)),
            });
        }
    }
    scene
}

fn seek_at(snapshot: &Snapshot, x: f64, width: f64) -> Option<Command> {
    if snapshot.duration_ms == 0 {
        return None;
    }
    let fraction = ((x - 18.0) / (width - 36.0).max(48.0)).clamp(0.0, 1.0);
    Some(Command::Seek {
        position_ms: (fraction * snapshot.duration_ms as f64).round() as u64,
    })
}

/// Configure the sole playback daemon for the SHA-pinned three-pass renderer,
/// when the private engine was bundled by the ARM release. Never launch an
/// alternate player or change EGL requirements of the Cairo menu itself.
fn configure_single_player_daemon(daemon: &mut ProcCommand, socket: &Path) {
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
    // Source-verified bundles carry their target libdrm. Keep the historical
    // device-local libdrm lookup for legacy packages only.
    if !serializer.is_file() {
        return;
    }
    let mut ld = if root.join("provenance.json").is_file() {
        if !lib.join("libdrm.so.2").is_file() || !lib.join("libdrm_tegra.so.0").is_file() {
            return;
        }
        lib.as_os_str().to_os_string()
    } else {
        if !system_drm.is_dir() {
            return;
        }
        let mut legacy = system_drm.as_os_str().to_os_string();
        legacy.push(":");
        legacy.push(&lib);
        legacy
    };
    let old = env::var_os("LD_LIBRARY_PATH").unwrap_or_default();
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
    // Keep the private decoder out of the shared plugin cache. Retain system
    // plugin discovery for the demuxers, parsers and audio pipeline.
    daemon.env("GST_REGISTRY", socket.with_extension("gst-registry.bin"));
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
        configure_single_player_daemon(&mut daemon, &path);
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
            loop_mode: LoopMode::Off,
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
            items.push(action("dismiss", "Close", "dismiss", ""));
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
                if s.duration_ms == 0 {
                    "—".into()
                } else {
                    time_string(s.duration_ms)
                }
            )));
            if let Some(error) = &s.error {
                items.push(MenuItem::status(
                    "error",
                    format!("Error: {}", error.chars().take(74).collect::<String>()),
                ));
            }
            items.push(action("previous", "Previous", "previous", ""));
            items.push(action(
                "toggle",
                if s.status == "Playing" {
                    "Pause"
                } else {
                    "Play"
                },
                "toggle",
                "",
            ));
            items.push(action("stop", "Stop", "stop", ""));
            items.push(action("next", "Next", "next", ""));
            items.push(action("seek-back", "Seek -15s", "seek_back", ""));
            items.push(action("seek-forward", "Seek +15s", "seek_forward", ""));
            items.push(action("scrub", "Seek position", "scrub", ""));
            items.push(action("loop", "Repeat mode", "loop", ""));
            items.push(action("volume-down", "- Volume", "volume_down", ""));
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
                MenuItem::status("gpu", "Hardware video: 854x480 at 24 fps")
                    .with_subtitle(s.gpu_status.clone()),
            );
        }
        Page::Queue { start, selected } => {
            items.push(MenuItem::status("queue-head", "Ordered playlist"));
            items.push(action("back", "Back to Player", "home", ""));
            let end = (start + 4).min(s.playlist.tracks.len());
            for i in *start..end {
                let marker = if Some(i) == s.playlist.current {
                    "[Now]"
                } else if i == *selected {
                    "[*]"
                } else {
                    "-"
                };
                items.push(action(
                    &format!("track-{i}"),
                    format!("{marker} {}", basename(&s.playlist.tracks[i])),
                    "queue_select",
                    i.to_string(),
                ));
            }
            items.push(action("q-prevpage", "Previous page", "queue_prevpage", ""));
            items.push(action("q-nextpage", "Next page", "queue_nextpage", ""));
            items.push(action("q-play", "Play selected", "queue_play", ""));
            items.push(action("q-up", "Move selected earlier", "queue_up", ""));
            items.push(action("q-down", "Move selected later", "queue_down", ""));
            items.push(action("q-remove", "Remove selected", "queue_remove", ""));
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
            items.push(action("f-back", "Back to Player", "home", ""));
            items.push(action("f-updir", "Parent directory", "parent_dir", ""));
            let end = (start + 4).min(files.len());
            for (i, entry) in files.iter().enumerate().take(end).skip(*start) {
                let name = entry.path.file_name().unwrap_or_default().to_string_lossy();
                items.push(action(
                    &format!("file-{i}"),
                    format!(
                        "{} {}",
                        if entry.directory { "Dir:" } else { "File:" },
                        name.chars().take(49).collect::<String>()
                    ),
                    "entry",
                    i.to_string(),
                ));
            }
            items.push(action("f-prev", "Previous files", "files_prevpage", ""));
            items.push(action("f-next", "More files", "files_nextpage", ""));
        }
    };
    MenuModel::new("player", "Media Player", items)
}
const CONTROLS_HEIGHT: u32 = 440;

fn controls_offset(height: u32) -> u32 {
    height.saturating_sub(CONTROLS_HEIGHT)
}

fn video_destination(
    width: u32,
    height: u32,
    frame_width: u32,
    frame_height: u32,
) -> Option<(i32, i32, i32, i32)> {
    let area_height = controls_offset(height) as u64;
    let width = u64::from(width);
    let (fw, fh) = (u64::from(frame_width), u64::from(frame_height));
    if width == 0 || area_height == 0 || fw == 0 || fh == 0 {
        return None;
    }
    let w = width.min(area_height * fw / fh).max(1);
    let h = area_height.min(w * fh / fw).max(1);
    Some((
        ((width - w) / 2) as i32,
        ((area_height - h) / 2) as i32,
        w as i32,
        h as i32,
    ))
}

fn position_controls(scene: &mut Scene, y: f64, height: f64) {
    for node in &mut scene.paint {
        match node {
            PaintNode::FillRect { rect, .. }
            | PaintNode::RoundedRect { rect, .. }
            | PaintNode::Chevron { rect, .. }
            | PaintNode::CheckMark { rect, .. }
            | PaintNode::ClipPush(rect) => rect.y += y,
            PaintNode::Text { origin, .. } => origin.y += y,
            PaintNode::Line { from, to, .. } => {
                from.y += y;
                to.y += y;
            }
            PaintNode::ClipPop => {}
        }
    }
    for hit in &mut scene.hits {
        hit.rect.y += y;
    }
    scene.viewport.height = height;
    scene.panel_rect.height = height;
    if y > 0.0 {
        scene.paint.insert(
            0,
            PaintNode::FillRect {
                rect: Rect::new(0.0, 0.0, scene.viewport.width, y),
                color: Color::rgb(0, 0, 0),
            },
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw(
    backend: &mut WaylandBackend,
    surface: SurfaceId,
    renderer: &CairoRenderer,
    _config: &ShellConfig,
    _model: &MenuModel,
    state: &MenuState,
    snapshot: &Snapshot,
    page: &Page,
    files: &[FileEntry],
    width: u32,
    height: u32,
    scale: i32,
) -> Result<Scene, String> {
    let offset = controls_offset(height) as f64;
    let viewport = Viewport::new(
        width as f64,
        height as f64 - offset,
        f64::from(scale.max(1)),
    );
    let mut scene = if matches!(page, Page::Now) {
        compact_scene(viewport, snapshot, state.selected_id.as_deref())
    } else {
        list_scene(
            viewport,
            snapshot,
            page,
            files,
            state.selected_id.as_deref(),
        )
    };
    position_controls(&mut scene, offset, height as f64);
    let mut buffer = renderer.render_scene(&scene).map_err(|e| e.to_string())?;
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
    send_control: &mut impl FnMut(Command, &mut Snapshot),
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
                *start = start.saturating_sub(4);
            }
            return;
        }
        "files_nextpage" => {
            if let Page::Files { start, .. } = page {
                if *start + 4 < files.len() {
                    *start += 4;
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
                *start = start.saturating_sub(4);
            }
            return;
        }
        "queue_nextpage" => {
            if let Page::Queue { start, .. } = page {
                if *start + 4 < snapshot.playlist.tracks.len() {
                    *start += 4;
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
            position_ms: snapshot.position_ms.saturating_sub(15_000),
        }),
        "seek_forward" => Some(Command::Seek {
            position_ms: snapshot.position_ms.saturating_add(15_000).min(
                if snapshot.duration_ms > 0 {
                    snapshot.duration_ms
                } else {
                    snapshot.position_ms.saturating_add(15_000)
                },
            ),
        }),
        "loop" => Some(Command::LoopMode {
            mode: match snapshot.playlist.loop_mode {
                LoopMode::Off => LoopMode::One,
                LoopMode::One => LoopMode::All,
                LoopMode::All => LoopMode::Off,
            },
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
        send_control(command, snapshot);
    }
}
type PlayerReply = (bool, Result<Reply, String>);

struct PlayerRpc {
    commands: mpsc::SyncSender<(Command, bool)>,
    replies: mpsc::Receiver<PlayerReply>,
    wake: UnixStream,
}

impl PlayerRpc {
    fn start() -> io::Result<Self> {
        let (wake, mut notify) = UnixStream::pair()?;
        wake.set_nonblocking(true)?;
        notify.set_nonblocking(true)?;
        let (commands, work) = mpsc::sync_channel::<(Command, bool)>(8);
        let (send_reply, replies) = mpsc::channel();
        std::thread::Builder::new()
            .name("nuraloumi-player-rpc".into())
            .spawn(move || {
                while let Ok((command, status)) = work.recv() {
                    if send_reply.send((status, send(&command, false))).is_err() {
                        break;
                    }
                    let _ = notify.write(&[1]);
                }
            })?;
        Ok(Self {
            commands,
            replies,
            wake,
        })
    }
}
fn release_video_on_close(
    backend: &mut WaylandBackend,
    surface: SurfaceId,
    receiver: Option<&player_frames::Receiver>,
    in_flight: &mut HashSet<u32>,
) {
    let Some(receiver) = receiver.filter(|r| r.peer_fd() >= 0) else {
        return;
    };
    if in_flight.is_empty() || backend.clear_video(surface).is_err() {
        return;
    }
    let deadline = Instant::now() + Duration::from_millis(500);
    while !in_flight.is_empty() && Instant::now() < deadline {
        if backend.flush().is_err() {
            break;
        }
        let mut fd = libc::pollfd {
            fd: backend.poll_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut fd, 1, 20) };
        let dispatched = if ready > 0 {
            backend.blocking_dispatch()
        } else {
            backend.dispatch_pending()
        };
        if dispatched.is_err() {
            break;
        }
        for seq in backend.take_video_releases() {
            in_flight.remove(&seq);
            let _ = receiver.acknowledge(2, seq);
        }
    }
}

fn run(
    config: ShellConfig,
    mut snap: Snapshot,
    fixture_mode: bool,
    open: Option<String>,
) -> Result<(), String> {
    let runtime = env::var_os("XDG_RUNTIME_DIR").ok_or("XDG_RUNTIME_DIR required")?;
    let mut video = if fixture_mode {
        None
    } else {
        Some(
            player_frames::Receiver::bind(Path::new(&runtime))
                .map_err(|e| format!("player video socket: {e}"))?,
        )
    };
    let mut backend = WaylandBackend::connect().map_err(|e| e.to_string())?;
    let surface = backend
        .create_window(
            config.menu_width.clamp(480, 854),
            720,
            "nuraloumi-player",
            "Media player",
        )
        .map_err(|e| e.to_string())?;
    backend
        .set_window_min_size(surface, 360, 496)
        .map_err(|e| e.to_string())?;
    let mut rpc = PlayerRpc::start().map_err(|e| e.to_string())?;
    let commands = rpc.commands.clone();
    let mut send_control = |command, snapshot: &mut Snapshot| {
        if fixture_mode {
            snapshot.error = Some("fixture mode: controls cannot output sound".into());
        } else if commands.try_send((command, false)).is_err() {
            snapshot.error = Some("Player is busy".into());
        }
    };
    if let Some(path) = open {
        send_control(Command::Add { path }, &mut snap);
    }
    let renderer = CairoRenderer::default();
    let mut page = Page::Now;
    let mut geometry = None;
    let mut menu = model(&snap, &page, &[]);
    let mut state = MenuState::new(&menu);
    let mut scene: Option<Scene> = None;
    let mut pointer_down: Option<String> = None;
    let mut pointer_seek: Option<f64> = None;
    let mut touch = TouchActivation::default();
    let mut dirty = true;
    let mut status_in_flight = false;
    let mut displayed_size = None;
    let mut next_status = Instant::now() + Duration::from_millis(250);
    let mut pending_video: Option<player_frames::Frame> = None;
    let mut in_flight = HashSet::new();
    let result = (|| loop {
        backend.flush().map_err(|e| e.to_string())?;
        let mut pollfds = [
            libc::pollfd {
                fd: backend.poll_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: video
                    .as_ref()
                    .filter(|v| v.peer_fd() < 0 && in_flight.is_empty())
                    .map_or(-1, |v| v.listener_fd()),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: video.as_ref().map_or(-1, |v| v.peer_fd()),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: rpc.wake.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let polled = unsafe { libc::poll(pollfds.as_mut_ptr(), pollfds.len() as _, 100) };
        if polled < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err("player poll failed".into());
        }
        if pollfds[0].revents != 0 {
            backend.blocking_dispatch().map_err(|e| e.to_string())?;
        } else {
            backend.dispatch_pending().map_err(|e| e.to_string())?;
        }
        let released = backend.take_video_releases();
        for seq in &released {
            in_flight.remove(seq);
        }
        let callbacks = backend.take_video_callbacks();
        if let Some(receiver) = video.as_mut() {
            let mut disconnect = false;
            if receiver.peer_fd() >= 0 {
                disconnect = callbacks
                    .into_iter()
                    .map(|seq| (1, seq))
                    .chain(released.into_iter().map(|seq| (2, seq)))
                    .any(|(kind, seq)| receiver.acknowledge(kind, seq).is_err());
            }
            if pollfds[1].revents & libc::POLLIN != 0 {
                if let Err(e) = receiver.accept() {
                    if e.kind() != io::ErrorKind::WouldBlock {
                        return Err(e.to_string());
                    }
                }
            }
            if !disconnect && pollfds[2].revents != 0 {
                match receiver.read() {
                    Ok(Some(player_frames::Packet::Frame(frame))) if pending_video.is_none() => {
                        pending_video = Some(frame)
                    }
                    Ok(Some(player_frames::Packet::Detach)) => {
                        pending_video = None;
                        backend.clear_video(surface).map_err(|e| e.to_string())?;
                        displayed_size = None;
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                    _ => disconnect = true,
                }
            }
            if disconnect {
                receiver.disconnect();
                pending_video = None;
                displayed_size = None;
                backend.clear_video(surface).map_err(|e| e.to_string())?;
            }
        }
        for ev in backend.drain_events().collect::<Vec<_>>() {
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
                    if let Some((fw, fh)) = displayed_size {
                        if let Some(destination) = video_destination(width, height, fw, fh) {
                            backend
                                .resize_video(surface, destination)
                                .map_err(|e| e.to_string())?;
                        }
                    }
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
                    Key::Left => {
                        requested = Some(MenuAction::Custom {
                            kind: "seek_back".into(),
                            payload: String::new(),
                        })
                    }
                    Key::Right => {
                        requested = Some(MenuAction::Custom {
                            kind: "seek_forward".into(),
                            payload: String::new(),
                        })
                    }
                    Key::Text(t) if t == "l" => {
                        requested = Some(MenuAction::Custom {
                            kind: "loop".into(),
                            payload: String::new(),
                        })
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
                        pointer_seek = if pointer_down.as_deref() == Some("scrub") {
                            Some(x)
                        } else {
                            None
                        };
                    } else {
                        if pointer_seek.take().is_some() && pointer_down.as_deref() == Some("scrub")
                        {
                            if let Some(command) =
                                seek_at(&snap, x, geometry.map_or(0.0, |g| g.0 as f64))
                            {
                                send_control(command, &mut snap);
                                dirty = true;
                            }
                        } else if pointer_down.is_some() && pointer_down == at {
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
                        if selected == "scrub" {
                            if let Some(command) =
                                seek_at(&snap, x, geometry.map_or(0.0, |g| g.0 as f64))
                            {
                                send_control(command, &mut snap);
                                dirty = true;
                            }
                        } else if Some(selected.as_str()) == released {
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
                if matches!(&action, MenuAction::Custom { kind, .. } if kind == "dismiss") {
                    return Ok(());
                }
                let files = match &page {
                    Page::Files { dir, .. } => browse_entries(dir),
                    _ => Vec::new(),
                };
                handle_action(action, &mut page, &files, &mut snap, &mut send_control);
                dirty = true;
            }
        }
        if pollfds[3].revents & libc::POLLIN != 0 {
            let mut wake_bytes = [0; 32];
            let _ = rpc.wake.read(&mut wake_bytes);
        }
        for (status, reply) in rpc.replies.try_iter() {
            if status {
                status_in_flight = false;
            }
            match reply {
                Ok(reply) => {
                    dirty |= !status
                        || reply.state.position_ms / 1000 != snap.position_ms / 1000
                        || reply.state.status != snap.status
                        || reply.state.playlist != snap.playlist
                        || reply.state.error != snap.error;
                    snap = reply.state;
                    if !reply.ok {
                        snap.error = reply.error;
                    }
                }
                Err(e) if !status => {
                    snap.error = Some(e);
                    dirty = true;
                }
                Err(_) => {}
            }
        }
        if !fixture_mode && Instant::now() >= next_status {
            next_status = Instant::now() + Duration::from_millis(250);
            if !status_in_flight {
                status_in_flight = rpc.commands.try_send((Command::Status, true)).is_ok();
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
                    &snap,
                    &page,
                    &files,
                    w,
                    h,
                    s,
                )?);
            }
            dirty = false;
        }
        if let (Some(frame), Some((width, height, _))) = (&pending_video, geometry) {
            if let Some(destination) = video_destination(width, height, frame.width, frame.height) {
                match backend.present_video(
                    surface,
                    frame.fd.as_fd(),
                    frame.width,
                    frame.height,
                    frame.stride,
                    frame.sequence,
                    destination,
                    true, // Grate exports bottom-up XR24 rows on Tegra20.
                ) {
                    Ok(()) => {
                        displayed_size = Some((frame.width, frame.height));
                        in_flight.insert(frame.sequence);
                        pending_video = None;
                    }
                    Err(BackendError::WouldBlock) => {}
                    Err(e) => return Err(e.to_string()),
                }
            }
        }
    })();
    release_video_on_close(&mut backend, surface, video.as_ref(), &mut in_flight);
    result
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
    if let Some(path) = args.open.as_ref().filter(|_| args.status) {
        if args.fixture {
            snapshot.error = Some("fixture mode: cannot open media".into());
        } else {
            send_and_update(Command::Add { path: path.clone() }, &mut snapshot, false);
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
    match run(cfg, snapshot, args.fixture, args.open) {
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
    fn compact_transport_hitboxes_are_disjoint_and_touch_sized() {
        for width in [360.0, 480.0, 550.0] {
            let scene = compact_scene(Viewport::new(width, 440.0, 1.0), &fixture(), None);
            let ids = [
                "previous",
                "seek-back",
                "stop",
                "toggle",
                "seek-forward",
                "next",
            ];
            let mut previous_right = 0.0;
            for id in ids {
                let rect = scene
                    .hits
                    .iter()
                    .find(|hit| hit.item_id == id)
                    .unwrap()
                    .rect;
                assert!(rect.width >= 48.0 && rect.height >= 48.0);
                assert!(rect.x >= previous_right);
                assert!(rect.right() <= width);
                assert_eq!(scene.hit_test(rect.x + 4.0, rect.y + 4.0), Some(id));
                previous_right = rect.right();
            }
        }
    }
    #[test]
    fn compact_player_shows_readable_transport_labels_and_video_profile() {
        let mut snapshot = fixture();
        for (status, toggle) in [("Paused", "Play"), ("Playing", "Pause")] {
            snapshot.status = status.into();
            let scene = compact_scene(Viewport::new(448.0, 440.0, 1.0), &snapshot, None);
            for expected in [
                "Back",
                "Prev",
                "-15",
                "Stop",
                toggle,
                "+15",
                "Next",
                "Hardware video: 854x480 at 24 fps",
            ] {
                assert!(
                    scene.paint.iter().any(|node| {
                        matches!(node, PaintNode::Text { text, .. } if text == expected)
                    }),
                    "missing label: {expected}"
                );
            }
        }
    }

    #[test]
    fn video_preserves_aspect_and_controls_stay_below_it() {
        let (x, y, width, height) = video_destination(550, 720, 854, 480).unwrap();
        assert_eq!(x * 2 + width, 550);
        assert!(y + height <= controls_offset(720) as i32);
        assert!((f64::from(width) / f64::from(height) - 854.0 / 480.0).abs() < 0.01);
        assert!(video_destination(480, 440, 854, 480).is_none());
        let mut scene = compact_scene(Viewport::new(550.0, 440.0, 1.0), &fixture(), None);
        position_controls(&mut scene, 280.0, 720.0);
        assert_eq!(scene.viewport.height, 720.0);
        assert!(scene.hits.iter().all(|hit| hit.rect.y >= 280.0));
        assert!(scene.hit_test(30.0, 160.0).is_none());
        assert_eq!(scene.hit_test(30.0, 440.0), Some("previous"));
    }

    #[test]
    fn scrubber_clamps_extremes_and_blocks_unknown_duration() {
        let mut s = fixture();
        assert_eq!(
            seek_at(&s, -999.0, 480.0),
            Some(Command::Seek { position_ms: 0 })
        );
        assert_eq!(
            seek_at(&s, 9999.0, 480.0),
            Some(Command::Seek {
                position_ms: 224_000
            })
        );
        s.duration_ms = 0;
        assert!(seek_at(&s, 100.0, 480.0).is_none());
        assert_eq!(
            compact_scene(Viewport::new(480.0, 440.0, 1.0), &s, None).hit_test(50.0, 230.0),
            None
        );
    }
    #[test]
    fn repeat_control_starts_off_and_remains_touch_accessible() {
        let mut s = fixture();
        for mode in [LoopMode::Off, LoopMode::One, LoopMode::All] {
            s.playlist.loop_mode = mode;
            let scene = compact_scene(Viewport::new(480.0, 440.0, 1.0), &s, None);
            let hit = scene.hits.iter().find(|hit| hit.item_id == "loop").unwrap();
            assert_eq!(
                scene.hit_test(hit.rect.x + 8.0, hit.rect.y + 8.0),
                Some("loop")
            );
        }
    }
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
