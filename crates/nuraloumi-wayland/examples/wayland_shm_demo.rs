use nuraloumi_wayland::{
    BackendError, Frame, Key, MenuConfig, PixelFormat, PlatformEvent, SurfaceId, WaylandBackend,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut backend = WaylandBackend::connect()?;
    let capabilities = backend.capabilities();
    if !capabilities.layer_shell {
        return Err(Box::new(BackendError::MissingGlobal("zwlr_layer_shell_v1")));
    }

    eprintln!("NuraLoumi Wayland demo capabilities: {capabilities:?}");
    for output in backend.outputs() {
        eprintln!("output: {output:?}");
    }

    let menu = backend.create_menu(MenuConfig {
        height: 160,
        margin_top: 24,
        ..MenuConfig::default()
    })?;
    let mut phase = 0u8;
    let mut geometry: Option<(u32, u32, i32)> = None;

    loop {
        backend.blocking_dispatch()?;
        let events: Vec<_> = backend.drain_events().collect();
        for event in events {
            eprintln!("event: {event:?}");
            match event.event {
                PlatformEvent::Configure {
                    width,
                    height,
                    scale,
                } if event.surface == Some(menu) => {
                    geometry = Some((width, height, scale));
                    draw_and_present(&mut backend, menu, width, height, scale, phase)?;
                }
                PlatformEvent::PointerButton { pressed: true, .. }
                | PlatformEvent::TouchDown { .. } => {
                    phase = phase.wrapping_add(37);
                    if let Some((width, height, scale)) = geometry {
                        draw_and_present(&mut backend, menu, width, height, scale, phase)?;
                    }
                }
                PlatformEvent::Key {
                    key: Key::Escape,
                    pressed: true,
                }
                | PlatformEvent::Close => {
                    backend.destroy_surface(menu)?;
                    backend.flush()?;
                    return Ok(());
                }
                PlatformEvent::Key { pressed: true, .. } => {
                    phase = phase.wrapping_add(19);
                    if let Some((width, height, scale)) = geometry {
                        draw_and_present(&mut backend, menu, width, height, scale, phase)?;
                    }
                }
                _ => {}
            }
        }
    }
}

fn draw_and_present(
    backend: &mut WaylandBackend,
    surface: SurfaceId,
    logical_width: u32,
    logical_height: u32,
    scale: i32,
    phase: u8,
) -> Result<(), BackendError> {
    let scale = scale.max(1) as u32;
    let width = logical_width.saturating_mul(scale);
    let height = logical_height.saturating_mul(scale);
    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .and_then(|count| count.checked_mul(4))
        .ok_or_else(|| BackendError::InvalidFrame("demo frame size overflow".to_owned()))?;
    let mut pixels = vec![0u8; pixel_count];

    for y in 0..height {
        for x in 0..width {
            let offset = ((y as usize * width as usize) + x as usize) * 4;
            let checker = (((x / (16 * scale)) + (y / (16 * scale))) & 1) as u8;
            pixels[offset] = phase.wrapping_add(if checker == 0 { 32 } else { 96 });
            pixels[offset + 1] = 40u8.wrapping_add((x % 128) as u8);
            pixels[offset + 2] = 120u8.wrapping_add((y % 96) as u8);
            pixels[offset + 3] = 0xff;
        }
    }

    match backend.present(
        surface,
        Frame::packed(width, height, PixelFormat::Argb8888, &pixels),
    ) {
        Err(BackendError::WouldBlock) => Ok(()),
        other => other,
    }
}
