use std::{collections::HashMap, os::fd::BorrowedFd};

use wayland_client::{
    delegate_noop, event_created_child,
    protocol::{
        wl_buffer, wl_callback, wl_output, wl_region, wl_subcompositor, wl_subsurface, wl_surface,
    },
    Proxy,
};
use wayland_protocols::wp::{
    linux_dmabuf::zv1::client::{zwp_linux_buffer_params_v1, zwp_linux_dmabuf_v1},
    viewporter::client::{wp_viewport, wp_viewporter},
};

use super::{
    wl_registry, BackendError, BackendState, Connection, Dispatch, QueueHandle, Result, SurfaceId,
    WaylandBackend,
};

const XR24: u32 = u32::from_le_bytes(*b"XR24");
const MAX_BUFFERS: usize = 4;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct VideoFrame {
    parent: SurfaceId,
    sequence: u32,
}

struct VideoImport {
    width: i32,
    height: i32,
    cancelled: bool,
}

struct VideoBuffer;

struct VideoSurface {
    surface: wl_surface::WlSurface,
    subsurface: wl_subsurface::WlSubsurface,
    viewport: wp_viewport::WpViewport,
    callback: Option<wl_callback::WlCallback>,
    destination: Option<(i32, i32, i32, i32)>,
}

impl VideoSurface {
    fn resize(&mut self, destination: (i32, i32, i32, i32)) -> bool {
        if self.destination == Some(destination) {
            return false;
        }
        let (x, y, width, height) = destination;
        self.subsurface.set_position(x, y);
        self.viewport.set_destination(width, height);
        self.destination = Some(destination);
        true
    }

    fn cancel_callback(&mut self, connection: &Connection) {
        if let Some(callback) = self.callback.take() {
            // wl_callback has no destructor request. The client backend drops
            // local event routing and retains its server-side ID until delete_id.
            let _ = connection.backend().destroy_object(&callback.id());
        }
    }
}

#[derive(Default)]
pub(super) struct VideoState {
    subcompositor: Option<(u32, wl_subcompositor::WlSubcompositor)>,
    viewporter: Option<(u32, wp_viewporter::WpViewporter)>,
    dmabuf: Option<(u32, zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1)>,
    surfaces: HashMap<SurfaceId, VideoSurface>,
    imports: HashMap<VideoFrame, VideoImport>,
    // Keep the wl_buffer alive even after its child surface is destroyed.
    // Returning a producer slot before wl_buffer.release is unsafe.
    in_use: HashMap<VideoFrame, wl_buffer::WlBuffer>,
    releases: Vec<u32>,
    callbacks: Vec<u32>,
}

impl VideoState {
    fn cancel_imports(&mut self, parent: SurfaceId) {
        for (key, import) in &mut self.imports {
            if key.parent == parent {
                import.cancelled = true;
            }
        }
    }

    pub(super) fn bind(
        &mut self,
        registry: &wl_registry::WlRegistry,
        name: u32,
        interface: &str,
        version: u32,
        qh: &QueueHandle<BackendState>,
    ) {
        match interface {
            "wl_subcompositor" if self.subcompositor.is_none() => {
                self.subcompositor = Some((name, registry.bind(name, 1, qh, ())));
            }
            "wp_viewporter" if self.viewporter.is_none() => {
                self.viewporter = Some((name, registry.bind(name, 1, qh, ())));
            }
            "zwp_linux_dmabuf_v1" if version >= 3 && self.dmabuf.is_none() => {
                self.dmabuf = Some((name, registry.bind(name, version.min(4), qh, ())));
            }
            _ => {}
        }
    }

    pub(super) fn remove_global(&mut self, name: u32) {
        if self
            .subcompositor
            .as_ref()
            .is_some_and(|(id, _)| *id == name)
        {
            self.subcompositor.take().unwrap().1.destroy();
        }
        if self.viewporter.as_ref().is_some_and(|(id, _)| *id == name) {
            self.viewporter.take().unwrap().1.destroy();
        }
        if self.dmabuf.as_ref().is_some_and(|(id, _)| *id == name) {
            self.dmabuf.take().unwrap().1.destroy();
        }
    }
}

impl WaylandBackend {
    fn video_parent(
        &self,
        parent: SurfaceId,
        destination: (i32, i32, i32, i32),
    ) -> Result<wl_surface::WlSurface> {
        let record = self
            .state
            .surfaces
            .get(&parent)
            .ok_or(BackendError::UnknownSurface(parent.0))?;
        if record.closed {
            return Err(BackendError::SurfaceClosed(parent.0));
        }
        if !record.configured {
            return Err(BackendError::SurfaceNotConfigured(parent.0));
        }
        let (x, y, width, height) = destination;
        if x < 0
            || y < 0
            || width <= 0
            || height <= 0
            || x as u64 + width as u64 > u64::from(record.width)
            || y as u64 + height as u64 > u64::from(record.height)
        {
            return Err(BackendError::InvalidFrame(
                "video destination outside window".into(),
            ));
        }
        Ok(record.surface.clone())
    }

    fn ensure_video_surface(
        &mut self,
        parent: SurfaceId,
        surface: &wl_surface::WlSurface,
    ) -> Result<()> {
        let video = &mut self.state.video;
        if video.surfaces.contains_key(&parent) {
            return Ok(());
        }
        let (_, subcompositor) = video
            .subcompositor
            .as_ref()
            .ok_or(BackendError::MissingGlobal("wl_subcompositor"))?;
        let (_, viewporter) = video
            .viewporter
            .as_ref()
            .ok_or(BackendError::MissingGlobal("wp_viewporter"))?;
        let compositor = self
            .state
            .compositor
            .as_ref()
            .ok_or(BackendError::MissingGlobal("wl_compositor"))?;
        let child = compositor.create_surface(&self.qh, ());
        let passthrough = compositor.create_region(&self.qh, ());
        child.set_input_region(Some(&passthrough));
        passthrough.destroy();
        let subsurface = subcompositor.get_subsurface(&child, surface, &self.qh, ());
        subsurface.set_desync();
        let viewport = viewporter.get_viewport(&child, &self.qh, ());
        video.surfaces.insert(
            parent,
            VideoSurface {
                surface: child,
                subsurface,
                viewport,
                callback: None,
                destination: None,
            },
        );
        Ok(())
    }

    /// Places one single-plane, linear XRGB8888 DMA-BUF in logical window coordinates.
    /// Producers must retain this buffer until the matching sequence is returned
    /// by take_video_releases.
    #[allow(clippy::too_many_arguments)]
    pub fn present_video(
        &mut self,
        parent: SurfaceId,
        fd: BorrowedFd<'_>,
        width: u32,
        height: u32,
        stride: u32,
        sequence: u32,
        destination: (i32, i32, i32, i32),
        vertically_inverted: bool,
    ) -> Result<()> {
        let parent_surface = self.video_parent(parent, destination)?;
        if width == 0
            || height == 0
            || width > (i32::MAX / 4) as u32
            || height > i32::MAX as u32
            || stride < width * 4
        {
            return Err(BackendError::InvalidFrame(
                "invalid XR24 DMA-BUF geometry".into(),
            ));
        }
        let key = VideoFrame { parent, sequence };
        let video = &self.state.video;
        let (_, dmabuf) = video
            .dmabuf
            .as_ref()
            .ok_or(BackendError::MissingGlobal("zwp_linux_dmabuf_v1"))?;
        let dmabuf = dmabuf.clone();
        if video.in_use.len() + video.imports.len() >= MAX_BUFFERS
            || video.in_use.contains_key(&key)
            || video.imports.keys().any(|key| key.parent == parent)
            || video
                .surfaces
                .get(&parent)
                .is_some_and(|child| child.callback.is_some())
        {
            return Err(BackendError::WouldBlock);
        }
        self.ensure_video_surface(parent, &parent_surface)?;
        let video = &mut self.state.video;
        let child = video
            .surfaces
            .get_mut(&parent)
            .expect("video surface created");
        child.surface.set_buffer_transform(if vertically_inverted {
            wl_output::Transform::Flipped180
        } else {
            wl_output::Transform::Normal
        });
        let moved = child.resize(destination);

        // Placement is synchronized to a parent commit even on a desynchronized
        // child. Keep that separate from ordinary frames and parent redraws.
        if moved {
            parent_surface.commit();
        }
        let params = dmabuf.create_params(&self.qh, key);
        params.add(fd, 0, 0, stride, 0, 0);
        // Asynchronous import reports unsupported buffers without disconnecting
        // the application as create_immed is allowed to do.
        params.create(
            width as i32,
            height as i32,
            XR24,
            zwp_linux_buffer_params_v1::Flags::empty(),
        );
        video.imports.insert(
            key,
            VideoImport {
                width: width as i32,
                height: height as i32,
                cancelled: false,
            },
        );
        self.flush()
    }

    /// Repositions the last video buffer without waiting for the next frame.
    pub fn resize_video(
        &mut self,
        parent: SurfaceId,
        destination: (i32, i32, i32, i32),
    ) -> Result<()> {
        let parent_surface = self.video_parent(parent, destination)?;
        if let Some(child) = self.state.video.surfaces.get_mut(&parent) {
            if child.resize(destination) {
                child.surface.commit();
                parent_surface.commit();
                self.flush()?;
            }
        }
        Ok(())
    }

    pub fn clear_video(&mut self, parent: SurfaceId) -> Result<()> {
        self.state.video.cancel_imports(parent);
        if let Some(child) = self.state.video.surfaces.get_mut(&parent) {
            child.cancel_callback(&self.connection);
            child.surface.attach(None, 0, 0);
            child.surface.commit();
            self.flush()?;
        }
        Ok(())
    }

    pub(super) fn destroy_video_surface(&mut self, parent: SurfaceId) {
        self.state.video.cancel_imports(parent);
        if let Some(mut child) = self.state.video.surfaces.remove(&parent) {
            child.cancel_callback(&self.connection);
            child.viewport.destroy();
            child.subsurface.destroy();
            child.surface.destroy();
        }
    }

    pub fn take_video_releases(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.state.video.releases)
    }

    pub fn take_video_callbacks(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.state.video.callbacks)
    }
}

impl Dispatch<zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1, VideoFrame> for BackendState {
    event_created_child!(BackendState, zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1, [
        zwp_linux_buffer_params_v1::EVT_CREATED_OPCODE => (wl_buffer::WlBuffer, VideoBuffer),
    ]);

    fn event(
        state: &mut Self,
        params: &zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1,
        event: zwp_linux_buffer_params_v1::Event,
        key: &VideoFrame,
        _connection: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let video = &mut state.video;
        match event {
            zwp_linux_buffer_params_v1::Event::Created { buffer } => {
                params.destroy();
                let import = video.imports.remove(key);
                if let Some((import, child)) = import
                    .filter(|import| !import.cancelled)
                    .zip(video.surfaces.get_mut(&key.parent))
                {
                    child.callback = Some(child.surface.frame(qh, *key));
                    child.surface.attach(Some(&buffer), 0, 0);
                    if child.surface.version() >= 4 {
                        child
                            .surface
                            .damage_buffer(0, 0, import.width, import.height);
                    } else if let Some((_, _, width, height)) = child.destination {
                        child.surface.damage(0, 0, width, height);
                    }
                    child.surface.commit();
                    video.in_use.insert(*key, buffer);
                } else {
                    buffer.destroy();
                    video.releases.push(key.sequence);
                }
            }
            zwp_linux_buffer_params_v1::Event::Failed => {
                params.destroy();
                if video.imports.remove(key).is_some() {
                    // This slot was never displayed. The producer gets its
                    // buffer back but receives no successful pacing callback.
                    video.releases.push(key.sequence);
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_buffer::WlBuffer, VideoBuffer> for BackendState {
    fn event(
        state: &mut Self,
        proxy: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        _data: &VideoBuffer,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = event {
            let in_use = &mut state.video.in_use;
            let key = in_use
                .iter()
                .find_map(|(key, buffer)| (buffer == proxy).then_some(*key));
            if let Some(key) = key {
                in_use.remove(&key);
                state.video.releases.push(key.sequence);
            }
            proxy.destroy();
        }
    }
}

impl Dispatch<wl_callback::WlCallback, VideoFrame> for BackendState {
    fn event(
        state: &mut Self,
        proxy: &wl_callback::WlCallback,
        event: wl_callback::Event,
        key: &VideoFrame,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            if let Some(child) = state.video.surfaces.get_mut(&key.parent) {
                if child.callback.as_ref() == Some(proxy) {
                    child.callback.take();
                    state.video.callbacks.push(key.sequence);
                }
            }
        }
    }
}

delegate_noop!(BackendState: ignore wl_surface::WlSurface);
delegate_noop!(BackendState: ignore wl_subcompositor::WlSubcompositor);
delegate_noop!(BackendState: ignore wl_subsurface::WlSubsurface);
delegate_noop!(BackendState: ignore wl_region::WlRegion);
delegate_noop!(BackendState: ignore wp_viewporter::WpViewporter);
delegate_noop!(BackendState: ignore wp_viewport::WpViewport);
delegate_noop!(BackendState: ignore zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1);
