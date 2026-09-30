use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    os::fd::AsRawFd,
};

use wayland_client::{
    delegate_noop,
    protocol::{
        wl_buffer, wl_compositor, wl_keyboard, wl_output, wl_pointer, wl_registry, wl_seat, wl_shm,
        wl_shm_pool, wl_surface, wl_touch,
    },
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum,
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, ZwlrLayerSurfaceV1},
};

use crate::{
    shm::{BufferKey, ShmBuffers},
    types::{semantic_key, sorted_touch_ids},
    BackendCapabilities, BackendError, BackendEvent, Frame, MenuConfig, OutputId, OutputInfo,
    OutputTransform, PanelConfig, PlatformEvent, Result, SurfaceId,
};

struct OutputRecord {
    proxy: wl_output::WlOutput,
    info: OutputInfo,
}

struct SurfaceRecord {
    surface: wl_surface::WlSurface,
    layer: ZwlrLayerSurfaceV1,
    requested_width: u32,
    requested_height: u32,
    width: u32,
    height: u32,
    scale: i32,
    configured: bool,
    closed: bool,
    entered_outputs: BTreeSet<OutputId>,
    buffers: ShmBuffers,
}

impl SurfaceRecord {
    fn expected_pixel_size(&self) -> Option<(u32, u32)> {
        if !self.configured || self.width == 0 || self.height == 0 {
            return None;
        }
        let scale = self.scale.max(1) as u32;
        Some((
            self.width.saturating_mul(scale),
            self.height.saturating_mul(scale),
        ))
    }
}

#[derive(Default)]
struct BackendState {
    compositor: Option<wl_compositor::WlCompositor>,
    compositor_global: Option<u32>,
    shm: Option<wl_shm::WlShm>,
    shm_global: Option<u32>,
    layer_shell: Option<ZwlrLayerShellV1>,
    layer_shell_global: Option<u32>,
    seat: Option<wl_seat::WlSeat>,
    seat_global: Option<u32>,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    touch: Option<wl_touch::WlTouch>,
    outputs: HashMap<OutputId, OutputRecord>,
    shm_argb8888: bool,
    shm_xrgb8888: bool,
    next_surface_id: u32,
    surfaces: HashMap<SurfaceId, SurfaceRecord>,
    events: VecDeque<BackendEvent>,
    pointer_surface: Option<SurfaceId>,
    pointer_x: f64,
    pointer_y: f64,
    keyboard_surface: Option<SurfaceId>,
    touch_surfaces: HashMap<i32, SurfaceId>,
    active_touches: BTreeSet<i32>,
    shift_down: bool,
}

pub struct WaylandBackend {
    connection: Connection,
    queue: EventQueue<BackendState>,
    qh: QueueHandle<BackendState>,
    state: BackendState,
}

impl WaylandBackend {
    pub fn connect() -> Result<Self> {
        let connection = Connection::connect_to_env()
            .map_err(|error| BackendError::Connect(error.to_string()))?;
        let queue = connection.new_event_queue();
        let qh = queue.handle();
        connection.display().get_registry(&qh, ());
        let mut backend = Self {
            connection,
            queue,
            qh,
            state: BackendState::default(),
        };

        backend.roundtrip()?;
        backend.roundtrip()?;

        if backend.state.compositor.is_none() {
            return Err(BackendError::MissingGlobal("wl_compositor"));
        }
        if backend.state.shm.is_none() {
            return Err(BackendError::MissingGlobal("wl_shm"));
        }
        if !backend.state.shm_argb8888 && !backend.state.shm_xrgb8888 {
            return Err(BackendError::UnsupportedShmFormat("ARGB8888 or XRGB8888"));
        }

        Ok(backend)
    }

    pub fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            compositor: self.state.compositor.is_some(),
            shm: self.state.shm.is_some(),
            layer_shell: self.state.layer_shell.is_some(),
            seat: self.state.seat.is_some(),
            output_count: self.state.outputs.len(),
            argb8888: self.state.shm_argb8888,
            xrgb8888: self.state.shm_xrgb8888,
        }
    }

    pub fn outputs(&self) -> Vec<OutputInfo> {
        let mut outputs: Vec<_> = self
            .state
            .outputs
            .values()
            .map(|record| record.info.clone())
            .collect();
        outputs.sort_by_key(|output| output.id);
        outputs
    }

    pub fn create_panel(&mut self, config: PanelConfig) -> Result<SurfaceId> {
        self.create_layer_surface(
            config.output,
            0,
            config.height,
            config.namespace,
            LayerRole::Panel {
                exclusive_zone: config.exclusive_zone,
            },
        )
    }

    pub fn create_menu(&mut self, config: MenuConfig) -> Result<SurfaceId> {
        self.create_layer_surface(
            config.output,
            config.width,
            config.height,
            config.namespace,
            LayerRole::Menu {
                margin_top: config.margin_top,
                margin_left: config.margin_left,
            },
        )
    }

    fn create_layer_surface(
        &mut self,
        output: Option<OutputId>,
        width: u32,
        height: u32,
        namespace: String,
        role: LayerRole,
    ) -> Result<SurfaceId> {
        let compositor = self
            .state
            .compositor
            .clone()
            .ok_or(BackendError::MissingGlobal("wl_compositor"))?;
        let layer_shell = self
            .state
            .layer_shell
            .clone()
            .ok_or(BackendError::MissingGlobal("zwlr_layer_shell_v1"))?;
        let output_proxy = output
            .map(|id| {
                self.state
                    .outputs
                    .get(&id)
                    .map(|record| record.proxy.clone())
                    .ok_or(BackendError::MissingGlobal("requested wl_output"))
            })
            .transpose()?;

        self.state.next_surface_id = self.state.next_surface_id.wrapping_add(1).max(1);
        let id = SurfaceId(self.state.next_surface_id);
        let surface = compositor.create_surface(&self.qh, id);

        let layer_kind = match role {
            LayerRole::Panel { .. } => zwlr_layer_shell_v1::Layer::Top,
            LayerRole::Menu { .. } => zwlr_layer_shell_v1::Layer::Overlay,
        };
        let layer = layer_shell.get_layer_surface(
            &surface,
            output_proxy.as_ref(),
            layer_kind,
            namespace,
            &self.qh,
            id,
        );
        layer.set_size(width, height);

        match role {
            LayerRole::Panel { exclusive_zone } => {
                layer.set_anchor(
                    zwlr_layer_surface_v1::Anchor::Top
                        | zwlr_layer_surface_v1::Anchor::Left
                        | zwlr_layer_surface_v1::Anchor::Right,
                );
                layer.set_exclusive_zone(exclusive_zone);
                layer
                    .set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);
            }
            LayerRole::Menu {
                margin_top,
                margin_left,
            } => {
                layer.set_anchor(
                    zwlr_layer_surface_v1::Anchor::Top | zwlr_layer_surface_v1::Anchor::Left,
                );
                layer.set_margin(margin_top, 0, 0, margin_left);
                layer.set_keyboard_interactivity(
                    zwlr_layer_surface_v1::KeyboardInteractivity::Exclusive,
                );
            }
        }

        self.state.surfaces.insert(
            id,
            SurfaceRecord {
                surface: surface.clone(),
                layer,
                requested_width: width,
                requested_height: height,
                width: 0,
                height: 0,
                scale: 1,
                configured: false,
                closed: false,
                entered_outputs: BTreeSet::new(),
                buffers: ShmBuffers::new(),
            },
        );
        surface.commit();
        self.connection
            .flush()
            .map_err(|error| BackendError::Dispatch(error.to_string()))?;
        Ok(id)
    }

    pub fn present(&mut self, id: SurfaceId, frame: Frame<'_>) -> Result<()> {
        if !self.capabilities().supports_format(frame.format) {
            return Err(BackendError::UnsupportedShmFormat(frame.format.name()));
        }
        let shm = self
            .state
            .shm
            .clone()
            .ok_or(BackendError::MissingGlobal("wl_shm"))?;
        let record = self
            .state
            .surfaces
            .get_mut(&id)
            .ok_or(BackendError::UnknownSurface(id.0))?;

        if record.closed {
            return Err(BackendError::SurfaceClosed(id.0));
        }
        let Some((expected_width, expected_height)) = record.expected_pixel_size() else {
            return Err(BackendError::SurfaceNotConfigured(id.0));
        };
        if frame.width != expected_width || frame.height != expected_height {
            return Err(BackendError::InvalidFrame(format!(
                "surface {} expects {}x{} pixels at scale {}, got {}x{}",
                id.0, expected_width, expected_height, record.scale, frame.width, frame.height
            )));
        }

        let buffer = record.buffers.acquire(id, frame, &shm, &self.qh)?;
        record.surface.set_buffer_scale(record.scale.max(1));
        record.surface.attach(Some(&buffer), 0, 0);
        if record.surface.version() >= 4 {
            record
                .surface
                .damage_buffer(0, 0, frame.width as i32, frame.height as i32);
        } else {
            record
                .surface
                .damage(0, 0, record.width as i32, record.height as i32);
        }
        record.surface.commit();
        self.connection
            .flush()
            .map_err(|error| BackendError::Dispatch(error.to_string()))?;
        Ok(())
    }

    pub fn destroy_surface(&mut self, id: SurfaceId) -> Result<()> {
        let mut record = self
            .state
            .surfaces
            .remove(&id)
            .ok_or(BackendError::UnknownSurface(id.0))?;
        if record.layer.is_alive() {
            record.layer.destroy();
        }
        if record.surface.is_alive() {
            record.surface.destroy();
        }
        record.buffers.destroy();
        Ok(())
    }

    pub fn blocking_dispatch(&mut self) -> Result<usize> {
        self.queue
            .blocking_dispatch(&mut self.state)
            .map_err(|error| BackendError::Dispatch(error.to_string()))
    }

    pub fn dispatch_pending(&mut self) -> Result<usize> {
        self.queue
            .dispatch_pending(&mut self.state)
            .map_err(|error| BackendError::Dispatch(error.to_string()))
    }

    pub fn roundtrip(&mut self) -> Result<usize> {
        self.queue
            .roundtrip(&mut self.state)
            .map_err(|error| BackendError::Dispatch(error.to_string()))
    }

    pub fn flush(&self) -> Result<()> {
        self.connection
            .flush()
            .map(|_| ())
            .map_err(|error| BackendError::Dispatch(error.to_string()))
    }

    pub fn poll_fd(&self) -> i32 {
        self.connection.backend().poll_fd().as_raw_fd()
    }

    pub fn next_event(&mut self) -> Option<BackendEvent> {
        self.state.events.pop_front()
    }

    pub fn drain_events(&mut self) -> impl Iterator<Item = BackendEvent> + '_ {
        self.state.events.drain(..)
    }
}

impl Drop for WaylandBackend {
    fn drop(&mut self) {
        let ids: Vec<_> = self.state.surfaces.keys().copied().collect();
        for id in ids {
            let _ = self.destroy_surface(id);
        }
        let _ = self.connection.flush();
    }
}

#[derive(Clone, Copy)]
enum LayerRole {
    Panel { exclusive_zone: i32 },
    Menu { margin_top: i32, margin_left: i32 },
}

impl Dispatch<wl_registry::WlRegistry, ()> for BackendState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _connection: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => match interface.as_str() {
                "wl_compositor" if state.compositor.is_none() => {
                    state.compositor = Some(registry.bind(name, version.min(6), qh, ()));
                    state.compositor_global = Some(name);
                }
                "wl_shm" if state.shm.is_none() => {
                    state.shm = Some(registry.bind(name, 1, qh, ()));
                    state.shm_global = Some(name);
                }
                "wl_seat" if state.seat.is_none() => {
                    state.seat = Some(registry.bind(name, version.min(9), qh, ()));
                    state.seat_global = Some(name);
                }
                "wl_output" => {
                    let id = OutputId(name);
                    let proxy = registry.bind(name, version.min(4), qh, id);
                    state.outputs.insert(
                        id,
                        OutputRecord {
                            proxy,
                            info: OutputInfo::new(id),
                        },
                    );
                }
                "zwlr_layer_shell_v1" if state.layer_shell.is_none() => {
                    state.layer_shell = Some(registry.bind(name, version.min(5), qh, ()));
                    state.layer_shell_global = Some(name);
                }
                _ => {}
            },
            wl_registry::Event::GlobalRemove { name } => {
                let id = OutputId(name);
                if state.outputs.remove(&id).is_some() {
                    for surface in state.surfaces.values_mut() {
                        surface.entered_outputs.remove(&id);
                    }
                    update_surface_scales(state);
                }
                if state.compositor_global == Some(name) {
                    state.compositor = None;
                }
                if state.shm_global == Some(name) {
                    state.shm = None;
                }
                if state.layer_shell_global == Some(name) {
                    state.layer_shell = None;
                }
                if state.seat_global == Some(name) {
                    state.seat = None;
                    state.pointer = None;
                    state.keyboard = None;
                    state.touch = None;
                }
            }
            _ => {}
        }
    }
}

delegate_noop!(BackendState: ignore wl_compositor::WlCompositor);
delegate_noop!(BackendState: ignore wl_shm_pool::WlShmPool);
delegate_noop!(BackendState: ignore ZwlrLayerShellV1);

impl Dispatch<wl_shm::WlShm, ()> for BackendState {
    fn event(
        state: &mut Self,
        _proxy: &wl_shm::WlShm,
        event: wl_shm::Event,
        _data: &(),
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_shm::Event::Format { format } = event {
            match format {
                WEnum::Value(wl_shm::Format::Argb8888) => state.shm_argb8888 = true,
                WEnum::Value(wl_shm::Format::Xrgb8888) => state.shm_xrgb8888 = true,
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_output::WlOutput, OutputId> for BackendState {
    fn event(
        state: &mut Self,
        _proxy: &wl_output::WlOutput,
        event: wl_output::Event,
        id: &OutputId,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let mut scale_changed = false;
        {
            let Some(output) = state.outputs.get_mut(id) else {
                return;
            };

            match event {
                wl_output::Event::Geometry {
                    physical_width,
                    physical_height,
                    transform,
                    ..
                } => {
                    output.info.physical_width_mm = physical_width;
                    output.info.physical_height_mm = physical_height;
                    output.info.transform = wayland_transform(transform);
                }
                wl_output::Event::Mode {
                    width,
                    height,
                    refresh,
                    ..
                } => {
                    output.info.mode_width = width;
                    output.info.mode_height = height;
                    output.info.refresh_mhz = refresh;
                }
                wl_output::Event::Scale { factor } => {
                    output.info.scale = factor.max(1);
                    scale_changed = true;
                }
                wl_output::Event::Name { name } => output.info.name = Some(name),
                wl_output::Event::Description { description } => {
                    output.info.description = Some(description)
                }
                _ => {}
            }
        }
        if scale_changed {
            update_surface_scales(state);
        }
    }
}

fn wayland_transform(transform: WEnum<wl_output::Transform>) -> OutputTransform {
    match transform {
        WEnum::Value(wl_output::Transform::Normal) => OutputTransform::Normal,
        WEnum::Value(wl_output::Transform::_90) => OutputTransform::Rotate90,
        WEnum::Value(wl_output::Transform::_180) => OutputTransform::Rotate180,
        WEnum::Value(wl_output::Transform::_270) => OutputTransform::Rotate270,
        WEnum::Value(wl_output::Transform::Flipped) => OutputTransform::Flipped,
        WEnum::Value(wl_output::Transform::Flipped90) => OutputTransform::Flipped90,
        WEnum::Value(wl_output::Transform::Flipped180) => OutputTransform::Flipped180,
        WEnum::Value(wl_output::Transform::Flipped270) => OutputTransform::Flipped270,
        _ => OutputTransform::Normal,
    }
}

impl Dispatch<wl_surface::WlSurface, SurfaceId> for BackendState {
    fn event(
        state: &mut Self,
        _proxy: &wl_surface::WlSurface,
        event: wl_surface::Event,
        id: &SurfaceId,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_surface::Event::Enter { output } => {
                let output_id = state
                    .outputs
                    .iter()
                    .find_map(|(output_id, record)| (record.proxy == output).then_some(*output_id));
                if let Some(output_id) = output_id {
                    if let Some(surface) = state.surfaces.get_mut(id) {
                        surface.entered_outputs.insert(output_id);
                    }
                    update_one_surface_scale(state, *id);
                }
            }
            wl_surface::Event::Leave { output } => {
                let output_id = state
                    .outputs
                    .iter()
                    .find_map(|(output_id, record)| (record.proxy == output).then_some(*output_id));
                if let Some(output_id) = output_id {
                    if let Some(surface) = state.surfaces.get_mut(id) {
                        surface.entered_outputs.remove(&output_id);
                    }
                    update_one_surface_scale(state, *id);
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, SurfaceId> for BackendState {
    fn event(
        state: &mut Self,
        layer: &ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        id: &SurfaceId,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                layer.ack_configure(serial);
                if let Some(surface) = state.surfaces.get_mut(id) {
                    surface.width = if width == 0 {
                        surface.requested_width
                    } else {
                        width
                    };
                    surface.height = if height == 0 {
                        surface.requested_height
                    } else {
                        height
                    };
                    surface.configured = true;
                    state.events.push_back(BackendEvent {
                        surface: Some(*id),
                        event: PlatformEvent::Configure {
                            width: surface.width,
                            height: surface.height,
                            scale: surface.scale,
                        },
                    });
                }
            }
            zwlr_layer_surface_v1::Event::Closed => {
                if let Some(surface) = state.surfaces.get_mut(id) {
                    surface.closed = true;
                }
                state.events.push_back(BackendEvent {
                    surface: Some(*id),
                    event: PlatformEvent::Close,
                });
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_buffer::WlBuffer, BufferKey> for BackendState {
    fn event(
        state: &mut Self,
        _proxy: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        key: &BufferKey,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = event {
            if let Some(surface) = state.surfaces.get_mut(&key.surface) {
                surface.buffers.release(*key);
            }
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for BackendState {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _data: &(),
        _connection: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities { capabilities } = event {
            let WEnum::Value(capabilities) = capabilities else {
                return;
            };
            let has_pointer = capabilities.contains(wl_seat::Capability::Pointer);
            let has_keyboard = capabilities.contains(wl_seat::Capability::Keyboard);
            let has_touch = capabilities.contains(wl_seat::Capability::Touch);

            if has_pointer && state.pointer.is_none() {
                state.pointer = Some(seat.get_pointer(qh, ()));
            } else if !has_pointer {
                state.pointer = None;
                state.pointer_surface = None;
            }

            if has_keyboard && state.keyboard.is_none() {
                state.keyboard = Some(seat.get_keyboard(qh, ()));
            } else if !has_keyboard {
                state.keyboard = None;
                state.keyboard_surface = None;
            }

            if has_touch && state.touch.is_none() {
                state.touch = Some(seat.get_touch(qh, ()));
            } else if !has_touch {
                state.touch = None;
                state.touch_surfaces.clear();
                state.active_touches.clear();
            }
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for BackendState {
    fn event(
        state: &mut Self,
        _proxy: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _data: &(),
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter {
                surface,
                surface_x,
                surface_y,
                ..
            } => {
                state.pointer_surface = find_surface(state, &surface);
                state.pointer_x = surface_x;
                state.pointer_y = surface_y;
                state.events.push_back(BackendEvent {
                    surface: state.pointer_surface,
                    event: PlatformEvent::PointerEnter {
                        x: surface_x,
                        y: surface_y,
                    },
                });
            }
            wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                ..
            } => {
                state.pointer_x = surface_x;
                state.pointer_y = surface_y;
                state.events.push_back(BackendEvent {
                    surface: state.pointer_surface,
                    event: PlatformEvent::PointerMove {
                        x: surface_x,
                        y: surface_y,
                    },
                });
            }
            wl_pointer::Event::Button {
                button,
                state: button_state,
                ..
            } => {
                let pressed =
                    matches!(button_state, WEnum::Value(wl_pointer::ButtonState::Pressed));
                state.events.push_back(BackendEvent {
                    surface: state.pointer_surface,
                    event: PlatformEvent::PointerButton {
                        x: state.pointer_x,
                        y: state.pointer_y,
                        pressed,
                        button,
                    },
                });
            }
            wl_pointer::Event::Leave { .. } => {
                state.events.push_back(BackendEvent {
                    surface: state.pointer_surface,
                    event: PlatformEvent::PointerLeave,
                });
                state.pointer_surface = None;
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_touch::WlTouch, ()> for BackendState {
    fn event(
        state: &mut Self,
        _proxy: &wl_touch::WlTouch,
        event: wl_touch::Event,
        _data: &(),
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_touch::Event::Down {
                id, surface, x, y, ..
            } => {
                let surface_id = find_surface(state, &surface);
                if let Some(surface_id) = surface_id {
                    state.touch_surfaces.insert(id, surface_id);
                }
                state.active_touches.insert(id);
                state.events.push_back(BackendEvent {
                    surface: surface_id,
                    event: PlatformEvent::TouchDown { id, x, y },
                });
            }
            wl_touch::Event::Motion { id, x, y, .. } => {
                state.events.push_back(BackendEvent {
                    surface: state.touch_surfaces.get(&id).copied(),
                    event: PlatformEvent::TouchMotion { id, x, y },
                });
            }
            wl_touch::Event::Up { id, .. } => {
                state.active_touches.remove(&id);
                let surface = state.touch_surfaces.remove(&id);
                state.events.push_back(BackendEvent {
                    surface,
                    event: PlatformEvent::TouchUp { id },
                });
            }
            wl_touch::Event::Cancel => {
                let ids = sorted_touch_ids(&state.active_touches);
                state.active_touches.clear();
                state.touch_surfaces.clear();
                state.events.push_back(BackendEvent {
                    surface: None,
                    event: PlatformEvent::TouchCancel { ids },
                });
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for BackendState {
    fn event(
        state: &mut Self,
        _proxy: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _data: &(),
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Enter { surface, .. } => {
                state.keyboard_surface = find_surface(state, &surface);
            }
            wl_keyboard::Event::Leave { .. } => state.keyboard_surface = None,
            wl_keyboard::Event::Key {
                key,
                state: key_state,
                ..
            } => {
                let pressed = matches!(key_state, WEnum::Value(wl_keyboard::KeyState::Pressed));
                if key == 42 || key == 54 {
                    state.shift_down = pressed;
                }
                let semantic = semantic_key(key, state.shift_down);
                state.events.push_back(BackendEvent {
                    surface: state.keyboard_surface,
                    event: PlatformEvent::Key {
                        key: semantic,
                        pressed,
                    },
                });
            }
            _ => {}
        }
    }
}

fn find_surface(state: &BackendState, proxy: &wl_surface::WlSurface) -> Option<SurfaceId> {
    state
        .surfaces
        .iter()
        .find_map(|(id, record)| (record.surface == *proxy).then_some(*id))
}

fn update_surface_scales(state: &mut BackendState) {
    let ids: Vec<_> = state.surfaces.keys().copied().collect();
    for id in ids {
        update_one_surface_scale(state, id);
    }
}

fn update_one_surface_scale(state: &mut BackendState, id: SurfaceId) {
    let scale = state
        .surfaces
        .get(&id)
        .map(|surface| {
            surface
                .entered_outputs
                .iter()
                .filter_map(|output_id| state.outputs.get(output_id))
                .map(|output| output.info.scale.max(1))
                .max()
                .unwrap_or(1)
        })
        .unwrap_or(1);

    if let Some(surface) = state.surfaces.get_mut(&id) {
        if surface.scale != scale {
            surface.scale = scale;
            if surface.configured {
                state.events.push_back(BackendEvent {
                    surface: Some(id),
                    event: PlatformEvent::Configure {
                        width: surface.width,
                        height: surface.height,
                        scale,
                    },
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_defaults_do_not_request_keyboard_focus() {
        let config = PanelConfig::default();
        assert_eq!(config.height, 48);
        assert_eq!(config.exclusive_zone, 48);
    }

    #[test]
    fn key_mapping_covers_required_controls() {
        assert_eq!(semantic_key(103, false), crate::Key::Up);
        assert_eq!(semantic_key(108, false), crate::Key::Down);
        assert_eq!(semantic_key(105, false), crate::Key::Left);
        assert_eq!(semantic_key(106, false), crate::Key::Right);
        assert_eq!(semantic_key(28, false), crate::Key::Enter);
        assert_eq!(semantic_key(1, false), crate::Key::Escape);
        assert_eq!(semantic_key(14, false), crate::Key::Backspace);
    }
}
