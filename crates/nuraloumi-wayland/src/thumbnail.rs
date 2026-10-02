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
const MAX_SOURCE_PIXELS: u64 = 4096 * 4096;
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

#[derive(Clone, Debug, Default)]
struct ListedToplevel {
    handle: Option<ExtForeignToplevelHandleV1>,
    title: String,
    app_id: Option<String>,
    identifier: Option<String>,
    committed: bool,
    closed: bool,
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

    let connection = Connection::connect_to_env()
        .map_err(|error| format!("Wayland thumbnail connect failed: {error}"))?;
    let mut queue = connection.new_event_queue::<CaptureState>();
    let qh = queue.handle();
    let _registry = connection.display().get_registry(&qh, ());
    let mut state = CaptureState::default();

    timed_roundtrip(&connection, &mut queue, &mut state, timeout)?;
    timed_roundtrip(&connection, &mut queue, &mut state, timeout)?;

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
        match capture_one(
            &connection,
            &mut queue,
            &qh,
            &mut state,
            &handle,
            &request.key,
            timeout,
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

    let mut resolved = Vec::new();
    for request in requests {
        let matches = if let Some(identifier) = request.protocol_identifier.as_deref() {
            listed
                .iter()
                .filter(|(entry, _)| entry.identifier.as_deref() == Some(identifier))
                .copied()
                .collect::<Vec<_>>()
        } else {
            listed
                .iter()
                .filter(|(entry, _)| entry.title == request.title && entry.app_id == request.app_id)
                .copied()
                .collect::<Vec<_>>()
        };

        match matches.as_slice() {
            [(_, handle)] => resolved.push((request.clone(), (*handle).clone())),
            [] => issues.push(format!("{}: no exact presentation match", request.key)),
            _ => issues.push(format!(
                "{}: ambiguous presentation match; thumbnail suppressed",
                request.key
            )),
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
    timeout: Duration,
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

    dispatch_until(connection, queue, state, timeout, |state| {
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

    dispatch_until(connection, queue, state, timeout, |state| {
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
        for pixel in buffer.map.chunks_exact_mut(4) {
            pixel[3] = 0xff;
        }
    }
    buffer.map.flush().ok();
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

fn validate_source_size(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("capture source has zero extent".to_owned());
    }
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or_else(|| "capture source size overflow".to_owned())?;
    if pixels > MAX_SOURCE_PIXELS {
        return Err(format!(
            "capture source {width}x{height} exceeds bounded pixel budget"
        ));
    }
    Ok(())
}

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
    let scale = f64::from(max_edge) / f64::from(width.max(height));
    let (target_width, target_height) = if scale >= 1.0 {
        (width, height)
    } else {
        (
            (f64::from(width) * scale).round().max(1.0) as u32,
            (f64::from(height) * scale).round().max(1.0) as u32,
        )
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
        let byte_len = stride
            .checked_mul(height)
            .ok_or_else(|| "capture buffer length overflow".to_owned())?;
        let byte_len_i32 =
            i32::try_from(byte_len).map_err(|_| "capture buffer exceeds i32 protocol limit")?;

        let fd = memfd_create("nuraloumi-thumbnail", MemfdFlags::CLOEXEC)
            .map_err(|error| format!("thumbnail memfd_create failed: {error}"))?;
        ftruncate(&fd, u64::from(byte_len))
            .map_err(|error| format!("thumbnail ftruncate failed: {error}"))?;
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

fn timed_roundtrip(
    connection: &Connection,
    queue: &mut EventQueue<CaptureState>,
    state: &mut CaptureState,
    timeout: Duration,
) -> Result<(), String> {
    let callback = connection.display().sync(&queue.handle(), ());
    connection
        .flush()
        .map_err(|error| format!("Wayland thumbnail flush failed: {error}"))?;
    let deadline = Instant::now() + timeout;
    while callback.is_alive() {
        dispatch_one(connection, queue, state, deadline)?;
        if Instant::now() >= deadline {
            return Err("Wayland thumbnail roundtrip timed out".to_owned());
        }
    }
    Ok(())
}

fn dispatch_until(
    connection: &Connection,
    queue: &mut EventQueue<CaptureState>,
    state: &mut CaptureState,
    timeout: Duration,
    done: impl Fn(&CaptureState) -> bool,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        queue
            .dispatch_pending(state)
            .map_err(|error| format!("Wayland thumbnail dispatch failed: {error}"))?;
        if done(state) {
            return Ok(());
        }
        dispatch_one(connection, queue, state, deadline)?;
        if Instant::now() >= deadline {
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

    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        drop(read);
        return Err("Wayland thumbnail operation timed out".to_owned());
    }
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

    #[test]
    fn source_size_budget_rejects_unbounded_capture() {
        assert!(validate_source_size(1280, 800).is_ok());
        assert!(validate_source_size(0, 800).is_err());
        assert!(validate_source_size(8192, 8192).is_err());
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
}
