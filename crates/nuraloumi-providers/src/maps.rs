//! Offline-first, bounded XYZ (slippy-map) PNG tile store.
//!
//! No networking occurs during lookup or viewport calculation. An explicit
//! TileFetcher::fetch_one call is required for one network tile request.
//! Integrators should call that method off the Wayland/render thread.
//! Cairo/wl_shm consumers can rasterize the returned PNG bytes independently.
//!
//! Cache layout: <root>/<zoom>/<x>/<y>.png. The configured root should be a
//! dedicated SD-card directory; a cache instance has one tile source.

use crate::command::{CommandLimits, CommandRunner, CommandSpec};
use crate::error::{ProviderError, ProviderErrorCategory};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

pub const TILE_SIZE: u32 = 256;
pub const MAX_ZOOM: u8 = 18;
pub const MAX_VISIBLE_TILES: usize = 64;
const MAX_VIEWPORT_EDGE: u32 = 4096;
const MAX_SCAN_ENTRIES: usize = 4096;
const MAX_CACHE_TILES: usize = 1024;
const MAX_CACHE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_TILE_BYTES: usize = 1024 * 1024;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn invalid(message: &str) -> ProviderError {
    ProviderError::parse(message)
}

fn permission(message: &str) -> ProviderError {
    ProviderError::new(ProviderErrorCategory::Permission, message)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileCoord {
    zoom: u8,
    x: u32,
    y: u32,
}

impl TileCoord {
    pub fn new(zoom: u8, x: u32, y: u32) -> Result<Self, ProviderError> {
        let extent = tile_extent(zoom)?;
        if x >= extent || y >= extent {
            return Err(invalid("tile x/y out of bounds for zoom"));
        }
        Ok(Self { zoom, x, y })
    }

    pub fn zoom(self) -> u8 {
        self.zoom
    }
    pub fn x(self) -> u32 {
        self.x
    }
    pub fn y(self) -> u32 {
        self.y
    }
}

fn tile_extent(zoom: u8) -> Result<u32, ProviderError> {
    if zoom > MAX_ZOOM {
        return Err(invalid("map zoom exceeds configured maximum"));
    }
    Ok(1_u32 << zoom)
}

fn world_size(zoom: u8) -> Result<f64, ProviderError> {
    Ok(f64::from(tile_extent(zoom)?) * f64::from(TILE_SIZE))
}

/// Tile positions are screen coordinates for a future Cairo paint pass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TilePlacement {
    pub coord: TileCoord,
    pub screen_x: f64,
    pub screen_y: f64,
}

/// Web-Mercator map view, in continuous world pixels, independent of renderer.
/// Positive drag deltas move the map in the same direction as the finger.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapView {
    pub zoom: u8,
    center_x: f64,
    center_y: f64,
}

impl MapView {
    pub fn from_lon_lat(lon: f64, lat: f64, zoom: u8) -> Result<Self, ProviderError> {
        let world = world_size(zoom)?;
        if !lon.is_finite() || !lat.is_finite() {
            return Err(invalid("non-finite map center"));
        }
        let wrapped_lon = (lon + 180.0).rem_euclid(360.0);
        let clamped_lat = lat.clamp(-85.05112878, 85.05112878).to_radians();
        let mercator_y = (1.0 - (clamped_lat.tan().asinh() / std::f64::consts::PI)) / 2.0;
        Ok(Self {
            zoom,
            center_x: wrapped_lon / 360.0 * world,
            center_y: (mercator_y * world).clamp(0.0, world),
        })
    }

    pub fn center_pixels(&self) -> (f64, f64) {
        (self.center_x, self.center_y)
    }

    pub fn drag_pixels(&mut self, finger_dx: f64, finger_dy: f64) -> Result<(), ProviderError> {
        if !finger_dx.is_finite() || !finger_dy.is_finite() {
            return Err(invalid("non-finite pan delta"));
        }
        let world = world_size(self.zoom)?;
        self.center_x = (self.center_x - finger_dx).rem_euclid(world);
        self.center_y = (self.center_y - finger_dy).clamp(0.0, world);
        Ok(())
    }

    /// Change zoom while preserving the world position below a touch/keyboard
    /// anchor, expressed in pixels from the viewport center.
    pub fn zoom_at(
        &mut self,
        next_zoom: u8,
        anchor_dx: f64,
        anchor_dy: f64,
    ) -> Result<(), ProviderError> {
        let old_world = world_size(self.zoom)?;
        let new_world = world_size(next_zoom)?;
        if !anchor_dx.is_finite() || !anchor_dy.is_finite() {
            return Err(invalid("non-finite zoom anchor"));
        }
        let factor = new_world / old_world;
        self.center_x = ((self.center_x + anchor_dx) * factor - anchor_dx).rem_euclid(new_world);
        self.center_y = ((self.center_y + anchor_dy) * factor - anchor_dy).clamp(0.0, new_world);
        self.zoom = next_zoom;
        Ok(())
    }

    /// Enumerates only visible tiles; wraps longitude, clips latitude and
    /// refuses viewports that would exceed the small-tablet paint budget.
    pub fn visible_tiles(
        &self,
        width: u32,
        height: u32,
    ) -> Result<Vec<TilePlacement>, ProviderError> {
        if width == 0 || height == 0 || width > MAX_VIEWPORT_EDGE || height > MAX_VIEWPORT_EDGE {
            return Err(invalid("unsupported map viewport size"));
        }
        let edge = f64::from(TILE_SIZE);
        let extent = i64::from(tile_extent(self.zoom)?);
        let left = self.center_x - f64::from(width) / 2.0;
        let top = self.center_y - f64::from(height) / 2.0;
        let first_x = (left / edge).floor() as i64;
        let last_x = ((left + f64::from(width)) / edge).ceil() as i64 - 1;
        let first_y = (top / edge).floor() as i64;
        let last_y = ((top + f64::from(height)) / edge).ceil() as i64 - 1;
        let mut out = Vec::new();
        for y in first_y.max(0)..=last_y.min(extent - 1) {
            for x in first_x..=last_x {
                if out.len() >= MAX_VISIBLE_TILES {
                    return Err(invalid("visible tile budget exceeded"));
                }
                out.push(TilePlacement {
                    coord: TileCoord::new(self.zoom, x.rem_euclid(extent) as u32, y as u32)?,
                    screen_x: (x as f64) * edge - left,
                    screen_y: (y as f64) * edge - top,
                });
            }
        }
        Ok(out)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TileCacheLimits {
    pub max_tiles: usize,
    pub max_bytes: u64,
    pub max_tile_bytes: usize,
}

impl Default for TileCacheLimits {
    fn default() -> Self {
        Self {
            max_tiles: 256,
            max_bytes: 32 * 1024 * 1024,
            max_tile_bytes: 512 * 1024,
        }
    }
}

impl TileCacheLimits {
    fn validate(self) -> Result<(), ProviderError> {
        if self.max_tiles == 0
            || self.max_tiles > MAX_CACHE_TILES
            || self.max_bytes == 0
            || self.max_bytes > MAX_CACHE_BYTES
            || self.max_tile_bytes == 0
            || self.max_tile_bytes > MAX_TILE_BYTES
            || self.max_bytes < self.max_tile_bytes as u64
        {
            return Err(invalid("invalid tile-cache limits"));
        }
        Ok(())
    }
}

#[derive(Debug)]
struct CacheEntry {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
}

/// Root-scoped cache with bounded disk usage, no startup fetch and no symlink
/// traversal inside its tree. Do not share a cache root with other writers.
#[derive(Debug, Clone)]
pub struct TileCache {
    root: PathBuf,
    limits: TileCacheLimits,
}

impl TileCache {
    pub fn new(root: impl Into<PathBuf>, limits: TileCacheLimits) -> Result<Self, ProviderError> {
        limits.validate()?;
        let root = root.into();
        if root.as_os_str().is_empty() {
            return Err(invalid("empty tile-cache root"));
        }
        Ok(Self { root, limits })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn tile_path(&self, tile: TileCoord) -> PathBuf {
        self.root
            .join(tile.zoom.to_string())
            .join(tile.x.to_string())
            .join(format!("{}.png", tile.y))
    }

    fn ensure_safe_parent(&self, tile: TileCoord) -> Result<(), ProviderError> {
        fs::create_dir_all(&self.root)
            .map_err(|e| ProviderError::from_io("creating tile cache", &e))?;
        let z = self.root.join(tile.zoom.to_string());
        let x = z.join(tile.x.to_string());
        for dir in [&self.root, &z, &x] {
            match fs::symlink_metadata(dir) {
                Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
                    return Err(permission("tile-cache path is not a plain directory"))
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => fs::create_dir(dir)
                    .map_err(|err| ProviderError::from_io("creating tile directory", &err))?,
                Err(e) => return Err(ProviderError::from_io("inspecting tile directory", &e)),
            }
        }
        Ok(())
    }

    /// Offline-only lookup. Never creates directories or contacts any service.
    pub fn lookup(&self, tile: TileCoord) -> Result<Option<Vec<u8>>, ProviderError> {
        let path = self.tile_path(tile);
        let meta = match fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(ProviderError::from_io("reading tile metadata", &e)),
        };
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err(permission("tile path is not a plain file"));
        }
        self.check_existing_parents(tile)?;
        if meta.len() > self.limits.max_tile_bytes as u64 {
            return Err(invalid("cached tile exceeds byte limit"));
        }
        let mut file = File::open(path).map_err(|e| ProviderError::from_io("opening tile", &e))?;
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(self.limits.max_tile_bytes as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| ProviderError::from_io("reading tile", &e))?;
        self.validate_tile(&bytes)?;
        Ok(Some(bytes))
    }

    fn check_existing_parents(&self, tile: TileCoord) -> Result<(), ProviderError> {
        for dir in [
            &self.root,
            &self.root.join(tile.zoom.to_string()),
            &self
                .root
                .join(tile.zoom.to_string())
                .join(tile.x.to_string()),
        ] {
            let meta = fs::symlink_metadata(dir)
                .map_err(|e| ProviderError::from_io("checking tile directory", &e))?;
            if !meta.is_dir() || meta.file_type().is_symlink() {
                return Err(permission("symlink or non-directory in tile path"));
            }
        }
        Ok(())
    }

    fn validate_tile(&self, bytes: &[u8]) -> Result<(), ProviderError> {
        // PNG signature and 256x256 IHDR. PNG CRC/inflate is verified by the
        // eventual image decoder, not by this lightweight cache.
        if bytes.len() < 33
            || bytes.len() > self.limits.max_tile_bytes
            || &bytes[..8] != b"\x89PNG\r\n\x1a\n"
            || &bytes[8..16] != b"\0\0\0\rIHDR"
            || u32::from_be_bytes(bytes[16..20].try_into().unwrap()) != TILE_SIZE
            || u32::from_be_bytes(bytes[20..24].try_into().unwrap()) != TILE_SIZE
        {
            return Err(invalid("invalid or oversized 256px PNG tile"));
        }
        Ok(())
    }

    /// Bounded cache insertion. Evicts only recognised numeric XYZ .png tile
    /// entries, oldest first. External/unrecognised files are never removed.
    pub fn insert(&self, tile: TileCoord, bytes: &[u8]) -> Result<(), ProviderError> {
        self.validate_tile(bytes)?;
        self.ensure_safe_parent(tile)?;
        let path = self.tile_path(tile);
        if let Ok(meta) = fs::symlink_metadata(&path) {
            if !meta.is_file() || meta.file_type().is_symlink() {
                return Err(permission("refusing to overwrite non-regular tile"));
            }
        }
        let mut entries = self.scan_entries()?;
        let mut size = 0u64;
        let mut count = 0usize;
        for entry in &entries {
            if entry.path != path {
                size = size.saturating_add(entry.size);
                count += 1;
            }
        }
        entries.retain(|entry| entry.path != path);
        entries.sort_by(|a, b| {
            a.modified
                .cmp(&b.modified)
                .then_with(|| a.path.cmp(&b.path))
        });
        for entry in &entries {
            if count < self.limits.max_tiles
                && size.saturating_add(bytes.len() as u64) <= self.limits.max_bytes
            {
                break;
            }
            fs::remove_file(&entry.path)
                .map_err(|e| ProviderError::from_io("evicting cached tile", &e))?;
            size = size.saturating_sub(entry.size);
            count -= 1;
        }
        if count >= self.limits.max_tiles
            || size.saturating_add(bytes.len() as u64) > self.limits.max_bytes
        {
            return Err(invalid("cache has no room for tile"));
        }
        let temp = Self::unique_temp(&path)?;
        let mut guard = TempFile { path: temp.clone() };
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| ProviderError::from_io("creating tile staging file", &e))?;
        file.write_all(bytes)
            .map_err(|e| ProviderError::from_io("writing tile", &e))?;
        file.flush()
            .map_err(|e| ProviderError::from_io("flushing tile", &e))?;
        drop(file);
        fs::rename(&temp, &path).map_err(|e| ProviderError::from_io("installing tile", &e))?;
        guard.path.clear();
        Ok(())
    }

    fn unique_temp(path: &Path) -> Result<PathBuf, ProviderError> {
        let parent = path
            .parent()
            .ok_or_else(|| invalid("tile path has no directory"))?;
        let seq = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        Ok(parent.join(format!(".tile-{}-{seq}.part", std::process::id())))
    }

    /// Inspect at most MAX_SCAN_ENTRIES directory entries and refuse to
    /// mutate a cache with excessive/unfamiliar directory fanout.
    fn scan_entries(&self) -> Result<Vec<CacheEntry>, ProviderError> {
        let mut files = Vec::new();
        let mut inspected = 0usize;
        if !self.root.exists() {
            return Ok(files);
        }
        self.scan_level(&self.root, 0, None, None, &mut inspected, &mut files)?;
        Ok(files)
    }

    fn scan_level(
        &self,
        dir: &Path,
        depth: u8,
        zoom: Option<u8>,
        x: Option<u32>,
        inspected: &mut usize,
        files: &mut Vec<CacheEntry>,
    ) -> Result<(), ProviderError> {
        let metadata = fs::symlink_metadata(dir)
            .map_err(|e| ProviderError::from_io("checking cache scan directory", &e))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(permission("non-directory or symlink in cache"));
        }
        for child in
            fs::read_dir(dir).map_err(|e| ProviderError::from_io("scanning tile cache", &e))?
        {
            *inspected += 1;
            if *inspected > MAX_SCAN_ENTRIES {
                return Err(invalid("tile cache directory fanout exceeds scan budget"));
            }
            let child = child.map_err(|e| ProviderError::from_io("scanning tile entry", &e))?;
            let path = child.path();
            let meta = fs::symlink_metadata(&path)
                .map_err(|e| ProviderError::from_io("scanning tile metadata", &e))?;
            if meta.file_type().is_symlink() {
                return Err(permission("symlink in tile cache"));
            }
            let name = child.file_name().to_string_lossy().into_owned();
            match depth {
                0 if meta.is_dir() => {
                    if let Ok(z) = name.parse::<u8>() {
                        if z <= MAX_ZOOM && name == z.to_string() {
                            self.scan_level(&path, 1, Some(z), None, inspected, files)?;
                        }
                    }
                }
                1 if meta.is_dir() => {
                    if let (Some(z), Ok(value)) = (zoom, name.parse::<u32>()) {
                        if name == value.to_string() && value < tile_extent(z)? {
                            self.scan_level(&path, 2, Some(z), Some(value), inspected, files)?;
                        }
                    }
                }
                2 if meta.is_file() => {
                    let valid_tile = match (zoom, x, name.strip_suffix(".png")) {
                        (Some(z), Some(x), Some(raw)) => raw.parse::<u32>().ok().is_some_and(|y| {
                            raw == y.to_string() && TileCoord::new(z, x, y).is_ok()
                        }),
                        _ => false,
                    };
                    // Count staging files left by abrupt process termination;
                    // otherwise repeated interrupted downloads can grow without bound.
                    let own_staging = name
                        .strip_prefix(".tile-")
                        .and_then(|name| name.strip_suffix(".part"))
                        .and_then(|name| name.split_once('-'))
                        .is_some_and(|(pid, seq)| {
                            !pid.is_empty()
                                && !seq.is_empty()
                                && pid.bytes().all(|b| b.is_ascii_digit())
                                && seq.bytes().all(|b| b.is_ascii_digit())
                        });
                    if valid_tile || own_staging {
                        files.push(CacheEntry {
                            path,
                            size: meta.len(),
                            modified: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                        });
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

struct TempFile {
    path: PathBuf,
}
impl Drop for TempFile {
    fn drop(&mut self) {
        if !self.path.as_os_str().is_empty() {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Source is opt-in, HTTPS-only, no credentials/query/fragment or redirects.
/// Deliberately has NO public OSM default endpoint or bulk-prefetch API.
#[derive(Debug, Clone)]
pub struct TileSource {
    base_url: String,
}

impl TileSource {
    pub fn new(base_url: &str) -> Result<Self, ProviderError> {
        let Some(remainder) = base_url.strip_prefix("https://") else {
            return Err(invalid("map tile source must use https"));
        };
        let (host, path) = remainder.split_once('/').unwrap_or((remainder, ""));
        if base_url.len() > 512
            || host.len() > 253
            || !host.contains('.')
            || host.starts_with('.')
            || host.ends_with('.')
            || host.eq_ignore_ascii_case("localhost")
            || host.ends_with(".local")
            || host
                .split('.')
                .all(|label| label.bytes().all(|b| b.is_ascii_digit()))
            || !host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
            || host
                .split('.')
                .any(|label| label.is_empty() || label.starts_with('-') || label.ends_with('-'))
            || path
                .split('/')
                .any(|segment| segment == "." || segment == "..")
            || !path.bytes().all(|b| {
                b.is_ascii_alphanumeric() || b == b'/' || b == b'-' || b == b'_' || b == b'.'
            })
        {
            return Err(invalid("invalid map tile source hostname/path"));
        }
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
        })
    }

    pub fn tile_url(&self, tile: TileCoord) -> String {
        format!("{}/{}/{}/{}.png", self.base_url, tile.zoom, tile.x, tile.y)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileFetch {
    AlreadyCached,
    Downloaded,
}

/// Performs at most one tile download per call, on explicit invocation only.
/// Caller must rate-limit across processes and comply with the tile provider's
/// cache and attribution policies. Do not run on the UI thread.
pub struct TileFetcher<R: CommandRunner> {
    runner: R,
    source: TileSource,
    min_gap: Duration,
    last_attempt: Option<Instant>,
}

impl<R: CommandRunner> TileFetcher<R> {
    pub fn new(runner: R, source: TileSource) -> Self {
        Self {
            runner,
            source,
            min_gap: Duration::from_secs(2),
            last_attempt: None,
        }
    }

    pub fn fetch_one(
        &mut self,
        cache: &TileCache,
        tile: TileCoord,
    ) -> Result<TileFetch, ProviderError> {
        if cache.lookup(tile)?.is_some() {
            return Ok(TileFetch::AlreadyCached);
        }
        if self
            .last_attempt
            .is_some_and(|last| last.elapsed() < self.min_gap)
        {
            return Err(ProviderError::unavailable(
                "single-tile fetch throttled; retry later",
            ));
        }
        self.last_attempt = Some(Instant::now());
        cache.ensure_safe_parent(tile)?;
        let temp = TileCache::unique_temp(&cache.tile_path(tile))?;
        // Never hand curl a pre-existing output path (including a symlink).
        let reserved = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| ProviderError::from_io("reserving download file", &e))?;
        drop(reserved);
        let guard = TempFile { path: temp.clone() };
        let output = self.runner.run(
            &CommandSpec::new("curl").args([
                "-q",
                "--fail",
                "--silent",
                "--show-error", // no redirect following
                "--max-redirs",
                "0",
                "--proto",
                "=https",
                "--proto-redir",
                "=https",
                "--connect-timeout",
                "2",
                "--max-time",
                "6",
                "--retry",
                "0",
                "--user-agent",
                "NuraLoumi-offline-maps/0.2 (single-tile opt-in)",
                "--max-filesize",
                &cache.limits.max_tile_bytes.to_string(),
                "--output",
                &temp.to_string_lossy(),
                "--url",
                &self.source.tile_url(tile),
            ]),
            CommandLimits::new(Duration::from_secs(8), 2048),
        )?;
        if output.status != 0 {
            return Err(ProviderError::backend(format!(
                "tile fetch failed (curl exit {})",
                output.status
            )));
        }
        let metadata = fs::symlink_metadata(&temp)
            .map_err(|e| ProviderError::from_io("checking downloaded tile", &e))?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > cache.limits.max_tile_bytes as u64
        {
            return Err(invalid(
                "downloaded tile exceeds size limit or is not a regular file",
            ));
        }
        let mut data = Vec::new();
        File::open(&temp)
            .map_err(|e| ProviderError::from_io("opening downloaded tile", &e))?
            .take(cache.limits.max_tile_bytes as u64 + 1)
            .read_to_end(&mut data)
            .map_err(|e| ProviderError::from_io("reading downloaded tile", &e))?;
        cache.insert(tile, &data)?;
        drop(guard);
        Ok(TileFetch::Downloaded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::CommandOutput;
    use std::sync::atomic::AtomicUsize;

    static TEST_SEQ: AtomicUsize = AtomicUsize::new(0);

    fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "nuraloumi-tiles-test-{}-{}",
            std::process::id(),
            TEST_SEQ.fetch_add(1, Ordering::Relaxed)
        ))
    }

    struct TestCache(TileCache);
    impl TestCache {
        fn new(limits: TileCacheLimits) -> Self {
            Self(TileCache::new(temp_root(), limits).unwrap())
        }
    }
    impl Drop for TestCache {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(self.0.root());
        }
    }

    fn tile() -> Vec<u8> {
        let hex = include_str!("../../../tests/fixtures/maps/tile_256.hex").trim();
        hex.as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    #[test]
    fn validates_zoom_coordinate_and_traversal_by_construction() {
        assert_eq!(TileCoord::new(0, 0, 0).unwrap().x(), 0);
        assert!(TileCoord::new(0, 1, 0).is_err());
        assert!(TileCoord::new(3, 8, 0).is_err());
        assert!(TileCoord::new(19, 0, 0).is_err());
        assert_eq!(TileCoord::new(18, 262143, 262143).unwrap().y(), 262143);
    }

    #[test]
    fn viewport_wraps_and_is_bounded() {
        let mut view = MapView::from_lon_lat(179.99, 0.0, 2).unwrap();
        view.drag_pixels(-30.0, 0.0).unwrap();
        let positions = view.visible_tiles(400, 300).unwrap();
        assert!(!positions.is_empty());
        assert!(positions.iter().all(|p| p.coord.x() < 4 && p.coord.y() < 4));
        assert!(view.visible_tiles(4096, 4096).is_err());
        assert!(MapView::from_lon_lat(f64::NAN, 0.0, 2).is_err());
    }

    #[test]
    fn anchored_zoom_preserves_anchor_world_point() {
        let mut v = MapView::from_lon_lat(15.0, 48.0, 5).unwrap();
        let before = v.center_pixels();
        v.zoom_at(6, 75.0, -35.0).unwrap();
        let after = v.center_pixels();
        assert!((after.0 + 75.0 - (before.0 + 75.0) * 2.0).abs() < 0.00001);
        assert!((after.1 - 35.0 - (before.1 - 35.0) * 2.0).abs() < 0.00001);
        assert!(v.zoom_at(255, 0.0, 0.0).is_err());
    }

    #[test]
    fn offline_miss_hit_and_corrupt_rejection() {
        let c = TestCache::new(TileCacheLimits::default());
        let xyz = TileCoord::new(3, 2, 4).unwrap();
        assert!(c.0.lookup(xyz).unwrap().is_none());
        c.0.insert(xyz, &tile()).unwrap();
        assert_eq!(c.0.lookup(xyz).unwrap().unwrap(), tile());
        fs::write(c.0.tile_path(xyz), b"<html>not a PNG").unwrap();
        assert!(c.0.lookup(xyz).is_err());
    }

    #[test]
    fn evicts_old_tiles_and_rejects_oversized_data() {
        let bytes = tile();
        let c = TestCache::new(TileCacheLimits {
            max_tiles: 2,
            max_bytes: 800,
            max_tile_bytes: 512,
        });
        let a = TileCoord::new(2, 0, 0).unwrap();
        let b = TileCoord::new(2, 0, 1).unwrap();
        let d = TileCoord::new(2, 0, 2).unwrap();
        c.0.insert(a, &bytes).unwrap();
        c.0.insert(b, &bytes).unwrap();
        c.0.insert(d, &bytes).unwrap();
        assert!(c.0.lookup(a).unwrap().is_none());
        assert!(c.0.lookup(b).unwrap().is_some());
        assert!(c.0.lookup(d).unwrap().is_some());
        assert!(c.0.insert(a, &vec![0; 513]).is_err());
        assert!(TileCache::new(
            temp_root(),
            TileCacheLimits {
                max_tiles: 0,
                ..Default::default()
            }
        )
        .is_err());
    }

    #[test]
    fn counts_and_evicts_orphaned_download_staging() {
        let c = TestCache::new(TileCacheLimits {
            max_tiles: 1,
            max_bytes: 512,
            max_tile_bytes: 512,
        });
        let a = TileCoord::new(2, 0, 0).unwrap();
        c.0.ensure_safe_parent(a).unwrap();
        let staged =
            c.0.tile_path(a)
                .parent()
                .unwrap()
                .join(".tile-123-456.part");
        fs::write(&staged, tile()).unwrap();
        c.0.insert(a, &tile()).unwrap();
        assert!(!staged.exists());
        assert!(c.0.lookup(a).unwrap().is_some());
    }

    #[test]
    fn rejects_oversized_download_before_unbounded_read() {
        let c = TestCache::new(TileCacheLimits::default());
        let a = TileCoord::new(2, 0, 1).unwrap();
        let source = TileSource::new("https://tiles.example.invalid").unwrap();
        let mut fetcher = TileFetcher::new(
            FixtureDownload {
                png: vec![42; 512 * 1024 + 1],
                calls: 0,
            },
            source,
        );
        assert!(fetcher.fetch_one(&c.0, a).is_err());
        assert!(c.0.lookup(a).unwrap().is_none());
        assert!(fs::read_dir(c.0.tile_path(a).parent().unwrap())
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".part")));
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlink_leaf_and_directory() {
        use std::os::unix::fs::symlink;
        let c = TestCache::new(TileCacheLimits::default());
        let xyz = TileCoord::new(3, 2, 4).unwrap();
        c.0.ensure_safe_parent(xyz).unwrap();
        symlink("/tmp", c.0.tile_path(xyz)).unwrap();
        assert!(c.0.insert(xyz, &tile()).is_err());
        assert!(c.0.lookup(xyz).is_err());
    }

    #[test]
    fn rejects_untrusted_tile_sources() {
        for url in [
            "http://tiles.example.org",
            "https://localhost",
            "https://u:p@tiles.example.org",
            "https://tiles.example.org/../x",
            "https://tiles.example.org/?token=123",
            "https://127.0.0.1:1234",
            "https://tiles.example.org/#foo",
        ] {
            assert!(TileSource::new(url).is_err(), "accepted: {url}");
        }
        let tile = TileCoord::new(4, 5, 6).unwrap();
        assert_eq!(
            TileSource::new("https://tiles.example.invalid/osm")
                .unwrap()
                .tile_url(tile),
            "https://tiles.example.invalid/osm/4/5/6.png"
        );
    }

    struct FixtureDownload {
        png: Vec<u8>,
        calls: usize,
    }
    impl CommandRunner for FixtureDownload {
        fn run(
            &mut self,
            spec: &CommandSpec,
            limits: CommandLimits,
        ) -> Result<CommandOutput, ProviderError> {
            assert_eq!(spec.program, "curl");
            assert!(limits.timeout <= Duration::from_secs(8));
            assert_eq!(spec.args.first().map(String::as_str), Some("-q"));
            assert!(spec.args.iter().any(|v| v == "--max-filesize"));
            let i = spec.args.iter().position(|v| v == "--output").unwrap();
            fs::write(&spec.args[i + 1], &self.png).unwrap();
            self.calls += 1;
            Ok(CommandOutput {
                status: 0,
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }

    #[test]
    fn explicit_bounded_single_fetch_and_offline_reuse() {
        let c = TestCache::new(TileCacheLimits::default());
        let source = TileSource::new("https://tiles.example.invalid").unwrap();
        let mut fetcher = TileFetcher::new(
            FixtureDownload {
                png: tile(),
                calls: 0,
            },
            source,
        );
        let a = TileCoord::new(1, 0, 0).unwrap();
        let b = TileCoord::new(1, 1, 1).unwrap();
        assert_eq!(fetcher.fetch_one(&c.0, a).unwrap(), TileFetch::Downloaded);
        assert_eq!(
            fetcher.fetch_one(&c.0, a).unwrap(),
            TileFetch::AlreadyCached
        );
        assert!(fetcher.fetch_one(&c.0, b).is_err()); // two-second throttle
        assert_eq!(fetcher.runner.calls, 1);
    }
}
