//! Deterministic viewer scene: Cairo only, no provider invocations or EGL.
use crate::{viewer_diagnostics::sanitize, viewer_image::Image};
use cairo::{Context, FontSlant, FontWeight, Format, ImageSurface};

pub enum Content {
    Image(Image),
    Diagnostic(String),
}

pub struct Scene {
    pub content: Content,
    pub name: String,
}

pub fn close_hit(x: f64, y: f64, width: u32, height: u32) -> bool {
    if !x.is_finite() || !y.is_finite() {
        return false;
    }
    let w = f64::from(width);
    let h = f64::from(height);
    (x >= w - 76.0 && x < w && (0.0..64.0).contains(&y))
        || ((0.0..=128.0).contains(&x) && y >= h - 64.0 && y < h)
}

fn rect(ctx: &Context, x: f64, y: f64, w: f64, h: f64, rgb: (f64, f64, f64)) -> Result<(), String> {
    ctx.set_source_rgb(rgb.0, rgb.1, rgb.2);
    ctx.rectangle(x, y, w, h);
    ctx.fill().map_err(|e| e.to_string())
}
fn label(ctx: &Context, text: &str, x: f64, y: f64, size: f64, muted: bool) -> Result<(), String> {
    ctx.select_font_face("Sans", FontSlant::Normal, FontWeight::Normal);
    ctx.set_font_size(size);
    if muted {
        ctx.set_source_rgb(0.65, 0.70, 0.75);
    } else {
        ctx.set_source_rgb(0.92, 0.94, 0.97);
    }
    ctx.move_to(x, y);
    ctx.show_text(text).map_err(|e| e.to_string())
}
fn lines(text: &str, max_columns: usize, max_lines: usize) -> Vec<String> {
    let mut result = Vec::new();
    for line in sanitize(text).split('\n') {
        let chars = line.chars().collect::<Vec<_>>();
        if chars.is_empty() {
            result.push(String::new());
        } else {
            for chunk in chars.chunks(max_columns.max(1)) {
                result.push(chunk.iter().collect());
                if result.len() >= max_lines {
                    return result;
                }
            }
        }
        if result.len() >= max_lines {
            break;
        }
    }
    result
}

/// Return real Cairo ARGB32 frame bytes, stride and physical dimensions.
/// A 32-MiB framebuffer ceiling protects small ARMv7 devices.
pub fn render(
    scene: &Scene,
    width: u32,
    height: u32,
    scale: i32,
) -> Result<(Vec<u8>, usize, u32, u32), String> {
    if width < 160 || height < 140 || !(1..=3).contains(&scale) {
        return Err("invalid viewer output geometry".into());
    }
    let w = width.checked_mul(scale as u32).ok_or("frame overflow")?;
    let h = height.checked_mul(scale as u32).ok_or("frame overflow")?;
    if u64::from(w) * u64::from(h) * 4 > 32 * 1024 * 1024 {
        return Err("viewer frame exceeds 32 MiB".into());
    }
    let mut surface =
        ImageSurface::create(Format::ARgb32, w as i32, h as i32).map_err(|e| e.to_string())?;
    let ctx = Context::new(&surface).map_err(|e| e.to_string())?;
    ctx.scale(f64::from(scale), f64::from(scale));
    rect(
        &ctx,
        0.0,
        0.0,
        f64::from(width),
        f64::from(height),
        (0.043, 0.051, 0.063),
    )?;
    rect(
        &ctx,
        0.0,
        0.0,
        f64::from(width),
        68.0,
        (0.085, 0.098, 0.119),
    )?;
    label(&ctx, "Viewer", 20.0, 28.0, 20.0, false)?;
    let name = lines(&scene.name, (width.saturating_sub(118) / 8) as usize, 1)
        .pop()
        .unwrap_or_default();
    label(&ctx, &name, 20.0, 51.0, 13.0, true)?;
    rect(
        &ctx,
        f64::from(width) - 64.0,
        12.0,
        52.0,
        44.0,
        (0.20, 0.23, 0.28),
    )?;
    label(&ctx, "X", f64::from(width) - 45.0, 41.0, 20.0, false)?;
    rect(
        &ctx,
        12.0,
        f64::from(height) - 56.0,
        106.0,
        44.0,
        (0.20, 0.23, 0.28),
    )?;
    label(&ctx, "Back", 36.0, f64::from(height) - 27.0, 17.0, false)?;
    match &scene.content {
        Content::Image(image) => {
            label(
                &ctx,
                &format!("{}  {} x {}", image.format, image.width, image.height),
                136.0,
                f64::from(height) - 28.0,
                14.0,
                true,
            )?;
            let left = 20.0;
            let top = 84.0;
            let avail_w = (f64::from(width) - 40.0).max(16.0);
            let avail_h = (f64::from(height) - 166.0).max(16.0);
            let iw = f64::from(image.width);
            let ih = f64::from(image.height);
            // The image may be arbitrarily tall and narrow; a minimum zoom would
            // spill into the toolbar at SL101-small configurations.
            let factor = (avail_w / iw).min(avail_h / ih).min(16.0);
            let dw = iw * factor;
            let dh = ih * factor;
            let x = left + (avail_w - dw) / 2.0;
            let y = top + (avail_h - dh) / 2.0;
            rect(&ctx, left, top, avail_w, avail_h, (0.075, 0.084, 0.103))?;
            let source = ImageSurface::create_for_data(
                image.argb.clone(),
                Format::ARgb32,
                image.width as i32,
                image.height as i32,
                (image.width * 4) as i32,
            )
            .map_err(|e| e.to_string())?;
            ctx.save().map_err(|e| e.to_string())?;
            ctx.rectangle(left, top, avail_w, avail_h);
            ctx.clip();
            ctx.translate(x, y);
            ctx.scale(factor, factor);
            ctx.set_source_surface(&source, 0.0, 0.0)
                .map_err(|e| e.to_string())?;
            ctx.source().set_filter(cairo::Filter::Bilinear);
            ctx.paint().map_err(|e| e.to_string())?;
            ctx.restore().map_err(|e| e.to_string())?;
        }
        Content::Diagnostic(info) => {
            rect(
                &ctx,
                12.0,
                78.0,
                (f64::from(width) - 24.0).max(1.0),
                (f64::from(height) - 148.0).max(1.0),
                (0.073, 0.083, 0.101),
            )?;
            label(&ctx, "File description", 24.0, 107.0, 19.0, false)?;
            let max_cols = ((width.saturating_sub(48) / 8) as usize).clamp(8, 160);
            let max_lines = (height.saturating_sub(156) / 19) as usize;
            for (idx, text) in lines(info, max_cols, max_lines).iter().enumerate() {
                label(&ctx, text, 24.0, 132.0 + idx as f64 * 19.0, 13.0, true)?;
            }
        }
    }
    drop(ctx);
    surface.flush();
    let stride = surface.stride() as usize;
    let pixels = surface.data().map_err(|e| e.to_string())?.to_vec();
    Ok((pixels, stride, w, h))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_cairo_image_pixels_and_letterbox() {
        let image = Image {
            width: 2,
            height: 1,
            argb: vec![0, 0, 255, 255, 0, 255, 0, 255],
            format: "PNG",
        };
        let scene = Scene {
            content: Content::Image(image),
            name: "a.png".into(),
        };
        let (buffer, stride, w, h) = render(&scene, 320, 240, 1).unwrap();
        assert_eq!((w, h), (320, 240));
        // The colored central pixels are drawn from actual ARGB source, not a placeholder.
        let center = &buffer[(120 * stride + 150 * 4)..][..4];
        assert!(center[2] > 100 || center[1] > 100);
        let outer = &buffer[(90 * stride + 25 * 4)..][..4];
        assert_ne!(outer, center);
    }
    #[test]
    fn extreme_aspect_ratio_cannot_paint_outside_image_area() {
        let image = Image {
            width: 1,
            height: 4096,
            argb: vec![0, 0, 255, 255].repeat(4096),
            format: "PNG",
        };
        let scene = Scene {
            content: Content::Image(image),
            name: "very-tall.png".into(),
        };
        let (pixels, stride, ..) = render(&scene, 160, 160, 1).unwrap();
        // The picture viewport ends at y=100 (top 84 + available 16).
        // Red must not bleed into the footer after aspect-ratio fit/clipping.
        for y in 102..155 {
            let rgb = &pixels[y * stride + 80 * 4..y * stride + 80 * 4 + 3];
            assert!(rgb[2] < 100, "red image escaped clipping at y={y}: {rgb:?}");
        }
    }
    #[test]
    fn diagnostic_scene_nonblank_and_hit_regions() {
        let scene = Scene {
            content: Content::Diagnostic("ASCII text\nfailed to read".into()),
            name: "x.txt".into(),
        };
        let (pixels, _, _, _) = render(&scene, 320, 240, 1).unwrap();
        assert!(pixels.iter().any(|byte| *byte != 0));
        assert!(close_hit(311.0, 30.0, 320, 240));
        assert!(close_hit(50.0, 218.0, 320, 240));
        assert!(!close_hit(155.0, 120.0, 320, 240));
    }
}
