//! Standalone offline map viewer. No downloads or command execution.
use cairo::ImageSurface;
use nuraloumi_providers::maps::{MapView, TileCache, TileCacheLimits};
use nuraloumi_shell::{
    dismiss_backdrop_margins, load_config_or_default,
    maps::{Change, MapSurface},
};
use nuraloumi_wayland::{
    BackendError, DismissBackdropConfig, Frame, MenuConfig, PixelFormat, PlatformEvent, SurfaceId,
    WaylandBackend,
};
use std::{env, path::PathBuf, process::ExitCode};

struct Options {
    cache: Option<PathBuf>,
    config: Option<PathBuf>,
    lon: f64,
    lat: f64,
    zoom: u8,
}
fn options() -> Result<Option<Options>, String> {
    let mut result = Options {
        cache: None,
        config: None,
        lon: 0.0,
        lat: 0.0,
        zoom: 3,
    };
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("nuraloumi-maps [--cache DIR] [--lon DEGREES] [--lat DEGREES] [--zoom 0..18] [--config PATH]\nOffline only. Controls: arrows pan, +/- zoom, drag to pan, R reload, Esc quit.");
                return Ok(None);
            }
            "--cache" => {
                result.cache = Some(PathBuf::from(args.next().ok_or("--cache requires DIR")?))
            }
            "--config" => {
                result.config = Some(PathBuf::from(args.next().ok_or("--config requires PATH")?))
            }
            "--lon" => {
                result.lon = args
                    .next()
                    .ok_or("--lon requires degrees")?
                    .parse()
                    .map_err(|_| "invalid longitude")?
            }
            "--lat" => {
                result.lat = args
                    .next()
                    .ok_or("--lat requires degrees")?
                    .parse()
                    .map_err(|_| "invalid latitude")?
            }
            "--zoom" => {
                result.zoom = args
                    .next()
                    .ok_or("--zoom requires level")?
                    .parse()
                    .map_err(|_| "invalid zoom")?
            }
            other => return Err(format!("unknown map option: {other}")),
        }
    }
    if !result.lon.is_finite()
        || !result.lat.is_finite()
        || !(-180.0..=180.0).contains(&result.lon)
        || !(-90.0..=90.0).contains(&result.lat)
        || result.zoom > 18
    {
        return Err("longitude, latitude or zoom out of range".into());
    }
    Ok(Some(result))
}
fn cache_root() -> Result<PathBuf, String> {
    if let Some(dir) = env::var_os("XDG_CACHE_HOME").filter(|value| !value.is_empty()) {
        let dir = PathBuf::from(dir);
        if !dir.is_absolute() {
            return Err("XDG_CACHE_HOME must be absolute".into());
        }
        return Ok(dir.join("nuraloumi/maps"));
    }
    let home = PathBuf::from(env::var_os("HOME").ok_or("set HOME or supply --cache DIR")?);
    if !home.is_absolute() {
        return Err("HOME must be absolute".into());
    }
    Ok(home.join(".cache/nuraloumi/maps"))
}
fn present(
    backend: &mut WaylandBackend,
    id: SurfaceId,
    frame: &mut ImageSurface,
) -> Result<(), String> {
    frame.flush();
    let (width, height, stride) = (
        frame.width() as u32,
        frame.height() as u32,
        frame.stride() as usize,
    );
    let pixels = frame.data().map_err(|_| "cannot borrow map frame pixels")?;
    match backend.present(
        id,
        Frame {
            width,
            height,
            stride,
            format: PixelFormat::Argb8888,
            pixels: &pixels,
        },
    ) {
        Ok(()) | Err(BackendError::WouldBlock) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}
fn transparent(backend: &mut WaylandBackend, id: SurfaceId, width: u32, height: u32, scale: i32) {
    if !(1..=3).contains(&scale) {
        return;
    }
    let (Some(w), Some(h)) = (
        width.checked_mul(scale as u32),
        height.checked_mul(scale as u32),
    ) else {
        return;
    };
    if u64::from(w) * u64::from(h) * 4 > 12 * 1024 * 1024 {
        return;
    }
    let pixels = vec![0_u8; w as usize * h as usize * 4];
    let _ = backend.present(id, Frame::packed(w, h, PixelFormat::Argb8888, &pixels));
}
fn close(backend: &mut WaylandBackend, id: SurfaceId, dismiss: Option<SurfaceId>) {
    if let Some(d) = dismiss {
        let _ = backend.destroy_surface(d);
    }
    let _ = backend.destroy_surface(id);
}
fn run(opts: Options) -> Result<(), String> {
    let config = load_config_or_default(opts.config.as_deref())?;
    let cache = TileCache::new(
        match opts.cache {
            Some(path) => path,
            None => cache_root()?,
        },
        TileCacheLimits::default(),
    )
    .map_err(|error| error.to_string())?;
    let view =
        MapView::from_lon_lat(opts.lon, opts.lat, opts.zoom).map_err(|error| error.to_string())?;
    let mut map = MapSurface::new(view, cache);
    let mut backend = WaylandBackend::connect().map_err(|error| error.to_string())?;
    backend
        .blocking_dispatch()
        .map_err(|error| error.to_string())?;
    let output = backend.outputs().into_iter().next();
    let (top, right, bottom, left) =
        dismiss_backdrop_margins(config.panel_edge, config.panel_height);
    let dismiss = backend
        .create_dismiss_backdrop(DismissBackdropConfig {
            margin_top: top,
            margin_right: right,
            margin_bottom: bottom,
            margin_left: left,
            output: output.as_ref().map(|value| value.id),
            namespace: "nuraloumi-maps-dismiss".into(),
        })
        .ok();
    let surface = backend
        .create_menu(MenuConfig {
            width: 800,
            height: 640,
            margin_top: config.panel_height as i32,
            margin_left: 0,
            output: output.as_ref().map(|value| value.id),
            namespace: "nuraloumi-maps".into(),
        })
        .map_err(|error| error.to_string())?;
    let mut geometry: Option<(u32, u32, i32)> = None;
    loop {
        backend
            .blocking_dispatch()
            .map_err(|error| error.to_string())?;
        let mut redraw = false;
        for incoming in backend.drain_events().collect::<Vec<_>>() {
            if dismiss.is_some_and(|id| incoming.surface == Some(id)) {
                if let Some(id) = dismiss {
                    match incoming.event {
                        PlatformEvent::Configure {
                            width,
                            height,
                            scale,
                        } => transparent(&mut backend, id, width, height, scale),
                        PlatformEvent::TouchDown { .. }
                        | PlatformEvent::PointerButton { pressed: true, .. }
                        | PlatformEvent::Close => {
                            close(&mut backend, surface, dismiss);
                            return Ok(());
                        }
                        _ => {}
                    }
                }
                continue;
            }
            if incoming.surface != Some(surface) {
                continue;
            }
            match incoming.event {
                PlatformEvent::Configure {
                    width,
                    height,
                    scale,
                } => {
                    geometry = Some((width, height, scale));
                    redraw = true;
                }
                PlatformEvent::Close => {
                    close(&mut backend, surface, dismiss);
                    return Ok(());
                }
                other => {
                    if let Some((width, height, _)) = geometry {
                        match map.event(&other, width, height) {
                            Change::None => {}
                            Change::Redraw => redraw = true,
                            Change::Close => {
                                close(&mut backend, surface, dismiss);
                                return Ok(());
                            }
                        }
                    }
                }
            }
        }
        if redraw {
            if let Some((w, h, s)) = geometry {
                let (mut frame, _) = map.render(w, h, s)?;
                present(&mut backend, surface, &mut frame)?;
            }
        }
    }
}
fn main() -> ExitCode {
    match options().and_then(|result| result.map(run).transpose()) {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("nuraloumi-maps: {error}");
            ExitCode::FAILURE
        }
    }
}
