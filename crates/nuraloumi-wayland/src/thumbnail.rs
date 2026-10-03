use std::{
    collections::HashMap,
    fs::File,
    os::fd::AsFd,
    time::{Duration, Instant},
};

use memmap2::{MmapMut, MmapOptions};
use rustix::{
    event::{poll, PollFd, PollFlags, Timespec},
    fs::{ftruncate, memfd_create, MemfdFlags},
};
use wayland_client::{
    delegate_noop, event_created_child,
    protocol::{wl_buffer, wl_callback, wl_registry, wl_shm, wl_shm_pool},
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum,
};
use wayland_protocols::ext::{
    foreign_toplevel_list::v1::client::{
        ext_foreign_toplevel_handle_v1::{self, ExtForeignToplevelHandleV1},
        ext_foreign_toplevel_list_v1::{self, ExtForeignToplevelListV1},
    },
    image_capture_source::v1::client::{
        ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1,
        ext_image_capture_source_v1::ExtImageCaptureSourceV1,
    },
    image_copy_capture::v1::client::{
        ext_image_copy_capture_frame_v1::{self, ExtImageCopyCaptureFrameV1},
        ext_image_copy_capture_manager_v1::{ExtImageCopyCaptureManagerV1, Options},
        ext_image_copy_capture_session_v1::{self, ExtImageCopyCaptureSessionV1},
    },
};

const DEFAULT_TIMEOUT: Duration = Duration::from_millis(900);
const BYTES_PER_PIXEL: u64 = 4;
const MAX_SOURCE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_THUMBNAIL_EDGE: u32 = 224;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToplevelThumbnailRequest {
    pub key: String,
    pub title: String,
    pub app_id: Option<String>,
    pub protocol_identifier: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToplevelThumbnail {
    pub key: String,
    pub width: u32,
    pub height: u32,
    /// Native-endian premultiplied ARGB32 pixels, packed width * 4.
    pub pixels: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ToplevelThumbnailCapabilities {
    pub ext_foreign_toplevel_list: bool,
    pub foreign_toplevel_capture_source: bool,
    pub image_copy_capture: bool,
    pub shm: bool,
}

impl ToplevelThumbnailCapabilities {
    pub const fn available(self) -> bool {
        self.ext_foreign_toplevel_list
            && self.foreign_toplevel_capture_source
            && self.image_copy_capture
            && self.shm
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ToplevelThumbnailReport {
    pub capabilities: ToplevelThumbnailCapabilities,
    pub thumbnails: Vec<ToplevelThumbnail>,
    pub issues: Vec<String>,
}

#[allow(dead_code)]
pub mod helper_wire {
    use super::*;

    const HELPER_REQUEST_MAGIC: &[u8; 8] = b"NLTHRQ01";
    const HELPER_RESPONSE_MAGIC: &[u8; 8] = b"NLTHRS01";
    const MAX_HELPER_REQUESTS: usize = 4;
    const MAX_HELPER_ISSUES: usize = 16;
    const MAX_HELPER_STRING_BYTES: usize = 4096;

    pub fn encode_thumbnail_helper_request(
        requests: &[ToplevelThumbnailRequest],
    ) -> Result<Vec<u8>, String> {
        if requests.len() > MAX_HELPER_REQUESTS {
            return Err(format!(
                "thumbnail helper accepts at most {MAX_HELPER_REQUESTS} requests"
            ));
        }
        let mut out = Vec::new();
        out.extend_from_slice(HELPER_REQUEST_MAGIC);
        push_u32(&mut out, requests.len())?;
        for request in requests {
            push_string(&mut out, &request.key)?;
            push_string(&mut out, &request.title)?;
            push_optional_string(&mut out, request.app_id.as_deref())?;
            push_optional_string(&mut out, request.protocol_identifier.as_deref())?;
        }
        Ok(out)
    }

    pub fn decode_thumbnail_helper_request(
        bytes: &[u8],
    ) -> Result<Vec<ToplevelThumbnailRequest>, String> {
        let mut cursor = HelperCursor::new(bytes);
        cursor.expect_magic(HELPER_REQUEST_MAGIC)?;
        let count = cursor.read_u32()? as usize;
        if count > MAX_HELPER_REQUESTS {
            return Err(format!(
                "thumbnail helper request count {count} exceeds {MAX_HELPER_REQUESTS}"
            ));
        }
        let mut requests = Vec::with_capacity(count);
        for _ in 0..count {
            requests.push(ToplevelThumbnailRequest {
                key: cursor.read_string()?,
                title: cursor.read_string()?,
                app_id: cursor.read_optional_string()?,
                protocol_identifier: cursor.read_optional_string()?,
            });
        }
        cursor.expect_end()?;
        Ok(requests)
    }

    pub fn encode_thumbnail_helper_report(
        report: &ToplevelThumbnailReport,
    ) -> Result<Vec<u8>, String> {
        if report.issues.len() > MAX_HELPER_ISSUES {
            return Err(format!(
                "thumbnail helper issue count {} exceeds {MAX_HELPER_ISSUES}",
                report.issues.len()
            ));
        }
        if report.thumbnails.len() > MAX_HELPER_REQUESTS {
            return Err(format!(
                "thumbnail helper thumbnail count {} exceeds {MAX_HELPER_REQUESTS}",
                report.thumbnails.len()
            ));
        }
        let mut out = Vec::new();
        out.extend_from_slice(HELPER_RESPONSE_MAGIC);
        let capabilities = report.capabilities;
        let mask = u8::from(capabilities.ext_foreign_toplevel_list)
            | (u8::from(capabilities.foreign_toplevel_capture_source) << 1)
            | (u8::from(capabilities.image_copy_capture) << 2)
            | (u8::from(capabilities.shm) << 3);
        out.push(mask);
        push_u32(&mut out, report.issues.len())?;
        for issue in &report.issues {
            push_string(&mut out, issue)?;
        }
        push_u32(&mut out, report.thumbnails.len())?;
        for thumbnail in &report.thumbnails {
            push_string(&mut out, &thumbnail.key)?;
            out.extend_from_slice(&thumbnail.width.to_le_bytes());
            out.extend_from_slice(&thumbnail.height.to_le_bytes());
            push_u32(&mut out, thumbnail.pixels.len())?;
            out.extend_from_slice(&thumbnail.pixels);
        }
        Ok(out)
    }

    pub fn decode_thumbnail_helper_report(bytes: &[u8]) -> Result<ToplevelThumbnailReport, String> {
        let mut cursor = HelperCursor::new(bytes);
        cursor.expect_magic(HELPER_RESPONSE_MAGIC)?;
        let mask = cursor.read_u8()?;
        let capabilities = ToplevelThumbnailCapabilities {
            ext_foreign_toplevel_list: mask & 1 != 0,
            foreign_toplevel_capture_source: mask & 2 != 0,
            image_copy_capture: mask & 4 != 0,
            shm: mask & 8 != 0,
        };
        let issue_count = cursor.read_u32()? as usize;
        if issue_count > MAX_HELPER_ISSUES {
            return Err(format!(
                "thumbnail helper issue count {issue_count} exceeds {MAX_HELPER_ISSUES}"
            ));
        }
        let mut issues = Vec::with_capacity(issue_count);
        for _ in 0..issue_count {
            issues.push(cursor.read_string()?);
        }
        let thumbnail_count = cursor.read_u32()? as usize;
        if thumbnail_count > MAX_HELPER_REQUESTS {
            return Err(format!(
                "thumbnail helper thumbnail count {thumbnail_count} exceeds {MAX_HELPER_REQUESTS}"
            ));
        }
        let mut thumbnails = Vec::with_capacity(thumbnail_count);
        for _ in 0..thumbnail_count {
            let key = cursor.read_string()?;
            let width = cursor.read_u32()?;
            let height = cursor.read_u32()?;
            let pixel_len = cursor.read_u32()? as usize;
            if pixel_len > MAX_SOURCE_BYTES as usize {
                return Err(format!(
                    "thumbnail helper pixel payload {pixel_len} exceeds {MAX_SOURCE_BYTES} bytes"
                ));
            }
            let pixels = cursor.read_bytes(pixel_len)?.to_vec();
            thumbnails.push(ToplevelThumbnail {
                key,
                width,
                height,
                pixels,
            });
        }
        cursor.expect_end()?;
        Ok(ToplevelThumbnailReport {
            capabilities,
            thumbnails,
            issues,
        })
    }

    fn push_u32(out: &mut Vec<u8>, value: usize) -> Result<(), String> {
        let value = u32::try_from(value).map_err(|_| "thumbnail helper length exceeds u32")?;
        out.extend_from_slice(&value.to_le_bytes());
        Ok(())
    }

    fn push_string(out: &mut Vec<u8>, value: &str) -> Result<(), String> {
        if value.len() > MAX_HELPER_STRING_BYTES {
            return Err(format!(
                "thumbnail helper string is {} bytes, limit is {MAX_HELPER_STRING_BYTES}",
                value.len()
            ));
        }
        push_u32(out, value.len())?;
        out.extend_from_slice(value.as_bytes());
        Ok(())
    }

    fn push_optional_string(out: &mut Vec<u8>, value: Option<&str>) -> Result<(), String> {
        match value {
            Some(value) => push_string(out, value),
            None => {
                out.extend_from_slice(&u32::MAX.to_le_bytes());
                Ok(())
            }
        }
    }

    struct HelperCursor<'a> {
        bytes: &'a [u8],
        offset: usize,
    }

    impl<'a> HelperCursor<'a> {
        fn new(bytes: &'a [u8]) -> Self {
            Self { bytes, offset: 0 }
        }

        fn expect_magic(&mut self, expected: &[u8; 8]) -> Result<(), String> {
            if self.read_bytes(expected.len())? != expected {
                return Err("thumbnail helper wire magic mismatch".to_owned());
            }
            Ok(())
        }

        fn read_u8(&mut self) -> Result<u8, String> {
            let byte = *self
                .read_bytes(1)?
                .first()
                .ok_or_else(|| "thumbnail helper wire truncated".to_owned())?;
            Ok(byte)
        }

        fn read_u32(&mut self) -> Result<u32, String> {
            let bytes: [u8; 4] = self
                .read_bytes(4)?
                .try_into()
                .map_err(|_| "thumbnail helper u32 decode failed")?;
            Ok(u32::from_le_bytes(bytes))
        }

        fn read_string(&mut self) -> Result<String, String> {
            let len = self.read_u32()? as usize;
            if len > MAX_HELPER_STRING_BYTES {
                return Err(format!(
                    "thumbnail helper string length {len} exceeds {MAX_HELPER_STRING_BYTES}"
                ));
            }
            let bytes = self.read_bytes(len)?;
            String::from_utf8(bytes.to_vec())
                .map_err(|_| "thumbnail helper string is not UTF-8".to_owned())
        }

        fn read_optional_string(&mut self) -> Result<Option<String>, String> {
            let len = self.read_u32()?;
            if len == u32::MAX {
                return Ok(None);
            }
            let len = len as usize;
            if len > MAX_HELPER_STRING_BYTES {
                return Err(format!(
                "thumbnail helper optional string length {len} exceeds {MAX_HELPER_STRING_BYTES}"
            ));
            }
            let bytes = self.read_bytes(len)?;
            String::from_utf8(bytes.to_vec())
                .map(Some)
                .map_err(|_| "thumbnail helper optional string is not UTF-8".to_owned())
        }

        fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], String> {
            let end = self
                .offset
                .checked_add(len)
                .ok_or_else(|| "thumbnail helper wire length overflow".to_owned())?;
            let bytes = self
                .bytes
                .get(self.offset..end)
                .ok_or_else(|| "thumbnail helper wire truncated".to_owned())?;
            self.offset = end;
            Ok(bytes)
        }

        fn expect_end(&self) -> Result<(), String> {
            if self.offset != self.bytes.len() {
                return Err("thumbnail helper wire has trailing bytes".to_owned());
            }
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Default)]
struct ListedToplevel {
    handle: Option<ExtForeignToplevelHandleV1>,
    title: String,
    app_id: Option<String>,
    identifier: Option<String>,
    committed: bool,
    closed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PresentationIdentity {
    title: String,
    app_id: Option<String>,
    identifier: Option<String>,
}

impl From<&ListedToplevel> for PresentationIdentity {
    fn from(value: &ListedToplevel) -> Self {
        Self {
            title: value.title.clone(),
            app_id: value.app_id.clone(),
            identifier: value.identifier.clone(),
        }
    }
}

#[derive(Clone, Debug, Default)]
struct CaptureProgress {
    width: u32,
    height: u32,
    shm_formats: Vec<wl_shm::Format>,
    constraints_done: bool,
    stopped: bool,
    frame_done: bool,
    frame_ready: bool,
    frame_failure: Option<String>,
}

#[derive(Default)]
struct CaptureState {
    shm: Option<wl_shm::WlShm>,
    toplevel_list: Option<ExtForeignToplevelListV1>,
    source_manager: Option<ExtForeignToplevelImageCaptureSourceManagerV1>,
    capture_manager: Option<ExtImageCopyCaptureManagerV1>,
    toplevels: HashMap<wayland_client::backend::ObjectId, ListedToplevel>,
    progress: CaptureProgress,
}

impl CaptureState {
    fn capabilities(&self) -> ToplevelThumbnailCapabilities {
        ToplevelThumbnailCapabilities {
            ext_foreign_toplevel_list: self.toplevel_list.is_some(),
            foreign_toplevel_capture_source: self.source_manager.is_some(),
            image_copy_capture: self.capture_manager.is_some(),
            shm: self.shm.is_some(),
        }
    }

    fn reset_progress(&mut self) {
        self.progress = CaptureProgress::default();
    }
}

pub fn capture_toplevel_thumbnails(
    requests: &[ToplevelThumbnailRequest],
) -> Result<ToplevelThumbnailReport, String> {
    capture_toplevel_thumbnails_with_timeout(requests, DEFAULT_TIMEOUT)
}

pub fn capture_toplevel_thumbnails_with_timeout(
    requests: &[ToplevelThumbnailRequest],
    timeout: Duration,
) -> Result<ToplevelThumbnailReport, String> {
    if requests.is_empty() {
        return Ok(ToplevelThumbnailReport::default());
    }

    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| "Wayland thumbnail timeout overflow".to_owned())?;
    let connection = Connection::connect_to_env()
        .map_err(|error| format!("Wayland thumbnail connect failed: {error}"))?;
    let mut queue = connection.new_event_queue::<CaptureState>();
    let qh = queue.handle();
    let _registry = connection.display().get_registry(&qh, ());
    let mut state = CaptureState::default();

    timed_roundtrip(&connection, &mut queue, &mut state, deadline)?;
    timed_roundtrip(&connection, &mut queue, &mut state, deadline)?;

    let capabilities = state.capabilities();
    if !capabilities.available() {
        return Ok(ToplevelThumbnailReport {
            capabilities,
            thumbnails: Vec::new(),
            issues: vec![format!(
                "per-toplevel capture unavailable: ext-list={} capture-source={} copy-capture={} shm={}",
                capabilities.ext_foreign_toplevel_list,
                capabilities.foreign_toplevel_capture_source,
                capabilities.image_copy_capture,
                capabilities.shm
            )],
        });
    }

    let mut issues = Vec::new();
    let matches = resolve_requests(&state, requests, &mut issues);
    let mut thumbnails = Vec::new();

    for (request, handle) in matches {
        if budget_exhausted(deadline, Instant::now()) {
            issues
                .push("thumbnail capture budget exhausted; remaining previews skipped".to_owned());
            break;
        }
        match capture_one(
            &connection,
            &mut queue,
            &qh,
            &mut state,
            &handle,
            &request.key,
            deadline,
        ) {
            Ok(thumbnail) => thumbnails.push(thumbnail),
            Err(error) => issues.push(format!("{}: {error}", request.key)),
        }
    }

    Ok(ToplevelThumbnailReport {
        capabilities,
        thumbnails,
        issues,
    })
}

fn resolve_presentation_index(
    listed: &[PresentationIdentity],
    request: &ToplevelThumbnailRequest,
) -> Result<usize, &'static str> {
    let matches = listed
        .iter()
        .enumerate()
        .filter(|(_, entry)| {
            if let Some(identifier) = request.protocol_identifier.as_deref() {
                entry.identifier.as_deref() == Some(identifier)
            } else {
                entry.title == request.title && entry.app_id == request.app_id
            }
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();

    match matches.as_slice() {
        [index] => Ok(*index),
        [] => Err("no exact presentation match"),
        _ => Err("ambiguous presentation match; thumbnail suppressed"),
    }
}

fn resolve_requests(
    state: &CaptureState,
    requests: &[ToplevelThumbnailRequest],
    issues: &mut Vec<String>,
) -> Vec<(ToplevelThumbnailRequest, ExtForeignToplevelHandleV1)> {
    let listed = state
        .toplevels
        .values()
        .filter(|entry| entry.committed && !entry.closed)
        .filter_map(|entry| entry.handle.as_ref().map(|handle| (entry, handle)))
        .collect::<Vec<_>>();
    let identities = listed
        .iter()
        .map(|(entry, _)| PresentationIdentity::from(*entry))
        .collect::<Vec<_>>();

    let mut resolved = Vec::new();
    for request in requests {
        match resolve_presentation_index(&identities, request) {
            Ok(index) => resolved.push((request.clone(), listed[index].1.clone())),
            Err(reason) => issues.push(format!("{}: {reason}", request.key)),
        }
    }
    resolved
}

fn capture_one(
    connection: &Connection,
    queue: &mut EventQueue<CaptureState>,
    qh: &QueueHandle<CaptureState>,
    state: &mut CaptureState,
    handle: &ExtForeignToplevelHandleV1,
    key: &str,
    deadline: Instant,
) -> Result<ToplevelThumbnail, String> {
    state.reset_progress();
    let source_manager = state
        .source_manager
        .as_ref()
        .ok_or_else(|| "foreign-toplevel capture source manager disappeared".to_owned())?
        .clone();
    let capture_manager = state
        .capture_manager
        .as_ref()
        .ok_or_else(|| "image-copy capture manager disappeared".to_owned())?
        .clone();
    let shm = state
        .shm
        .as_ref()
        .ok_or_else(|| "wl_shm disappeared".to_owned())?
        .clone();

    let source = source_manager.create_source(handle, qh, ());
    let session = capture_manager.create_session(&source, Options::empty(), qh, ());

    dispatch_until(connection, queue, state, deadline, |state| {
        state.progress.constraints_done || state.progress.stopped
    })?;

    if state.progress.stopped {
        destroy_capture_objects(None, Some(session), Some(source));
        return Err("capture session stopped before constraints".to_owned());
    }

    let width = state.progress.width;
    let height = state.progress.height;
    validate_source_size(width, height)?;
    let format = choose_shm_format(&state.progress.shm_formats)
        .ok_or_else(|| "compositor offered no ARGB8888/XRGB8888 SHM capture format".to_owned())?;

    let mut buffer = CaptureBuffer::allocate(width, height, format, &shm, qh)?;
    state.progress.frame_done = false;
    state.progress.frame_ready = false;
    state.progress.frame_failure = None;

    let frame = session.create_frame(qh, ());
    frame.attach_buffer(&buffer.buffer);
    frame.damage_buffer(0, 0, width as i32, height as i32);
    frame.capture();
    connection
        .flush()
        .map_err(|error| format!("thumbnail capture flush failed: {error}"))?;

    dispatch_until(connection, queue, state, deadline, |state| {
        state.progress.frame_done || state.progress.stopped
    })?;

    if state.progress.stopped {
        destroy_capture_objects(Some(frame), Some(session), Some(source));
        buffer.destroy();
        return Err("capture session stopped".to_owned());
    }
    if let Some(reason) = state.progress.frame_failure.clone() {
        destroy_capture_objects(Some(frame), Some(session), Some(source));
        buffer.destroy();
        return Err(format!("capture frame failed: {reason}"));
    }
    if !state.progress.frame_ready {
        destroy_capture_objects(Some(frame), Some(session), Some(source));
        buffer.destroy();
        return Err("capture completed without ready event".to_owned());
    }

    if format == wl_shm::Format::Xrgb8888 {
        for pixel in buffer.map.as_chunks_mut::<4>().0 {
            pixel[3] = 0xff;
        }
    }
    let (thumb_width, thumb_height, pixels) =
        downsample_argb32(&buffer.map, width, height, MAX_THUMBNAIL_EDGE)?;

    destroy_capture_objects(Some(frame), Some(session), Some(source));
    buffer.destroy();

    Ok(ToplevelThumbnail {
        key: key.to_owned(),
        width: thumb_width,
        height: thumb_height,
        pixels,
    })
}

fn choose_shm_format(formats: &[wl_shm::Format]) -> Option<wl_shm::Format> {
    formats
        .iter()
        .copied()
        .find(|format| *format == wl_shm::Format::Argb8888)
        .or_else(|| {
            formats
                .iter()
                .copied()
                .find(|format| *format == wl_shm::Format::Xrgb8888)
        })
}

fn source_byte_len(width: u32, height: u32) -> Result<u64, String> {
    if width == 0 || height == 0 {
        return Err("capture source has zero extent".to_owned());
    }
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or_else(|| "capture source size overflow".to_owned())?;
    let bytes = pixels
        .checked_mul(BYTES_PER_PIXEL)
        .ok_or_else(|| "capture source byte length overflow".to_owned())?;
    if bytes > MAX_SOURCE_BYTES {
        return Err(format!(
            "capture source {width}x{height} requires {bytes} bytes, exceeding the {MAX_SOURCE_BYTES}-byte budget"
        ));
    }
    Ok(bytes)
}

fn validate_source_size(width: u32, height: u32) -> Result<(), String> {
    source_byte_len(width, height).map(|_| ())
}

#[allow(unexpected_cfgs)]
fn downsample_argb32(
    source: &[u8],
    width: u32,
    height: u32,
    max_edge: u32,
) -> Result<(u32, u32, Vec<u8>), String> {
    validate_source_size(width, height)?;
    if max_edge == 0 {
        return Err("thumbnail max edge must be non-zero".to_owned());
    }
    #[cfg(not(nuraloumi_thumbnail_helper))]
    let (target_width, target_height) = {
        let scale = f64::from(max_edge) / f64::from(width.max(height));
        if scale >= 1.0 {
            (width, height)
        } else {
            (
                (f64::from(width) * scale).round().max(1.0) as u32,
                (f64::from(height) * scale).round().max(1.0) as u32,
            )
        }
    };
    #[cfg(nuraloumi_thumbnail_helper)]
    let (target_width, target_height) = {
        let largest = width.max(height);
        if largest <= max_edge {
            (width, height)
        } else {
            let rounded_scale = |value: u32| -> u32 {
                let numerator = u64::from(value) * u64::from(max_edge) + u64::from(largest / 2);
                u32::try_from(numerator / u64::from(largest))
                    .unwrap_or(u32::MAX)
                    .max(1)
            };
            (rounded_scale(width), rounded_scale(height))
        }
    };
    let source_stride = width as usize * 4;
    let required = source_stride
        .checked_mul(height as usize)
        .ok_or_else(|| "capture byte length overflow".to_owned())?;
    if source.len() < required {
        return Err(format!(
            "capture buffer has {} bytes but requires {required}",
            source.len()
        ));
    }

    if target_width == width && target_height == height {
        return Ok((width, height, source[..required].to_vec()));
    }

    let mut target = vec![0_u8; target_width as usize * target_height as usize * 4];
    for y in 0..target_height {
        let source_y = (u64::from(y) * u64::from(height) / u64::from(target_height)) as usize;
        for x in 0..target_width {
            let source_x = (u64::from(x) * u64::from(width) / u64::from(target_width)) as usize;
            let source_offset = source_y * source_stride + source_x * 4;
            let target_offset = (y as usize * target_width as usize + x as usize) * 4;
            target[target_offset..target_offset + 4]
                .copy_from_slice(&source[source_offset..source_offset + 4]);
        }
    }
    Ok((target_width, target_height, target))
}

struct CaptureBuffer {
    _file: File,
    map: MmapMut,
    buffer: wl_buffer::WlBuffer,
}

impl CaptureBuffer {
    fn allocate(
        width: u32,
        height: u32,
        format: wl_shm::Format,
        shm: &wl_shm::WlShm,
        qh: &QueueHandle<CaptureState>,
    ) -> Result<Self, String> {
        validate_source_size(width, height)?;
        let stride = width
            .checked_mul(4)
            .ok_or_else(|| "capture stride overflow".to_owned())?;
        let byte_len = source_byte_len(width, height)?;
        let byte_len_i32 =
            i32::try_from(byte_len).map_err(|_| "capture buffer exceeds i32 protocol limit")?;

        let fd = memfd_create("nuraloumi-thumbnail", MemfdFlags::CLOEXEC)
            .map_err(|error| format!("thumbnail memfd_create failed: {error}"))?;
        ftruncate(&fd, byte_len).map_err(|error| format!("thumbnail ftruncate failed: {error}"))?;
        let file = File::from(fd);
        let map = unsafe {
            MmapOptions::new()
                .len(byte_len as usize)
                .map_mut(&file)
                .map_err(|error| format!("thumbnail mmap failed: {error}"))?
        };
        let pool = shm.create_pool(file.as_fd(), byte_len_i32, qh, ());
        let buffer = pool.create_buffer(
            0,
            width as i32,
            height as i32,
            stride as i32,
            format,
            qh,
            (),
        );
        pool.destroy();
        Ok(Self {
            _file: file,
            map,
            buffer,
        })
    }

    fn destroy(&mut self) {
        if self.buffer.is_alive() {
            self.buffer.destroy();
        }
    }
}

fn destroy_capture_objects(
    frame: Option<ExtImageCopyCaptureFrameV1>,
    session: Option<ExtImageCopyCaptureSessionV1>,
    source: Option<ExtImageCaptureSourceV1>,
) {
    if let Some(frame) = frame {
        if frame.is_alive() {
            frame.destroy();
        }
    }
    if let Some(session) = session {
        if session.is_alive() {
            session.destroy();
        }
    }
    if let Some(source) = source {
        if source.is_alive() {
            source.destroy();
        }
    }
}

fn budget_exhausted(deadline: Instant, now: Instant) -> bool {
    now >= deadline
}

fn remaining_budget(deadline: Instant, now: Instant) -> Option<Duration> {
    deadline
        .checked_duration_since(now)
        .filter(|remaining| !remaining.is_zero())
}

fn timed_roundtrip(
    connection: &Connection,
    queue: &mut EventQueue<CaptureState>,
    state: &mut CaptureState,
    deadline: Instant,
) -> Result<(), String> {
    let callback = connection.display().sync(&queue.handle(), ());
    connection
        .flush()
        .map_err(|error| format!("Wayland thumbnail flush failed: {error}"))?;
    while callback.is_alive() {
        dispatch_one(connection, queue, state, deadline)?;
        if callback.is_alive() && budget_exhausted(deadline, Instant::now()) {
            return Err("Wayland thumbnail roundtrip timed out".to_owned());
        }
    }
    Ok(())
}

fn dispatch_until(
    connection: &Connection,
    queue: &mut EventQueue<CaptureState>,
    state: &mut CaptureState,
    deadline: Instant,
    done: impl Fn(&CaptureState) -> bool,
) -> Result<(), String> {
    loop {
        queue
            .dispatch_pending(state)
            .map_err(|error| format!("Wayland thumbnail dispatch failed: {error}"))?;
        if done(state) {
            return Ok(());
        }
        dispatch_one(connection, queue, state, deadline)?;
        if budget_exhausted(deadline, Instant::now()) {
            return Err("Wayland thumbnail operation timed out".to_owned());
        }
    }
}

fn dispatch_one(
    connection: &Connection,
    queue: &mut EventQueue<CaptureState>,
    state: &mut CaptureState,
    deadline: Instant,
) -> Result<(), String> {
    connection
        .flush()
        .map_err(|error| format!("Wayland thumbnail flush failed: {error}"))?;

    let Some(read) = queue.prepare_read() else {
        queue
            .dispatch_pending(state)
            .map_err(|error| format!("Wayland thumbnail dispatch failed: {error}"))?;
        return Ok(());
    };

    let Some(remaining) = remaining_budget(deadline, Instant::now()) else {
        drop(read);
        return Err("Wayland thumbnail operation timed out".to_owned());
    };
    let timespec = Timespec {
        tv_sec: i64::try_from(remaining.as_secs()).unwrap_or(i64::MAX),
        tv_nsec: i64::from(remaining.subsec_nanos()),
    };
    let mut fds = [PollFd::new(connection, PollFlags::IN)];
    let ready = poll(&mut fds, Some(&timespec))
        .map_err(|error| format!("Wayland thumbnail poll failed: {error}"))?;
    if ready == 0 {
        drop(read);
        return Err("Wayland thumbnail operation timed out".to_owned());
    }
    let revents = fds[0].revents();
    if revents.intersects(PollFlags::ERR | PollFlags::HUP | PollFlags::NVAL) {
        drop(read);
        return Err(format!("Wayland thumbnail socket failed: {revents:?}"));
    }
    read.read()
        .map_err(|error| format!("Wayland thumbnail read failed: {error}"))?;
    queue
        .dispatch_pending(state)
        .map_err(|error| format!("Wayland thumbnail dispatch failed: {error}"))?;
    Ok(())
}

impl Dispatch<wl_registry::WlRegistry, ()> for CaptureState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _connection: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_shm" if state.shm.is_none() => {
                    state.shm = Some(registry.bind(name, version.min(1), qh, ()));
                }
                "ext_foreign_toplevel_list_v1" if state.toplevel_list.is_none() => {
                    state.toplevel_list = Some(registry.bind(name, version.min(1), qh, ()));
                }
                "ext_foreign_toplevel_image_capture_source_manager_v1"
                    if state.source_manager.is_none() =>
                {
                    state.source_manager = Some(registry.bind(name, version.min(1), qh, ()));
                }
                "ext_image_copy_capture_manager_v1" if state.capture_manager.is_none() => {
                    state.capture_manager = Some(registry.bind(name, version.min(1), qh, ()));
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<ExtForeignToplevelListV1, ()> for CaptureState {
    fn event(
        state: &mut Self,
        _proxy: &ExtForeignToplevelListV1,
        event: ext_foreign_toplevel_list_v1::Event,
        _data: &(),
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } => {
                state.toplevels.insert(
                    toplevel.id(),
                    ListedToplevel {
                        handle: Some(toplevel),
                        ..ListedToplevel::default()
                    },
                );
            }
            ext_foreign_toplevel_list_v1::Event::Finished => {
                state.toplevel_list = None;
            }
            _ => {}
        }
    }

    event_created_child!(CaptureState, ExtForeignToplevelListV1, [
        ext_foreign_toplevel_list_v1::EVT_TOPLEVEL_OPCODE => (ExtForeignToplevelHandleV1, ())
    ]);
}

impl Dispatch<ExtForeignToplevelHandleV1, ()> for CaptureState {
    fn event(
        state: &mut Self,
        proxy: &ExtForeignToplevelHandleV1,
        event: ext_foreign_toplevel_handle_v1::Event,
        _data: &(),
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let entry = state.toplevels.entry(proxy.id()).or_default();
        if entry.handle.is_none() {
            entry.handle = Some(proxy.clone());
        }
        match event {
            ext_foreign_toplevel_handle_v1::Event::Title { title } => entry.title = title,
            ext_foreign_toplevel_handle_v1::Event::AppId { app_id } => entry.app_id = Some(app_id),
            ext_foreign_toplevel_handle_v1::Event::Identifier { identifier } => {
                entry.identifier = Some(identifier)
            }
            ext_foreign_toplevel_handle_v1::Event::Done => entry.committed = true,
            ext_foreign_toplevel_handle_v1::Event::Closed => entry.closed = true,
            _ => {}
        }
    }
}

impl Dispatch<ExtImageCopyCaptureSessionV1, ()> for CaptureState {
    fn event(
        state: &mut Self,
        _proxy: &ExtImageCopyCaptureSessionV1,
        event: ext_image_copy_capture_session_v1::Event,
        _data: &(),
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            ext_image_copy_capture_session_v1::Event::BufferSize { width, height } => {
                state.progress.width = width;
                state.progress.height = height;
            }
            ext_image_copy_capture_session_v1::Event::ShmFormat {
                format: WEnum::Value(format),
            } => {
                if !state.progress.shm_formats.contains(&format) {
                    state.progress.shm_formats.push(format);
                }
            }
            ext_image_copy_capture_session_v1::Event::Done => {
                state.progress.constraints_done = true;
            }
            ext_image_copy_capture_session_v1::Event::Stopped => {
                state.progress.stopped = true;
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtImageCopyCaptureFrameV1, ()> for CaptureState {
    fn event(
        state: &mut Self,
        _proxy: &ExtImageCopyCaptureFrameV1,
        event: ext_image_copy_capture_frame_v1::Event,
        _data: &(),
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            ext_image_copy_capture_frame_v1::Event::Ready => {
                state.progress.frame_ready = true;
                state.progress.frame_done = true;
            }
            ext_image_copy_capture_frame_v1::Event::Failed { reason } => {
                state.progress.frame_failure = Some(format!("{reason:?}"));
                state.progress.frame_done = true;
            }
            _ => {}
        }
    }
}

delegate_noop!(CaptureState: ignore wl_callback::WlCallback);
delegate_noop!(CaptureState: ignore wl_shm::WlShm);
delegate_noop!(CaptureState: ignore wl_shm_pool::WlShmPool);
delegate_noop!(CaptureState: ignore wl_buffer::WlBuffer);
delegate_noop!(CaptureState: ignore ExtForeignToplevelImageCaptureSourceManagerV1);
delegate_noop!(CaptureState: ignore ExtImageCaptureSourceV1);
delegate_noop!(CaptureState: ignore ExtImageCopyCaptureManagerV1);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downsample_is_bounded_and_preserves_argb_words() {
        let width = 4;
        let height = 2;
        let mut source = Vec::new();
        for value in 0_u8..8 {
            source.extend_from_slice(&[value, value, value, 0xff]);
        }

        let (out_width, out_height, pixels) =
            downsample_argb32(&source, width, height, 2).expect("downsample");
        assert_eq!((out_width, out_height), (2, 1));
        assert_eq!(pixels.len(), 8);
        assert_eq!(&pixels[0..4], &[0, 0, 0, 0xff]);
        assert_eq!(&pixels[4..8], &[2, 2, 2, 0xff]);
    }

    fn identity(
        title: &str,
        app_id: Option<&str>,
        identifier: Option<&str>,
    ) -> PresentationIdentity {
        PresentationIdentity {
            title: title.into(),
            app_id: app_id.map(str::to_owned),
            identifier: identifier.map(str::to_owned),
        }
    }

    fn request(
        title: &str,
        app_id: Option<&str>,
        identifier: Option<&str>,
    ) -> ToplevelThumbnailRequest {
        ToplevelThumbnailRequest {
            key: "window".into(),
            title: title.into(),
            app_id: app_id.map(str::to_owned),
            protocol_identifier: identifier.map(str::to_owned),
        }
    }

    #[test]
    fn source_size_budget_rejects_unbounded_capture() {
        assert_eq!(source_byte_len(1280, 800), Ok(4_096_000));
        assert_eq!(source_byte_len(2560, 1600), Ok(16_384_000));
        assert!(validate_source_size(0, 800).is_err());
        assert!(validate_source_size(4096, 4096).is_err());
    }

    #[test]
    fn protocol_identifier_takes_precedence_over_titles() {
        let listed = vec![
            identity("Same title", Some("app"), Some("stable-a")),
            identity("Same title", Some("app"), Some("stable-b")),
        ];
        let request = request("Wrong transient title", Some("other"), Some("stable-b"));
        assert_eq!(resolve_presentation_index(&listed, &request), Ok(1));
    }

    #[test]
    fn duplicate_protocol_identifier_fails_closed() {
        let listed = vec![
            identity("One", Some("app.one"), Some("duplicate-id")),
            identity("Two", Some("app.two"), Some("duplicate-id")),
        ];
        assert_eq!(
            resolve_presentation_index(
                &listed,
                &request("ignored", Some("ignored"), Some("duplicate-id"))
            ),
            Err("ambiguous presentation match; thumbnail suppressed")
        );
    }

    #[test]
    fn unique_title_app_fallback_is_allowed_without_identifier() {
        let listed = vec![
            identity("Terminal", Some("foot"), None),
            identity("Files", Some("thunar"), None),
        ];
        assert_eq!(
            resolve_presentation_index(&listed, &request("Files", Some("thunar"), None)),
            Ok(1)
        );
    }

    #[test]
    fn duplicate_title_app_fallback_fails_closed() {
        let listed = vec![
            identity("Terminal", Some("foot"), None),
            identity("Terminal", Some("foot"), None),
        ];
        assert_eq!(
            resolve_presentation_index(&listed, &request("Terminal", Some("foot"), None)),
            Err("ambiguous presentation match; thumbnail suppressed")
        );
    }

    #[test]
    fn missing_presentation_match_fails_closed() {
        let listed = vec![identity("Terminal", Some("foot"), Some("stable-a"))];
        assert_eq!(
            resolve_presentation_index(
                &listed,
                &request("Terminal", Some("foot"), Some("stable-missing"))
            ),
            Err("no exact presentation match")
        );
    }

    #[test]
    fn total_budget_helpers_do_not_reset_between_stages() {
        let start = Instant::now();
        let deadline = start + Duration::from_millis(350);
        assert_eq!(
            remaining_budget(deadline, start + Duration::from_millis(100)),
            Some(Duration::from_millis(250))
        );
        assert_eq!(remaining_budget(deadline, deadline), None);
        assert!(budget_exhausted(
            deadline,
            deadline + Duration::from_millis(1)
        ));
    }

    #[test]
    fn capability_requires_all_standard_protocol_pieces() {
        let complete = ToplevelThumbnailCapabilities {
            ext_foreign_toplevel_list: true,
            foreign_toplevel_capture_source: true,
            image_copy_capture: true,
            shm: true,
        };
        assert!(complete.available());
        assert!(!ToplevelThumbnailCapabilities {
            image_copy_capture: false,
            ..complete
        }
        .available());
    }

    #[test]
    fn helper_wire_round_trips_requests_and_reports() {
        let requests = vec![ToplevelThumbnailRequest {
            key: "tl:1".into(),
            title: "Terminal".into(),
            app_id: Some("foot".into()),
            protocol_identifier: Some("stable-1".into()),
        }];
        let request_bytes =
            helper_wire::encode_thumbnail_helper_request(&requests).expect("encode request");
        assert_eq!(
            helper_wire::decode_thumbnail_helper_request(&request_bytes).expect("decode request"),
            requests
        );

        let report = ToplevelThumbnailReport {
            capabilities: ToplevelThumbnailCapabilities {
                ext_foreign_toplevel_list: true,
                foreign_toplevel_capture_source: true,
                image_copy_capture: true,
                shm: true,
            },
            thumbnails: vec![ToplevelThumbnail {
                key: "tl:1".into(),
                width: 2,
                height: 1,
                pixels: vec![1, 2, 3, 4, 5, 6, 7, 8],
            }],
            issues: vec!["bounded warning".into()],
        };
        let response_bytes =
            helper_wire::encode_thumbnail_helper_report(&report).expect("encode report");
        assert_eq!(
            helper_wire::decode_thumbnail_helper_report(&response_bytes).expect("decode report"),
            report
        );
    }
}
