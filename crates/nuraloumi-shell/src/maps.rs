//! Bounded offline Cairo XYZ tile viewport and interaction controller.
//! Paint code reads only accepted local TileCache data; never downloads tiles.
use cairo::{Context, FontSlant, FontWeight, Format, ImageSurface};
use nuraloumi_providers::maps::{MapView, TileCache, TileCoord, MAX_ZOOM, TILE_SIZE};
use nuraloumi_wayland::{Key, PlatformEvent};
use std::{
    collections::{HashMap, VecDeque},
    io::Cursor,
};

const TOP: f64 = 56.0;
const BOTTOM: f64 = 32.0;
const MAX_DECODED: usize = 16; // 16 * 256 * 256 * 4 = 4 MiB.
const MAX_FRAME: u64 = 12 * 1024 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    None,
    Redraw,
    Close,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TileStats {
    pub ready: usize,
    pub missing: usize,
    pub invalid: usize,
}
enum Tile {
    Ready(ImageSurface),
    Missing,
    Invalid,
}

/// Draws cached OSM-style XYZ tiles in a CPU-only ARGB32 surface.
pub struct MapSurface {
    view: MapView,
    cache: TileCache,
    decoded: HashMap<TileCoord, Tile>,
    lru: VecDeque<TileCoord>,
    touch: Option<(i32, f64, f64)>,
    pointer: Option<(f64, f64)>,
}
impl MapSurface {
    pub fn new(view: MapView, cache: TileCache) -> Self {
        Self {
            view,
            cache,
            decoded: HashMap::new(),
            lru: VecDeque::new(),
            touch: None,
            pointer: None,
        }
    }
    pub fn view(&self) -> MapView {
        self.view
    }
    pub fn reload(&mut self) {
        self.decoded.clear();
        self.lru.clear();
    }

    fn tile(&mut self, coord: TileCoord) -> &Tile {
        if !self.decoded.contains_key(&coord) {
            while self.decoded.len() >= MAX_DECODED {
                if let Some(evict) = self.lru.pop_front() {
                    self.decoded.remove(&evict);
                }
            }
            let tile = match self.cache.lookup(coord) {
                Ok(None) => Tile::Missing,
                Err(_) => Tile::Invalid,
                Ok(Some(bytes)) => match ImageSurface::create_from_png(&mut Cursor::new(bytes)) {
                    Ok(img)
                        if img.width() == TILE_SIZE as i32 && img.height() == TILE_SIZE as i32 =>
                    {
                        Tile::Ready(img)
                    }
                    _ => Tile::Invalid,
                },
            };
            self.decoded.insert(coord, tile);
        }
        self.lru.retain(|entry| *entry != coord);
        self.lru.push_back(coord);
        self.decoded.get(&coord).expect("tile was inserted")
    }
    pub fn zoom(&mut self, step: i8) -> bool {
        let next = i16::from(self.view.zoom) + i16::from(step);
        (0..=i16::from(MAX_ZOOM)).contains(&next) && self.view.zoom_at(next as u8, 0.0, 0.0).is_ok()
    }
    fn pan(&mut self, dx: f64, dy: f64) -> Change {
        if (dx != 0.0 || dy != 0.0) && self.view.drag_pixels(dx, dy).is_ok() {
            Change::Redraw
        } else {
            Change::None
        }
    }
    fn toolbar_click(&mut self, x: f64, y: f64, width: u32) -> Change {
        if !(4.0..TOP - 4.0).contains(&y) {
            return Change::None;
        }
        let right = f64::from(width);
        let delta = if (right - 108.0..right - 60.0).contains(&x) {
            -1
        } else if (right - 56.0..right - 8.0).contains(&x) {
            1
        } else {
            0
        };
        if delta != 0 && self.zoom(delta) {
            Change::Redraw
        } else {
            Change::None
        }
    }
    /// Event coordinates are logical Wayland surface pixels. The backend
    /// reports contacts but not native pinch events: use single-finger drag only.
    pub fn event(&mut self, event: &PlatformEvent, width: u32, height: u32) -> Change {
        match event {
            PlatformEvent::Key { pressed: true, key } => match key {
                Key::Escape => Change::Close,
                Key::Text(t) if t == "q" => Change::Close,
                Key::Text(t) if t == "r" || t == "R" => {
                    self.reload();
                    Change::Redraw
                }
                Key::Left => self.pan(128.0, 0.0),
                Key::Right => self.pan(-128.0, 0.0),
                Key::Up => self.pan(0.0, 128.0),
                Key::Down => self.pan(0.0, -128.0),
                Key::Text(t) if t == "+" || t == "=" => {
                    if self.zoom(1) {
                        Change::Redraw
                    } else {
                        Change::None
                    }
                }
                Key::Text(t) if t == "-" => {
                    if self.zoom(-1) {
                        Change::Redraw
                    } else {
                        Change::None
                    }
                }
                _ => Change::None,
            },
            PlatformEvent::TouchDown { id, x, y } => {
                if self.touch.is_some() || !x.is_finite() || !y.is_finite() {
                    return Change::None;
                }
                if *y < TOP {
                    return self.toolbar_click(*x, *y, width);
                }
                if *y < f64::from(height) - BOTTOM {
                    self.touch = Some((*id, *x, *y));
                }
                Change::None
            }
            PlatformEvent::TouchMotion { id, x, y } => {
                if let Some((active, previous_x, previous_y)) = self.touch {
                    if active == *id && x.is_finite() && y.is_finite() {
                        self.touch = Some((active, *x, *y));
                        return self.pan(*x - previous_x, *y - previous_y);
                    }
                }
                Change::None
            }
            PlatformEvent::TouchUp { id } => {
                if self.touch.is_some_and(|(active, _, _)| active == *id) {
                    self.touch = None;
                }
                Change::None
            }
            PlatformEvent::TouchCancel { ids } => {
                if self
                    .touch
                    .is_some_and(|(active, _, _)| ids.is_empty() || ids.contains(&active))
                {
                    self.touch = None;
                }
                Change::None
            }
            PlatformEvent::PointerButton {
                button: 0x110,
                x,
                y,
                pressed,
            } => {
                if !pressed {
                    self.pointer = None;
                } else if *y < TOP {
                    self.pointer = None;
                    return self.toolbar_click(*x, *y, width);
                } else if x.is_finite() && y.is_finite() && *y < f64::from(height) - BOTTOM {
                    self.pointer = Some((*x, *y));
                }
                Change::None
            }
            PlatformEvent::PointerMove { x, y } => {
                if let Some((old_x, old_y)) = self.pointer {
                    if x.is_finite() && y.is_finite() {
                        self.pointer = Some((*x, *y));
                        return self.pan(*x - old_x, *y - old_y);
                    }
                }
                Change::None
            }
            PlatformEvent::PointerLeave => {
                self.pointer = None;
                Change::None
            }
            _ => Change::None,
        }
    }
    /// Frame allocation is bounded even for malicious compositor configure values.
    pub fn render(
        &mut self,
        width: u32,
        height: u32,
        scale: i32,
    ) -> Result<(ImageSurface, TileStats), String> {
        if !(240..=2048).contains(&width)
            || !(160..=2048).contains(&height)
            || !(1..=3).contains(&scale)
        {
            return Err("unsupported viewport or scale".into());
        }
        let pw = width.checked_mul(scale as u32).ok_or("width overflow")?;
        let ph = height.checked_mul(scale as u32).ok_or("height overflow")?;
        if u64::from(pw) * u64::from(ph) * 4 > MAX_FRAME {
            return Err("frame memory budget exceeded".into());
        }
        let map_height = height - (TOP + BOTTOM) as u32;
        let placements = self
            .view
            .visible_tiles(width, map_height)
            .map_err(|e| e.to_string())?;
        let image = ImageSurface::create(Format::ARgb32, pw as i32, ph as i32)
            .map_err(|e| e.to_string())?;
        let cr = Context::new(&image).map_err(|e| e.to_string())?;
        cr.scale(f64::from(scale), f64::from(scale));
        cr.set_source_rgb(0.043, 0.051, 0.059);
        cr.paint().map_err(|e| e.to_string())?;
        cr.rectangle(0.0, TOP, f64::from(width), f64::from(map_height));
        cr.clip();
        let mut stats = TileStats::default();
        for tile in placements {
            let (x, y) = (tile.screen_x, tile.screen_y + TOP);
            cr.save().map_err(|e| e.to_string())?;
            cr.rectangle(x, y, 256.0, 256.0);
            cr.clip();
            match self.tile(tile.coord) {
                Tile::Ready(img) => {
                    cr.set_source_surface(img, x, y)
                        .map_err(|e| e.to_string())?;
                    cr.paint().map_err(|e| e.to_string())?;
                    stats.ready += 1;
                }
                Tile::Missing => {
                    placeholder(&cr, x, y, tile.coord, false)?;
                    stats.missing += 1;
                }
                Tile::Invalid => {
                    placeholder(&cr, x, y, tile.coord, true)?;
                    stats.invalid += 1;
                }
            }
            cr.restore().map_err(|e| e.to_string())?;
        }
        cr.reset_clip();
        toolbar(&cr, width, height, self.view.zoom)?;
        drop(cr);
        image.flush();
        Ok((image, stats))
    }
}
fn text(cr: &Context, content: &str, x: f64, y: f64, size: f64) -> Result<(), String> {
    cr.select_font_face("Sans", FontSlant::Normal, FontWeight::Normal);
    cr.set_font_size(size);
    cr.move_to(x, y);
    cr.show_text(content).map_err(|e| e.to_string())
}
fn placeholder(
    cr: &Context,
    x: f64,
    y: f64,
    coord: TileCoord,
    invalid: bool,
) -> Result<(), String> {
    if invalid {
        cr.set_source_rgb(0.20, 0.095, 0.11);
    } else {
        cr.set_source_rgb(0.10, 0.12, 0.15);
    }
    cr.rectangle(x, y, 256.0, 256.0);
    cr.fill().map_err(|e| e.to_string())?;
    cr.set_source_rgb(0.29, 0.33, 0.38);
    cr.set_line_width(1.0);
    cr.rectangle(x + 0.5, y + 0.5, 255.0, 255.0);
    cr.move_to(x + 4.0, y + 4.0);
    cr.line_to(x + 252.0, y + 252.0);
    cr.move_to(x + 252.0, y + 4.0);
    cr.line_to(x + 4.0, y + 252.0);
    cr.stroke().map_err(|e| e.to_string())?;
    cr.set_source_rgb(0.90, 0.91, 0.93);
    text(
        cr,
        if invalid {
            "INVALID TILE"
        } else {
            "OFFLINE / NOT CACHED"
        },
        x + 14.0,
        y + 116.0,
        14.0,
    )?;
    text(
        cr,
        &format!("{}/{}/{}.png", coord.zoom(), coord.x(), coord.y()),
        x + 14.0,
        y + 140.0,
        12.0,
    )
}
fn toolbar(cr: &Context, width: u32, height: u32, zoom: u8) -> Result<(), String> {
    let (w, h) = (f64::from(width), f64::from(height));
    cr.set_source_rgb(0.063, 0.071, 0.082);
    cr.rectangle(0.0, 0.0, w, TOP);
    cr.rectangle(0.0, h - BOTTOM, w, BOTTOM);
    cr.fill().map_err(|e| e.to_string())?;
    cr.set_source_rgb(0.58, 0.62, 0.68);
    text(cr, &format!("OFFLINE MAP / z{zoom}"), 14.0, 34.0, 17.0)?;
    text(
        cr,
        "Arrows pan | +/- zoom | R reload | Esc close",
        14.0,
        h - 11.0,
        12.0,
    )?;
    for (x, symbol) in [(w - 108.0, "-"), (w - 56.0, "+")] {
        cr.set_source_rgb(0.13, 0.15, 0.19);
        cr.rectangle(x, 4.0, 48.0, 48.0);
        cr.fill().map_err(|e| e.to_string())?;
        cr.set_source_rgb(0.58, 0.62, 0.68);
        cr.set_line_width(1.0);
        cr.rectangle(x + 0.5, 4.5, 47.0, 47.0);
        cr.stroke().map_err(|e| e.to_string())?;
        text(cr, symbol, x + 16.0, 37.0, 28.0)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use nuraloumi_providers::maps::TileCacheLimits;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };
    static SEQ: AtomicU64 = AtomicU64::new(0);
    struct Fixture {
        root: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "nuraloumi-map-ui-{}-{}",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self { root }
        }
        fn cache(&self) -> TileCache {
            TileCache::new(&self.root, TileCacheLimits::default()).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    fn viewer(cache: TileCache) -> MapSurface {
        MapSurface::new(MapView::from_lon_lat(0.0, 0.0, 3).unwrap(), cache)
    }
    fn png() -> Vec<u8> {
        let surface = ImageSurface::create(Format::ARgb32, 256, 256).unwrap();
        let cr = Context::new(&surface).unwrap();
        cr.set_source_rgb(0.12, 0.55, 0.48);
        cr.paint().unwrap();
        drop(cr);
        let mut bytes = Vec::new();
        surface.write_to_png(&mut bytes).unwrap();
        bytes
    }
    #[test]
    fn fixture_tiles_missing_and_corrupt_are_distinct() {
        let dir = Fixture::new();
        let cache = dir.cache();
        let coords = MapView::from_lon_lat(0.0, 0.0, 3)
            .unwrap()
            .visible_tiles(512, 300)
            .unwrap();
        cache.insert(coords[0].coord, &png()).unwrap();
        cache.insert(coords[1].coord, &png()).unwrap();
        let mut broken = png();
        broken.truncate(33);
        fs::write(cache.tile_path(coords[1].coord), broken).unwrap();
        let mut viewer = viewer(cache);
        let (image, stats) = viewer.render(512, 388, 1).unwrap();
        assert_eq!(image.width(), 512);
        assert_eq!(stats.ready, 1);
        assert_eq!(stats.invalid, 1);
        assert!(stats.missing >= 1);
        assert_eq!(stats, viewer.render(512, 388, 1).unwrap().1);
        assert!(viewer.decoded.len() <= MAX_DECODED);
    }
    #[test]
    fn tile_is_painted_at_deterministic_world_to_screen_position() {
        let dir = Fixture::new();
        let cache = dir.cache();
        let placements = MapView::from_lon_lat(0.0, 0.0, 3)
            .unwrap()
            .visible_tiles(512, 300)
            .unwrap();
        // World center=(1024,1024), logical map top-left=(768,874).
        assert_eq!(placements[0].screen_x, 0.0);
        assert_eq!(placements[0].screen_y, -106.0);
        cache.insert(placements[0].coord, &png()).unwrap();
        let mut viewer = viewer(cache);
        let (mut image, stats) = viewer.render(512, 388, 1).unwrap();
        assert_eq!(stats.ready, 1);
        // Tile y=-106 + toolbar 56 = -50; pixel (30,80) lies within it.
        let stride = image.stride() as usize;
        let data = image.data().unwrap();
        let offset = 80 * stride + 30 * 4;
        let pixel = u32::from_ne_bytes(data[offset..offset + 4].try_into().unwrap());
        // Compare against the fixture's source pixel rather than assuming
        // Cairo's floating-point to 8-bit rounding on every platform.
        let mut original = ImageSurface::create_from_png(&mut Cursor::new(png())).unwrap();
        let original_stride = original.stride() as usize;
        let source = original.data().unwrap();
        let source_offset = 30 * original_stride + 30 * 4;
        let expected =
            u32::from_ne_bytes(source[source_offset..source_offset + 4].try_into().unwrap());
        assert_eq!(pixel, expected);
    }

    #[test]
    fn toolbar_hit_targets_zoom_without_starting_drag() {
        let dir = Fixture::new();
        let mut viewer = viewer(dir.cache());
        assert_eq!(
            viewer.event(
                &PlatformEvent::TouchDown {
                    id: 2,
                    x: 482.0,
                    y: 25.0
                },
                512,
                388
            ),
            Change::Redraw
        );
        assert_eq!(viewer.view().zoom, 4);
        assert_eq!(
            viewer.event(
                &PlatformEvent::TouchMotion {
                    id: 2,
                    x: 400.0,
                    y: 300.0
                },
                512,
                388
            ),
            Change::None
        );
        assert_eq!(
            viewer.event(
                &PlatformEvent::PointerButton {
                    x: 425.0,
                    y: 25.0,
                    pressed: true,
                    button: 0x110
                },
                512,
                388
            ),
            Change::Redraw
        );
        assert_eq!(viewer.view().zoom, 3);
    }

    #[test]
    fn arrow_drag_zoom_and_contact_filter() {
        let dir = Fixture::new();
        let mut viewer = viewer(dir.cache());
        let start = viewer.view().center_pixels();
        assert_eq!(
            viewer.event(
                &PlatformEvent::Key {
                    key: Key::Right,
                    pressed: true
                },
                512,
                400
            ),
            Change::Redraw
        );
        assert_eq!(viewer.view().center_pixels().0, start.0 + 128.0);
        viewer.event(
            &PlatformEvent::TouchDown {
                id: 7,
                x: 200.0,
                y: 180.0,
            },
            512,
            400,
        );
        viewer.event(
            &PlatformEvent::TouchDown {
                id: 8,
                x: 220.0,
                y: 180.0,
            },
            512,
            400,
        );
        assert_eq!(
            viewer.event(
                &PlatformEvent::TouchMotion {
                    id: 8,
                    x: 400.0,
                    y: 180.0
                },
                512,
                400
            ),
            Change::None
        );
        assert_eq!(
            viewer.event(
                &PlatformEvent::TouchMotion {
                    id: 7,
                    x: 220.0,
                    y: 190.0
                },
                512,
                400
            ),
            Change::Redraw
        );
        assert_eq!(
            viewer.view().center_pixels(),
            (start.0 + 108.0, start.1 - 10.0)
        );
        viewer.event(&PlatformEvent::TouchUp { id: 7 }, 512, 400);
        assert_eq!(
            viewer.event(
                &PlatformEvent::TouchMotion {
                    id: 7,
                    x: 290.0,
                    y: 190.0
                },
                512,
                400
            ),
            Change::None
        );
        assert_eq!(
            viewer.event(
                &PlatformEvent::Key {
                    key: Key::Text("+".into()),
                    pressed: true
                },
                512,
                400
            ),
            Change::Redraw
        );
        assert_eq!(viewer.view().zoom, 4);
        assert_eq!(
            viewer.event(
                &PlatformEvent::Key {
                    key: Key::Text("-".into()),
                    pressed: true
                },
                512,
                400
            ),
            Change::Redraw
        );
        assert_eq!(viewer.view().zoom, 3);
    }
    #[test]
    fn memory_and_zoom_are_bounded() {
        let dir = Fixture::new();
        let mut viewer = viewer(dir.cache());
        assert!(viewer.render(2048, 2048, 2).is_err());
        assert!(viewer.render(230, 400, 1).is_err());
        assert!(viewer.render(500, 400, 0).is_err());
        for _ in 0..25 {
            viewer.zoom(1);
        }
        assert_eq!(viewer.view().zoom, MAX_ZOOM);
        assert!(!viewer.zoom(1));
        for _ in 0..25 {
            viewer.zoom(-1);
        }
        assert_eq!(viewer.view().zoom, 0);
        assert!(!viewer.zoom(-1));
    }
    #[test]
    fn empty_cache_renders_explicit_missing_placeholder() {
        let dir = Fixture::new();
        let mut viewer = viewer(dir.cache());
        let (mut image, stats) = viewer.render(512, 388, 1).unwrap();
        assert_eq!(stats.ready, 0);
        assert!(stats.missing >= 1);
        assert!(image.data().unwrap().iter().any(|byte| *byte != 0));
    }
}
