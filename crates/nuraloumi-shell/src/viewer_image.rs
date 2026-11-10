//! Decode only bounded PNG/JPEG into native-endian premultiplied Cairo ARGB32.
use std::{
    fs::OpenOptions,
    io::{Cursor, Read},
    path::Path,
};
pub const MAX_FILE_BYTES: u64 = 12 * 1024 * 1024;
pub const MAX_DIMENSION: u32 = 4096;
pub const MAX_PIXELS: u64 = 4_000_000;

#[derive(Debug, Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub argb: Vec<u8>,
    pub format: &'static str,
}

fn dimensions(width: u32, height: u32) -> Result<usize, String> {
    let pixels = u64::from(width) * u64::from(height);
    if width == 0
        || height == 0
        || width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || pixels > MAX_PIXELS
    {
        return Err("image dimensions/pixel count exceed viewer limits".into());
    }
    usize::try_from(pixels).map_err(|_| "pixel count overflow".into())
}

fn argb(
    rgb: &[u8],
    width: u32,
    height: u32,
    channels: usize,
    format: &'static str,
) -> Result<Image, String> {
    let pixels = dimensions(width, height)?;
    if !matches!(channels, 1..=4)
        || rgb.len()
            != pixels
                .checked_mul(channels)
                .ok_or("decoded image overflow")?
    {
        return Err("invalid decoded image length".into());
    }
    let mut data = vec![0u8; pixels * 4];
    for (src, dest) in rgb.chunks_exact(channels).zip(data.chunks_exact_mut(4)) {
        let (r, g, b, a) = match channels {
            1 => (src[0], src[0], src[0], 255),
            2 => (src[0], src[0], src[0], src[1]),
            3 => (src[0], src[1], src[2], 255),
            4 => (src[0], src[1], src[2], src[3]),
            _ => unreachable!(),
        };
        let premul = |c: u8| ((u16::from(c) * u16::from(a) + 127) / 255) as u8;
        // Both SL101 ARMv7 and development x86_64 are little-endian.
        dest.copy_from_slice(&[premul(b), premul(g), premul(r), a]);
    }
    Ok(Image {
        width,
        height,
        argb: data,
        format,
    })
}

pub fn load(path: &Path) -> Result<Image, String> {
    // O_NONBLOCK prevents a raced-in FIFO/device from blocking the UI on open.
    // A regular file ignores this flag. Only Linux is deployed on the SL101.
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let mut input = options
        .open(path)
        .map_err(|e| format!("cannot open image: {e}"))?;
    let metadata = input
        .metadata()
        .map_err(|e| format!("cannot stat image: {e}"))?;
    if !metadata.is_file() {
        return Err("not a regular image file".into());
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err("image input exceeds 12 MiB".into());
    }
    let mut bytes = Vec::new();
    input
        .by_ref()
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read image: {e}"))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err("image input exceeds 12 MiB".into());
    }
    decode(&bytes)
}

pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let mut d = png::Decoder::new(Cursor::new(bytes));
        d.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = d
            .read_info()
            .map_err(|e| format!("invalid PNG header: {e}"))?;
        dimensions(reader.info().width, reader.info().height)?;
        let size = reader.output_buffer_size();
        if size > MAX_PIXELS as usize * 4 {
            return Err("PNG output exceeds limit".into());
        }
        let mut raw = vec![0u8; size];
        let frame = reader
            .next_frame(&mut raw)
            .map_err(|e| format!("invalid PNG payload: {e}"))?;
        let channels = match frame.color_type {
            png::ColorType::Grayscale => 1,
            png::ColorType::GrayscaleAlpha => 2,
            png::ColorType::Rgb => 3,
            png::ColorType::Rgba => 4,
            _ => return Err("unsupported PNG output color type".into()),
        };
        argb(
            &raw[..frame.buffer_size()],
            frame.width,
            frame.height,
            channels,
            "PNG",
        )
    } else if bytes.starts_with(&[0xff, 0xd8]) {
        let mut d = jpeg_decoder::Decoder::new(Cursor::new(bytes));
        d.read_info()
            .map_err(|e| format!("invalid JPEG header: {e}"))?;
        let info = d.info().ok_or("JPEG dimensions unavailable")?;
        dimensions(u32::from(info.width), u32::from(info.height))?;
        let channels = match info.pixel_format {
            jpeg_decoder::PixelFormat::L8 => 1,
            jpeg_decoder::PixelFormat::RGB24 => 3,
            _ => return Err("unsupported JPEG color type".into()),
        };
        let raw = d
            .decode()
            .map_err(|e| format!("invalid JPEG payload: {e}"))?;
        argb(
            &raw,
            u32::from(info.width),
            u32::from(info.height),
            channels,
            "JPEG",
        )
    } else {
        Err("not a PNG or JPEG image".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deceptive_extension_is_not_an_image() {
        assert!(decode(b"GIF89a").is_err());
        assert!(decode(b"hi there").is_err());
    }
    #[test]
    fn malformed_and_absurd_dimensions_are_rejected() {
        assert!(decode(b"\x89PNG\r\n\x1a\nINVALID").is_err());
        assert!(decode(&[0xff, 0xd8, 0xff]).is_err());
        assert!(dimensions(65535, 65535).is_err());
        assert!(dimensions(0, 2).is_err());
    }
    #[test]
    fn actual_rgba_png_roundtrip() {
        let mut encoded = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut encoded, 2, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&[255, 0, 0, 255, 0, 255, 0, 255])
                .unwrap();
        }
        let decoded = decode(&encoded).unwrap();
        assert_eq!(
            (decoded.width, decoded.height, decoded.format),
            (2, 1, "PNG")
        );
        assert_eq!(decoded.argb, [0, 0, 255, 255, 0, 255, 0, 255]);
    }
    #[test]
    fn actual_jpeg_pixels() {
        let rgb = [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
        let mut jpeg = Vec::new();
        jpeg_encoder::Encoder::new(&mut jpeg, 95)
            .encode(&rgb, 2, 2, jpeg_encoder::ColorType::Rgb)
            .unwrap();
        let image = decode(&jpeg).unwrap();
        assert_eq!((image.width, image.height, image.format), (2, 2, "JPEG"));
        assert_eq!(image.argb.len(), 16);
        assert!(image
            .argb
            .chunks_exact(4)
            .any(|px| px[2] > px[1].saturating_add(30)));
    }
    #[test]
    fn rgba_premultiplication() {
        let image = argb(&[200, 100, 50, 128], 1, 1, 4, "test").unwrap();
        assert_eq!(image.argb, [25, 50, 100, 128]);
    }
}
