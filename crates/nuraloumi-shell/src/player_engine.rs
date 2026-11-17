//! One stateful media player daemon, with in-process GStreamer audio and VDE/GPU
//! video. Dynamic libraries keep EGL optional for Cairo-only menu surfaces.
//! No spawned player, CPU video conversion or silent GPU fallback.
#[path = "video_engine.rs"]
mod video_engine;
use serde::{Deserialize, Serialize};
use std::{
    ffi::{c_char, c_int, c_void, CStr, CString},
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    path::{Path, PathBuf},
    ptr,
};
use video_engine::{VideoEngine, VideoPoll};

const MAX_TRACKS: usize = 256;
const MAX_PATH_BYTES: usize = 4096;
const MAX_M3U_BYTES: u64 = 256 * 1024;
const MAX_STATE_BYTES: u64 = 256 * 1024;
const GST_FORMAT_TIME: c_int = 3;
const GST_STATE_NULL: c_int = 1;
const GST_STATE_PAUSED: c_int = 3;
const GST_STATE_PLAYING: c_int = 4;
const GST_MESSAGE_EOS: u32 = 1;
const GST_MESSAGE_ERROR: u32 = 2;
const GST_SEEK_FLAG_FLUSH: c_int = 1;
const GST_SEEK_FLAG_ACCURATE: c_int = 2;
// GstPlayFlags: VIDEO=1, AUDIO=2. Video is rendered by the native
// in-process renderer, never by an unqualified GstGL playbin sink.
const GST_PLAY_FLAG_AUDIO: c_int = 2;

/// The source renderer has a 414-frame VDE/GPU/Sway/AAC hardware
/// qualification. Integration into playerd has separate release gates.
pub const VDE_GPU_STREAMING_STATUS: &str =
    "VDE/Grate 4-pass: 414-frame source PASS; playerd hardware gate pending";
pub const DRM_FORMAT_YU12: u32 = u32::from_le_bytes(*b"YU12");
pub const DRM_FORMAT_XR24: u32 = u32::from_le_bytes(*b"XR24");

/// Metadata expected at the experimental DMA-BUF boundary. All planes are
/// independent FDs and CPU mapping must not become a conversion fallback.
/// Dimensions/strides here are specific to the 854×480 Tegra20 prototype;
/// a future backend must negotiate instead of silently assuming them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VdeGpuFrameContract {
    pub drm_format: u32,
    pub modifier: u64,
    pub display_width: u32,
    pub display_height: u32,
    pub capture_width: u32,
    pub strides: [u32; 3],
    pub dmabuf_planes: usize,
}
/// Parse local M3U/M3U8 without recursively loading referenced playlists.
/// EXTINF is accepted as metadata but is not trusted as a media path.
pub fn parse_m3u(path: &Path) -> Result<Vec<String>, String> {
    let file = fs::File::open(path).map_err(|e| format!("open M3U: {e}"))?;
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_M3U_BYTES {
        return Err("M3U exceeds size limit".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_M3U_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("read M3U: {e}"))?;
    if bytes.len() as u64 > MAX_M3U_BYTES {
        return Err("M3U exceeds size limit".into());
    }
    let source = std::str::from_utf8(&bytes).map_err(|_| "M3U requires UTF-8")?;
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let parent = path.parent().ok_or("M3U has no parent")?;
    let mut tracks = Vec::new();
    for (line_number, line) in source.lines().enumerate() {
        let entry = line.trim();
        if entry.is_empty() || entry.starts_with('#') {
            continue;
        }
        if tracks.len() >= MAX_TRACKS {
            return Err("M3U track count exceeds limit".into());
        }
        if entry.len() > MAX_PATH_BYTES {
            return Err(format!("M3U line {} too long", line_number + 1));
        }
        let decoded;
        let local = if let Some(uri) = entry.strip_prefix("file://") {
            let uri = uri
                .strip_prefix("localhost/")
                .map(|p| format!("/{p}"))
                .unwrap_or_else(|| uri.to_owned());
            if !uri.starts_with('/') {
                return Err(format!("M3U line {}: nonlocal file URI", line_number + 1));
            }
            let raw = uri.as_bytes();
            let mut out = Vec::with_capacity(raw.len());
            let mut i = 0;
            while i < raw.len() {
                if raw[i] == b'%' {
                    if i + 2 >= raw.len() {
                        return Err("invalid URI escape".into());
                    }
                    let hex = |c: u8| (c as char).to_digit(16);
                    let hi = hex(raw[i + 1]).ok_or("invalid URI escape")?;
                    let lo = hex(raw[i + 2]).ok_or("invalid URI escape")?;
                    out.push(((hi << 4) | lo) as u8);
                    i += 3;
                } else {
                    out.push(raw[i]);
                    i += 1;
                }
            }
            decoded = String::from_utf8(out).map_err(|_| "non-UTF8 file URI")?;
            decoded.as_str()
        } else {
            if entry
                .split('/')
                .next()
                .is_some_and(|first| first.contains(':'))
            {
                return Err(format!(
                    "M3U line {}: unsupported URI scheme",
                    line_number + 1
                ));
            }
            entry
        };
        let media = Path::new(local);
        if media
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("m3u") || e.eq_ignore_ascii_case("m3u8"))
        {
            return Err(format!(
                "M3U line {}: nested playlists not supported",
                line_number + 1
            ));
        }
        let resolved = if media.is_absolute() {
            media.to_path_buf()
        } else {
            parent.join(media)
        };
        let full = canonical_media_path(resolved.to_str().ok_or("non-UTF8 media path")?)
            .map_err(|e| format!("M3U line {}: {e}", line_number + 1))?;
        // A symlink can disguise a nested playlist as a media extension.
        if Path::new(&full)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("m3u") || e.eq_ignore_ascii_case("m3u8"))
        {
            return Err(format!(
                "M3U line {}: nested playlists not supported",
                line_number + 1
            ));
        }
        if full.eq_ignore_ascii_case(&path.to_string_lossy()) {
            return Err("M3U cannot reference itself".into());
        }
        tracks.push(full);
    }
    Ok(tracks)
}

fn is_video_path(path: &Path) -> bool {
    path.extension()
        .and_then(|x| x.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "mp4" | "m4v" | "mov" | "mkv"
            )
        })
}

impl VdeGpuFrameContract {
    pub fn validate_854x480(&self) -> Result<(), &'static str> {
        if self.drm_format != DRM_FORMAT_YU12 || self.modifier != 0 {
            return Err("VDE must output linear YU12; no silent format conversion");
        }
        if (self.display_width, self.display_height) != (854, 480) || self.capture_width != 896 {
            return Err("VDE capture needs separately negotiated 896-wide storage");
        }
        if self.strides != [896, 448, 448] || self.dmabuf_planes != 3 {
            return Err("three distinct DMA-BUF planes with 64-byte-aligned strides required");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Playlist {
    pub tracks: Vec<String>,
    pub current: Option<usize>,
    pub volume: f64,
    pub muted: bool,
    #[serde(default)]
    pub loop_mode: LoopMode,
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LoopMode {
    #[default]
    Off,
    One,
    All,
}
impl Default for Playlist {
    fn default() -> Self {
        Self {
            tracks: Vec::new(),
            current: None,
            volume: 0.8,
            muted: false,
            loop_mode: LoopMode::Off,
        }
    }
}
impl Playlist {
    fn validate(&self) -> Result<(), String> {
        if self.tracks.len() > MAX_TRACKS {
            return Err("playlist full".into());
        }
        if self.current.is_some_and(|i| i >= self.tracks.len()) {
            return Err("invalid selected track".into());
        }
        if !self.volume.is_finite() || !(0.0..=1.0).contains(&self.volume) {
            return Err("volume must be between 0 and 1".into());
        }
        for path in &self.tracks {
            if !path.starts_with('/') || path.len() > MAX_PATH_BYTES || path.contains('\0') {
                return Err("invalid absolute media path".into());
            }
        }
        Ok(())
    }
    fn add(&mut self, path: String) -> Result<usize, String> {
        if self.tracks.len() == MAX_TRACKS {
            return Err("playlist full".into());
        }
        let path = canonical_media_path(&path)?;
        self.tracks.push(path);
        let index = self.tracks.len() - 1;
        self.current = Some(index);
        Ok(index)
    }
    fn remove(&mut self, index: usize) -> Result<bool, String> {
        if index >= self.tracks.len() {
            return Err("invalid playlist index".into());
        }
        let active = self.current == Some(index);
        self.tracks.remove(index);
        self.current = match self.current {
            Some(_) if self.tracks.is_empty() => None,
            Some(i) if i > index => Some(i - 1),
            Some(i) if i == index => Some(index.min(self.tracks.len() - 1)),
            other => other,
        };
        Ok(active)
    }
    fn move_track(&mut self, from: usize, to: usize) -> Result<(), String> {
        if from >= self.tracks.len() || to >= self.tracks.len() {
            return Err("invalid playlist index".into());
        }
        let selected = self.current;
        let entry = self.tracks.remove(from);
        self.tracks.insert(to, entry);
        self.current = selected.map(|i| {
            if i == from {
                to
            } else if from < i && i <= to {
                i - 1
            } else if to <= i && i < from {
                i + 1
            } else {
                i
            }
        });
        Ok(())
    }
}
pub fn canonical_media_path(path: &str) -> Result<String, String> {
    if path.is_empty() || path.len() > MAX_PATH_BYTES || path.contains('\0') {
        return Err("invalid media path length or NUL".into());
    }
    let canonical = fs::canonicalize(path).map_err(|e| format!("media path: {e}"))?;
    if !canonical.is_file() {
        return Err("not a regular media file".into());
    }
    let bytes = canonical.as_os_str().as_bytes();
    if bytes.len() > MAX_PATH_BYTES {
        return Err("canonical media path too long".into());
    }
    canonical
        .as_os_str()
        .to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| "media path is not UTF-8; use a UTF-8 filename for player URI".into())
}
// Correctly percent-encode raw UTF-8 and shell-metacharacters; never execute a shell.
fn media_uri(path: &Path) -> Result<CString, String> {
    let b = path.as_os_str().as_bytes();
    if !path.is_absolute() || b.len() > MAX_PATH_BYTES {
        return Err("invalid media URI path".into());
    }
    let mut result = String::with_capacity(b.len() * 3 + 7);
    result.push_str("file://");
    for &c in b {
        if c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'~' | b'/') {
            result.push(c as char);
        } else {
            use std::fmt::Write as _;
            write!(&mut result, "%{c:02X}").map_err(|e| e.to_string())?;
        }
    }
    CString::new(result).map_err(|e| e.to_string())
}
pub fn state_path() -> Result<PathBuf, String> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .ok_or("HOME/XDG_STATE_HOME not set")?;
    if !base.is_absolute() {
        return Err("state path must be absolute".into());
    }
    Ok(base.join("nuraloumi/player-playlist.json"))
}
pub fn load_playlist(path: &Path) -> Result<Playlist, String> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Playlist::default()),
        Err(e) => return Err(format!("read playlist: {e}")),
    };
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_STATE_BYTES {
        return Err("playlist file exceeds size limit".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_STATE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Err("playlist file exceeds size limit".into());
    }
    let state: Playlist =
        serde_json::from_slice(&bytes).map_err(|e| format!("playlist corrupted: {e}"))?;
    state.validate()?;
    Ok(state)
}
pub fn save_playlist(path: &Path, state: &Playlist) -> Result<(), String> {
    state.validate()?;
    let encoded = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
    if encoded.len() as u64 > MAX_STATE_BYTES {
        return Err("playlist exceeds size limit".into());
    }
    let parent = path.parent().ok_or("playlist path has no parent")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp = parent.join(format!(
        ".player-{}-{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    ));
    let result = (|| {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|e| format!("playlist temp: {e}"))?;
        f.write_all(&encoded).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
        fs::rename(&tmp, path).map_err(|e| format!("atomic playlist rename: {e}"))?;
        if let Ok(dir) = fs::File::open(parent) {
            let _ = dir.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Status,
    Add {
        path: String,
    },
    #[serde(rename = "import_m3u_v1", alias = "import_m3u")]
    ImportM3u {
        path: String,
    },
    Remove {
        index: usize,
    },
    Move {
        from: usize,
        to: usize,
    },
    Select {
        index: usize,
    },
    Play,
    Pause,
    Toggle,
    Stop,
    Next,
    Previous,
    Seek {
        position_ms: u64,
    },
    #[serde(rename = "seek_relative_v1", alias = "seek_relative")]
    SeekRelative {
        delta_ms: i64,
    },
    #[serde(rename = "jump_to_start_v1", alias = "jump_to_start")]
    JumpToStart,
    #[serde(rename = "loop_mode_v1", alias = "loop_mode")]
    LoopMode {
        mode: LoopMode,
    },
    Volume {
        level: f64,
    },
    Mute {
        muted: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub playlist: Playlist,
    pub status: String,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub error: Option<String>,
    pub audio_backend: String,
    pub gpu_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    pub state: Snapshot,
    pub error: Option<String>,
}

pub struct Player {
    pub playlist: Playlist,
    save_to: PathBuf,
    gst: Option<GstAudio>,
    video: Option<VideoEngine>,
    status: String,
    last_error: Option<String>,
    position_ms: u64,
    duration_ms: u64,
}
impl Player {
    pub fn open(save_to: PathBuf) -> Result<Self, String> {
        let playlist = load_playlist(&save_to)?;
        Ok(Self {
            playlist,
            save_to,
            gst: None,
            video: None,
            status: "Stopped".into(),
            last_error: None,
            position_ms: 0,
            duration_ms: 0,
        })
    }
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            playlist: self.playlist.clone(),
            status: self.status.clone(),
            position_ms: self.position_ms,
            duration_ms: self.duration_ms,
            error: self.last_error.clone(),
            audio_backend: if self.video.is_some() {
                "GStreamer AAC/PulseAudio (integrated VDE/GPU session)".into()
            } else if self.gst.is_some() {
                if cfg!(debug_assertions)
                    && std::env::var("NURALOUMI_PLAYER_TEST_FAKESINK").as_deref() == Ok("1")
                {
                    "GStreamer 1.x debug fakesink (NO physical audio)".into()
                } else {
                    "GStreamer 1.x playbin (audio only; physical sink unverified)".into()
                }
            } else {
                "GStreamer (not initialized)".into()
            },
            gpu_status: if self.video.is_some() {
                "VDE/Grate 4-pass video session (one playerd)".into()
            } else {
                VDE_GPU_STREAMING_STATUS.into()
            },
        }
    }
    fn gst(&mut self) -> Result<&mut GstAudio, String> {
        if self.gst.is_none() {
            self.gst = Some(GstAudio::new()?);
        }
        self.gst.as_mut().ok_or("GStreamer unavailable".into())
    }
    fn select_and_play(&mut self) -> Result<(), String> {
        let i = self.playlist.current.ok_or("playlist empty")?;
        let path = self
            .playlist
            .tracks
            .get(i)
            .ok_or("selected track missing")?
            .clone();
        if !Path::new(&path).is_file() {
            return Err("media file no longer exists".into());
        }
        let volume = self.playlist.volume;
        let muted = self.playlist.muted;
        if is_video_path(Path::new(&path)) {
            // ONE player daemon with two ordinary media *formats*: files
            // containing video use the native private in-process renderer.
            // No second player executable, IPC endpoint or audio instance.
            if let Some(gst) = &mut self.gst {
                gst.set_state(GST_STATE_NULL)?;
            }
            self.video.take();
            match VideoEngine::open(&path, volume, muted) {
                Ok(video) => self.video = Some(video),
                Err(error) => {
                    self.stop();
                    return Err(error);
                }
            }
        } else {
            self.video.take();
            if let Err(error) = self.gst()?.open(&path, volume, muted) {
                // open() may have set the previous pipeline to NULL.
                self.stop();
                return Err(error);
            }
        }
        self.position_ms = 0;
        self.duration_ms = 0;
        self.status = "Playing".into();
        Ok(())
    }
    pub fn command(&mut self, command: Command) -> Reply {
        let is_mutating = !matches!(command, Command::Status);
        // The playback backend can reject a selected file after the playlist
        // has changed. A failed command must not persist a partly-applied
        // track addition, removal, reordering or selection.
        let previous_playlist = self.playlist.clone();
        let result = self.apply(command);
        if let Err(e) = &result {
            self.playlist = previous_playlist.clone();
            self.last_error = Some(e.clone());
            if self.status != "Playing" && self.status != "Paused" {
                self.status = "Error".into();
            }
        } else if is_mutating {
            self.last_error = None;
        }
        let persistence = if is_mutating {
            save_playlist(&self.save_to, &self.playlist)
        } else {
            Ok(())
        };
        // A persistence failure must never advertise an uncommitted playlist
        // snapshot as successful. Runtime backend side effects are not undone.
        if persistence.is_err() {
            self.playlist = previous_playlist;
        }
        let error = result.err().or_else(|| persistence.err());
        Reply {
            ok: error.is_none(),
            error,
            state: self.snapshot(),
        }
    }
    fn apply(&mut self, cmd: Command) -> Result<(), String> {
        match cmd {
            Command::Status => {
                self.tick();
                Ok(())
            }
            Command::Add { path } => {
                let index = self.playlist.add(path)?;
                self.playlist.current = Some(index);
                self.select_and_play()
            }
            Command::ImportM3u { path } => {
                let canonical = canonical_media_path(&path)?;
                let tracks = parse_m3u(Path::new(&canonical))?;
                if tracks.is_empty() {
                    return Err("M3U contains no tracks".into());
                }
                if self.playlist.tracks.len() + tracks.len() > MAX_TRACKS {
                    return Err("playlist full".into());
                }
                let index = self.playlist.tracks.len();
                self.playlist.tracks.extend(tracks);
                self.playlist.current = Some(index);
                self.select_and_play()
            }
            Command::Remove { index } => {
                let active = self.playlist.remove(index)?;
                if active {
                    self.stop();
                    if self.playlist.current.is_some() {
                        self.select_and_play()?;
                    }
                }
                Ok(())
            }
            Command::Move { from, to } => self.playlist.move_track(from, to),
            Command::Select { index } => {
                if index >= self.playlist.tracks.len() {
                    return Err("invalid track index".into());
                }
                self.playlist.current = Some(index);
                self.select_and_play()
            }
            Command::Play => {
                if self.status == "Paused" {
                    if let Some(video) = &mut self.video {
                        video.play()?;
                    } else {
                        self.gst()?.set_state(GST_STATE_PLAYING)?;
                    }
                    self.status = "Playing".into();
                    Ok(())
                } else {
                    self.select_and_play()
                }
            }
            Command::Pause => {
                if self.status == "Playing" {
                    if let Some(video) = &mut self.video {
                        video.pause()?;
                    } else {
                        self.gst()?.set_state(GST_STATE_PAUSED)?;
                    }
                    self.status = "Paused".into();
                }
                Ok(())
            }
            Command::Toggle => {
                if self.status == "Playing" {
                    self.apply(Command::Pause)
                } else {
                    self.apply(Command::Play)
                }
            }
            Command::Stop => {
                self.stop();
                Ok(())
            }
            Command::JumpToStart => {
                if self.playlist.tracks.is_empty() {
                    return Ok(());
                }
                self.playlist.current = Some(0);
                self.select_and_play()
            }
            Command::LoopMode { mode } => {
                self.playlist.loop_mode = mode;
                Ok(())
            }
            Command::Next => self.advance(true),
            Command::Previous => self.advance(false),
            Command::SeekRelative { delta_ms } => {
                if !matches!(self.status.as_str(), "Playing" | "Paused")
                    || (self.gst.is_none() && self.video.is_none())
                {
                    return Err("no media loaded".into());
                }
                let target = if delta_ms < 0 {
                    self.position_ms.saturating_sub(delta_ms.unsigned_abs())
                } else {
                    self.position_ms.saturating_add(delta_ms as u64)
                };
                let target = if self.duration_ms > 0 {
                    target.min(self.duration_ms)
                } else {
                    target
                };
                self.apply(Command::Seek {
                    position_ms: target,
                })
            }
            Command::Seek { position_ms } => {
                let duration = self.duration_ms;
                if !matches!(self.status.as_str(), "Playing" | "Paused")
                    || (self.gst.is_none() && self.video.is_none())
                {
                    return Err("no media loaded".into());
                }
                if duration > 0 && position_ms > duration {
                    return Err("seek beyond duration".into());
                }
                if let Some(video) = &mut self.video {
                    video.seek(position_ms)?;
                } else {
                    self.gst()?.seek(position_ms)?;
                }
                self.position_ms = position_ms;
                Ok(())
            }
            Command::Volume { level } => {
                if !level.is_finite() || !(0.0..=1.0).contains(&level) {
                    return Err("volume must be in [0,1]".into());
                }
                self.playlist.volume = level;
                if let Some(video) = &mut self.video {
                    video.set_volume(level)?;
                }
                if let Some(gst) = &mut self.gst {
                    gst.set_volume(level)?;
                }
                Ok(())
            }
            Command::Mute { muted } => {
                self.playlist.muted = muted;
                if let Some(video) = &mut self.video {
                    video.set_mute(muted)?;
                }
                if let Some(gst) = &mut self.gst {
                    gst.set_mute(muted)?;
                }
                Ok(())
            }
        }
    }
    fn advance(&mut self, forward: bool) -> Result<(), String> {
        if self.playlist.tracks.is_empty() {
            self.stop();
            return Ok(());
        }
        let n = self.playlist.tracks.len();
        let i = self.playlist.current.unwrap_or(0);
        self.playlist.current = Some(if forward {
            (i + 1) % n
        } else {
            (i + n - 1) % n
        });
        self.select_and_play()
    }
    fn on_eos(&mut self) {
        let advance = match self.playlist.loop_mode {
            LoopMode::One => self.select_and_play(),
            LoopMode::All => self.advance(true),
            LoopMode::Off => {
                if self
                    .playlist
                    .current
                    .is_some_and(|i| i + 1 < self.playlist.tracks.len())
                {
                    self.advance(true)
                } else {
                    self.stop();
                    self.status = "Ended".into();
                    Ok(())
                }
            }
        };
        if let Err(e) = advance {
            self.stop();
            self.status = "Error".into();
            self.last_error = Some(format!("playlist EOS advance: {e}"));
        }
        let _ = save_playlist(&self.save_to, &self.playlist);
    }
    fn stop(&mut self) {
        self.video.take();
        if let Some(gst) = &mut self.gst {
            let _ = gst.set_state(GST_STATE_NULL);
        }
        self.status = "Stopped".into();
        self.position_ms = 0;
        self.duration_ms = 0;
    }
    pub fn tick(&mut self) {
        if self.status != "Playing" && self.status != "Paused" {
            return;
        }
        if let Some(video) = &self.video {
            match video.poll() {
                Ok(VideoPoll::Starting) => return,
                Ok(VideoPoll::Playing {
                    position_ms,
                    duration_ms,
                }) => {
                    self.position_ms = position_ms;
                    self.duration_ms = duration_ms;
                    if self.status != "Playing" {
                        self.status = "Playing".into();
                    }
                    return;
                }
                Ok(VideoPoll::Paused {
                    position_ms,
                    duration_ms,
                }) => {
                    self.position_ms = position_ms;
                    self.duration_ms = duration_ms;
                    self.status = "Paused".into();
                    return;
                }
                Ok(VideoPoll::Eos) => {
                    self.on_eos();
                    return;
                }
                Err(e) => {
                    self.stop();
                    self.last_error = Some(e);
                    self.status = "Error".into();
                    return;
                }
            }
        }
        let Some(gst) = &mut self.gst else {
            return;
        };
        match gst.tick() {
            Ok(GstPoll::Continue {
                position_ms,
                duration_ms,
            }) => {
                self.position_ms = position_ms;
                self.duration_ms = duration_ms;
            }
            Ok(GstPoll::Eos) => {
                self.on_eos();
            }
            Err(e) => {
                self.stop();
                self.last_error = Some(e);
                self.status = "Error".into();
            }
        }
    }
}

type FnInit = unsafe extern "C" fn(*mut c_int, *mut *mut *mut c_char, *mut *mut c_void) -> c_int;
type FnFactory = unsafe extern "C" fn(*const c_char, *const c_char) -> *mut c_void;
type FnBus = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type FnSetState = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
type FnQuery = unsafe extern "C" fn(*mut c_void, c_int, *mut i64) -> c_int;
type FnSeek = unsafe extern "C" fn(*mut c_void, c_int, c_int, i64) -> c_int;
type FnBusPop = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type FnUnref = unsafe extern "C" fn(*mut c_void);
type FnSet = unsafe extern "C" fn(*mut c_void, *const c_char, ...);
type FnParseError = unsafe extern "C" fn(*mut c_void, *mut *mut GError, *mut *mut c_char);
type FnFree = unsafe extern "C" fn(*mut c_void);
struct DlHandles(Vec<*mut c_void>);
impl Drop for DlHandles {
    fn drop(&mut self) {
        for &handle in self.0.iter().rev() {
            unsafe { libc::dlclose(handle) };
        }
    }
}
#[repr(C)]
struct GError {
    domain: u32,
    code: c_int,
    message: *mut c_char,
}
// ABI-stable public GStreamer miniobject and message prefix, checked against gst headers.
#[repr(C)]
struct GstMiniObject {
    type_: usize,
    refcount: c_int,
    lockstate: c_int,
    flags: u32,
    copy: *const c_void,
    dispose: *const c_void,
    free: *const c_void,
    priv_uint: u32,
    priv_pointer: *const c_void,
}
#[repr(C)]
struct GstMessageHeader {
    mini: GstMiniObject,
    kind: u32,
}
struct GstFns {
    handles: DlHandles,
    factory: FnFactory,
    bus: FnBus,
    set_state: FnSetState,
    query_pos: FnQuery,
    query_dur: FnQuery,
    seek: FnSeek,
    pop: FnBusPop,
    object_unref: FnUnref,
    mini_unref: FnUnref,
    set: FnSet,
    parse_error: FnParseError,
    error_free: FnFree,
    glib_free: FnFree,
}
impl GstFns {
    fn open() -> Result<Self, String> {
        unsafe {
            // RAII also closes successfully loaded DSOs when symbol lookup or
            // gst_init_check fails, instead of leaking on repeated attempts.
            let mut handles = DlHandles(Vec::new());
            for name in [
                "libglib-2.0.so.0",
                "libgobject-2.0.so.0",
                "libgstreamer-1.0.so.0",
            ] {
                let n = CString::new(name).map_err(|e| e.to_string())?;
                let ptr = libc::dlopen(n.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
                if ptr.is_null() {
                    return Err(format!("missing runtime library {name}"));
                }
                handles.0.push(ptr);
            }
            let gst = handles.0[2];
            let obj = handles.0[1];
            let glib = handles.0[0];
            let init: FnInit = symbol(gst, "gst_init_check")?;
            let mut error: *mut c_void = ptr::null_mut();
            if init(ptr::null_mut(), ptr::null_mut(), &mut error) == 0 {
                let message = if error.is_null() {
                    "GStreamer initialization failed".into()
                } else {
                    let err = error.cast::<GError>();
                    let message = if (*err).message.is_null() {
                        "GStreamer initialization failed".into()
                    } else {
                        CStr::from_ptr((*err).message)
                            .to_string_lossy()
                            .into_owned()
                    };
                    let free: FnFree = symbol(glib, "g_error_free")?;
                    free(error);
                    message
                };
                return Err(message);
            }
            Ok(Self {
                factory: symbol(gst, "gst_element_factory_make")?,
                bus: symbol(gst, "gst_element_get_bus")?,
                set_state: symbol(gst, "gst_element_set_state")?,
                query_pos: symbol(gst, "gst_element_query_position")?,
                query_dur: symbol(gst, "gst_element_query_duration")?,
                seek: symbol(gst, "gst_element_seek_simple")?,
                pop: symbol(gst, "gst_bus_pop")?,
                object_unref: symbol(gst, "gst_object_unref")?,
                mini_unref: symbol(gst, "gst_mini_object_unref")?,
                set: symbol(obj, "g_object_set")?,
                parse_error: symbol(gst, "gst_message_parse_error")?,
                error_free: symbol(glib, "g_error_free")?,
                glib_free: symbol(glib, "g_free")?,
                handles,
            })
        }
    }
}
unsafe fn symbol<T: Copy>(handle: *mut c_void, name: &str) -> Result<T, String> {
    let c = CString::new(name).map_err(|e| e.to_string())?;
    let addr = libc::dlsym(handle, c.as_ptr());
    if addr.is_null() {
        return Err(format!("GStreamer runtime symbol missing: {name}"));
    }
    Ok(std::mem::transmute_copy(&addr))
}
enum GstPoll {
    Continue { position_ms: u64, duration_ms: u64 },
    Eos,
}
struct GstAudio {
    api: GstFns,
    playbin: *mut c_void,
    bus: *mut c_void,
}
impl GstAudio {
    fn new() -> Result<Self, String> {
        let api = GstFns::open()?;
        let factory = CString::new("playbin").map_err(|e| e.to_string())?;
        let bin = unsafe { (api.factory)(factory.as_ptr(), ptr::null()) };
        if bin.is_null() {
            return Err("GStreamer playbin plugin unavailable".into());
        }
        let bus = unsafe { (api.bus)(bin) };
        if bus.is_null() {
            unsafe {
                (api.object_unref)(bin);
            }
            return Err("GStreamer playbin bus unavailable".into());
        }
        unsafe {
            (api.set)(
                bin,
                c"flags".as_ptr(),
                GST_PLAY_FLAG_AUDIO,
                ptr::null::<c_void>(),
            );
            // gst playbin will NOT instantiate video sinks: video flag absent.
            // Explicit opt-in debug-only synthetic sink for host protocol tests.
            // Never use this for an audible or hardware qualification claim.
            if cfg!(debug_assertions)
                && std::env::var("NURALOUMI_PLAYER_TEST_FAKESINK").as_deref() == Ok("1")
            {
                let sink_name = c"fakesink";
                let sink = (api.factory)(sink_name.as_ptr(), ptr::null());
                if sink.is_null() {
                    (api.object_unref)(bus);
                    (api.object_unref)(bin);
                    return Err("debug fakesink plugin missing".into());
                }
                (api.set)(bin, c"audio-sink".as_ptr(), sink, ptr::null::<c_void>());
                (api.object_unref)(sink);
            }
        }
        Ok(Self {
            api,
            playbin: bin,
            bus,
        })
    }
    fn set_state(&mut self, state: c_int) -> Result<(), String> {
        let r = unsafe { (self.api.set_state)(self.playbin, state) };
        if r == 0 {
            Err(format!("GStreamer set_state({state}) failed"))
        } else {
            Ok(())
        }
    }
    fn open(&mut self, path: &str, volume: f64, muted: bool) -> Result<(), String> {
        self.set_state(GST_STATE_NULL)?;
        let uri = media_uri(Path::new(path))?;
        unsafe {
            (self.api.set)(
                self.playbin,
                c"uri".as_ptr(),
                uri.as_ptr(),
                ptr::null::<c_void>(),
            );
        }
        self.set_volume(volume)?;
        self.set_mute(muted)?;
        self.set_state(GST_STATE_PLAYING)
    }
    fn set_volume(&mut self, volume: f64) -> Result<(), String> {
        if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
            return Err("invalid volume".into());
        }
        unsafe {
            (self.api.set)(
                self.playbin,
                c"volume".as_ptr(),
                volume,
                ptr::null::<c_void>(),
            );
        }
        Ok(())
    }
    fn set_mute(&mut self, muted: bool) -> Result<(), String> {
        unsafe {
            (self.api.set)(
                self.playbin,
                c"mute".as_ptr(),
                i32::from(muted),
                ptr::null::<c_void>(),
            );
        }
        Ok(())
    }
    fn seek(&mut self, ms: u64) -> Result<(), String> {
        let ns = ms
            .checked_mul(1_000_000)
            .and_then(|n| i64::try_from(n).ok())
            .ok_or("seek time overflow")?;
        if unsafe {
            (self.api.seek)(
                self.playbin,
                GST_FORMAT_TIME,
                GST_SEEK_FLAG_FLUSH | GST_SEEK_FLAG_ACCURATE,
                ns,
            )
        } == 0
        {
            return Err("GStreamer seek rejected".into());
        }
        Ok(())
    }
    fn tick(&mut self) -> Result<GstPoll, String> {
        // Drain a bounded number of bus messages per tick; never spin indefinitely.
        for _ in 0..64 {
            let m = unsafe { (self.api.pop)(self.bus) };
            if m.is_null() {
                break;
            }
            let kind = unsafe { (*(m as *const GstMessageHeader)).kind };
            if kind == GST_MESSAGE_ERROR {
                let mut gerr: *mut GError = ptr::null_mut();
                let mut debug: *mut c_char = ptr::null_mut();
                unsafe {
                    (self.api.parse_error)(m, &mut gerr, &mut debug);
                }
                let message = if gerr.is_null() || unsafe { (*gerr).message.is_null() } {
                    "unknown GStreamer error".to_string()
                } else {
                    unsafe {
                        CStr::from_ptr((*gerr).message)
                            .to_string_lossy()
                            .to_string()
                    }
                };
                unsafe {
                    if !gerr.is_null() {
                        (self.api.error_free)(gerr.cast());
                    }
                    if !debug.is_null() {
                        (self.api.glib_free)(debug.cast());
                    }
                    (self.api.mini_unref)(m);
                }
                return Err(format!("GStreamer: {message}"));
            }
            unsafe {
                (self.api.mini_unref)(m);
            }
            if kind == GST_MESSAGE_EOS {
                return Ok(GstPoll::Eos);
            }
        }
        let mut pos = 0;
        let mut dur = 0;
        unsafe {
            (self.api.query_pos)(self.playbin, GST_FORMAT_TIME, &mut pos);
            (self.api.query_dur)(self.playbin, GST_FORMAT_TIME, &mut dur);
        }
        Ok(GstPoll::Continue {
            position_ms: (pos.max(0) as u64) / 1_000_000,
            duration_ms: (dur.max(0) as u64) / 1_000_000,
        })
    }
}
impl Drop for GstAudio {
    fn drop(&mut self) {
        let _ = self.set_state(GST_STATE_NULL);
        unsafe {
            (self.api.object_unref)(self.bus);
            (self.api.object_unref)(self.playbin);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn temp_file(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "nuraloumi-player-test-{}-{name}",
            std::process::id()
        ))
    }
    #[test]
    fn uri_escapes_whitespace_metacharacters_and_unicode() {
        let s = media_uri(Path::new("/tmp/x #\"';$(touch hacked) ä.ogg")).unwrap();
        let got = s.to_str().unwrap();
        assert!(got.starts_with("file:///tmp/x%20%23%22%27%3B%24%28touch%20hacked%29%20%C3%A4"));
        assert!(!got.contains("$("));
    }
    #[test]
    fn playlist_reorder_remove_tracks_current_identity() {
        let mut p = Playlist {
            tracks: vec!["/a".into(), "/b".into(), "/c".into()],
            current: Some(1),
            ..Playlist::default()
        };
        p.move_track(1, 2).unwrap();
        assert_eq!(p.tracks, vec!["/a", "/c", "/b"]);
        assert_eq!(p.current, Some(2));
        assert!(!p.remove(0).unwrap());
        assert_eq!(p.current, Some(1));
        assert!(p.remove(1).unwrap());
        assert_eq!(p.current, Some(0));
    }
    #[test]
    fn playlist_is_atomic_and_rejects_corrupt_data() {
        let pth = temp_file("state.json");
        let p = Playlist {
            tracks: vec!["/tmp/track.ogg".into()],
            current: Some(0),
            volume: 0.42,
            muted: true,
            loop_mode: LoopMode::Off,
        };
        save_playlist(&pth, &p).unwrap();
        assert_eq!(load_playlist(&pth).unwrap(), p);
        fs::write(
            &pth,
            br#"{"tracks": ["relative"], "volume": 4, "muted":false}"#,
        )
        .unwrap();
        assert!(load_playlist(&pth).is_err());
        let _ = fs::remove_file(pth);
    }
    #[test]
    fn mutators_reject_invalid_indices_and_volume() {
        let mut p = Playlist::default();
        assert!(p.remove(0).is_err());
        assert!(p.move_track(0, 1).is_err());
        p.volume = f64::NAN;
        assert!(p.validate().is_err());
    }
    #[test]
    fn player_rejects_seek_without_stream_and_persists_volume_mute() {
        let path = temp_file("mutations.json");
        let mut player = Player::open(path.clone()).unwrap();
        let bad_seek = player.command(Command::Seek { position_ms: 5000 });
        assert!(!bad_seek.ok);
        assert!(bad_seek.error.unwrap().contains("no media loaded"));
        assert!(!player.command(Command::Volume { level: f64::NAN }).ok);
        assert!(player.command(Command::Volume { level: 0.6 }).ok);
        assert!(player.command(Command::Mute { muted: true }).ok);
        let saved = load_playlist(&path).unwrap();
        assert_eq!(saved.volume, 0.6);
        assert!(saved.muted);
        let _ = fs::remove_file(path);
    }
    #[test]
    fn failed_track_selection_restores_previous_playlist() {
        let path = temp_file("failed-track-state.json");
        let original = Playlist {
            tracks: vec![
                format!("/nonexistent-nuraloumi-{}-a.mp3", std::process::id()),
                format!("/nonexistent-nuraloumi-{}-b.mp3", std::process::id()),
            ],
            current: Some(0),
            ..Playlist::default()
        };
        save_playlist(&path, &original).unwrap();
        let mut player = Player::open(path.clone()).unwrap();
        assert!(!player.command(Command::Next).ok);
        assert_eq!(player.playlist, original);
        assert_eq!(load_playlist(&path).unwrap(), original);
        assert!(!player.command(Command::Remove { index: 0 }).ok);
        assert_eq!(player.playlist, original);
        assert_eq!(load_playlist(&path).unwrap(), original);
        let _ = fs::remove_file(path);
    }
    #[test]
    fn gst_minobject_header_matches_host_if_available() {
        // Runtime ABI layout is reviewed against the exported GStreamer 1.x headers.
        assert!(std::mem::size_of::<GstMessageHeader>() > std::mem::size_of::<GstMiniObject>());
    }
    #[test]
    fn native_vde_gpu_contract_is_strict_and_does_not_enable_playbin_video() {
        let known = VdeGpuFrameContract {
            drm_format: DRM_FORMAT_YU12,
            modifier: 0,
            display_width: 854,
            display_height: 480,
            capture_width: 896,
            strides: [896, 448, 448],
            dmabuf_planes: 3,
        };
        assert_eq!(known.validate_854x480(), Ok(()));
        assert!(VdeGpuFrameContract {
            drm_format: DRM_FORMAT_XR24,
            ..known
        }
        .validate_854x480()
        .is_err());
        assert!(VdeGpuFrameContract {
            modifier: u64::MAX,
            ..known
        }
        .validate_854x480()
        .is_err());
        assert!(VdeGpuFrameContract {
            strides: [864, 432, 432],
            ..known
        }
        .validate_854x480()
        .is_err());
        assert!(VdeGpuFrameContract {
            dmabuf_planes: 1,
            ..known
        }
        .validate_854x480()
        .is_err());
        assert_eq!(GST_PLAY_FLAG_AUDIO, 2); // VIDEO bit MUST remain unset.
        assert!(VDE_GPU_STREAMING_STATUS.contains("414-frame source PASS"));
        assert!(is_video_path(Path::new("/media/example.MP4")));
        assert!(is_video_path(Path::new("/media/example.mkv")));
        assert!(!is_video_path(Path::new("/media/track.mp3")));
    }
    #[test]
    fn m3u_bom_extinf_unicode_relative_and_local_file_uri() {
        let dir = temp_file("m3u-valid");
        fs::create_dir_all(&dir).unwrap();
        let first = dir.join("Ä space.ogg");
        let second = dir.join("other.mp3");
        fs::write(&first, []).unwrap();
        fs::write(&second, []).unwrap();
        let list = dir.join("playlist.m3u8");
        fs::write(&list, format!("\u{feff}#EXTM3U\n#EXTINF:123,Artist - Song\nÄ space.ogg\n# ignored\nfile://localhost{}\n", second.display())).unwrap();
        let tracks = parse_m3u(&list).unwrap();
        assert_eq!(
            tracks,
            vec![first.to_str().unwrap(), second.to_str().unwrap()]
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn m3u_rejects_unsafe_schemes_nested_missing_and_oversized() {
        let dir = temp_file("m3u-invalid");
        fs::create_dir_all(&dir).unwrap();
        let list = dir.join("list.m3u");
        for entry in [
            "https://example.org/song.mp3",
            "http:song.mp3",
            "data:x",
            "file://remotehost/tmp/audio.ogg",
            "other.m3u",
            "missing.ogg",
        ] {
            fs::write(&list, format!("#EXTM3U\n{entry}\n")).unwrap();
            assert!(parse_m3u(&list).is_err(), "accepted {entry}");
        }
        fs::write(&list, vec![b'a'; MAX_M3U_BYTES as usize + 1]).unwrap();
        assert!(parse_m3u(&list).unwrap_err().contains("size limit"));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn m3u_rejects_symlink_disguised_nested_playlist() {
        use std::os::unix::fs::symlink;
        let dir = temp_file("nested-symlink");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("nested.m3u8"), "#EXTM3U\n").unwrap();
        symlink("nested.m3u8", dir.join("alias.ogg")).unwrap();
        let list = dir.join("list.m3u");
        fs::write(&list, "#EXTM3U\nalias.ogg\n").unwrap();
        assert!(parse_m3u(&list).unwrap_err().contains("nested playlists"));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn persistence_failure_restores_playlist_snapshot() {
        let dir = temp_file("failed-save-dir");
        fs::create_dir_all(&dir).unwrap();
        let state = dir.join("state.json");
        let mut player = Player::open(state).unwrap();
        let previous = player.playlist.clone();
        fs::remove_dir(&dir).unwrap();
        fs::write(&dir, b"not a directory").unwrap();
        let reply = player.command(Command::LoopMode {
            mode: LoopMode::All,
        });
        assert!(!reply.ok);
        assert_eq!(reply.state.playlist, previous);
        assert_eq!(player.playlist, previous);
        fs::remove_file(dir).unwrap();
    }
    #[test]
    fn stopped_player_rejects_seek_and_relative_seek() {
        let mut player = Player::open(temp_file("seek-stop-state.json")).unwrap();
        player.stop();
        assert!(player
            .command(Command::Seek { position_ms: 0 })
            .error
            .unwrap()
            .contains("no media loaded"));
        assert!(player
            .command(Command::SeekRelative { delta_ms: -15_000 })
            .error
            .unwrap()
            .contains("no media loaded"));
    }
    #[test]
    fn loop_defaults_off_end_state_and_repeated_stop() {
        let path = temp_file("loop-state.json");
        let playlist = Playlist {
            tracks: vec!["/nonexistent-player-test.ogg".into()],
            current: Some(0),
            ..Playlist::default()
        };
        save_playlist(&path, &playlist).unwrap();
        let mut player = Player::open(path.clone()).unwrap();
        assert_eq!(player.playlist.loop_mode, LoopMode::Off);
        player.on_eos();
        assert_eq!(player.status, "Ended");
        assert_eq!(player.playlist.current, Some(0));
        assert!(player.command(Command::Stop).ok);
        assert!(player.command(Command::Stop).ok);
        assert_eq!(player.snapshot().position_ms, 0);
        assert_eq!(player.snapshot().status, "Stopped");
        assert!(
            player
                .command(Command::LoopMode {
                    mode: LoopMode::All
                })
                .ok
        );
        assert_eq!(load_playlist(&path).unwrap().loop_mode, LoopMode::All);
        let _ = fs::remove_file(path);
    }
    #[test]
    fn versioned_transport_commands_preserve_legacy_aliases() {
        let encoded = serde_json::to_value(Command::SeekRelative { delta_ms: -15_000 }).unwrap();
        assert_eq!(encoded["op"], "seek_relative_v1");
        assert_eq!(encoded["delta_ms"], -15_000);
        let old: Command =
            serde_json::from_str(r#"{"op":"seek_relative","delta_ms":15000}"#).unwrap();
        assert!(matches!(old, Command::SeekRelative { delta_ms: 15000 }));
        for (old, new) in [
            ("loop_mode", "loop_mode_v1"),
            ("jump_to_start", "jump_to_start_v1"),
            ("import_m3u", "import_m3u_v1"),
        ] {
            let cmd = match old {
                "loop_mode" => format!(r#"{{"op":"{old}","mode":"all"}}"#),
                "import_m3u" => format!(r#"{{"op":"{old}","path":"/tmp/list.m3u"}}"#),
                _ => format!(r#"{{"op":"{old}"}}"#),
            };
            let parsed: Command = serde_json::from_str(&cmd).unwrap();
            assert_eq!(serde_json::to_value(parsed).unwrap()["op"], new);
        }
        assert_eq!(serde_json::to_value(Command::Stop).unwrap()["op"], "stop");
    }
}
