//! Transient Sway workspace picker. Super+digit is owned by Sway, not this
//! layer-shell surface: each binding invokes the CLI, which forwards the
//! selection to an existing overlay and independently switches the compositor.
//! Sway's Super_L/Super_R --release bindings send the closing message.
//!
//! Inactive Sway workspaces have no screencopy surface. Keep bounded previews
//! of workspaces last seen on screen; never mislabel an unrelated output as a
//! screenshot of an unseen workspace.

pub use nuraloumi_providers::{ActionProvider, ActionResult, Provider, ProviderError};
// This shared provider file also contains screenshot save/clipboard actions
// used by the standalone screenshot surface, but not by this picker.
#[allow(dead_code)]
#[path = "../../../nuraloumi-providers/src/screenshot.rs"]
mod screenshot;

use cairo::{Context, FontSlant, FontWeight, Format, ImageSurface};
use nuraloumi_providers::{CommandLimits, CommandRunner, CommandSpec, SystemCommandRunner};
use nuraloumi_wayland::{
    BackendError, Frame, Key, MenuConfig, PixelFormat, PlatformEvent, WaylandBackend,
};
use screenshot::{
    CaptureSelection, ScreenshotAction, ScreenshotPreview, ScreenshotProvider, ScreenshotSource,
    MAX_PREVIEW_PIXELS,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::raw::{c_int, c_short, c_ulong};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const IPC_LIMITS: CommandLimits = CommandLimits::new(Duration::from_secs(2), 128 * 1024);
const PREVIEW_LIMIT: usize = MAX_PREVIEW_PIXELS * 4 + 12;
const SESSION_TIMEOUT: Duration = Duration::from_secs(15);
const POLLIN: c_short = 1;
const MAX_CARDS: usize = 9;

#[repr(C)]
struct PollFd {
    fd: c_int,
    events: c_short,
    revents: c_short,
}

unsafe extern "C" {
    fn poll(fds: *mut PollFd, nfds: c_ulong, timeout: c_int) -> c_int;
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct SwayWorkspace {
    num: i32,
    name: String,
    focused: bool,
    #[serde(default)]
    output: String,
}

#[derive(Debug, Clone, Serialize)]
struct Card {
    num: u8,
    name: String,
    selected: bool,
    screenshot: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Request {
    Select(u8),
    Release,
    Fixture,
    Help,
}

fn parse_request(args: impl IntoIterator<Item = String>) -> Result<Request, String> {
    let mut args = args.into_iter();
    let arg = args.next().ok_or("expected a digit 1..9 or --release")?;
    if args.next().is_some() {
        return Err("expected exactly one argument".into());
    }
    match arg.as_str() {
        "-h" | "--help" => Ok(Request::Help),
        "--release" => Ok(Request::Release),
        "--fixture" => Ok(Request::Fixture),
        _ => arg
            .parse::<u8>()
            .ok()
            .filter(|num| (1..=9).contains(num) && arg.len() == 1)
            .map(Request::Select)
            .ok_or_else(|| "workspace must be a single integer from 1 to 9".to_owned()),
    }
}

fn parsed_workspaces(json: &str) -> Result<Vec<SwayWorkspace>, String> {
    let mut all: Vec<SwayWorkspace> = serde_json::from_str(json)
        .map_err(|error| format!("invalid swaymsg workspace JSON: {error}"))?;
    all.retain(|ws| (1..=9).contains(&ws.num));
    // A Sway workspace name can be user-defined and arbitrarily long.
    // Bound it before the Cairo text path, but never use it as a command.
    for workspace in &mut all {
        workspace.name = workspace
            .name
            .chars()
            .filter(|ch| !ch.is_control())
            .take(48)
            .collect();
    }
    all.sort_by_key(|ws| ws.num);
    all.dedup_by_key(|ws| ws.num);
    Ok(all)
}

fn cards(
    workspaces: &[SwayWorkspace],
    selected: u8,
    previews: &BTreeMap<u8, ScreenshotPreview>,
) -> Vec<Card> {
    let mut cards = workspaces
        .iter()
        .take(MAX_CARDS)
        .map(|ws| Card {
            num: ws.num as u8,
            name: ws.name.clone(),
            selected: ws.num == i32::from(selected),
            screenshot: previews.contains_key(&(ws.num as u8)),
        })
        .collect::<Vec<_>>();
    if !cards.iter().any(|card| card.num == selected) {
        cards.push(Card {
            num: selected,
            name: selected.to_string(),
            selected: true,
            screenshot: previews.contains_key(&selected),
        });
        cards.sort_by_key(|card| card.num);
    }
    cards
}

fn sway_workspaces(runner: &mut impl CommandRunner) -> Result<Vec<SwayWorkspace>, String> {
    let result = runner
        .run(
            &CommandSpec::new("swaymsg").args(["-r", "-t", "get_workspaces"]),
            IPC_LIMITS,
        )
        .map_err(|error| error.to_string())?;
    if result.status != 0 {
        return Err(format!("swaymsg get_workspaces: {}", result.stderr));
    }
    parsed_workspaces(&result.stdout)
}

fn switch_to(runner: &mut impl CommandRunner, num: u8) -> Result<(), String> {
    if !(1..=9).contains(&num) {
        return Err("workspace must be 1..9".into());
    }
    let result = runner
        .run(
            &CommandSpec::new("swaymsg")
                .args(["-r", "-t", "command"])
                .arg(format!("workspace number {num}")),
            IPC_LIMITS,
        )
        .map_err(|error| error.to_string())?;
    if result.status != 0 {
        return Err(format!("swaymsg switch: {}", result.stderr));
    }
    let replies: Vec<serde_json::Value> =
        serde_json::from_str(&result.stdout).map_err(|error| error.to_string())?;
    if !replies
        .iter()
        .any(|reply| reply.get("success") == Some(&serde_json::Value::Bool(true)))
        || replies
            .iter()
            .any(|reply| reply.get("success") != Some(&serde_json::Value::Bool(true)))
    {
        return Err(format!("Sway rejected workspace {num}: {}", result.stdout));
    }
    Ok(())
}

fn session_root() -> Result<PathBuf, String> {
    let runtime = PathBuf::from(env::var_os("XDG_RUNTIME_DIR").ok_or("XDG_RUNTIME_DIR unset")?);
    if !runtime.is_absolute() {
        return Err("XDG_RUNTIME_DIR must be absolute".into());
    }
    let owner = unsafe { geteuid() };
    validate_private_dir(&runtime, owner)?;
    let socket = env::var("SWAYSOCK").map_err(|_| "SWAYSOCK unset; not a Sway session")?;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    socket.hash(&mut hash);
    let dir = runtime.join(format!("nuraloumi-sway-switcher-{:016x}", hash.finish()));
    match fs::create_dir(&dir) {
        Ok(()) => {
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
                .map_err(|error| error.to_string())?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(format!("creating switcher state: {error}")),
    }
    validate_private_dir(&dir, owner)?;
    Ok(dir)
}

fn validate_private_dir(dir: &Path, uid: u32) -> Result<(), String> {
    let metadata = fs::symlink_metadata(dir).map_err(|error| error.to_string())?;
    if !metadata.file_type().is_dir()
        || metadata.uid() != uid
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(format!(
            "switcher directory is not private: {}",
            dir.display()
        ));
    }
    Ok(())
}

fn timestamp_ns() -> Result<u128, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|delta| delta.as_nanos())
        .map_err(|error| error.to_string())
}

fn record_release(dir: &Path) -> Result<(), String> {
    let temporary = dir.join(format!(".release-{}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    let result = (|| -> std::io::Result<()> {
        file.write_all(&timestamp_ns().map_err(std::io::Error::other)?.to_le_bytes())?;
        file.flush()?;
        fs::rename(&temporary, dir.join("release.stamp"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|error| error.to_string())
}

fn released_since(dir: &Path, started: u128) -> bool {
    let mut bytes = Vec::new();
    let read = File::open(dir.join("release.stamp"))
        .and_then(|file| file.take(17).read_to_end(&mut bytes));
    read.is_ok()
        && bytes.len() == 16
        && u128::from_le_bytes(bytes.as_slice().try_into().unwrap()) >= started
}

fn send_request(path: &Path, value: u8) -> std::io::Result<()> {
    UnixDatagram::unbound()?.send_to(&[value], path).map(|_| ())
}

struct SocketOwner {
    path: PathBuf,
    inode: u64,
}
impl Drop for SocketOwner {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.path)
            .is_ok_and(|meta| meta.ino() == self.inode && meta.file_type().is_socket())
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn forward_or_bind(
    path: &Path,
    request: Request,
) -> Result<Option<(UnixDatagram, SocketOwner)>, String> {
    let value = match request {
        Request::Select(n) => n,
        _ => return Err("invalid live request".into()),
    };
    match UnixDatagram::bind(path) {
        Ok(socket) => {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                .map_err(|error| error.to_string())?;
            let inode = fs::symlink_metadata(path)
                .map_err(|error| error.to_string())?
                .ino();
            Ok(Some((
                socket,
                SocketOwner {
                    path: path.to_owned(),
                    inode,
                },
            )))
        }
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            if send_request(path, value).is_ok() {
                return Ok(None);
            }
            // A stale socket, never a symlink or arbitrary file, may be
            // replaced only inside the user-owned private runtime directory.
            let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
            if !metadata.file_type().is_socket() || metadata.uid() != unsafe { geteuid() } {
                return Err("refusing to replace non-owned or non-socket IPC path".into());
            }
            fs::remove_file(path).map_err(|error| error.to_string())?;
            let socket = UnixDatagram::bind(path).map_err(|error| error.to_string())?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                .map_err(|error| error.to_string())?;
            let inode = fs::symlink_metadata(path)
                .map_err(|error| error.to_string())?
                .ino();
            Ok(Some((
                socket,
                SocketOwner {
                    path: path.to_owned(),
                    inode,
                },
            )))
        }
        Err(error) => Err(format!("creating workspace switcher IPC: {error}")),
    }
}

unsafe extern "C" {
    fn geteuid() -> u32;
}

fn preview_path(dir: &Path, num: u8) -> PathBuf {
    dir.join(format!("{num}.argb"))
}

fn save_preview(dir: &Path, num: u8, preview: &ScreenshotPreview) -> Result<(), String> {
    if !(1..=9).contains(&num)
        || preview.width == 0
        || preview.height == 0
        || (preview.width as usize * preview.height as usize) > MAX_PREVIEW_PIXELS
        || preview.argb32.len() != preview.width as usize * preview.height as usize * 4
    {
        return Err("invalid workspace screenshot".into());
    }
    let temp = dir.join(format!(".{num}.{}.tmp", std::process::id()));
    let path = preview_path(dir, num);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|error| error.to_string())?;
    let result = (|| -> std::io::Result<()> {
        file.write_all(b"NSW1")?;
        file.write_all(&preview.width.to_le_bytes())?;
        file.write_all(&preview.height.to_le_bytes())?;
        file.write_all(&preview.argb32)?;
        file.flush()?;
        fs::rename(&temp, &path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map_err(|error| error.to_string())
}

fn load_preview(dir: &Path, num: u8) -> Option<ScreenshotPreview> {
    let mut bytes = Vec::new();
    File::open(preview_path(dir, num))
        .ok()?
        .take((PREVIEW_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() < 12 || bytes.len() > PREVIEW_LIMIT || &bytes[..4] != b"NSW1" {
        return None;
    }
    let width = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
    let height = u32::from_le_bytes(bytes[8..12].try_into().ok()?);
    let pixels = (width as usize).checked_mul(height as usize)?;
    if width == 0 || height == 0 || pixels > MAX_PREVIEW_PIXELS || bytes.len() != pixels * 4 + 12 {
        return None;
    }
    Some(ScreenshotPreview {
        width,
        height,
        argb32: bytes[12..].to_vec(),
    })
}

fn capture_visible(workspaces: &[SwayWorkspace], dir: &Path) {
    let Some(current) = workspaces.iter().find(|workspace| workspace.focused) else {
        return;
    };
    let output = current.output.as_str();
    let selection = if !output.is_empty()
        && output.len() <= 64
        && !output.starts_with('-')
        && output
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        CaptureSelection::Output(output.to_string())
    } else {
        // Never assign a whole multi-output image to one workspace.
        return;
    };
    if let Ok(mut provider) = ScreenshotProvider::new(ScreenshotSource::Grim, selection) {
        if provider.execute(ScreenshotAction::Capture).is_ok() {
            if let Ok(Some(preview)) = provider.snapshot().map(|s| s.preview) {
                let _ = save_preview(dir, current.num as u8, &preview);
            }
        }
    }
}

fn loaded_previews(dir: &Path) -> BTreeMap<u8, ScreenshotPreview> {
    (1..=9)
        .filter_map(|num| load_preview(dir, num).map(|preview| (num, preview)))
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn rectangle(
    ctx: &Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    r: f64,
    g: f64,
    b: f64,
) -> Result<(), String> {
    ctx.set_source_rgb(r, g, b);
    ctx.rectangle(x, y, width, height);
    ctx.fill().map_err(|error| error.to_string())
}

fn draw_frame(
    width: u32,
    height: u32,
    scale: i32,
    cards: &[Card],
    previews: &BTreeMap<u8, ScreenshotPreview>,
) -> Result<(Vec<u8>, usize), String> {
    let s = scale.max(1) as u32;
    let pw = width.checked_mul(s).ok_or("surface width overflow")?;
    let ph = height.checked_mul(s).ok_or("surface height overflow")?;
    let mut image = ImageSurface::create(Format::ARgb32, pw as i32, ph as i32)
        .map_err(|error| error.to_string())?;
    let ctx = Context::new(&image).map_err(|error| error.to_string())?;
    ctx.scale(f64::from(s), f64::from(s));
    rectangle(
        &ctx,
        0.0,
        0.0,
        f64::from(width),
        f64::from(height),
        0.055,
        0.064,
        0.078,
    )?;
    ctx.select_font_face("Sans", FontSlant::Normal, FontWeight::Bold);
    ctx.set_font_size(23.0);
    ctx.set_source_rgb(0.91, 0.93, 0.95);
    ctx.move_to(18.0, 32.0);
    ctx.show_text("Workspaces")
        .map_err(|error| error.to_string())?;
    ctx.select_font_face("Sans", FontSlant::Normal, FontWeight::Normal);
    ctx.set_font_size(13.0);
    ctx.set_source_rgb(0.65, 0.70, 0.76);
    ctx.move_to(180.0, 31.0);
    ctx.show_text("Release Super to close")
        .map_err(|error| error.to_string())?;

    let cols = if cards.len() <= 2 {
        cards.len().max(1)
    } else {
        3
    };
    let rows = cards.len().div_ceil(cols);
    let gap = 12.0;
    let pad = 18.0;
    let top = 54.0;
    let card_width =
        ((f64::from(width) - pad * 2.0 - gap * (cols - 1) as f64) / cols as f64).max(1.0);
    let card_height =
        ((f64::from(height) - top - pad - gap * (rows - 1) as f64) / rows as f64).max(1.0);
    for (i, card) in cards.iter().enumerate() {
        let x = pad + (i % cols) as f64 * (card_width + gap);
        let y = top + (i / cols) as f64 * (card_height + gap);
        rectangle(&ctx, x, y, card_width, card_height, 0.10, 0.12, 0.15)?;
        rectangle(
            &ctx,
            x + 5.0,
            y + 5.0,
            card_width - 10.0,
            card_height - 37.0,
            0.055,
            0.068,
            0.09,
        )?;
        if let Some(preview) = previews.get(&card.num) {
            let source = ImageSurface::create_for_data(
                preview.argb32.clone(),
                Format::ARgb32,
                preview.width as i32,
                preview.height as i32,
                (preview.width * 4) as i32,
            )
            .map_err(|error| error.to_string())?;
            ctx.save().map_err(|error| error.to_string())?;
            ctx.translate(x + 5.0, y + 5.0);
            ctx.scale(
                (card_width - 10.0) / f64::from(preview.width),
                (card_height - 37.0) / f64::from(preview.height),
            );
            ctx.set_source_surface(&source, 0.0, 0.0)
                .map_err(|error| error.to_string())?;
            ctx.paint().map_err(|error| error.to_string())?;
            ctx.restore().map_err(|error| error.to_string())?;
        } else {
            ctx.set_font_size(13.0);
            ctx.set_source_rgb(0.55, 0.61, 0.69);
            ctx.move_to(x + 12.0, y + card_height / 2.0);
            ctx.show_text("No cached screenshot")
                .map_err(|error| error.to_string())?;
        }
        ctx.set_font_size(17.0);
        ctx.set_source_rgb(0.94, 0.95, 0.97);
        ctx.move_to(x + 11.0, y + card_height - 10.0);
        ctx.show_text(&format!("{}  {}", card.num, card.name))
            .map_err(|error| error.to_string())?;
        if card.selected {
            ctx.set_source_rgb(0.23, 0.55, 1.0);
            ctx.set_line_width(4.0);
        } else {
            ctx.set_source_rgb(0.25, 0.28, 0.33);
            ctx.set_line_width(1.0);
        }
        ctx.rectangle(x + 2.0, y + 2.0, card_width - 4.0, card_height - 4.0);
        ctx.stroke().map_err(|error| error.to_string())?;
    }
    drop(ctx);
    image.flush();
    let stride = image.stride() as usize;
    let bytes = image.data().map_err(|error| error.to_string())?.to_vec();
    Ok((bytes, stride))
}

fn super_release(key: &Key, pressed: bool) -> bool {
    if pressed {
        return false;
    }
    match key {
        Key::Raw(125 | 126) => true,
        Key::Modified { key, .. } => super_release(key, false),
        _ => false,
    }
}

fn drain_requests(socket: &UnixDatagram, selected: &mut u8) -> Result<bool, String> {
    let mut bytes = [0; 8];
    loop {
        match socket.recv(&mut bytes) {
            Ok(1) if bytes[0] == 0 => return Ok(true),
            Ok(1) if (1..=9).contains(&bytes[0]) => *selected = bytes[0],
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(false),
            Err(error) => return Err(format!("workspace IPC receive: {error}")),
        }
    }
}

fn live(
    initial: u8,
    socket: UnixDatagram,
    owner: SocketOwner,
    dir: &Path,
    started: u128,
) -> Result<(), String> {
    socket
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let mut runner = SystemCommandRunner;
    let mut selected = initial;
    let mut workspaces = sway_workspaces(&mut runner)?;
    // This may take several seconds on Tegra20. A release sent in the
    // meantime must be honored before creating a visible layer surface.
    std::thread::sleep(Duration::from_millis(65));
    capture_visible(&workspaces, dir);
    let before = selected;
    if released_since(dir, started) || drain_requests(&socket, &mut selected)? {
        return Ok(());
    }
    if before != selected {
        workspaces = sway_workspaces(&mut runner)?;
    }
    let mut backend = WaylandBackend::connect().map_err(|error| error.to_string())?;
    if !backend.capabilities().layer_shell {
        return Err("Sway layer-shell is unavailable".into());
    }
    backend
        .blocking_dispatch()
        .map_err(|error| error.to_string())?;
    let before = selected;
    if released_since(dir, started) || drain_requests(&socket, &mut selected)? {
        return Ok(());
    }
    if before != selected {
        workspaces = sway_workspaces(&mut runner)?;
    }
    let output = backend.outputs().into_iter().next();
    let (out_width, out_height) = output
        .as_ref()
        .and_then(|info| info.logical_size())
        .unwrap_or((1280, 800));
    let width = out_width.saturating_sub(36).clamp(220, 1100);
    let height = out_height.saturating_sub(85).clamp(200, 660);
    let surface = backend
        .create_menu(MenuConfig {
            width,
            height,
            margin_top: 52,
            margin_left: 18,
            output: output.map(|info| info.id),
            namespace: "nuraloumi-sway-workspace-switcher".into(),
        })
        .map_err(|error| error.to_string())?;
    let mut geometry = None::<(u32, u32, i32)>;
    let mut redraw = false;
    let start = Instant::now();
    let mut closed = false;
    while !closed && start.elapsed() < SESSION_TIMEOUT {
        backend.flush().map_err(|error| error.to_string())?;
        let mut fds = [
            PollFd {
                fd: backend.poll_fd(),
                events: POLLIN,
                revents: 0,
            },
            PollFd {
                fd: socket.as_raw_fd(),
                events: POLLIN,
                revents: 0,
            },
        ];
        let remaining = SESSION_TIMEOUT
            .saturating_sub(start.elapsed())
            .as_millis()
            .min(i32::MAX as u128) as i32;
        // SAFETY: two valid borrowed file descriptors and a correctly laid out
        // pollfd array; poll does not retain the pointer.
        let ready = unsafe { poll(fds.as_mut_ptr(), fds.len() as c_ulong, remaining) };
        if ready < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(format!(
                "workspace poll: {}",
                std::io::Error::last_os_error()
            ));
        }
        if fds[1].revents & POLLIN != 0 {
            let previous = selected;
            closed = drain_requests(&socket, &mut selected)?;
            if !closed && selected != previous {
                if let Ok(latest) = sway_workspaces(&mut runner) {
                    workspaces = latest;
                }
                redraw = true;
            }
        }
        if closed {
            break;
        }
        if fds[0].revents & POLLIN != 0 {
            backend
                .blocking_dispatch()
                .map_err(|error| error.to_string())?;
            for event in backend.drain_events() {
                if event.surface != Some(surface) {
                    continue;
                }
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
                    } => {
                        closed = true;
                    }
                    PlatformEvent::Key { key, pressed } if super_release(&key, pressed) => {
                        closed = true;
                    }
                    _ => {}
                }
            }
        }
        if redraw && !closed {
            if let Some((w, h, scale)) = geometry {
                // Loading a newly completed cache file never blocks command dispatch.
                let previews = loaded_previews(dir);
                let model = cards(&workspaces, selected, &previews);
                let (pixels, stride) = draw_frame(w, h, scale, &model, &previews)?;
                let result = backend.present(
                    surface,
                    Frame {
                        width: w * scale.max(1) as u32,
                        height: h * scale.max(1) as u32,
                        stride,
                        format: PixelFormat::Argb8888,
                        pixels: &pixels,
                    },
                );
                if let Err(error) = result {
                    if !matches!(error, BackendError::WouldBlock) {
                        return Err(error.to_string());
                    }
                    // A later buffer-release dispatch gives us another try.
                    redraw = true;
                } else {
                    redraw = false;
                }
            }
        }
    }
    // Do not capture our own layer-shell overlay. Destroy it and give Sway a
    // presentation interval before caching the newly visible workspace.
    backend
        .destroy_surface(surface)
        .map_err(|error| error.to_string())?;
    backend.roundtrip().map_err(|error| error.to_string())?;
    // A new Super+digit must not be forwarded to the old, closing session
    // while screenshot capture or Wayland cleanup is still in progress.
    drop(owner);
    drop(socket);
    std::thread::sleep(Duration::from_millis(65));
    // If another shortcut has already created its overlay, don't capture it
    // and accidentally cache that transient UI as a desktop screenshot.
    if !dir.join("session.sock").exists() {
        if let Ok(current) = sway_workspaces(&mut runner) {
            capture_visible(&current, dir);
        }
    }
    Ok(())
}

fn entry(request: Request) -> Result<(), String> {
    match request {
        Request::Help => {
            println!("nuraloumi-sway-workspace-switcher [1-9 | --release | --fixture]\nUse Sway bindsym $mod+1..9 to invoke selection and bindsym --release Super_L/Super_R to close.");
            return Ok(());
        }
        Request::Fixture => {
            let ws = parsed_workspaces(
                r#"[{"num":1,"name":"1:work","focused":false,"output":"DSI-1"},{"num":3,"name":"3","focused":true,"output":"DSI-1"}]"#,
            )?;
            println!(
                "{}",
                serde_json::to_string_pretty(&cards(&ws, 3, &BTreeMap::new()))
                    .map_err(|error| error.to_string())?
            );
            return Ok(());
        }
        _ => {}
    }
    let started = timestamp_ns()?;
    let dir = session_root()?;
    let request_socket = dir.join("session.sock");
    if request == Request::Release {
        // Also persist the release when the original exec has not yet
        // created its socket (or is still busy capturing a preview).
        record_release(&dir)?;
        let _ = send_request(&request_socket, 0);
        return Ok(());
    }
    if let Request::Select(num) = request {
        // Always switch first; the overlay must not gate workspace changes.
        switch_to(&mut SystemCommandRunner, num)?;
    }
    let Some((socket, owner)) = forward_or_bind(&request_socket, request)? else {
        return Ok(());
    };
    match request {
        Request::Select(num) => live(num, socket, owner, &dir, started),
        _ => unreachable!(),
    }
}

fn main() -> ExitCode {
    match parse_request(env::args().skip(1)).and_then(entry) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("nuraloumi-sway-workspace-switcher: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nuraloumi_providers::{CommandOutput, FixtureCommandRunner};

    fn output(stdout: &str) -> CommandOutput {
        CommandOutput {
            status: 0,
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    #[test]
    fn cli_never_accepts_unvalidated_workspace_commands() {
        assert_eq!(parse_request(["5".into()]).unwrap(), Request::Select(5));
        assert_eq!(
            parse_request(["--release".into()]).unwrap(),
            Request::Release
        );
        for arg in ["0", "10", "-1", "1; exec sh", "01", "--switch", "a"] {
            assert!(parse_request([arg.into()]).is_err(), "{arg}");
        }
    }

    #[test]
    fn sway_list_and_last_pressed_selection_are_deterministic() {
        let workspaces = parsed_workspaces(
            r#"[
          {"num":3,"name":"3:code","focused":true,"output":"DSI-1"},
          {"num":1,"name":"1:web","focused":false,"output":"DSI-1"},
          {"num":10,"name":"10","focused":false,"output":"DSI-1"}
        ]"#,
        )
        .unwrap();
        assert_eq!(workspaces.len(), 2);
        let cards = cards(&workspaces, 2, &BTreeMap::new());
        assert_eq!(cards.iter().map(|c| c.num).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(
            cards
                .iter()
                .filter(|c| c.selected)
                .map(|c| c.num)
                .collect::<Vec<_>>(),
            [2]
        );
        assert!(!cards.iter().any(|card| card.screenshot));
        assert!(parsed_workspaces(r#"[{"num":"malicious","name":"x","focused":true}]"#).is_err());
    }

    #[test]
    fn exact_sway_argv_and_success_reply_are_required() {
        let mut runner = FixtureCommandRunner::default();
        runner.insert(
            CommandSpec::new("swaymsg").args(["-r", "-t", "get_workspaces"]),
            output(r#"[{"num":4,"name":"4","focused":true,"output":"DSI-1"}]"#),
        );
        assert_eq!(sway_workspaces(&mut runner).unwrap()[0].num, 4);
        runner.insert(
            CommandSpec::new("swaymsg").args(["-r", "-t", "command", "workspace number 4"]),
            output(r#"[{"success":true}]"#),
        );
        switch_to(&mut runner, 4).unwrap();
        assert!(switch_to(&mut runner, 0).is_err());
        runner.insert(
            CommandSpec::new("swaymsg").args(["-r", "-t", "command", "workspace number 3"]),
            output(r#"[{"success":false,"error":"denied"}]"#),
        );
        assert!(switch_to(&mut runner, 3).is_err());
    }

    #[test]
    fn repeat_selection_and_release_share_exactly_one_socket() {
        let dir = env::temp_dir().join(format!("sway-switcher-ipc-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.sock");
        let (socket, owner) = forward_or_bind(&path, Request::Select(1))
            .unwrap()
            .expect("first invocation owns the overlay socket");
        socket
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        assert!(forward_or_bind(&path, Request::Select(7))
            .unwrap()
            .is_none());
        let mut data = [0u8; 8];
        assert_eq!(socket.recv(&mut data).unwrap(), 1);
        assert_eq!(data[0], 7);
        send_request(&path, 0).unwrap();
        assert_eq!(socket.recv(&mut data).unwrap(), 1);
        assert_eq!(data[0], 0);
        drop(owner);
        assert!(!path.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn early_release_is_persisted_and_does_not_close_next_session() {
        let dir = env::temp_dir().join(format!("sway-switcher-release-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let selected_at = timestamp_ns().unwrap();
        record_release(&dir).unwrap();
        assert!(
            released_since(&dir, selected_at),
            "release during startup must close"
        );
        let after_release = timestamp_ns().unwrap();
        assert!(
            !released_since(&dir, after_release + 1),
            "an old release must not close a later selection"
        );
        fs::write(dir.join("release.stamp"), [0u8; 17]).unwrap();
        assert!(
            !released_since(&dir, selected_at),
            "malformed release is ignored"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ipc_directory_rejects_symlink_and_public_permissions() {
        use std::os::unix::fs::symlink;
        let dir = env::temp_dir().join(format!("sway-switcher-dir-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        let uid = unsafe { geteuid() };
        assert!(validate_private_dir(&dir, uid).is_ok());
        let link = dir.with_extension("link");
        symlink(&dir, &link).unwrap();
        assert!(validate_private_dir(&link, uid).is_err());
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(validate_private_dir(&dir, uid).is_err());
        let _ = fs::remove_file(link);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn pending_selection_followed_by_release_closes_without_mapping() {
        let dir = env::temp_dir().join(format!("sway-switcher-early-ipc-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.sock");
        let (socket, owner) = forward_or_bind(&path, Request::Select(1)).unwrap().unwrap();
        socket.set_nonblocking(true).unwrap();
        send_request(&path, 7).unwrap();
        send_request(&path, 0).unwrap();
        let mut selected = 1;
        assert!(drain_requests(&socket, &mut selected).unwrap());
        assert_eq!(selected, 7);
        drop(owner);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn cached_preview_is_bounded_and_rejects_corruption() {
        let dir = env::temp_dir().join(format!("sway-switcher-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let img = ScreenshotPreview {
            width: 3,
            height: 2,
            argb32: vec![255; 24],
        };
        save_preview(&dir, 4, &img).unwrap();
        assert_eq!(load_preview(&dir, 4).unwrap().argb32, img.argb32);
        assert!(save_preview(&dir, 0, &img).is_err());
        fs::write(preview_path(&dir, 4), b"NSW1invalid").unwrap();
        assert!(load_preview(&dir, 4).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn blue_selection_and_preview_render_without_compositor() {
        let img = ScreenshotPreview {
            width: 2,
            height: 2,
            argb32: vec![255; 16],
        };
        let previews = BTreeMap::from([(2, img)]);
        let cards = cards(&[], 2, &previews);
        assert!(cards[0].selected && cards[0].screenshot);
        let (pixels, stride) = draw_frame(640, 400, 1, &cards, &previews).unwrap();
        assert!(stride >= 640 * 4);
        assert_eq!(pixels.len(), stride * 400);
        assert!(super_release(&Key::Raw(125), false));
        assert!(!super_release(&Key::Raw(125), true));
    }
}
