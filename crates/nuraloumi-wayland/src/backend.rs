use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    os::fd::AsRawFd,
};

use wayland_client::{
    delegate_noop, event_created_child,
    protocol::{
        wl_buffer, wl_compositor, wl_keyboard, wl_output, wl_pointer, wl_registry, wl_seat, wl_shm,
        wl_shm_pool, wl_surface, wl_touch,
    },
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum,
};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1::{self, ExtForeignToplevelHandleV1},
    ext_foreign_toplevel_list_v1::{self, ExtForeignToplevelListV1},
};
use wayland_protocols::ext::workspace::v1::client::{
    ext_workspace_group_handle_v1::{self, ExtWorkspaceGroupHandleV1},
    ext_workspace_handle_v1::{self, ExtWorkspaceHandleV1},
    ext_workspace_manager_v1::{self, ExtWorkspaceManagerV1},
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, ZwlrLayerSurfaceV1},
};

use crate::{
    foreign_toplevel::{parse_wlr_state, ForeignToplevelStore},
    shm::{BufferKey, ShmBuffers},
    types::{semantic_key, sorted_touch_ids},
    workspace::WorkspaceStore,
    BackendCapabilities, BackendError, BackendEvent, DismissBackdropConfig, Frame, MenuConfig,
    Modifiers, OutputId, OutputInfo, OutputTransform, PanelConfig, PanelEdge, PlatformEvent,
    Result, SurfaceId, ToplevelEvent, ToplevelId, ToplevelInfo, ToplevelSource, WorkspaceEvent,
    WorkspaceId, WorkspaceInfo,
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
    redraw_pending: bool,
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
    modifier_keys: BTreeSet<u32>,
    foreign_toplevel: ForeignToplevelStore,
    workspace: WorkspaceStore,
}

pub struct WaylandBackend {
    connection: Connection,
    registry: wl_registry::WlRegistry,
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
        let registry = connection.display().get_registry(&qh, ());
        let mut backend = Self {
            connection,
            registry,
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
            toplevel: self.state.foreign_toplevel.capabilities(),
            workspace: self.state.workspace.capabilities(),
        }
    }

    pub fn toplevels(&self) -> Vec<ToplevelInfo> {
        self.state.foreign_toplevel.toplevels()
    }

    pub fn drain_toplevel_events(&mut self) -> impl Iterator<Item = ToplevelEvent> + '_ {
        self.state.foreign_toplevel.drain_events()
    }

    pub fn workspaces(&self) -> Vec<WorkspaceInfo> {
        self.state.workspace.workspaces()
    }

    pub fn drain_workspace_events(&mut self) -> impl Iterator<Item = WorkspaceEvent> + '_ {
        self.state.workspace.drain_events()
    }

    pub fn activate_workspace(&mut self, id: WorkspaceId) -> Result<()> {
        let handle = self.state.workspace.activatable_handle(id).ok_or_else(|| {
            BackendError::Dispatch(format!("workspace {id} is unknown or not activatable"))
        })?;
        let manager = self.state.workspace.manager().ok_or_else(|| {
            BackendError::Dispatch("ext-workspace manager is not available".to_owned())
        })?;
        handle.activate();
        manager.commit();
        self.flush()
    }

    pub fn activate_toplevel(&mut self, id: ToplevelId) -> Result<()> {
        let handle = self.state.foreign_toplevel.wlr_handle(id).ok_or_else(|| {
            BackendError::Dispatch(format!("toplevel {id} is unknown or not controllable"))
        })?;
        let seat = self
            .state
            .seat
            .as_ref()
            .ok_or(BackendError::MissingGlobal("wl_seat"))?;
        handle.activate(seat);
        self.flush()
    }

    pub fn set_toplevel_fullscreen(
        &mut self,
        id: ToplevelId,
        fullscreen: bool,
        output: Option<OutputId>,
    ) -> Result<()> {
        let handle = self.state.foreign_toplevel.wlr_handle(id).ok_or_else(|| {
            BackendError::Dispatch(format!("toplevel {id} is unknown or not controllable"))
        })?;
        if handle.version() < 2 {
            return Err(BackendError::Dispatch(
                "foreign-toplevel fullscreen requires protocol version 2+".to_owned(),
            ));
        }
        if fullscreen {
            let output_proxy = output
                .map(|output_id| {
                    self.state
                        .outputs
                        .get(&output_id)
                        .map(|record| record.proxy.clone())
                        .ok_or_else(|| {
                            BackendError::Dispatch(format!("unknown output {}", output_id.0))
                        })
                })
                .transpose()?;
            handle.set_fullscreen(output_proxy.as_ref());
        } else {
            handle.unset_fullscreen();
        }
        self.flush()
    }

    pub fn close_toplevel(&mut self, id: ToplevelId) -> Result<()> {
        let handle = self.state.foreign_toplevel.wlr_handle(id).ok_or_else(|| {
            BackendError::Dispatch(format!("toplevel {id} is unknown or not controllable"))
        })?;
        handle.close();
        self.flush()
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
        self.create_panel_at(config, PanelEdge::Top)
    }

    /// Create a panel on any output edge while preserving the historical
    /// top-panel behavior of create_panel(). PanelConfig::height is treated as
    /// the panel thickness for vertical edges.
    pub fn create_panel_at(&mut self, config: PanelConfig, edge: PanelEdge) -> Result<SurfaceId> {
        validate_panel_config(&config)?;
        let (width, height) = panel_size(edge, config.height);
        self.create_layer_surface(
            config.output,
            width,
            height,
            config.namespace,
            LayerRole::Panel {
                edge,
                exclusive_zone: config.exclusive_zone,
            },
        )
    }

    pub fn create_menu(&mut self, config: MenuConfig) -> Result<SurfaceId> {
        validate_menu_config(&config)?;
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

    /// Create a transparent, full-output overlay intended to receive input that
    /// lands outside a higher transient menu surface.
    ///
    /// Create this surface immediately before the menu so the menu stacks above
    /// it. The shell must present an ARGB buffer after Configure before relying
    /// on it for input. Non-negative margins can leave a persistent panel strip
    /// uncovered and interactive.
    pub fn create_dismiss_backdrop(&mut self, config: DismissBackdropConfig) -> Result<SurfaceId> {
        validate_dismiss_backdrop_config(&config)?;
        self.create_layer_surface(
            config.output,
            0,
            0,
            config.namespace,
            LayerRole::DismissBackdrop {
                margin_top: config.margin_top,
                margin_right: config.margin_right,
                margin_bottom: config.margin_bottom,
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

        let layer_kind = layer_kind(role);
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
            LayerRole::Panel {
                edge,
                exclusive_zone,
            } => {
                layer.set_anchor(panel_anchor(edge));
                layer.set_exclusive_zone(exclusive_zone);
            }
            LayerRole::Menu {
                margin_top,
                margin_left,
            } => {
                layer.set_anchor(
                    zwlr_layer_surface_v1::Anchor::Top | zwlr_layer_surface_v1::Anchor::Left,
                );
                layer.set_margin(margin_top, 0, 0, margin_left);
            }
            LayerRole::DismissBackdrop {
                margin_top,
                margin_right,
                margin_bottom,
                margin_left,
            } => {
                layer.set_anchor(dismiss_backdrop_anchor());
                layer.set_margin(margin_top, margin_right, margin_bottom, margin_left);
                layer.set_exclusive_zone(0);
            }
        }
        layer.set_keyboard_interactivity(layer_keyboard_interactivity(role));

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
                redraw_pending: false,
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
        let surface_version = record.surface.version();
        let scale = record.scale.max(1);
        if scale > 1 && surface_version < 3 {
            return Err(BackendError::UnsupportedBufferScale {
                surface_version,
                scale,
            });
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

        let buffer = match record.buffers.acquire(id, frame, &shm, &self.qh) {
            Ok(buffer) => {
                record.redraw_pending = false;
                buffer
            }
            Err(BackendError::WouldBlock) => {
                record.redraw_pending = true;
                return Err(BackendError::WouldBlock);
            }
            Err(error) => return Err(error),
        };
        if surface_version >= 3 {
            record.surface.set_buffer_scale(scale);
        }
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

    fn refresh_workspace_binding(&mut self) -> Result<()> {
        if self.state.workspace.has_binding() {
            return Ok(());
        }
        if let Some((name, version)) = self.state.workspace.global() {
            let manager: ExtWorkspaceManagerV1 =
                self.registry.bind(name, version.min(1), &self.qh, ());
            self.state.workspace.bind(manager);
            self.flush()?;
        }
        Ok(())
    }

    pub fn destroy_surface(&mut self, id: SurfaceId) -> Result<()> {
        let mut record = self
            .state
            .surfaces
            .remove(&id)
            .ok_or(BackendError::UnknownSurface(id.0))?;
        clear_surface_input_routes(&mut self.state, id);
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
        let dispatched = self
            .queue
            .blocking_dispatch(&mut self.state)
            .map_err(|error| BackendError::Dispatch(error.to_string()))?;
        self.refresh_foreign_toplevel_binding()?;
        self.refresh_workspace_binding()?;
        Ok(dispatched)
    }

    pub fn dispatch_pending(&mut self) -> Result<usize> {
        let dispatched = self
            .queue
            .dispatch_pending(&mut self.state)
            .map_err(|error| BackendError::Dispatch(error.to_string()))?;
        self.refresh_foreign_toplevel_binding()?;
        self.refresh_workspace_binding()?;
        Ok(dispatched)
    }

    pub fn roundtrip(&mut self) -> Result<usize> {
        let dispatched = self
            .queue
            .roundtrip(&mut self.state)
            .map_err(|error| BackendError::Dispatch(error.to_string()))?;
        self.refresh_foreign_toplevel_binding()?;
        self.refresh_workspace_binding()?;
        Ok(dispatched)
    }

    fn refresh_foreign_toplevel_binding(&mut self) -> Result<()> {
        if self.state.foreign_toplevel.has_binding() {
            return Ok(());
        }
        if let Some((name, version)) = self.state.foreign_toplevel.wlr_global() {
            let manager: ZwlrForeignToplevelManagerV1 =
                self.registry.bind(name, version.min(3), &self.qh, ());
            self.state.foreign_toplevel.bind_wlr(manager);
            self.flush()?;
        } else if let Some((name, version)) = self.state.foreign_toplevel.ext_global() {
            let list: ExtForeignToplevelListV1 =
                self.registry.bind(name, version.min(1), &self.qh, ());
            self.state.foreign_toplevel.bind_ext(list);
            self.flush()?;
        }
        Ok(())
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
    Panel {
        edge: PanelEdge,
        exclusive_zone: i32,
    },
    Menu {
        margin_top: i32,
        margin_left: i32,
    },
    DismissBackdrop {
        margin_top: i32,
        margin_right: i32,
        margin_bottom: i32,
        margin_left: i32,
    },
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
                "zwlr_foreign_toplevel_manager_v1" => {
                    state.foreign_toplevel.record_wlr_global(name, version);
                }
                "ext_foreign_toplevel_list_v1" => {
                    state.foreign_toplevel.record_ext_global(name, version);
                }
                "ext_workspace_manager_v1" => {
                    state.workspace.record_global(name, version);
                }
                _ => {}
            },
            wl_registry::Event::GlobalRemove { name } => {
                state.foreign_toplevel.remove_global(name);
                state.workspace.remove_global(name);
                let id = OutputId(name);
                if let Some(output) = state.outputs.remove(&id) {
                    release_output(output.proxy);
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
                    if let Some(layer_shell) = state.layer_shell.take() {
                        if layer_shell.is_alive() {
                            layer_shell.destroy();
                        }
                    }
                }
                if state.seat_global == Some(name) {
                    release_input_devices(state);
                    if let Some(seat) = state.seat.take() {
                        release_seat(seat);
                    }
                }
            }
            _ => {}
        }
    }
}

delegate_noop!(BackendState: ignore wl_compositor::WlCompositor);
delegate_noop!(BackendState: ignore wl_shm_pool::WlShmPool);
delegate_noop!(BackendState: ignore ZwlrLayerShellV1);

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for BackendState {
    fn event(
        state: &mut Self,
        _proxy: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _data: &(),
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } => {
                state.foreign_toplevel.register_wlr(toplevel);
            }
            zwlr_foreign_toplevel_manager_v1::Event::Finished => {
                state
                    .foreign_toplevel
                    .manager_finished(ToplevelSource::WlrManagement);
            }
            _ => {}
        }
    }

    event_created_child!(BackendState, ZwlrForeignToplevelManagerV1, [
        zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for BackendState {
    fn event(
        state: &mut Self,
        proxy: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _data: &(),
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let object = proxy.id();
        match event {
            zwlr_foreign_toplevel_handle_v1::Event::Title { title } => {
                state.foreign_toplevel.set_wlr_title(&object, title);
            }
            zwlr_foreign_toplevel_handle_v1::Event::AppId { app_id } => {
                state.foreign_toplevel.set_wlr_app_id(&object, app_id);
            }
            zwlr_foreign_toplevel_handle_v1::Event::State { state: raw } => {
                state
                    .foreign_toplevel
                    .set_wlr_state(&object, parse_wlr_state(&raw));
            }
            zwlr_foreign_toplevel_handle_v1::Event::OutputEnter { output } => {
                if let Some(id) = output_id_for_proxy(state, &output) {
                    state.foreign_toplevel.wlr_output_enter(&object, id);
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::OutputLeave { output } => {
                if let Some(id) = output_id_for_proxy(state, &output) {
                    state.foreign_toplevel.wlr_output_leave(&object, id);
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::Done => {
                state.foreign_toplevel.commit_wlr(&object);
            }
            zwlr_foreign_toplevel_handle_v1::Event::Closed => {
                if let Some(handle) = state.foreign_toplevel.close_wlr(&object) {
                    if handle.is_alive() {
                        handle.destroy();
                    }
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtForeignToplevelListV1, ()> for BackendState {
    fn event(
        state: &mut Self,
        proxy: &ExtForeignToplevelListV1,
        event: ext_foreign_toplevel_list_v1::Event,
        _data: &(),
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } => {
                state.foreign_toplevel.register_ext(toplevel);
            }
            ext_foreign_toplevel_list_v1::Event::Finished => {
                state
                    .foreign_toplevel
                    .manager_finished(ToplevelSource::ExtList);
                if proxy.is_alive() {
                    proxy.destroy();
                }
            }
            _ => {}
        }
    }

    event_created_child!(BackendState, ExtForeignToplevelListV1, [
        ext_foreign_toplevel_list_v1::EVT_TOPLEVEL_OPCODE => (ExtForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ExtForeignToplevelHandleV1, ()> for BackendState {
    fn event(
        state: &mut Self,
        proxy: &ExtForeignToplevelHandleV1,
        event: ext_foreign_toplevel_handle_v1::Event,
        _data: &(),
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let object = proxy.id();
        match event {
            ext_foreign_toplevel_handle_v1::Event::Title { title } => {
                state.foreign_toplevel.set_ext_title(&object, title);
            }
            ext_foreign_toplevel_handle_v1::Event::AppId { app_id } => {
                state.foreign_toplevel.set_ext_app_id(&object, app_id);
            }
            ext_foreign_toplevel_handle_v1::Event::Identifier { identifier } => {
                state
                    .foreign_toplevel
                    .set_ext_identifier(&object, identifier);
            }
            ext_foreign_toplevel_handle_v1::Event::Done => {
                state.foreign_toplevel.commit_ext(&object);
            }
            ext_foreign_toplevel_handle_v1::Event::Closed => {
                if let Some(handle) = state.foreign_toplevel.close_ext(&object) {
                    if handle.is_alive() {
                        handle.destroy();
                    }
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtWorkspaceManagerV1, ()> for BackendState {
    fn event(
        state: &mut Self,
        _proxy: &ExtWorkspaceManagerV1,
        event: ext_workspace_manager_v1::Event,
        _data: &(),
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            ext_workspace_manager_v1::Event::Workspace { workspace } => {
                state.workspace.register(workspace);
            }
            ext_workspace_manager_v1::Event::Done => {
                state.workspace.commit_done();
            }
            ext_workspace_manager_v1::Event::Finished => {
                state.workspace.finish();
            }
            _ => {}
        }
    }

    event_created_child!(BackendState, ExtWorkspaceManagerV1, [
        ext_workspace_manager_v1::EVT_WORKSPACE_GROUP_OPCODE => (ExtWorkspaceGroupHandleV1, ()),
        ext_workspace_manager_v1::EVT_WORKSPACE_OPCODE => (ExtWorkspaceHandleV1, ()),
    ]);
}

impl Dispatch<ExtWorkspaceGroupHandleV1, ()> for BackendState {
    fn event(
        _state: &mut Self,
        proxy: &ExtWorkspaceGroupHandleV1,
        event: ext_workspace_group_handle_v1::Event,
        _data: &(),
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if matches!(event, ext_workspace_group_handle_v1::Event::Removed) && proxy.is_alive() {
            proxy.destroy();
        }
    }
}

impl Dispatch<ExtWorkspaceHandleV1, ()> for BackendState {
    fn event(
        state: &mut Self,
        proxy: &ExtWorkspaceHandleV1,
        event: ext_workspace_handle_v1::Event,
        _data: &(),
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let object = proxy.id();
        match event {
            ext_workspace_handle_v1::Event::Id { id } => {
                state.workspace.set_protocol_id(&object, id);
            }
            ext_workspace_handle_v1::Event::Name { name } => {
                state.workspace.set_name(&object, name);
            }
            ext_workspace_handle_v1::Event::State {
                state: WEnum::Value(flags),
            } => {
                state.workspace.set_state(&object, flags.bits());
            }
            ext_workspace_handle_v1::Event::Capabilities {
                capabilities: WEnum::Value(capabilities),
            } => {
                state
                    .workspace
                    .set_capabilities(&object, capabilities.bits());
            }
            ext_workspace_handle_v1::Event::Removed => {
                state.workspace.mark_removed(&object);
            }
            _ => {}
        }
    }
}

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
                    flags,
                    width,
                    height,
                    refresh,
                } => {
                    if mode_is_current(flags) {
                        output.info.mode_width = width;
                        output.info.mode_height = height;
                        output.info.refresh_mhz = refresh;
                    }
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

fn output_id_for_proxy(state: &BackendState, proxy: &wl_output::WlOutput) -> Option<OutputId> {
    state
        .outputs
        .iter()
        .find_map(|(id, record)| (record.proxy.id() == proxy.id()).then_some(*id))
}

fn validate_panel_config(config: &PanelConfig) -> Result<()> {
    if config.height == 0 {
        return Err(BackendError::InvalidSurfaceConfig(
            "panel thickness must be non-zero".to_owned(),
        ));
    }
    Ok(())
}

fn validate_menu_config(config: &MenuConfig) -> Result<()> {
    if config.width == 0 || config.height == 0 {
        return Err(BackendError::InvalidSurfaceConfig(
            "menu width and height must both be non-zero".to_owned(),
        ));
    }
    Ok(())
}

fn validate_dismiss_backdrop_config(config: &DismissBackdropConfig) -> Result<()> {
    if [
        config.margin_top,
        config.margin_right,
        config.margin_bottom,
        config.margin_left,
    ]
    .into_iter()
    .any(|margin| margin < 0)
    {
        return Err(BackendError::InvalidSurfaceConfig(
            "dismiss backdrop margins must be non-negative".to_owned(),
        ));
    }
    Ok(())
}

fn layer_kind(role: LayerRole) -> zwlr_layer_shell_v1::Layer {
    match role {
        LayerRole::Panel { .. } => zwlr_layer_shell_v1::Layer::Top,
        LayerRole::Menu { .. } | LayerRole::DismissBackdrop { .. } => {
            zwlr_layer_shell_v1::Layer::Overlay
        }
    }
}

fn layer_keyboard_interactivity(role: LayerRole) -> zwlr_layer_surface_v1::KeyboardInteractivity {
    match role {
        LayerRole::Menu { .. } => zwlr_layer_surface_v1::KeyboardInteractivity::Exclusive,
        LayerRole::Panel { .. } | LayerRole::DismissBackdrop { .. } => {
            zwlr_layer_surface_v1::KeyboardInteractivity::None
        }
    }
}

fn dismiss_backdrop_anchor() -> zwlr_layer_surface_v1::Anchor {
    zwlr_layer_surface_v1::Anchor::Top
        | zwlr_layer_surface_v1::Anchor::Right
        | zwlr_layer_surface_v1::Anchor::Bottom
        | zwlr_layer_surface_v1::Anchor::Left
}

fn panel_size(edge: PanelEdge, thickness: u32) -> (u32, u32) {
    if edge.is_horizontal() {
        (0, thickness)
    } else {
        (thickness, 0)
    }
}

fn panel_anchor(edge: PanelEdge) -> zwlr_layer_surface_v1::Anchor {
    match edge {
        PanelEdge::Top => {
            zwlr_layer_surface_v1::Anchor::Top
                | zwlr_layer_surface_v1::Anchor::Left
                | zwlr_layer_surface_v1::Anchor::Right
        }
        PanelEdge::Bottom => {
            zwlr_layer_surface_v1::Anchor::Bottom
                | zwlr_layer_surface_v1::Anchor::Left
                | zwlr_layer_surface_v1::Anchor::Right
        }
        PanelEdge::Left => {
            zwlr_layer_surface_v1::Anchor::Left
                | zwlr_layer_surface_v1::Anchor::Top
                | zwlr_layer_surface_v1::Anchor::Bottom
        }
        PanelEdge::Right => {
            zwlr_layer_surface_v1::Anchor::Right
                | zwlr_layer_surface_v1::Anchor::Top
                | zwlr_layer_surface_v1::Anchor::Bottom
        }
    }
}

fn mode_is_current(flags: WEnum<wl_output::Mode>) -> bool {
    match flags {
        WEnum::Value(flags) => flags.contains(wl_output::Mode::Current),
        // Preserve the well-known CURRENT bit even if a newer compositor adds
        // mode flag bits unknown to this generated protocol version.
        WEnum::Unknown(raw) => raw & 0x1 != 0,
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
            let retry = state.surfaces.get_mut(&key.surface).and_then(|surface| {
                let released = surface.buffers.release(*key);
                if take_redraw_retry(&mut surface.redraw_pending, released)
                    && surface.configured
                    && !surface.closed
                {
                    Some(PlatformEvent::Configure {
                        width: surface.width,
                        height: surface.height,
                        scale: surface.scale,
                    })
                } else {
                    None
                }
            });

            if let Some(event) = retry {
                state.events.push_back(BackendEvent {
                    surface: Some(key.surface),
                    event,
                });
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
                if let Some(pointer) = state.pointer.take() {
                    release_pointer(pointer);
                }
                state.pointer_surface = None;
            }

            if has_keyboard && state.keyboard.is_none() {
                state.keyboard = Some(seat.get_keyboard(qh, ()));
            } else if !has_keyboard {
                if let Some(keyboard) = state.keyboard.take() {
                    release_keyboard(keyboard);
                }
                state.keyboard_surface = None;
                state.modifier_keys.clear();
            }

            if has_touch && state.touch.is_none() {
                state.touch = Some(seat.get_touch(qh, ()));
            } else if !has_touch {
                if let Some(touch) = state.touch.take() {
                    release_touch(touch);
                }
                cancel_active_touches(state);
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
                let WEnum::Value(button_state) = button_state else {
                    return;
                };
                let pressed = matches!(button_state, wl_pointer::ButtonState::Pressed);
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
            wl_touch::Event::Cancel => cancel_active_touches(state),
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
            wl_keyboard::Event::Leave { .. } => {
                state.keyboard_surface = None;
                state.modifier_keys.clear();
            }
            wl_keyboard::Event::Key {
                key,
                state: key_state,
                ..
            } => {
                let WEnum::Value(key_state) = key_state else {
                    return;
                };
                let pressed = matches!(key_state, wl_keyboard::KeyState::Pressed);
                update_modifier_key(&mut state.modifier_keys, key, pressed);
                let modifiers = modifiers_from_keys(&state.modifier_keys);
                let semantic = semantic_key(key, modifiers.shift);
                let semantic = if modifiers.is_empty() {
                    semantic
                } else {
                    crate::Key::Modified {
                        key: Box::new(semantic),
                        modifiers,
                    }
                };
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

fn release_output(output: wl_output::WlOutput) {
    if output.version() >= 3 && output.is_alive() {
        output.release();
    }
}

fn release_pointer(pointer: wl_pointer::WlPointer) {
    if pointer.version() >= 3 && pointer.is_alive() {
        pointer.release();
    }
}

fn release_keyboard(keyboard: wl_keyboard::WlKeyboard) {
    if keyboard.version() >= 3 && keyboard.is_alive() {
        keyboard.release();
    }
}

fn update_modifier_key(keys: &mut BTreeSet<u32>, key: u32, pressed: bool) {
    if !is_modifier_key(key) {
        return;
    }
    if pressed {
        keys.insert(key);
    } else {
        keys.remove(&key);
    }
}

fn is_modifier_key(key: u32) -> bool {
    matches!(key, 29 | 42 | 54 | 56 | 97 | 100 | 125 | 126)
}

fn modifiers_from_keys(keys: &BTreeSet<u32>) -> Modifiers {
    Modifiers {
        ctrl: keys.contains(&29) || keys.contains(&97),
        alt: keys.contains(&56) || keys.contains(&100),
        shift: keys.contains(&42) || keys.contains(&54),
        super_key: keys.contains(&125) || keys.contains(&126),
    }
}

fn release_touch(touch: wl_touch::WlTouch) {
    if touch.version() >= 3 && touch.is_alive() {
        touch.release();
    }
}

fn release_seat(seat: wl_seat::WlSeat) {
    if seat.version() >= 5 && seat.is_alive() {
        seat.release();
    }
}

fn cancel_active_touches(state: &mut BackendState) {
    let ids = sorted_touch_ids(&state.active_touches);
    state.active_touches.clear();
    state.touch_surfaces.clear();
    if !ids.is_empty() {
        state.events.push_back(BackendEvent {
            surface: None,
            event: PlatformEvent::TouchCancel { ids },
        });
    }
}

fn clear_surface_input_routes(state: &mut BackendState, id: SurfaceId) {
    if state.pointer_surface == Some(id) {
        state.pointer_surface = None;
    }
    if state.keyboard_surface == Some(id) {
        state.keyboard_surface = None;
    }

    let touch_ids: Vec<_> = state
        .touch_surfaces
        .iter()
        .filter_map(|(touch_id, surface_id)| (*surface_id == id).then_some(*touch_id))
        .collect();
    for touch_id in touch_ids {
        state.touch_surfaces.remove(&touch_id);
        state.active_touches.remove(&touch_id);
    }
}

fn release_input_devices(state: &mut BackendState) {
    if let Some(pointer) = state.pointer.take() {
        release_pointer(pointer);
    }
    if let Some(keyboard) = state.keyboard.take() {
        release_keyboard(keyboard);
    }
    if let Some(touch) = state.touch.take() {
        release_touch(touch);
    }
    state.pointer_surface = None;
    state.keyboard_surface = None;
    state.modifier_keys.clear();
    cancel_active_touches(state);
}

fn take_redraw_retry(pending: &mut bool, released: bool) -> bool {
    if released && *pending {
        *pending = false;
        true
    } else {
        false
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
    fn modifier_tracking_handles_both_sides_without_stuck_state() {
        let mut keys = BTreeSet::new();
        update_modifier_key(&mut keys, 29, true);
        update_modifier_key(&mut keys, 97, true);
        update_modifier_key(&mut keys, 42, true);
        update_modifier_key(&mut keys, 56, true);
        update_modifier_key(&mut keys, 125, true);

        assert_eq!(
            modifiers_from_keys(&keys),
            Modifiers {
                ctrl: true,
                alt: true,
                shift: true,
                super_key: true,
            }
        );

        update_modifier_key(&mut keys, 29, false);
        assert!(modifiers_from_keys(&keys).ctrl);
        update_modifier_key(&mut keys, 97, false);
        assert!(!modifiers_from_keys(&keys).ctrl);

        update_modifier_key(&mut keys, 42, false);
        update_modifier_key(&mut keys, 56, false);
        update_modifier_key(&mut keys, 125, false);
        assert!(modifiers_from_keys(&keys).is_empty());

        update_modifier_key(&mut keys, 30, true);
        assert!(keys.is_empty());
    }

    #[test]
    fn panel_defaults_do_not_request_keyboard_focus() {
        let config = PanelConfig::default();
        assert_eq!(config.height, 48);
        assert_eq!(config.exclusive_zone, 48);
        assert_eq!(panel_size(PanelEdge::Top, config.height), (0, 48));
    }

    #[test]
    fn panel_edge_plans_fill_the_perpendicular_axis() {
        assert_eq!(panel_size(PanelEdge::Top, 52), (0, 52));
        assert_eq!(panel_size(PanelEdge::Bottom, 52), (0, 52));
        assert_eq!(panel_size(PanelEdge::Left, 52), (52, 0));
        assert_eq!(panel_size(PanelEdge::Right, 52), (52, 0));

        let left = panel_anchor(PanelEdge::Left);
        assert!(left.contains(zwlr_layer_surface_v1::Anchor::Left));
        assert!(left.contains(zwlr_layer_surface_v1::Anchor::Top));
        assert!(left.contains(zwlr_layer_surface_v1::Anchor::Bottom));
        assert!(!left.contains(zwlr_layer_surface_v1::Anchor::Right));
    }

    #[test]
    fn surface_configs_reject_zero_thickness_and_menu_extent() {
        let panel = PanelConfig {
            height: 0,
            ..PanelConfig::default()
        };
        assert!(matches!(
            validate_panel_config(&panel),
            Err(BackendError::InvalidSurfaceConfig(_))
        ));

        let menu = MenuConfig {
            width: 0,
            ..MenuConfig::default()
        };
        assert!(matches!(
            validate_menu_config(&menu),
            Err(BackendError::InvalidSurfaceConfig(_))
        ));

        let backdrop = DismissBackdropConfig {
            margin_top: -1,
            ..DismissBackdropConfig::default()
        };
        assert!(matches!(
            validate_dismiss_backdrop_config(&backdrop),
            Err(BackendError::InvalidSurfaceConfig(_))
        ));
        assert!(validate_panel_config(&PanelConfig::default()).is_ok());
        assert!(validate_menu_config(&MenuConfig::default()).is_ok());
        assert!(validate_dismiss_backdrop_config(&DismissBackdropConfig::default()).is_ok());
    }

    #[test]
    fn dismiss_backdrop_plan_is_full_output_overlay_without_keyboard_focus() {
        let role = LayerRole::DismissBackdrop {
            margin_top: 48,
            margin_right: 0,
            margin_bottom: 0,
            margin_left: 0,
        };
        assert!(matches!(
            layer_kind(role),
            zwlr_layer_shell_v1::Layer::Overlay
        ));
        assert!(matches!(
            layer_keyboard_interactivity(role),
            zwlr_layer_surface_v1::KeyboardInteractivity::None
        ));

        let anchor = dismiss_backdrop_anchor();
        assert!(anchor.contains(zwlr_layer_surface_v1::Anchor::Top));
        assert!(anchor.contains(zwlr_layer_surface_v1::Anchor::Right));
        assert!(anchor.contains(zwlr_layer_surface_v1::Anchor::Bottom));
        assert!(anchor.contains(zwlr_layer_surface_v1::Anchor::Left));

        assert!(matches!(
            layer_keyboard_interactivity(LayerRole::Menu {
                margin_top: 0,
                margin_left: 0,
            }),
            zwlr_layer_surface_v1::KeyboardInteractivity::Exclusive
        ));
    }

    #[test]
    fn destroying_surface_clears_only_its_input_routes() {
        let dead = SurfaceId(7);
        let live = SurfaceId(9);
        let mut state = BackendState {
            pointer_surface: Some(dead),
            keyboard_surface: Some(dead),
            touch_surfaces: HashMap::from([(3, dead), (8, live)]),
            active_touches: BTreeSet::from([3, 8]),
            ..BackendState::default()
        };

        clear_surface_input_routes(&mut state, dead);

        assert_eq!(state.pointer_surface, None);
        assert_eq!(state.keyboard_surface, None);
        assert_eq!(state.touch_surfaces, HashMap::from([(8, live)]));
        assert_eq!(state.active_touches, BTreeSet::from([8]));
    }

    #[test]
    fn touch_cancel_ids_are_sorted_and_drained() {
        let mut state = BackendState::default();
        state.active_touches.insert(8);
        state.active_touches.insert(3);
        state.touch_surfaces.insert(8, SurfaceId(1));
        state.touch_surfaces.insert(3, SurfaceId(1));

        cancel_active_touches(&mut state);
        assert!(state.active_touches.is_empty());
        assert!(state.touch_surfaces.is_empty());
        assert_eq!(
            state.events.pop_front(),
            Some(BackendEvent {
                surface: None,
                event: PlatformEvent::TouchCancel { ids: vec![3, 8] },
            })
        );
    }

    #[test]
    fn current_mode_accepts_known_and_future_flag_sets() {
        assert!(mode_is_current(WEnum::Value(wl_output::Mode::Current)));
        assert!(!mode_is_current(WEnum::Value(wl_output::Mode::Preferred)));
        assert!(mode_is_current(WEnum::Unknown(0x8000_0001)));
    }

    #[test]
    fn redraw_retry_is_one_shot_per_real_release() {
        let mut pending = true;
        assert!(!take_redraw_retry(&mut pending, false));
        assert!(pending);
        assert!(take_redraw_retry(&mut pending, true));
        assert!(!pending);
        assert!(!take_redraw_retry(&mut pending, true));
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
