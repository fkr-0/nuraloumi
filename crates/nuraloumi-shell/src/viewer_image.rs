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

// EXIF is an untrusted TIFF payload inside JPEG APP1. Only orientation is
// needed: no external metadata parser, recursive IFD traversal or allocations.
fn tiff_u16(input: &[u8], offset: usize, little: bool) -> Option<u16> {
    let bytes: [u8; 2] = input.get(offset..offset.checked_add(2)?)?.try_into().ok()?;
    Some(if little {
        u16::from_le_bytes(bytes)
    } else {
        u16::from_be_bytes(bytes)
    })
}
fn tiff_u32(input: &[u8], offset: usize, little: bool) -> Option<u32> {
    let bytes: [u8; 4] = input.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(if little {
        u32::from_le_bytes(bytes)
    } else {
        u32::from_be_bytes(bytes)
    })
}
fn tiff_orientation(tiff: &[u8]) -> Option<u8> {
    let little = match tiff.get(..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    if tiff_u16(tiff, 2, little)? != 42 {
        return None;
    }
    let offset = usize::try_from(tiff_u32(tiff, 4, little)?).ok()?;
    let count = usize::from(tiff_u16(tiff, offset, little)?);
    // Each IFD entry is 12 bytes. Validate the entire range once: the
    // enclosing JPEG APP1 segment already bounds even an unusually large IFD.
    let entries_start = offset.checked_add(2)?;
    let entries_end = entries_start.checked_add(count.checked_mul(12)?)?;
    tiff.get(entries_start..entries_end)?;
    for idx in 0..count {
        let entry = offset.checked_add(2)?.checked_add(idx.checked_mul(12)?)?;
        // TIFF IFD entries consist of tag, type, count, and inline value/offset.
        tiff.get(entry..entry.checked_add(12)?)?;
        if tiff_u16(tiff, entry, little)? != 0x0112 {
            continue;
        }
        if tiff_u16(tiff, entry + 2, little)? != 3 || tiff_u32(tiff, entry + 4, little)? != 1 {
            return None;
        }
        return u8::try_from(tiff_u16(tiff, entry + 8, little)?)
            .ok()
            .filter(|orientation| (1..=8).contains(orientation));
    }
    None
}

fn jpeg_orientation(input: &[u8]) -> u8 {
    if !input.starts_with(&[0xff, 0xd8]) {
        return 1;
    }
    let mut offset = 2usize;
    while offset < input.len() {
        if input[offset] != 0xff {
            break;
        }
        while input.get(offset) == Some(&0xff) {
            offset += 1;
        }
        let Some(&marker) = input.get(offset) else {
            break;
        };
        offset += 1;
        if marker == 0xd9 || marker == 0xda {
            break;
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        let Some(length) = input.get(offset..offset.saturating_add(2)) else {
            break;
        };
        let length = usize::from(u16::from_be_bytes([length[0], length[1]]));
        if length < 2 {
            break;
        }
        let Some(end) = offset.checked_add(length) else {
            break;
        };
        let Some(payload) = input.get(offset + 2..end) else {
            break;
        };
        if marker == 0xe1 && payload.starts_with(b"Exif\0\0") {
            if let Some(orientation) = tiff_orientation(&payload[6..]) {
                return orientation;
            }
        }
        offset = end;
    }
    1
}

fn argb_oriented(
    rgb: &[u8],
    source_width: u32,
    source_height: u32,
    channels: usize,
    format: &'static str,
    orientation: u8,
) -> Result<Image, String> {
    let pixels = dimensions(source_width, source_height)?;
    if !matches!(channels, 1..=4)
        || rgb.len()
            != pixels
                .checked_mul(channels)
                .ok_or("decoded image overflow")?
    {
        return Err("invalid decoded image length".into());
    }
    let (width, height) = if (5..=8).contains(&orientation) {
        (source_height, source_width)
    } else {
        (source_width, source_height)
    };
    dimensions(width, height)?;
    let mut data = vec![0u8; pixels * 4];
    for (index, source) in rgb.chunks_exact(channels).enumerate() {
        let sx = index as u32 % source_width;
        let sy = index as u32 / source_width;
        let (dx, dy) = match orientation {
            2 => (source_width - 1 - sx, sy),
            3 => (source_width - 1 - sx, source_height - 1 - sy),
            4 => (sx, source_height - 1 - sy),
            5 => (sy, sx),
            6 => (source_height - 1 - sy, sx),
            7 => (source_height - 1 - sy, source_width - 1 - sx),
            8 => (sy, source_width - 1 - sx),
            _ => (sx, sy),
        };
        let dst = &mut data[(dy as usize * width as usize + dx as usize) * 4..][..4];
        let (r, g, b, a) = match channels {
            1 => (source[0], source[0], source[0], 255),
            2 => (source[0], source[0], source[0], source[1]),
            3 => (source[0], source[1], source[2], 255),
            4 => (source[0], source[1], source[2], source[3]),
            _ => unreachable!(),
        };
        let premul = |c: u8| ((u16::from(c) * u16::from(a) + 127) / 255) as u8;
        // Linux ARMv7 and x86_64: Cairo ARgb32 bytes are BGRA (premultiplied).
        dst.copy_from_slice(&[premul(b), premul(g), premul(r), a]);
    }
    Ok(Image {
        width,
        height,
        argb: data,
        format,
    })
}

fn argb(
    rgb: &[u8],
    width: u32,
    height: u32,
    channels: usize,
    format: &'static str,
) -> Result<Image, String> {
    argb_oriented(rgb, width, height, channels, format, 1)
}

pub fn load(path: &Path) -> Result<Image, String> {
    // O_NONBLOCK prevents a raced-in FIFO/device from blocking the UI on open.
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
        argb_oriented(
            &raw,
            u32::from(info.width),
            u32::from(info.height),
            channels,
            "JPEG",
            jpeg_orientation(bytes),
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
        assert_eq!(jpeg_orientation(b"\xff\xd8\xff\xe1\xff\xffExif\0\0"), 1);
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
    fn all_eight_exif_orientations_transform_pixels() {
        // Source matrix: 1 2 / 3 4 / 5 6. The R channel encodes its 1-based index.
        let rgb = [10, 0, 0, 20, 0, 0, 30, 0, 0, 40, 0, 0, 50, 0, 0, 60, 0, 0];
        let fixtures: &[(u8, (u32, u32), [u8; 6])] = &[
            (1, (2, 3), [10, 20, 30, 40, 50, 60]),
            (2, (2, 3), [20, 10, 40, 30, 60, 50]),
            (3, (2, 3), [60, 50, 40, 30, 20, 10]),
            (4, (2, 3), [50, 60, 30, 40, 10, 20]),
            (5, (3, 2), [10, 30, 50, 20, 40, 60]),
            (6, (3, 2), [50, 30, 10, 60, 40, 20]),
            (7, (3, 2), [60, 40, 20, 50, 30, 10]),
            (8, (3, 2), [20, 40, 60, 10, 30, 50]),
        ];
        for &(orientation, size, expected) in fixtures {
            let image = argb_oriented(&rgb, 2, 3, 3, "JPEG", orientation).unwrap();
            assert_eq!(
                (image.width, image.height),
                size,
                "orientation={orientation}"
            );
            let actual: Vec<u8> = image.argb.chunks_exact(4).map(|pixel| pixel[2]).collect();
            assert_eq!(&actual, &expected, "orientation={orientation}");
        }
    }
    #[test]
    fn big_endian_exif_and_truncated_ifd_are_safe() {
        let big = b"MM\x00\x2a\x00\x00\x00\x08\x00\x01\x01\x12\x00\x03\x00\x00\x00\x01\x00\x08\x00\x00\x00\x00\x00\x00";
        assert_eq!(tiff_orientation(big), Some(8));
        assert_eq!(tiff_orientation(&big[..16]), None);
        let mut malicious = big.to_vec();
        malicious[4..8].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(tiff_orientation(&malicious), None);
    }
    #[test]
    fn actual_jpeg_with_exif_app1_rotates_on_decode() {
        let rgb = [
            255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 50, 80, 90, 100, 120, 130,
        ];
        let mut encoded = Vec::new();
        jpeg_encoder::Encoder::new(&mut encoded, 95)
            .encode(&rgb, 2, 3, jpeg_encoder::ColorType::Rgb)
            .unwrap();
        let mut exif =
            b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0".to_vec();
        assert_eq!(exif.len(), 32);
        let mut jpeg = encoded[..2].to_vec();
        jpeg.extend_from_slice(&[0xff, 0xe1]);
        jpeg.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
        jpeg.append(&mut exif);
        jpeg.extend_from_slice(&encoded[2..]);
        assert_eq!(jpeg_orientation(&jpeg), 6);
        let oriented = decode(&jpeg).unwrap();
        assert_eq!((oriented.width, oriented.height), (3, 2));
        assert_eq!(oriented.argb.len(), 24);
    }
    #[test]
    fn rgba_premultiplication() {
        let image = argb(&[200, 100, 50, 128], 1, 1, 4, "test").unwrap();
        assert_eq!(image.argb, [25, 50, 100, 128]);
    }
}
