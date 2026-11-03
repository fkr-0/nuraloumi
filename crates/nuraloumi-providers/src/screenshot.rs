//! Bounded PPM capture via grim, software preview, and lossless PNG save/copy.
//! Snapshot is read-only. Capture, save and copy require distinct explicit actions.
use crate::{ActionProvider, ActionResult, Provider, ProviderError};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub const MAX_CAPTURE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_PIXELS: usize = 2_097_152;
pub const MAX_PREVIEW_PIXELS: usize = 320 * 220;
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(4);
const COPY_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureSelection {
    All,
    Output(String),
    Geometry(String),
}
impl CaptureSelection {
    fn validate(&self) -> Result<(), ProviderError> {
        let valid = match self {
            Self::All => true,
            Self::Output(s) => {
                !s.is_empty()
                    && s.len() <= 64
                    && !s.starts_with('-')
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            }
            Self::Geometry(s) => {
                if let Some((xy, wh)) = s.split_once(' ') {
                    if let (Some((x, y)), Some((w, h))) = (xy.split_once(','), wh.split_once('x')) {
                        x.parse::<i32>().is_ok()
                            && y.parse::<i32>().is_ok()
                            && w.parse::<u32>().is_ok_and(|v| (1..=2048).contains(&v))
                            && h.parse::<u32>().is_ok_and(|v| (1..=2048).contains(&v))
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
        };
        if valid {
            Ok(())
        } else {
            Err(ProviderError::parse("invalid screenshot output selection"))
        }
    }
    fn args(&self) -> Vec<String> {
        let mut args = vec!["-t".into(), "ppm".into()];
        match self {
            Self::All => {}
            Self::Output(name) => args.extend(["-o".into(), name.clone()]),
            Self::Geometry(region) => args.extend(["-g".into(), region.clone()]),
        };
        args.push("-".into());
        args
    }
}

#[derive(Debug, Clone)]
pub enum ScreenshotSource {
    Grim,
    Fixture(Vec<u8>),
}
#[derive(Debug, Clone)]
pub struct ScreenshotImage {
    pub width: u32,
    pub height: u32,
    rgb: Vec<u8>,
}
#[derive(Debug, Clone)]
pub struct ScreenshotPreview {
    pub width: u32,
    pub height: u32,
    pub argb32: Vec<u8>,
}
#[derive(Debug, Clone)]
pub struct ScreenshotSnapshot {
    pub selection: CaptureSelection,
    pub image_width: Option<u32>,
    pub image_height: Option<u32>,
    pub preview: Option<ScreenshotPreview>,
}
#[derive(Debug, Clone)]
pub enum ScreenshotAction {
    Capture,
    Save { pictures_dir: PathBuf, stem: String },
    Copy,
}
#[derive(Debug, Clone)]
pub struct ScreenshotProvider {
    source: ScreenshotSource,
    selection: CaptureSelection,
    image: Option<ScreenshotImage>,
}
impl ScreenshotProvider {
    pub fn new(
        source: ScreenshotSource,
        selection: CaptureSelection,
    ) -> Result<Self, ProviderError> {
        selection.validate()?;
        if matches!(&source, ScreenshotSource::Fixture(bytes) if bytes.len() > MAX_CAPTURE_BYTES) {
            return Err(ProviderError::parse("oversized screenshot fixture"));
        }
        Ok(Self {
            source,
            selection,
            image: None,
        })
    }
}
impl Provider for ScreenshotProvider {
    type Snapshot = ScreenshotSnapshot;
    fn snapshot(&mut self) -> Result<Self::Snapshot, ProviderError> {
        Ok(ScreenshotSnapshot {
            selection: self.selection.clone(),
            image_width: self.image.as_ref().map(|i| i.width),
            image_height: self.image.as_ref().map(|i| i.height),
            preview: self
                .image
                .as_ref()
                .map(ScreenshotImage::preview)
                .transpose()?,
        })
    }
}
impl ActionProvider<ScreenshotAction> for ScreenshotProvider {
    fn execute(&mut self, action: ScreenshotAction) -> Result<ActionResult, ProviderError> {
        match action {
            ScreenshotAction::Capture => {
                let data = match &self.source {
                    ScreenshotSource::Fixture(bytes) => bytes.clone(),
                    ScreenshotSource::Grim => run_grim(&self.selection)?,
                };
                let parsed = ScreenshotImage::parse_ppm(&data)?;
                let message = format!("captured {}x{}", parsed.width, parsed.height);
                self.image = Some(parsed); // retain previous on failed capture
                Ok(ActionResult::executed(message))
            }
            ScreenshotAction::Save { pictures_dir, stem } => {
                let image = self
                    .image
                    .as_ref()
                    .ok_or_else(|| ProviderError::unavailable("capture before saving"))?;
                let path = image.save_png(&pictures_dir, &stem)?;
                Ok(ActionResult::executed(format!("saved {}", path.display())))
            }
            ScreenshotAction::Copy => {
                let image = self
                    .image
                    .as_ref()
                    .ok_or_else(|| ProviderError::unavailable("capture before copying"))?;
                if matches!(self.source, ScreenshotSource::Fixture(_)) {
                    return Ok(ActionResult::dry_run("fixture: clipboard not modified"));
                }
                copy_png(image)?;
                Ok(ActionResult::executed("copied image/png to clipboard"))
            }
        }
    }
}
fn parse_token<'a>(bytes: &'a [u8], at: &mut usize) -> Result<&'a [u8], ProviderError> {
    loop {
        match bytes.get(*at) {
            Some(b) if b.is_ascii_whitespace() => *at += 1,
            Some(b'#') => {
                while let Some(b) = bytes.get(*at) {
                    *at += 1;
                    if *b == b'\n' {
                        break;
                    }
                }
            }
            _ => break,
        }
    }
    let start = *at;
    while bytes
        .get(*at)
        .is_some_and(|b| !b.is_ascii_whitespace() && *b != b'#')
    {
        *at += 1;
    }
    if start == *at || *at - start > 16 {
        return Err(ProviderError::parse("invalid PPM header"));
    }
    Ok(&bytes[start..*at])
}
fn dimension(bytes: &[u8]) -> Result<u32, ProviderError> {
    let n = std::str::from_utf8(bytes)
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .ok_or_else(|| ProviderError::parse("invalid PPM dimension"))?;
    if (1..=2048).contains(&n) {
        Ok(n)
    } else {
        Err(ProviderError::parse("PPM dimension outside bound"))
    }
}
impl ScreenshotImage {
    pub fn parse_ppm(bytes: &[u8]) -> Result<Self, ProviderError> {
        if bytes.len() > MAX_CAPTURE_BYTES {
            return Err(ProviderError::parse("screenshot exceeds byte cap"));
        }
        let mut at = 0;
        if parse_token(bytes, &mut at)? != b"P6" {
            return Err(ProviderError::parse("expected P6 PPM"));
        }
        let width = dimension(parse_token(bytes, &mut at)?)?;
        let height = dimension(parse_token(bytes, &mut at)?)?;
        if parse_token(bytes, &mut at)? != b"255" {
            return Err(ProviderError::parse("expected RGB8 PPM"));
        }
        let sep = *bytes
            .get(at)
            .ok_or_else(|| ProviderError::parse("missing raster separator"))?;
        if !sep.is_ascii_whitespace() {
            return Err(ProviderError::parse("invalid raster separator"));
        }
        at += 1;
        if sep == b'\r' && bytes.get(at) == Some(&b'\n') {
            at += 1;
        }
        let pixels = (width as usize)
            .checked_mul(height as usize)
            .ok_or_else(|| ProviderError::parse("pixel overflow"))?;
        if pixels > MAX_PIXELS {
            return Err(ProviderError::parse("pixel cap exceeded"));
        }
        if bytes.len().checked_sub(at) != pixels.checked_mul(3) {
            return Err(ProviderError::parse("PPM raster length mismatch"));
        }
        Ok(Self {
            width,
            height,
            rgb: bytes[at..].to_vec(),
        })
    }
    pub fn preview(&self) -> Result<ScreenshotPreview, ProviderError> {
        let mut width = self.width.min(320);
        let mut height =
            (u64::from(self.height) * u64::from(width)).div_ceil(u64::from(self.width)) as u32;
        if height > 220 {
            height = 220;
            width =
                (u64::from(self.width) * u64::from(height)).div_ceil(u64::from(self.height)) as u32;
        }
        let pixels = width as usize * height as usize;
        if pixels == 0 || pixels > MAX_PREVIEW_PIXELS {
            return Err(ProviderError::parse("preview cap exceeded"));
        }
        let mut argb32 = vec![0; pixels * 4];
        for y in 0..height as usize {
            for x in 0..width as usize {
                let sx = x * self.width as usize / width as usize;
                let sy = y * self.height as usize / height as usize;
                let src = (sy * self.width as usize + sx) * 3;
                let dst = (y * width as usize + x) * 4;
                argb32[dst..dst + 4].copy_from_slice(
                    &u32::from_be_bytes([255, self.rgb[src], self.rgb[src + 1], self.rgb[src + 2]])
                        .to_ne_bytes(),
                );
            }
        }
        Ok(ScreenshotPreview {
            width,
            height,
            argb32,
        })
    }
    pub fn write_png(&self, out: &mut impl Write) -> io::Result<()> {
        out.write_all(b"\x89PNG\r\n\x1a\n")?;
        let mut ihdr = Vec::from(self.width.to_be_bytes());
        ihdr.extend_from_slice(&self.height.to_be_bytes());
        ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // RGB8, no interlace
        png_chunk(out, b"IHDR", &ihdr)?;
        let mut s1 = 1u32;
        let mut s2 = 0u32;
        for (index, row) in self.rgb.chunks_exact(self.width as usize * 3).enumerate() {
            let mut scanline = Vec::with_capacity(row.len() + 1);
            scanline.push(0); // filter None
            scanline.extend_from_slice(row);
            for &v in &scanline {
                s1 = (s1 + u32::from(v)) % 65521;
                s2 = (s2 + s1) % 65521;
            }
            let mut block = Vec::with_capacity(scanline.len() + 11);
            if index == 0 {
                block.extend_from_slice(&[0x78, 0x01]);
            }
            block.push(u8::from(index + 1 == self.height as usize)); // last DEFLATE stored block
            let len = scanline.len() as u16; // max 6145
            block.extend_from_slice(&len.to_le_bytes());
            block.extend_from_slice(&(!len).to_le_bytes());
            block.extend_from_slice(&scanline);
            if index + 1 == self.height as usize {
                block.extend_from_slice(&((s2 << 16) | s1).to_be_bytes());
            }
            png_chunk(out, b"IDAT", &block)?;
        }
        png_chunk(out, b"IEND", &[])
    }
    pub fn save_png(&self, dir: &Path, stem: &str) -> Result<PathBuf, ProviderError> {
        if stem.is_empty()
            || stem.len() > 48
            || !stem
                .bytes()
                .next()
                .is_some_and(|b| b.is_ascii_alphanumeric())
            || !stem
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(ProviderError::parse("unsafe screenshot basename"));
        }
        fs::create_dir_all(dir)
            .map_err(|e| ProviderError::from_io("create Pictures directory", &e))?;
        let meta = fs::symlink_metadata(dir)
            .map_err(|e| ProviderError::from_io("inspect Pictures directory", &e))?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err(ProviderError::permission(
                "Pictures directory must not be a symlink",
            ));
        }
        let path = dir.join(format!("{stem}.png"));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&path)
            .map_err(|e| ProviderError::from_io("create new screenshot", &e))?;
        if let Err(e) = self.write_png(&mut file).and_then(|_| file.flush()) {
            let _ = fs::remove_file(&path);
            return Err(ProviderError::from_io("write screenshot", &e));
        }
        Ok(path)
    }
}
fn crc(mut c: u32, bytes: &[u8]) -> u32 {
    for &v in bytes {
        c ^= u32::from(v);
        for _ in 0..8 {
            c = (c >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(c & 1));
        }
    }
    c
}
fn png_chunk(out: &mut impl Write, kind: &[u8; 4], bytes: &[u8]) -> io::Result<()> {
    out.write_all(&(bytes.len() as u32).to_be_bytes())?;
    out.write_all(kind)?;
    out.write_all(bytes)?;
    out.write_all(&(crc(crc(!0, kind), bytes) ^ !0).to_be_bytes())
}
// Only Linux is targeted (labwc/wlroots on the SL101). The standard library
// does not expose nonblocking ChildStd{in,out,err}, so use the minimal Linux
// fcntl/kill ABI here rather than creating readers/writers that cannot be
// interrupted by an operation deadline. F_GETFL=3, F_SETFL=4,
// O_NONBLOCK=0o4000 and SIGKILL=9 on Linux, including ARMv7.
unsafe extern "C" {
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
}

fn nonblocking(fd: &impl AsRawFd) -> io::Result<()> {
    // SAFETY: the descriptor is an owned, live pipe; fcntl only changes its
    // file-status flags. Negative results are converted to io::Error.
    let flags = unsafe { fcntl(fd.as_raw_fd(), 3) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { fcntl(fd.as_raw_fd(), 4, flags | 0o4000) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// A private process group permits deadline cleanup of descendants that
/// inherit a pipe even after the direct child has exited. Never reap the
/// group leader until all I/O has finished: while it is alive or unreaped,
/// its PID/PGID cannot be reused by an unrelated process group.
struct IsolatedChild {
    child: Child,
    armed: bool,
}
impl IsolatedChild {
    fn spawn(mut command: Command, context: &'static str) -> Result<Self, ProviderError> {
        command.process_group(0);
        Ok(Self {
            child: command
                .spawn()
                .map_err(|e| ProviderError::from_io(context, &e))?,
            armed: true,
        })
    }

    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        match self.child.try_wait()? {
            Some(status) => {
                self.armed = false; // reaped; it must never be signalled again
                Ok(Some(status))
            }
            None => Ok(None),
        }
    }
}
impl Drop for IsolatedChild {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        // SAFETY: process_group(0) created a new group whose leader is this
        // still-unreaped child; a negative PID therefore cannot target the
        // caller or an unrelated process group. Its children share that group
        // unless they explicitly daemonize and leave it.
        if let Ok(pgid) = i32::try_from(self.child.id()) {
            let _ = unsafe { kill(-pgid, 9) };
        }
        let _ = self.child.kill(); // fallback if group signalling failed
                                   // Never perform an unbounded wait as part of timeout cleanup.
        let limit = Instant::now() + Duration::from_millis(100);
        while Instant::now() < limit {
            match self.child.try_wait() {
                Ok(Some(_)) | Err(_) => break,
                Ok(None) => thread::sleep(Duration::from_millis(2)),
            }
        }
    }
}

/// One bounded nonblocking drain. A descendant holding a pipe open produces
/// WouldBlock, not an indefinite read_to_end or join. The caller checks its
/// end-to-end deadline independently of direct-child exit.
fn drain(
    pipe: &mut impl Read,
    into: &mut Vec<u8>,
    cap: usize,
    deadline: Instant,
) -> io::Result<bool> {
    let mut chunk = [0_u8; 16 * 1024];
    for _ in 0..32 {
        if Instant::now() >= deadline {
            return Err(io::Error::from(io::ErrorKind::TimedOut));
        }
        match pipe.read(&mut chunk) {
            Ok(0) => return Ok(true),
            Ok(n) => {
                if n > cap.saturating_sub(into.len()) {
                    return Err(io::Error::from(io::ErrorKind::InvalidData));
                }
                into.extend_from_slice(&chunk[..n]);
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(false),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(false)
}

fn capture_command(command: Command, timeout: Duration) -> Result<Vec<u8>, ProviderError> {
    let deadline = Instant::now() + timeout;
    let mut child = IsolatedChild::spawn(command, "start grim")?;
    let mut stdout = child
        .child
        .stdout
        .take()
        .ok_or_else(|| ProviderError::backend("grim stdout unavailable"))?;
    let mut stderr = child
        .child
        .stderr
        .take()
        .ok_or_else(|| ProviderError::backend("grim stderr unavailable"))?;
    nonblocking(&stdout).map_err(|e| ProviderError::from_io("grim stdout flags", &e))?;
    nonblocking(&stderr).map_err(|e| ProviderError::from_io("grim stderr flags", &e))?;
    let mut image = Vec::new();
    let mut errors = Vec::new();
    let (mut out_eof, mut err_eof) = (false, false);
    loop {
        if Instant::now() >= deadline {
            return Err(ProviderError::timeout("grim screenshot timed out"));
        }
        if !out_eof {
            out_eof =
                drain(&mut stdout, &mut image, MAX_CAPTURE_BYTES, deadline).map_err(|e| match e
                    .kind()
                {
                    io::ErrorKind::TimedOut => ProviderError::timeout("grim screenshot timed out"),
                    io::ErrorKind::InvalidData => ProviderError::backend("grim byte cap exceeded"),
                    _ => ProviderError::from_io("read grim image", &e),
                })?;
        }
        if !err_eof {
            err_eof =
                drain(&mut stderr, &mut errors, 4096, deadline).map_err(|e| match e.kind() {
                    io::ErrorKind::TimedOut => ProviderError::timeout("grim screenshot timed out"),
                    io::ErrorKind::InvalidData => {
                        ProviderError::backend("grim stderr cap exceeded")
                    }
                    _ => ProviderError::from_io("read grim stderr", &e),
                })?;
        }
        // Do not call try_wait before both pipes reach EOF. A child that
        // exited but left pipes with a grandchild must remain unreaped, so
        // its process group remains safely addressable on timeout.
        if out_eof && err_eof {
            if let Some(status) = child
                .try_wait()
                .map_err(|e| ProviderError::from_io("wait for grim", &e))?
            {
                if !status.success() {
                    return Err(ProviderError::backend(format!(
                        "grim failed: {}",
                        String::from_utf8_lossy(&errors)
                    )));
                }
                return Ok(image);
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn run_grim(selection: &CaptureSelection) -> Result<Vec<u8>, ProviderError> {
    selection.validate()?;
    let mut command = Command::new("grim");
    command
        .args(selection.args())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    capture_command(command, CAPTURE_TIMEOUT)
}

// A memory-backed bounded writer avoids a background PNG producer thread.
// Every chunk checks the same deadline that later applies to wl-copy I/O.
struct TimedPng {
    bytes: Vec<u8>,
    deadline: Instant,
}
impl Write for TimedPng {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if Instant::now() >= self.deadline {
            return Err(io::Error::from(io::ErrorKind::TimedOut));
        }
        if data.len() > MAX_CAPTURE_BYTES.saturating_sub(self.bytes.len()) {
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }
        self.bytes.extend_from_slice(data);
        Ok(data.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn write_command(command: Command, bytes: &[u8], deadline: Instant) -> Result<(), ProviderError> {
    if Instant::now() >= deadline {
        return Err(ProviderError::timeout("wl-copy timed out"));
    }
    let mut child = IsolatedChild::spawn(command, "start wl-copy")?;
    let mut stdin = child
        .child
        .stdin
        .take()
        .ok_or_else(|| ProviderError::backend("clipboard pipe unavailable"))?;
    nonblocking(&stdin).map_err(|e| ProviderError::from_io("clipboard pipe flags", &e))?;
    let mut sent = 0;
    loop {
        if Instant::now() >= deadline {
            return Err(ProviderError::timeout("wl-copy timed out"));
        }
        if sent < bytes.len() {
            match stdin.write(&bytes[sent..]) {
                Ok(0) => return Err(ProviderError::backend("clipboard pipe made no progress")),
                Ok(n) => sent += n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(ProviderError::from_io("send PNG to clipboard", &e)),
            }
        }
        if sent == bytes.len() {
            drop(stdin); // send EOF before waiting for the consumer
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    loop {
        if Instant::now() >= deadline {
            return Err(ProviderError::timeout("wl-copy timed out"));
        }
        match child
            .try_wait()
            .map_err(|e| ProviderError::from_io("wait for wl-copy", &e))?
        {
            Some(status) if status.success() => return Ok(()),
            Some(_) => return Err(ProviderError::backend("wl-copy failed")),
            None => thread::sleep(Duration::from_millis(5)),
        }
    }
}

fn copy_png(image: &ScreenshotImage) -> Result<(), ProviderError> {
    let deadline = Instant::now() + COPY_TIMEOUT;
    let mut png = TimedPng {
        bytes: Vec::new(),
        deadline,
    };
    image.write_png(&mut png).map_err(|e| match e.kind() {
        io::ErrorKind::TimedOut => ProviderError::timeout("PNG encoding timed out"),
        _ => ProviderError::from_io("encode PNG for clipboard", &e),
    })?;
    let mut command = Command::new("wl-copy");
    command
        .args(["--type", "image/png"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    write_command(command, &png.bytes, deadline)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let mut b = b"P6\n#color fixture\n2 2\n255\n".to_vec();
        b.extend_from_slice(&[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255]);
        b
    }
    #[test]
    fn fixture_is_inert_and_preview_is_native_argb() {
        let mut p =
            ScreenshotProvider::new(ScreenshotSource::Fixture(fixture()), CaptureSelection::All)
                .unwrap();
        assert!(p.snapshot().unwrap().preview.is_none());
        assert!(p.execute(ScreenshotAction::Copy).is_err());
        p.execute(ScreenshotAction::Capture).unwrap();
        let s = p.snapshot().unwrap();
        assert_eq!((s.image_width, s.image_height), (Some(2), Some(2)));
        assert_eq!(
            &s.preview.unwrap().argb32[..4],
            &u32::from_be_bytes([255, 255, 0, 0]).to_ne_bytes()
        );
        assert!(p.execute(ScreenshotAction::Copy).unwrap().dry_run);
    }
    #[test]
    fn reject_bad_ppm_selection_and_oversize() {
        for b in [
            b"P3\n1 1\n255\nx".as_slice(),
            b"P6\n0 1\n255\n".as_slice(),
            b"P6\n2 2\n255\nxxx".as_slice(),
        ] {
            assert!(ScreenshotImage::parse_ppm(b).is_err());
        }
        assert!(ScreenshotProvider::new(
            ScreenshotSource::Fixture(fixture()),
            CaptureSelection::Output("-bad".into())
        )
        .is_err());
        assert!(ScreenshotProvider::new(
            ScreenshotSource::Fixture(fixture()),
            CaptureSelection::Geometry("0,0 99999x2".into())
        )
        .is_err());
        assert!(ScreenshotProvider::new(
            ScreenshotSource::Fixture(vec![0; MAX_CAPTURE_BYTES + 1]),
            CaptureSelection::All
        )
        .is_err());
    }
    #[test]
    fn png_has_valid_chunks_and_is_not_overwritten() {
        let image = ScreenshotImage::parse_ppm(&fixture()).unwrap();
        let mut png = Vec::new();
        image.write_png(&mut png).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let mut off = 8;
        while off < png.len() {
            let len = u32::from_be_bytes(png[off..off + 4].try_into().unwrap()) as usize;
            let end = off + 8 + len;
            assert_eq!(
                u32::from_be_bytes(png[end..end + 4].try_into().unwrap()),
                crc(crc(!0, &png[off + 4..off + 8]), &png[off + 8..end]) ^ !0
            );
            off = end + 4;
        }
        assert_eq!(&png[png.len() - 8..png.len() - 4], b"IEND");
        let dir = std::env::temp_dir().join(format!("nur05-{}-{}", std::process::id(), 42));
        let _ = fs::remove_dir_all(&dir);
        assert!(image.save_png(&dir, "../escape").is_err());
        let path = image.save_png(&dir, "shot_1").unwrap();
        assert!(path.ends_with("shot_1.png"));
        assert!(image.save_png(&dir, "shot_1").is_err());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn capture_argv_is_exact_and_rejects_option_injection() {
        let all = CaptureSelection::All;
        assert_eq!(all.args(), ["-t", "ppm", "-"]);
        let named = CaptureSelection::Output("eDP-1".into());
        named.validate().unwrap();
        assert_eq!(named.args(), ["-t", "ppm", "-o", "eDP-1", "-"]);
        let rectangle = CaptureSelection::Geometry("0,42 320x200".into());
        rectangle.validate().unwrap();
        assert_eq!(rectangle.args(), ["-t", "ppm", "-g", "0,42 320x200", "-"]);
        for invalid in [
            CaptureSelection::Output("-bad".into()),
            CaptureSelection::Output("A\n--danger".into()),
            CaptureSelection::Geometry("0,0 3x4;touch /tmp/unsafe".into()),
            CaptureSelection::Geometry("0,0 -4x1".into()),
        ] {
            assert!(invalid.validate().is_err());
        }
    }

    #[test]
    fn parser_does_not_consume_binary_whitespace_pixels() {
        // The first RGB byte (line feed) is raster data, not another delimiter.
        let input = b"P6\n1 1\n255\n\n\t\x20";
        let img = ScreenshotImage::parse_ppm(input).unwrap();
        assert_eq!((img.width, img.height), (1, 1));
        assert_eq!(img.rgb, [10, 9, 32]);
        assert!(ScreenshotImage::parse_ppm(b"P6\n2048 2048\n255\n").is_err());
    }

    #[test]
    fn preview_memory_is_capped() {
        let image = ScreenshotImage {
            width: 2048,
            height: 1024,
            rgb: vec![0; 2048 * 1024 * 3],
        };
        assert!(image.preview().unwrap().argb32.len() <= MAX_PREVIEW_PIXELS * 4);
    }
    // These child processes exit while descendants still hold their inherited
    // pipes. Without end-to-end I/O deadlines, the old join() calls hung
    // forever even though try_wait() had already observed the direct exit.
    #[test]
    fn descendant_retained_stdout_and_stderr_have_bounded_capture() {
        for script in [
            "sleep 30 & printf 'direct child exited\n'",
            "sleep 30 1>&2 & printf 'direct child exited\n' >&2",
        ] {
            let mut command = Command::new("sh");
            command
                .args(["-c", script])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let start = Instant::now();
            let result = capture_command(command, Duration::from_millis(180));
            assert_eq!(
                result.unwrap_err().category.as_str(),
                "timeout",
                "descendant-held pipe must reach the deadline: {script}"
            );
            assert!(
                start.elapsed() < Duration::from_secs(2),
                "capture exceeded bounded cleanup: {script}"
            );
        }
    }

    #[test]
    fn descendant_retained_stdin_has_bounded_clipboard_write() {
        // The grandchild keeps stdin open but never drains it. A 1 MiB
        // payload exceeds a Linux pipe's capacity even if the parent exited.
        let mut command = Command::new("sh");
        command
            .args(["-c", "sleep 30 <&0 & exit 0"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let start = Instant::now();
        let result = write_command(
            command,
            &vec![7_u8; 1024 * 1024],
            Instant::now() + Duration::from_millis(180),
        );
        assert_eq!(result.unwrap_err().category.as_str(), "timeout");
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn drained_child_pipes_complete_without_spurious_deadline() {
        let mut producer = Command::new("sh");
        producer
            .args(["-c", "printf done"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        assert_eq!(
            capture_command(producer, Duration::from_secs(1)).unwrap(),
            b"done"
        );
        let mut consumer = Command::new("sh");
        consumer
            .args(["-c", "cat >/dev/null"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        write_command(
            consumer,
            &vec![42_u8; 1024 * 1024],
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap();
    }
}
