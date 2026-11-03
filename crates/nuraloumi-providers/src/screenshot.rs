//! Bounded PPM capture via grim, software preview, lossless streaming PNG save/copy.
//! Snapshot is read-only. Capture, save and copy require distinct explicit actions.
use crate::{ActionProvider, ActionResult, Provider, ProviderError};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
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
fn limited(reader: impl Read, limit: usize, exceed: Arc<AtomicBool>) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        exceed.store(true, Ordering::Relaxed);
    }
    Ok(bytes)
}
fn run_grim(selection: &CaptureSelection) -> Result<Vec<u8>, ProviderError> {
    selection.validate()?;
    let mut child = Command::new("grim")
        .args(selection.args())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ProviderError::from_io("start grim", &e))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ProviderError::backend("grim stdout unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ProviderError::backend("grim stderr unavailable"))?;
    let exceeded = Arc::new(AtomicBool::new(false));
    let out_exceed = Arc::clone(&exceeded);
    let output = thread::spawn(move || limited(stdout, MAX_CAPTURE_BYTES, out_exceed));
    let errors = thread::spawn(move || limited(stderr, 4096, Arc::new(AtomicBool::new(false))));
    let start = Instant::now();
    let result = loop {
        if exceeded.load(Ordering::Relaxed) || start.elapsed() >= CAPTURE_TIMEOUT {
            let oversized = exceeded.load(Ordering::Relaxed);
            let _ = child.kill();
            let _ = child.wait();
            let _ = output.join();
            let _ = errors.join();
            return Err(if oversized {
                ProviderError::backend("grim byte cap exceeded")
            } else {
                ProviderError::timeout("grim screenshot timed out")
            });
        }
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = output.join();
                let _ = errors.join();
                return Err(ProviderError::from_io("wait for grim", &e));
            }
        }
    };
    let image = output
        .join()
        .map_err(|_| ProviderError::backend("grim reader panic"))?
        .map_err(|e| ProviderError::from_io("read grim image", &e))?;
    let error = errors
        .join()
        .map_err(|_| ProviderError::backend("grim stderr reader panic"))?
        .map_err(|e| ProviderError::from_io("read grim stderr", &e))?;
    if image.len() > MAX_CAPTURE_BYTES {
        return Err(ProviderError::backend("grim image too large"));
    }
    if !result.success() {
        return Err(ProviderError::backend(format!(
            "grim failed: {}",
            String::from_utf8_lossy(&error)
        )));
    }
    Ok(image)
}
fn copy_png(image: &ScreenshotImage) -> Result<(), ProviderError> {
    let mut child = Command::new("wl-copy")
        .args(["--type", "image/png"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| ProviderError::from_io("start wl-copy", &e))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| ProviderError::backend("clipboard pipe unavailable"))?;
    let frame = image.clone();
    let writer = thread::spawn(move || {
        let mut pipe = io::BufWriter::new(stdin);
        frame.write_png(&mut pipe)?;
        // Report any final buffered write failure instead of losing
        // the PNG IEND trailer silently during BufWriter::drop.
        pipe.flush()
    });
    let start = Instant::now();
    let status = loop {
        if start.elapsed() >= COPY_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            let _ = writer.join();
            return Err(ProviderError::timeout("wl-copy timed out"));
        }
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = writer.join();
                return Err(ProviderError::from_io("wait for wl-copy", &e));
            }
        }
    };
    writer
        .join()
        .map_err(|_| ProviderError::backend("clipboard writer panic"))?
        .map_err(|e| ProviderError::from_io("send PNG to clipboard", &e))?;
    if !status.success() {
        return Err(ProviderError::backend("wl-copy failed"));
    }
    Ok(())
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
}
