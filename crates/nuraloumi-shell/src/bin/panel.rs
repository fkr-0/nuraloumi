use nuraloumi_core::{DARK_THEME, LIGHT_THEME};
use nuraloumi_providers::{
    session::OwnedDesktopSession, ActionProvider, ActionResult, ApplicationProvider,
    ApplicationSnapshot, AudioAction, AudioProvider, BacklightAction, BacklightProvider,
    BluetoothAction, BluetoothProvider, Health, MediaAction, MediaProvider, NetworkAction,
    NetworkProvider, NotificationAction, NotificationProvider, ProbeSnapshot, ProcessProvider,
    ProcessSnapshot, Provider, SessionAction, SessionProvider, SnapshotMeta, SystemCommandRunner,
};
use nuraloumi_render_cairo::{
    CairoRenderer, Color, HitRegion as RenderHitRegion, PaintNode, Point, Rect, RenderOptions,
    RowKind, Scene, ScrollWindow, TextStyle, Theme as RenderTheme, Viewport,
};
use nuraloumi_shell::config_reload::{prepare_from_file, PreparedConfigReload};
use nuraloumi_shell::ipc::IpcResponse;
use nuraloumi_shell::surface_catalog::{IpcRuntime, SurfaceCatalog, SurfaceObservation};
use nuraloumi_shell::{
    apply_launcher_preferences, blank_press_requests_dismiss, build_family,
    dismiss_backdrop_enabled_for_renderer, dismiss_backdrop_margins, dispatch_keybinding,
    execute_window_command, launcher_search_input, load_config_or_default, load_fixture_snapshot,
    panel_affordances, parse_desktop_command, parse_family, parse_window_command,
    record_user_application_activation, refresh_user_launcher_state, render_menu_follow_selection,
    resolve_app_activation, successful_action_requests_dismiss, update_user_launcher_state,
    window_entries, ActionReport, AppActivation, ApplicationEntry as ShellApplicationEntry,
    BluetoothDeviceEntry, ControlCenterTab, DesktopCommand, DesktopControlCapabilities,
    DesktopEntry, FixtureSnapshot, HitRegion as ShellHitRegion, KeybindingRegistry,
    MediaPlayerEntry as ShellMediaPlayerEntry, MenuAction, MenuFamily,
    NotificationHistoryEntry as ShellNotificationHistoryEntry, OverviewMode, PanelAffordance,
    PanelController, PanelEdge as ShellPanelEdge, PlatformEvent as ShellPlatformEvent,
    ProviderValue, SceneTransitionClock, ShellConfig, ShellState, TaskEntry as ShellTaskEntry,
    Theme as ShellTheme, ValueState, WifiNetworkEntry, WindowCommand, WindowControlCapabilities,
    WindowThumbnailEntry,
};
use nuraloumi_wayland::{
    BackendCapabilities as WaylandCapabilities, BackendError, DismissBackdropConfig, Frame,
    MenuConfig as WaylandMenuConfig, PanelConfig as WaylandPanelConfig,
    PanelEdge as WaylandPanelEdge, PixelFormat, PlatformEvent as WaylandEvent, SurfaceId,
    WaylandBackend, WorkspaceId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

const PANEL_MENU_GAP: u32 = 6;

// musl's ARM `round` object in the generated SL101 sysroot uses d16, which is
// outside Tegra20's VFPv3-D16 register file. Cairo preview painting reaches
// f64::round() only after a thumbnail is available, so provide a target-local
// IEEE-754 implementation that performs the rounding decision in integer bits.
// Host builds keep the platform libc implementation.
#[cfg(target_arch = "arm")]
#[no_mangle]
pub extern "C" fn round(value: f64) -> f64 {
    sl101_round_bits(value)
}

#[cfg(any(test, target_arch = "arm"))]
fn sl101_round_bits(value: f64) -> f64 {
    const SIGN: u64 = 1_u64 << 63;
    const EXPONENT_MASK: u64 = 0x7ff;
    const FRACTION_BITS: i32 = 52;
    const EXPONENT_BIAS: i32 = 1023;

    let bits = value.to_bits();
    let raw_exponent = ((bits >> FRACTION_BITS) & EXPONENT_MASK) as i32;
    if raw_exponent == EXPONENT_MASK as i32 {
        return value;
    }

    let exponent = raw_exponent - EXPONENT_BIAS;
    if exponent >= FRACTION_BITS {
        return value;
    }
    if exponent < 0 {
        let sign = bits & SIGN;
        if exponent == -1 {
            return f64::from_bits(sign | (u64::from(EXPONENT_BIAS as u32) << FRACTION_BITS));
        }
        return f64::from_bits(sign);
    }

    let fractional_bits = (FRACTION_BITS - exponent) as u32;
    let fractional_mask = (1_u64 << fractional_bits) - 1;
    let fraction = bits & fractional_mask;
    if fraction == 0 {
        return value;
    }
    let half = 1_u64 << (fractional_bits - 1);
    let truncated = bits & !fractional_mask;
    let rounded = if fraction >= half {
        truncated + (1_u64 << fractional_bits)
    } else {
        truncated
    };
    f64::from_bits(rounded)
}

const HELP: &str = r#"nuraloumi-panel — NuraLoumi top panel

USAGE:
    nuraloumi-panel [OPTIONS]

OPTIONS:
    --headless                 Run without a compositor (default)
    --live                     Open a native Wayland/Cairo wl_shm panel
    --providers <path>         Load deterministic JSON/TOML provider snapshot
    --config <path>            Load JSON/TOML shell geometry/theme config
    --dump-keybindings         Print effective keybinding registry as JSON and exit
    --open <family>            Open launcher|control-center|network|audio|system initially
    --reduced-motion           Force reduced-motion state
    --enable-renderer-switch   Allow confirmed compositor restart in live mode
    --enable-power-actions     Allow confirmed suspend/reboot/poweroff in live mode
    -h, --help                 Show this help

LIVE MODE:
    Uses a layer-shell panel with keyboard interactivity disabled. Pointer/touch
    activation opens one transient interactive menu; the panel itself never
    requests keyboard focus. Without --providers, panel values come from the
    bounded live provider probe. Destructive power actions stay dry-run unless
    --enable-power-actions is supplied after UI confirmation.
"#;

#[derive(Debug, Default)]
struct Args {
    live: bool,
    providers: Option<PathBuf>,
    config: Option<PathBuf>,
    dump_keybindings: bool,
    open: Option<String>,
    reduced_motion: bool,
    enable_power_actions: bool,
    enable_renderer_switch: bool,
}

fn refresh_workspace_snapshot(snapshot: &mut FixtureSnapshot, backend: &WaylandBackend) {
    let capabilities = backend.capabilities().workspace;
    snapshot.desktop_capabilities = DesktopControlCapabilities {
        list: capabilities.list,
        switch: capabilities.activate,
        window_membership: false,
        move_window: false,
        sticky_window: false,
    };
    snapshot.desktops = backend
        .workspaces()
        .into_iter()
        .map(|workspace| DesktopEntry {
            id: workspace.id.to_string(),
            label: workspace.name,
            active: workspace.state.active,
            urgent: workspace.state.urgent,
            hidden: workspace.state.hidden,
            switchable: workspace.can_activate,
            window_count: 0,
        })
        .collect();
}

fn parse_workspace_id(value: &str) -> Result<WorkspaceId, String> {
    value
        .parse::<WorkspaceId>()
        .map_err(|error| format!("invalid opaque workspace id {value:?}: {error}"))
}

#[derive(Debug, Serialize)]
struct Output {
    mode: &'static str,
    config: ShellConfig,
    panel: PanelController,
    affordances: Vec<PanelAffordance>,
    active_menu: Option<ShellState>,
}

struct LiveMenu {
    surface: SurfaceId,
    dismiss_surface: Option<SurfaceId>,
    family: MenuFamily,
    shell: ShellState,
    geometry: Option<(u32, u32, i32)>,
    scene: Option<Scene>,
    touch_regions: BTreeMap<i32, Option<ShellHitRegion>>,
    motion: Option<SceneTransitionClock>,
}

fn surface_name(family: MenuFamily) -> &'static str {
    match family {
        MenuFamily::Launcher => "launcher",
        MenuFamily::ControlCenter => "control-center",
        MenuFamily::Network => "network",
        MenuFamily::Bluetooth => "bluetooth",
        MenuFamily::Display => "display",
        MenuFamily::Audio => "audio",
        MenuFamily::Power => "power",
        MenuFamily::Tasks => "tasks",
        MenuFamily::Windows => "windows",
        MenuFamily::System => "system",
    }
}

fn live_surface_observations(
    catalog: &SurfaceCatalog,
    menu: Option<&LiveMenu>,
) -> Vec<SurfaceObservation> {
    catalog.observe(
        menu.map(|current| surface_name(current.family)),
        menu.and_then(|current| current.shell.state.selected_id.as_deref()),
        menu.map_or(0, |current| {
            current.shell.state.visible_items(&current.shell.menu).len()
        }),
        std::process::id(),
    )
}

#[derive(Clone, Copy)]
struct LivePolicy {
    execute_provider_actions: bool,
    enable_power_actions: bool,
    enable_renderer_switch: bool,
}

#[derive(Debug, Deserialize)]
struct NotificationInvokeRequest {
    id: u32,
    action: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PanelTarget {
    family: MenuFamily,
    focus_search: bool,
    control_center_tab: Option<ControlCenterTab>,
}

#[derive(Clone, Copy)]
struct PanelActivationContext<'a> {
    output: Option<&'a nuraloumi_wayland::OutputInfo>,
    policy: LivePolicy,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct PanelSearchView {
    query: String,
    focused: bool,
}

// Wayland's pointer protocol does not expose whether a click was synthesized
// from a touchscreen. Correlate position, surface and a short time window
// instead of globally disabling the physical mouse after a touch.
#[derive(Clone, Copy, Debug)]
struct TouchPosition {
    surface: Option<SurfaceId>,
    x: f64,
    y: f64,
}

impl TouchPosition {
    fn near(self, other: Self) -> bool {
        self.surface == other.surface
            && (self.x - other.x).powi(2) + (self.y - other.y).powi(2) <= 28.0_f64.powi(2)
    }
}

#[derive(Default)]
struct SyntheticPointerFilter {
    touches: BTreeMap<i32, TouchPosition>,
    last_release: Option<(std::time::Instant, TouchPosition)>,
    suppressed_press: Option<TouchPosition>,
}

impl SyntheticPointerFilter {
    fn down(&mut self, id: i32, position: TouchPosition) {
        self.touches.insert(id, position);
    }

    fn motion(&mut self, id: i32, position: TouchPosition) {
        if let Some(previous) = self.touches.get_mut(&id) {
            *previous = position;
        }
    }

    fn up(&mut self, id: i32, now: std::time::Instant) {
        if let Some(position) = self.touches.remove(&id) {
            self.last_release = Some((now, position));
        }
    }

    fn cancel(&mut self, ids: &[i32]) {
        for id in ids {
            self.touches.remove(id);
        }
    }

    fn suppress_pointer(
        &mut self,
        position: TouchPosition,
        pressed: bool,
        now: std::time::Instant,
    ) -> bool {
        const DUPLICATE_WINDOW: std::time::Duration = std::time::Duration::from_millis(180);
        const CROSS_SURFACE_WINDOW: std::time::Duration = std::time::Duration::from_millis(80);

        if !pressed {
            return self
                .suppressed_press
                .take()
                .is_some_and(|down| down.near(position));
        }

        // An input sequence can be retargeted to a just-created menu/backdrop
        // after the touch released on the panel. Limit that special case to
        // 80 ms; a real pointer elsewhere on the panel is never blocked.
        let recent = self.last_release.is_some_and(|(at, touch)| {
            let elapsed = now.saturating_duration_since(at);
            elapsed <= DUPLICATE_WINDOW
                && (touch.near(position)
                    || (touch.surface != position.surface && elapsed <= CROSS_SURFACE_WINDOW))
        });
        let concurrent = self.touches.values().any(|touch| touch.near(position));
        let synthetic = recent || concurrent;
        self.suppressed_press = synthetic.then_some(position);
        synthetic
    }
}

fn should_dismiss_for_focus_change(previous: Option<&str>, current: Option<&str>) -> bool {
    // The menu itself may steal keyboard focus, clearing the previous
    // foreign-toplevel's focused flag without activating another window.
    current.is_some() && current != previous
}

fn update_panel_touch_press(pressed: &mut Option<String>, current: Option<&str>) {
    if pressed.as_deref() != current {
        *pressed = None;
    }
}

fn begin_panel_touch(presses: &mut BTreeMap<i32, Option<String>>, id: i32, target: Option<String>) {
    if presses.is_empty() {
        presses.insert(id, target);
    } else {
        // A multi-finger gesture is not a succession of independent clicks.
        for pressed in presses.values_mut() {
            *pressed = None;
        }
        presses.insert(id, None);
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("nuraloumi-panel: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    if env::args()
        .skip(1)
        .any(|arg| arg == "-h" || arg == "--help")
    {
        print!("{HELP}");
        return Ok(());
    }

    let args = parse_args()?;
    if args.enable_power_actions && !args.live {
        return Err("--enable-power-actions requires --live".into());
    }
    if args.enable_renderer_switch && !args.live {
        return Err("--enable-renderer-switch requires --live".into());
    }
    let mut config = load_config_or_default(args.config.as_deref())?;
    if args.reduced_motion {
        config.reduced_motion = true;
    }
    config.validate()?;
    let keybindings = KeybindingRegistry::from_config(&config.keybindings)?;
    if args.dump_keybindings {
        println!(
            "{}",
            serde_json::to_string_pretty(&keybindings.dump())
                .map_err(|error| format!("failed to serialize keybindings: {error}"))?
        );
        return Ok(());
    }

    let defer_live_provider_refresh = should_defer_live_provider_refresh(&args);
    let mut snapshot = if let Some(path) = args.providers.as_ref() {
        load_fixture_snapshot(path)?
    } else {
        FixtureSnapshot::default()
    };
    if args.live {
        let initial_family = args.open.as_deref().map(parse_family).transpose()?;
        return run_live(
            config,
            snapshot,
            keybindings,
            initial_family,
            LivePolicy {
                execute_provider_actions: args.providers.is_none(),
                enable_power_actions: args.enable_power_actions,
                enable_renderer_switch: args.enable_renderer_switch,
            },
            defer_live_provider_refresh,
            args.config.clone(),
            args.reduced_motion,
        );
    }

    apply_launcher_preferences(&mut snapshot.applications, &config.launcher);
    run_headless(config, snapshot, args.open.as_deref())
}

fn should_defer_live_provider_refresh(args: &Args) -> bool {
    args.live && args.providers.is_none()
}

fn run_headless(
    config: ShellConfig,
    snapshot: FixtureSnapshot,
    open: Option<&str>,
) -> Result<(), String> {
    let mut panel = PanelController::default();
    let active_menu = if let Some(family_name) = open {
        let family = parse_family(family_name)?;
        panel.open_menu(family, true);
        Some(ShellState::new(
            build_family(family, &snapshot),
            config.reduced_motion,
        )?)
    } else {
        None
    };

    let output = Output {
        mode: "headless",
        config,
        panel,
        affordances: live_panel_affordances(&snapshot, &PanelSearchView::default()),
        active_menu,
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&output)
            .map_err(|err| format!("failed to serialize headless output: {err}"))?
    );
    Ok(())
}

// The Wayland queue is never driven from the IPC thread. poll(2) waits on
// the compositor and a one-byte IPC owner wakeup without an idle timer.
// dispatch_pending handles already buffered events before the descriptor wait.
fn wait_for_panel_events(backend: &mut WaylandBackend, ipc: &IpcRuntime) -> Result<(), String> {
    #[repr(C)]
    struct PollFd {
        fd: i32,
        events: i16,
        revents: i16,
    }
    unsafe extern "C" {
        fn poll(fds: *mut PollFd, count: usize, timeout_ms: i32) -> i32;
    }
    const POLLIN: i16 = 0x0001;
    const POLLERR: i16 = 0x0008;
    const POLLHUP: i16 = 0x0010;
    const POLLNVAL: i16 = 0x0020;

    if backend.dispatch_pending().map_err(|e| e.to_string())? > 0 {
        return Ok(());
    }
    backend.flush().map_err(|e| e.to_string())?;
    let mut fds = [
        PollFd {
            fd: backend.poll_fd(),
            events: POLLIN,
            revents: 0,
        },
        PollFd {
            fd: ipc.wake_fd(),
            events: POLLIN,
            revents: 0,
        },
    ];
    loop {
        // SAFETY: two initialized C-compatible pollfd values are valid for
        // the duration of this synchronous call; neither FD is closed here.
        let result = unsafe { poll(fds.as_mut_ptr(), fds.len(), 1000) };
        if result >= 0 {
            break;
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(format!("Wayland/IPC event wait failed: {error}"));
        }
    }
    if fds[0].revents & (POLLERR | POLLHUP | POLLNVAL) != 0 {
        return Err("Wayland compositor connection became unavailable".into());
    }
    if fds[1].revents & (POLLERR | POLLHUP | POLLNVAL) != 0 {
        return Err("IPC owner wake descriptor became unavailable".into());
    }
    if fds[0].revents & POLLIN != 0 {
        backend
            .roundtrip()
            .map_err(|e| format!("Wayland dispatch failed: {e}"))?;
    }
    Ok(())
}

/// Complete a reload on the Wayland owner. Menu refresh is staged on a clone
/// before any authoritative state is swapped, preserving selection on errors.
#[allow(clippy::too_many_arguments)]
fn apply_live_config_reload(
    request: &nuraloumi_shell::surface_catalog::OwnerReloadRequest,
    config_path: Option<&Path>,
    forced_reduced_motion: bool,
    config: &mut ShellConfig,
    keybindings: &mut KeybindingRegistry,
    snapshot: &mut FixtureSnapshot,
    base_applications: &[ShellApplicationEntry],
    active_menu: &mut Option<LiveMenu>,
    panel_scene: &mut Option<Scene>,
    backend: &mut WaylandBackend,
    panel_surface: SurfaceId,
    renderer: &CairoRenderer,
    panel_geometry: Option<(u32, u32, i32)>,
) -> IpcResponse {
    let id = &request.id;
    let prepared = match prepare_from_file(config, config_path, forced_reduced_motion) {
        Ok(plan) => plan,
        Err(error) => return IpcResponse::failure(id, error.code, error.message),
    };
    if !prepared.changed {
        return IpcResponse::success(
            id,
            serde_json::json!({
                "applied": false, "changed": false, "restart_required": false,
                "source_version": prepared.source_version, "diagnostics": prepared.diagnostics,
            }),
        );
    }
    let PreparedConfigReload {
        config: next_config,
        keybindings: next_keybindings,
        diagnostics,
        source_version,
        ..
    } = prepared;

    let mut staged_apps = base_applications.to_vec();
    apply_launcher_preferences(&mut staged_apps, &next_config.launcher);
    let mut staged_snapshot = snapshot.clone();
    staged_snapshot.applications = staged_apps.clone();
    if let Err(message) = refresh_user_launcher_state(&mut staged_snapshot, &next_config.launcher) {
        return IpcResponse::failure(id, "launcher_state_error", message);
    }
    staged_apps = staged_snapshot.applications.clone();

    let staged_shell = if let Some(menu) = active_menu.as_ref() {
        let mut shell = menu.shell.clone();
        shell.reduced_motion = next_config.reduced_motion;
        if let Err(message) = shell.refresh_family(menu.family, &staged_snapshot) {
            return IpcResponse::failure(id, "apply_error", message);
        }
        Some(shell)
    } else {
        None
    };

    // Avoid applying a candidate whose caller already exhausted its bounded
    // IPC deadline while parsing or staging on a slow device.
    if request.expired() {
        return IpcResponse::failure(id, "reload_timeout", "reload expired before commit");
    }

    let old_config = std::mem::replace(config, next_config);
    let old_keybindings = std::mem::replace(keybindings, next_keybindings);
    let old_apps = std::mem::replace(&mut snapshot.applications, staged_apps);
    let old_recent = std::mem::replace(
        &mut snapshot.recent_application_ids,
        staged_snapshot.recent_application_ids.clone(),
    );
    let old_panel_scene = panel_scene.take();
    let (old_shell, old_menu_scene) = if let Some(menu) = active_menu.as_mut() {
        let shell = std::mem::replace(
            &mut menu.shell,
            staged_shell.expect("active menu has staged shell"),
        );
        (Some(shell), menu.scene.take())
    } else {
        (None, None)
    };

    // No provider action or input event can interleave this owner-thread
    // transaction. On an application/render error restore all in-memory state.
    let redraw = (|| -> Result<(), String> {
        if let Some(menu) = active_menu.as_mut() {
            if let Some(geometry) = menu.geometry {
                menu.scene = Some(render_menu_and_present(
                    backend,
                    menu.surface,
                    renderer,
                    &menu.shell,
                    snapshot,
                    config,
                    (geometry, None),
                )?);
            }
        }
        if let Some(geometry) = panel_geometry {
            let search = panel_search_view(active_menu.as_ref());
            *panel_scene = Some(render_panel_and_present(
                backend,
                panel_surface,
                renderer,
                snapshot,
                config,
                &search,
                geometry,
            )?);
        }
        Ok(())
    })();

    if let Err(message) = redraw {
        *config = old_config;
        *keybindings = old_keybindings;
        snapshot.applications = old_apps;
        snapshot.recent_application_ids = old_recent;
        *panel_scene = old_panel_scene;
        if let (Some(menu), Some(shell)) = (active_menu.as_mut(), old_shell) {
            menu.shell = shell;
            menu.scene = old_menu_scene;
        }
        return IpcResponse::failure(id, "apply_error", message);
    }
    if config.reduced_motion {
        if let Some(menu) = active_menu.as_mut() {
            menu.motion = None;
        }
    }
    IpcResponse::success(
        id,
        serde_json::json!({
            "applied": true, "changed": true, "restart_required": false,
            "source_version": source_version, "diagnostics": diagnostics,
        }),
    )
}

#[allow(clippy::too_many_arguments)]
fn run_live(
    config: ShellConfig,
    mut snapshot: FixtureSnapshot,
    mut keybindings: KeybindingRegistry,
    initial_family: Option<MenuFamily>,
    policy: LivePolicy,
    defer_live_provider_refresh: bool,
    config_path: Option<PathBuf>,
    forced_reduced_motion: bool,
) -> Result<(), String> {
    let mut config = config;
    // Keep the unmodified application inventory to make label/pin changes
    // reversible (including removing a previously applied override).
    let mut base_applications = snapshot.applications.clone();
    apply_launcher_preferences(&mut snapshot.applications, &config.launcher);
    if policy.execute_provider_actions {
        refresh_user_launcher_state(&mut snapshot, &config.launcher)?;
    }
    let mut backend =
        WaylandBackend::connect().map_err(|error| format!("Wayland connection failed: {error}"))?;
    let capabilities = backend.capabilities();
    if !capabilities.layer_shell {
        return Err("compositor does not advertise zwlr_layer_shell_v1".into());
    }
    if !capabilities.argb8888 {
        return Err("compositor does not advertise wl_shm ARGB8888".into());
    }
    if policy.execute_provider_actions {
        refresh_window_snapshot(&mut snapshot, &backend);
        refresh_workspace_snapshot(&mut snapshot, &backend);
    }

    let output = backend.outputs().into_iter().next();
    let panel_surface = backend
        .create_panel_at(
            WaylandPanelConfig {
                height: config.panel_height,
                exclusive_zone: i32::try_from(config.panel_height)
                    .map_err(|_| "panel height does not fit exclusive zone".to_owned())?,
                output: output.as_ref().map(|output| output.id),
                namespace: "nuraloumi-panel".into(),
            },
            wayland_panel_edge(config.panel_edge),
        )
        .map_err(|error| format!("failed to create panel surface: {error}"))?;

    let renderer = CairoRenderer::default();
    let mut panel = PanelController::default();
    let mut panel_geometry: Option<(u32, u32, i32)> = None;
    let mut panel_scene: Option<Scene> = None;
    let mut panel_pointer_press: Option<String> = None;
    let mut panel_touch_press: BTreeMap<i32, Option<String>> = BTreeMap::new();
    let mut synthetic_pointer = SyntheticPointerFilter::default();
    let mut active_menu: Option<LiveMenu> = None;
    let mut panel_first_frame_presented = false;
    let mut live_provider_probe = defer_live_provider_refresh.then(|| {
        std::thread::spawn(|| {
            let probe = ProbeSnapshot::live();
            let applications = ApplicationProvider::system().snapshot();
            let tasks = ProcessProvider::system().snapshot();
            (probe, applications, tasks)
        })
    });
    let mut thumbnail_probe: Option<
        std::thread::JoinHandle<Result<ThumbnailHelperReport, String>>,
    > = None;
    let mut thumbnail_probe_started = false;
    let mut last_status_probe = std::time::Instant::now();
    let mut periodic_status_probe: Option<std::thread::JoinHandle<ProbeSnapshot>> = None;

    if let Some(family) = initial_family {
        active_menu = Some(open_live_menu(
            &mut backend,
            &config,
            &snapshot,
            output.as_ref(),
            &mut panel,
            family,
        )?);
    }

    // The panel is the sole IPC owner. Its worker observes bounded immutable
    // state, never touches Wayland, and can launch only a verified sibling
    // one-shot toast in response to an explicit incoming IPC notify request.
    // Unobserved standalone notification-center state suppresses that launch.
    let binary =
        env::current_exe().map_err(|error| format!("cannot locate panel executable: {error}"))?;
    let binary_dir = binary
        .parent()
        .ok_or_else(|| "panel executable has no containing directory".to_owned())?;
    let surface_catalog = SurfaceCatalog::discover(binary_dir, &[]);
    let mut ipc_runtime = IpcRuntime::start(
        &nuraloumi_shell::ipc::default_socket_path(),
        live_surface_observations(&surface_catalog, active_menu.as_ref()),
        binary_dir.join("nuraloumi-toast"),
    )?;

    loop {
        let launcher_open = active_menu
            .as_ref()
            .is_some_and(|menu| menu.family == MenuFamily::Launcher);
        if policy.execute_provider_actions && launcher_open && !thumbnail_probe_started {
            thumbnail_probe = start_window_thumbnail_probe(&backend);
            thumbnail_probe_started = true;
        } else if !launcher_open {
            thumbnail_probe_started = false;
        }

        let menu_animating = active_menu
            .as_ref()
            .is_some_and(|menu| menu.motion.is_some());
        if menu_animating || thumbnail_probe.is_some() {
            std::thread::sleep(SceneTransitionClock::frame_interval());
            backend
                .roundtrip()
                .map_err(|error| format!("Wayland animation roundtrip failed: {error}"))?;
        } else if panel_first_frame_presented && live_provider_probe.is_some() {
            backend
                .roundtrip()
                .map_err(|error| format!("Wayland startup roundtrip failed: {error}"))?;
        } else if panel_first_frame_presented {
            // Idle dispatch is event-driven on both Wayland and IPC owner-wake
            // descriptors. There is no periodic roundtrip/polling timer.
            wait_for_panel_events(&mut backend, &ipc_runtime)?;
        } else {
            backend
                .blocking_dispatch()
                .map_err(|error| format!("Wayland dispatch failed: {error}"))?;
        }
        ipc_runtime.clear_wakeup();
        while let Some(request) = ipc_runtime.take_reload() {
            if request.expired() {
                let response = IpcResponse::failure(
                    &request.id,
                    "reload_timeout",
                    "owner received an expired reload request",
                );
                request.respond(response);
                continue;
            }
            let response = apply_live_config_reload(
                &request,
                config_path.as_deref(),
                forced_reduced_motion,
                &mut config,
                &mut keybindings,
                &mut snapshot,
                &base_applications,
                &mut active_menu,
                &mut panel_scene,
                &mut backend,
                panel_surface,
                &renderer,
                panel_geometry,
            );
            request.respond(response);
        }
        let toplevel_changed = backend.drain_toplevel_events().count() > 0;
        let workspace_changed = backend.drain_workspace_events().count() > 0;
        if policy.execute_provider_actions && (toplevel_changed || workspace_changed) {
            let prev_focused = toplevel_changed
                .then(|| {
                    snapshot
                        .windows
                        .iter()
                        .find(|window| window.focused)
                        .map(|window| window.id.clone())
                })
                .flatten();
            if toplevel_changed {
                refresh_window_snapshot(&mut snapshot, &backend);
                thumbnail_probe_started = false;
            }
            if workspace_changed {
                refresh_workspace_snapshot(&mut snapshot, &backend);
            }
            if toplevel_changed && active_menu.is_some() {
                let new_focused = snapshot
                    .windows
                    .iter()
                    .find(|window| window.focused)
                    .map(|window| window.id.clone());
                if should_dismiss_for_focus_change(prev_focused.as_deref(), new_focused.as_deref())
                {
                    close_live_menu(&mut backend, &mut panel, &mut active_menu)?;
                }
            }
            if let Some(menu) = active_menu.as_mut() {
                if policy.execute_provider_actions && menu.family == MenuFamily::ControlCenter {
                    refresh_control_center_tab_snapshot(
                        &mut snapshot,
                        menu.shell.control_center_tab,
                    );
                }
                menu.shell.refresh_family(menu.family, &snapshot)?;
                if let Some(menu_geometry) = menu.geometry {
                    menu.scene = Some(render_menu_and_present(
                        &mut backend,
                        menu.surface,
                        &renderer,
                        &menu.shell,
                        &snapshot,
                        &config,
                        (
                            menu_geometry,
                            menu.motion.as_ref().map(|clock| clock.sample().transition),
                        ),
                    )?);
                } else {
                    menu.scene = None;
                }
            }
            panel_scene = None;
        }
        let events: Vec<_> = backend.drain_events().collect();

        for event in events {
            // Correlate touch and synthesized pointer across panel, menu and
            // backdrop surfaces before any of them can dispatch an action.
            match &event.event {
                WaylandEvent::TouchDown { id, x, y }
                    if event.surface == Some(panel_surface)
                        || active_menu
                            .as_ref()
                            .is_some_and(|menu| event.surface == Some(menu.surface)) =>
                {
                    synthetic_pointer.down(
                        *id,
                        TouchPosition {
                            surface: event.surface,
                            x: *x,
                            y: *y,
                        },
                    );
                }
                WaylandEvent::TouchDown { id, x, y }
                    if active_menu
                        .as_ref()
                        .is_some_and(|menu| event.surface == menu.dismiss_surface) =>
                {
                    // Backdrop destruction may prevent a matching TouchUp.
                    // Treat it as a completed tap before it can be retargeted
                    // into an emulated pointer click on the exposed panel.
                    synthetic_pointer.down(
                        *id,
                        TouchPosition {
                            surface: event.surface,
                            x: *x,
                            y: *y,
                        },
                    );
                    synthetic_pointer.up(*id, std::time::Instant::now());
                }
                WaylandEvent::TouchMotion { id, x, y } => {
                    synthetic_pointer.motion(
                        *id,
                        TouchPosition {
                            surface: event.surface,
                            x: *x,
                            y: *y,
                        },
                    );
                }
                WaylandEvent::TouchUp { id } => {
                    synthetic_pointer.up(*id, std::time::Instant::now());
                }
                WaylandEvent::TouchCancel { ids } => synthetic_pointer.cancel(ids),
                WaylandEvent::PointerButton {
                    x,
                    y,
                    pressed,
                    button,
                } if *button == 0x110
                    && synthetic_pointer.suppress_pointer(
                        TouchPosition {
                            surface: event.surface,
                            x: *x,
                            y: *y,
                        },
                        *pressed,
                        std::time::Instant::now(),
                    ) =>
                {
                    continue;
                }
                _ => {}
            }
            if event.surface == Some(panel_surface) {
                match event.event {
                    WaylandEvent::Configure {
                        width,
                        height,
                        scale,
                    } => {
                        panel_geometry = Some((width, height, scale));
                        let search = panel_search_view(active_menu.as_ref());
                        panel_scene = Some(render_panel_and_present(
                            &mut backend,
                            panel_surface,
                            &renderer,
                            &snapshot,
                            &config,
                            &search,
                            (width, height, scale),
                        )?);

                        // The first recovery-visible frame is committed and flushed before
                        // any live provider result is applied.
                        panel_first_frame_presented = true;
                    }
                    WaylandEvent::PointerButton { x, y, pressed, .. } => {
                        let hit = panel_scene
                            .as_ref()
                            .and_then(|scene| scene.hit_test(x, y))
                            .map(str::to_owned);
                        if pressed {
                            panel_pointer_press = hit;
                        } else {
                            let pressed_id = panel_pointer_press.take();
                            if pressed_id.is_some() && pressed_id == hit {
                                if let Some(id) = hit {
                                    activate_panel_target(
                                        &mut backend,
                                        &config,
                                        &mut snapshot,
                                        &mut panel,
                                        &mut active_menu,
                                        &id,
                                        PanelActivationContext {
                                            output: output.as_ref(),
                                            policy,
                                        },
                                    )?;
                                    panel_scene = None;
                                }
                            }
                        }
                    }
                    WaylandEvent::TouchDown { id, x, y } => {
                        let hit = panel_scene
                            .as_ref()
                            .and_then(|scene| scene.hit_test(x, y))
                            .map(str::to_owned);
                        begin_panel_touch(&mut panel_touch_press, id, hit);
                    }
                    WaylandEvent::TouchMotion { id, x, y } => {
                        if let Some(pressed) = panel_touch_press.get_mut(&id) {
                            let current =
                                panel_scene.as_ref().and_then(|scene| scene.hit_test(x, y));
                            update_panel_touch_press(pressed, current);
                        }
                    }
                    WaylandEvent::TouchUp { id } => {
                        panel_pointer_press = None;
                        if let Some(Some(item_id)) = panel_touch_press.remove(&id) {
                            activate_panel_target(
                                &mut backend,
                                &config,
                                &mut snapshot,
                                &mut panel,
                                &mut active_menu,
                                &item_id,
                                PanelActivationContext {
                                    output: output.as_ref(),
                                    policy,
                                },
                            )?;
                            panel_scene = None;
                        }
                    }
                    WaylandEvent::TouchCancel { ids } => {
                        for id in ids {
                            panel_touch_press.remove(&id);
                        }
                    }
                    WaylandEvent::Close => {
                        close_live_menu(&mut backend, &mut panel, &mut active_menu)?;
                        backend
                            .destroy_surface(panel_surface)
                            .map_err(|error| format!("failed to destroy panel surface: {error}"))?;
                        backend.flush().map_err(|error| {
                            format!("failed to flush Wayland connection: {error}")
                        })?;
                        return Ok(());
                    }
                    _ => {}
                }
                continue;
            }

            let dismiss_surface = active_menu.as_ref().and_then(|menu| menu.dismiss_surface);
            if let Some(dismiss_surface) = dismiss_surface {
                if event.surface == Some(dismiss_surface) {
                    match event.event {
                        WaylandEvent::Configure {
                            width,
                            height,
                            scale,
                        } => {
                            present_transparent_surface(
                                &mut backend,
                                dismiss_surface,
                                width,
                                height,
                                scale,
                            )?;
                        }
                        WaylandEvent::PointerButton { pressed: true, .. }
                        | WaylandEvent::TouchDown { .. }
                            if config.menu_dismissal.outside_press =>
                        {
                            close_live_menu(&mut backend, &mut panel, &mut active_menu)?;
                            panel_scene = None;
                        }
                        WaylandEvent::Close => {
                            close_live_menu(&mut backend, &mut panel, &mut active_menu)?;
                            panel_scene = None;
                        }
                        _ => {}
                    }
                    continue;
                }
            }

            let Some(menu) = active_menu.as_mut() else {
                continue;
            };
            if event.surface.is_some() && event.surface != Some(menu.surface) {
                continue;
            }

            let mut redraw = false;
            let mut close = false;
            let mut switch_family: Option<MenuFamily> = None;
            let mut report = None;

            match event.event {
                WaylandEvent::Configure {
                    width,
                    height,
                    scale,
                } if event.surface == Some(menu.surface) => {
                    menu.geometry = Some((width, height, scale));
                    redraw = true;
                }
                WaylandEvent::PointerButton { x, y, pressed, .. } => {
                    let input_enabled = menu
                        .motion
                        .as_ref()
                        .is_none_or(|clock| !clock.blocks_hit_testing());
                    let region = input_enabled
                        .then(|| {
                            menu.scene
                                .as_ref()
                                .and_then(|scene| shell_hit_region(scene, x, y))
                        })
                        .flatten();
                    if pressed
                        && blank_press_requests_dismiss(
                            config.menu_dismissal,
                            input_enabled,
                            region.is_some(),
                        )
                    {
                        close = true;
                    } else {
                        report =
                            menu.shell
                                .handle_platform_event(ShellPlatformEvent::PointerButton {
                                    region,
                                    pressed,
                                });
                        redraw = true;
                    }
                }
                WaylandEvent::TouchDown { id, x, y } => {
                    let input_enabled = menu
                        .motion
                        .as_ref()
                        .is_none_or(|clock| !clock.blocks_hit_testing());
                    let region = input_enabled
                        .then(|| {
                            menu.scene
                                .as_ref()
                                .and_then(|scene| shell_hit_region(scene, x, y))
                        })
                        .flatten();
                    if blank_press_requests_dismiss(
                        config.menu_dismissal,
                        input_enabled,
                        region.is_some(),
                    ) {
                        close = true;
                    } else {
                        menu.touch_regions.insert(id, region.clone());
                        report = menu
                            .shell
                            .handle_platform_event(ShellPlatformEvent::TouchDown { id, region });
                        redraw = true;
                    }
                }
                WaylandEvent::TouchMotion { id, x, y } => {
                    let input_enabled = menu
                        .motion
                        .as_ref()
                        .is_none_or(|clock| !clock.blocks_hit_testing());
                    let region = input_enabled
                        .then(|| {
                            menu.scene
                                .as_ref()
                                .and_then(|scene| shell_hit_region(scene, x, y))
                        })
                        .flatten();
                    menu.touch_regions.insert(id, region);
                }
                WaylandEvent::TouchUp { id } => {
                    let region = menu.touch_regions.remove(&id).flatten();
                    report = menu
                        .shell
                        .handle_platform_event(ShellPlatformEvent::TouchUp { id, region });
                    redraw = true;
                }
                WaylandEvent::TouchCancel { ids } => {
                    for id in ids {
                        menu.touch_regions.remove(&id);
                        let _ = menu
                            .shell
                            .handle_platform_event(ShellPlatformEvent::TouchUp {
                                id,
                                region: None,
                            });
                    }
                    redraw = true;
                }
                WaylandEvent::Key { key, pressed: true } => {
                    if let Some(key_report) =
                        dispatch_keybinding(&keybindings, &mut menu.shell, &key)
                    {
                        report = Some(key_report);
                        redraw = true;
                    }
                }
                WaylandEvent::Close => close = true,
                _ => {}
            }

            if let Some(action_report) = report {
                if matches!(
                    &action_report,
                    nuraloumi_shell::ActionReport::SearchChanged { .. }
                ) {
                    menu.shell.refresh_family(menu.family, &snapshot)?;
                    panel_scene = None;
                }
                eprintln!(
                    "nuraloumi-panel-action: {}",
                    serde_json::to_string(&action_report)
                        .map_err(|error| format!("failed to serialize action report: {error}"))?
                );

                if let nuraloumi_shell::ActionReport::Dispatched {
                    action: MenuAction::Custom { kind, payload },
                    ..
                } = &action_report
                {
                    if kind == "menu.open" {
                        switch_family = Some(parse_family(payload)?);
                    } else if kind == "overview.mode" {
                        menu.shell
                            .set_overview_mode(OverviewMode::parse(payload)?, &snapshot)?;
                        menu.motion =
                            (!config.reduced_motion).then(|| SceneTransitionClock::new(false));
                        panel_scene = None;
                        redraw = true;
                    } else if kind == "control.tab" {
                        let tab = ControlCenterTab::parse(payload)?;
                        if policy.execute_provider_actions {
                            refresh_control_center_tab_snapshot(&mut snapshot, tab);
                        }
                        menu.shell.set_control_center_tab(tab, &snapshot)?;
                        menu.motion =
                            (!config.reduced_motion).then(|| SceneTransitionClock::new(false));
                        panel_scene = None;
                        redraw = true;
                    }
                }

                if policy.execute_provider_actions {
                    if let ActionReport::Dispatched { action, .. } = &action_report {
                        let close_after_success =
                            successful_action_requests_dismiss(config.menu_dismissal, action);
                        let is_navigation = matches!(
                            action,
                            MenuAction::Custom { kind, .. }
                                if kind == "menu.open"
                                    || kind == "overview.mode"
                                    || kind == "control.tab"
                        );
                        let launcher_action_handled = if policy.execute_provider_actions {
                            if let MenuAction::Custom { kind, payload } = action {
                                update_user_launcher_state(
                                    &mut snapshot,
                                    &config.launcher,
                                    kind,
                                    payload,
                                )?
                            } else {
                                false
                            }
                        } else {
                            false
                        };
                        if launcher_action_handled {
                            menu.shell.refresh_family(menu.family, &snapshot)?;
                            panel_scene = None;
                            redraw = true;
                        }
                        if !is_navigation && !launcher_action_handled {
                            if let Some(command) = parse_desktop_command(action, &snapshot)? {
                                match command {
                                    DesktopCommand::Switch { desktop_id } => {
                                        let id = parse_workspace_id(&desktop_id)?;
                                        backend
                                            .activate_workspace(id)
                                            .map_err(|error| error.to_string())?;
                                        if close_after_success {
                                            close = true;
                                        } else {
                                            redraw = true;
                                        }
                                    }
                                    DesktopCommand::MoveWindow { .. } => {
                                        return Err(
                                            "move-window-to-desktop is not exposed by the active Wayland protocols"
                                                .into(),
                                        );
                                    }
                                }
                            } else if let Some(command) =
                                parse_window_command(action, &snapshot.windows)?
                            {
                                let focused = matches!(command, WindowCommand::Focus(_));
                                execute_window_command(&mut backend, command)?;
                                if focused && !close_after_success {
                                    backend.roundtrip().map_err(|error| {
                                        format!("toplevel focus roundtrip failed: {error}")
                                    })?;
                                }
                                if close_after_success {
                                    close = true;
                                } else {
                                    if focused {
                                        refresh_window_snapshot(&mut snapshot, &backend);
                                        menu.shell.refresh_family(menu.family, &snapshot)?;
                                        panel_scene = None;
                                    }
                                    redraw = true;
                                }
                            } else {
                                let app_launch = matches!(
                                    action,
                                    MenuAction::Custom { kind, .. } if kind == "app.launch"
                                );
                                let mut app_reused = false;
                                if let MenuAction::Custom { kind, payload } = action {
                                    if kind == "app.launch" {
                                        match resolve_app_activation(
                                            payload,
                                            &snapshot.windows,
                                            &config.launcher,
                                        ) {
                                            AppActivation::AlreadyFocused => {
                                                if let Err(error) =
                                                    record_user_application_activation(
                                                        &mut snapshot,
                                                        &config.launcher,
                                                        payload,
                                                    )
                                                {
                                                    eprintln!(
                                                        "nuraloumi-launcher-state-error: {error}"
                                                    );
                                                }
                                                close = close_after_success;
                                                if !close {
                                                    menu.shell
                                                        .refresh_family(menu.family, &snapshot)?;
                                                    panel_scene = None;
                                                    redraw = true;
                                                }
                                                app_reused = true;
                                            }
                                            AppActivation::FocusWindow(window_id) => {
                                                let focus = MenuAction::Custom {
                                                    kind: "window.focus".into(),
                                                    payload: window_id,
                                                };
                                                let command =
                                                    parse_window_command(&focus, &snapshot.windows)?
                                                        .ok_or_else(|| {
                                                            "resolved app focus did not produce a window command"
                                                                .to_owned()
                                                        })?;
                                                execute_window_command(&mut backend, command)?;
                                                if let Err(error) =
                                                    record_user_application_activation(
                                                        &mut snapshot,
                                                        &config.launcher,
                                                        payload,
                                                    )
                                                {
                                                    eprintln!(
                                                        "nuraloumi-launcher-state-error: {error}"
                                                    );
                                                }
                                                close = close_after_success;
                                                if !close {
                                                    backend.roundtrip().map_err(|error| {
                                                        format!(
                                                            "toplevel focus roundtrip failed: {error}"
                                                        )
                                                    })?;
                                                    refresh_window_snapshot(
                                                        &mut snapshot,
                                                        &backend,
                                                    );
                                                    menu.shell
                                                        .refresh_family(menu.family, &snapshot)?;
                                                    panel_scene = None;
                                                    redraw = true;
                                                }
                                                app_reused = true;
                                            }
                                            AppActivation::Launch => {}
                                        }
                                    }
                                }
                                if !app_reused {
                                    match execute_live_action(
                                        action,
                                        policy.enable_power_actions,
                                        policy.enable_renderer_switch,
                                    ) {
                                        Ok(Some(result)) => {
                                            eprintln!(
                                            "nuraloumi-provider-result: executed={} dry_run={} message={}",
                                            result.executed, result.dry_run, result.message
                                        );
                                            if successful_compositor_restart(action, &result) {
                                                eprintln!(
                                                    "nuraloumi-panel: compositor restart completed; exiting stale Wayland session"
                                                );
                                                return Ok(());
                                            }
                                            if result.executed && !result.dry_run {
                                                if let MenuAction::Custom { kind, payload } = action
                                                {
                                                    if kind == "app.launch" {
                                                        if let Err(error) =
                                                            record_user_application_activation(
                                                                &mut snapshot,
                                                                &config.launcher,
                                                                payload,
                                                            )
                                                        {
                                                            eprintln!("nuraloumi-launcher-state-error: {error}");
                                                        }
                                                    }
                                                }
                                            }
                                            if close_after_success && result.executed {
                                                close = true;
                                            } else {
                                                let lazy_control = refresh_lazy_control_action(
                                                    &mut snapshot,
                                                    action,
                                                );
                                                if !lazy_control {
                                                    refresh_probe_snapshot(
                                                        &mut snapshot,
                                                        &ProbeSnapshot::live(),
                                                        policy.enable_renderer_switch,
                                                    );
                                                    refresh_window_snapshot(
                                                        &mut snapshot,
                                                        &backend,
                                                    );
                                                    refresh_workspace_snapshot(
                                                        &mut snapshot,
                                                        &backend,
                                                    );
                                                }
                                                menu.shell
                                                    .refresh_family(menu.family, &snapshot)?;
                                                panel_scene = None;
                                                redraw = true;
                                            }
                                        }
                                        Ok(None) => {}
                                        Err(error) => {
                                            eprintln!("nuraloumi-provider-error: {error}");
                                            let lazy_control =
                                                refresh_lazy_control_action(&mut snapshot, action);
                                            if !lazy_control && !app_launch {
                                                refresh_probe_snapshot(
                                                    &mut snapshot,
                                                    &ProbeSnapshot::live(),
                                                    policy.enable_renderer_switch,
                                                );
                                                refresh_window_snapshot(&mut snapshot, &backend);
                                                refresh_workspace_snapshot(&mut snapshot, &backend);
                                            }
                                            menu.shell.refresh_family(menu.family, &snapshot)?;
                                            panel_scene = None;
                                            redraw = true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                if matches!(action_report, nuraloumi_shell::ActionReport::SurfaceClosed) {
                    close = true;
                }
            }

            if menu.shell.closed {
                close = true;
            }

            if let Some(family) = switch_family {
                menu.family = family;
                if family != MenuFamily::Launcher && menu.shell.search_focused {
                    let _ = menu.shell.focus_search(false);
                }
                if policy.execute_provider_actions && family == MenuFamily::ControlCenter {
                    refresh_control_center_tab_snapshot(&mut snapshot, ControlCenterTab::Media);
                }
                menu.shell.refresh_family(family, &snapshot)?;
                menu.motion = (!config.reduced_motion).then(|| SceneTransitionClock::new(false));
                panel_scene = None;
                redraw = true;
            }

            if close {
                close_live_menu(&mut backend, &mut panel, &mut active_menu)?;
                panel_scene = None;
                continue;
            }

            if redraw {
                if let Some(geometry) = menu.geometry {
                    menu.scene = Some(render_menu_and_present(
                        &mut backend,
                        menu.surface,
                        &renderer,
                        &menu.shell,
                        &snapshot,
                        &config,
                        (
                            geometry,
                            menu.motion.as_ref().map(|clock| clock.sample().transition),
                        ),
                    )?);
                }
            }
        }

        if panel_first_frame_presented
            && live_provider_probe
                .as_ref()
                .is_some_and(|probe| probe.is_finished())
        {
            let (probe, applications, tasks) = live_provider_probe
                .take()
                .expect("finished live provider probe must exist")
                .join()
                .map_err(|_| "initial live provider probe panicked".to_owned())?;
            refresh_probe_snapshot(&mut snapshot, &probe, policy.enable_renderer_switch);
            apply_application_snapshot(
                &mut snapshot,
                applications,
                &nuraloumi_shell::LauncherPreferences::default(),
            );
            base_applications = snapshot.applications.clone();
            apply_launcher_preferences(&mut snapshot.applications, &config.launcher);
            if policy.execute_provider_actions {
                refresh_user_launcher_state(&mut snapshot, &config.launcher)?;
            }
            apply_process_snapshot(&mut snapshot, tasks);
            if policy.execute_provider_actions {
                refresh_window_snapshot(&mut snapshot, &backend);
                refresh_workspace_snapshot(&mut snapshot, &backend);
            }
            if let Some(menu) = active_menu.as_mut() {
                if policy.execute_provider_actions && menu.family == MenuFamily::ControlCenter {
                    refresh_control_center_tab_snapshot(
                        &mut snapshot,
                        menu.shell.control_center_tab,
                    );
                }
                menu.shell.refresh_family(menu.family, &snapshot)?;
                if let Some(menu_geometry) = menu.geometry {
                    menu.scene = Some(render_menu_and_present(
                        &mut backend,
                        menu.surface,
                        &renderer,
                        &menu.shell,
                        &snapshot,
                        &config,
                        (
                            menu_geometry,
                            menu.motion.as_ref().map(|clock| clock.sample().transition),
                        ),
                    )?);
                } else {
                    menu.scene = None;
                }
            }
            panel_scene = None;
            eprintln!("nuraloumi-panel-provider-refresh: initial live snapshot ready");
        }

        // Periodic snapshots are asynchronous: neither the 2-second clock helper
        // nor the battery/network probes may stall Wayland input handling.
        if policy.execute_provider_actions
            && live_provider_probe.is_none()
            && periodic_status_probe.is_none()
            && last_status_probe.elapsed() >= std::time::Duration::from_secs(30)
        {
            last_status_probe = std::time::Instant::now();
            periodic_status_probe = Some(std::thread::spawn(ProbeSnapshot::live));
        }
        if periodic_status_probe
            .as_ref()
            .is_some_and(|probe| probe.is_finished())
        {
            let probe = periodic_status_probe
                .take()
                .expect("finished periodic probe exists")
                .join()
                .map_err(|_| "periodic provider probe panicked".to_owned())?;
            refresh_probe_snapshot(&mut snapshot, &probe, policy.enable_renderer_switch);
            if let Some(menu) = active_menu.as_mut() {
                menu.shell.refresh_family(menu.family, &snapshot)?;
                if let Some(geometry) = menu.geometry {
                    menu.scene = Some(render_menu_and_present(
                        &mut backend,
                        menu.surface,
                        &renderer,
                        &menu.shell,
                        &snapshot,
                        &config,
                        (geometry, None),
                    )?);
                }
            }
            panel_scene = None;
        }

        if thumbnail_probe
            .as_ref()
            .is_some_and(|probe| probe.is_finished())
        {
            let result = thumbnail_probe
                .take()
                .expect("finished thumbnail probe must exist")
                .join()
                .map_err(|_| "window thumbnail probe panicked".to_owned())?;
            if let Some(menu) = active_menu.as_mut() {
                if menu.family == MenuFamily::Launcher {
                    apply_window_thumbnail_report(&mut snapshot, result);
                    menu.shell.refresh_family(MenuFamily::Launcher, &snapshot)?;
                    if let Some(menu_geometry) = menu.geometry {
                        menu.scene = Some(render_menu_and_present(
                            &mut backend,
                            menu.surface,
                            &renderer,
                            &menu.shell,
                            &snapshot,
                            &config,
                            (
                                menu_geometry,
                                menu.motion.as_ref().map(|clock| clock.sample().transition),
                            ),
                        )?);
                    }
                    panel_scene = None;
                }
            }
        }

        if let Some(menu) = active_menu.as_mut() {
            if let (Some(clock), Some(menu_geometry)) = (menu.motion.as_ref(), menu.geometry) {
                let sample = clock.sample();
                menu.scene = Some(render_menu_and_present(
                    &mut backend,
                    menu.surface,
                    &renderer,
                    &menu.shell,
                    &snapshot,
                    &config,
                    (menu_geometry, Some(sample.transition)),
                )?);
                if sample.complete {
                    menu.motion = None;
                }
            }
        }

        if panel_scene.is_none() {
            if let Some(geometry) = panel_geometry {
                let search = panel_search_view(active_menu.as_ref());
                panel_scene = Some(render_panel_and_present(
                    &mut backend,
                    panel_surface,
                    &renderer,
                    &snapshot,
                    &config,
                    &search,
                    geometry,
                )?);
            }
        }

        ipc_runtime.publish(live_surface_observations(
            &surface_catalog,
            active_menu.as_ref(),
        ));

        if panel_first_frame_presented && live_provider_probe.is_some() {
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
    }
}

fn open_live_menu(
    backend: &mut WaylandBackend,
    config: &ShellConfig,
    snapshot: &FixtureSnapshot,
    output: Option<&nuraloumi_wayland::OutputInfo>,
    panel: &mut PanelController,
    family: MenuFamily,
) -> Result<LiveMenu, String> {
    let requested_width = output
        .and_then(|output| {
            let scale = output.scale.max(1) as u32;
            (output.mode_width > 0).then_some(output.mode_width as u32 / scale)
        })
        .map(|width| config.menu_width.min(width.saturating_sub(16).max(240)))
        .unwrap_or(config.menu_width);
    let requested_height = output
        .and_then(|output| {
            let scale = output.scale.max(1) as u32;
            (output.mode_height > 0).then_some(output.mode_height as u32 / scale)
        })
        .map(|height| {
            height
                .saturating_sub(
                    config
                        .panel_height
                        .saturating_add(PANEL_MENU_GAP)
                        .saturating_add(16),
                )
                .clamp(240, 720)
        })
        .unwrap_or(640);

    let panel_menu_offset = config.panel_height.saturating_add(PANEL_MENU_GAP);
    let margin_top = if config.panel_edge == ShellPanelEdge::Top {
        panel_menu_offset as i32
    } else {
        0
    };
    let margin_left = if config.panel_edge == ShellPanelEdge::Left {
        panel_menu_offset as i32
    } else {
        0
    };
    let dismiss_backdrop_enabled = dismiss_backdrop_enabled_for_renderer(
        config.menu_dismissal,
        env::var("WLR_RENDERER").ok().as_deref(),
        env::var("MESA_LOADER_DRIVER_OVERRIDE").ok().as_deref(),
    );
    if config.menu_dismissal.outside_press && !dismiss_backdrop_enabled {
        eprintln!(
            "nuraloumi-menu-dismiss: outside backdrop suppressed for unqualified Grate renderer; close-after-action remains enabled"
        );
    }
    let dismiss_surface = if dismiss_backdrop_enabled {
        let (margin_top, margin_right, margin_bottom, margin_left) =
            dismiss_backdrop_margins(config.panel_edge, config.panel_height);
        Some(
            backend
                .create_dismiss_backdrop(DismissBackdropConfig {
                    margin_top,
                    margin_right,
                    margin_bottom,
                    margin_left,
                    output: output.map(|output| output.id),
                    namespace: "nuraloumi-panel-dismiss".into(),
                })
                .map_err(|error| format!("failed to create panel dismiss backdrop: {error}"))?,
        )
    } else {
        None
    };
    let surface = match backend.create_menu(WaylandMenuConfig {
        width: requested_width,
        height: requested_height,
        margin_top,
        margin_left,
        output: output.map(|output| output.id),
        namespace: "nuraloumi-panel-menu".into(),
    }) {
        Ok(surface) => surface,
        Err(error) => {
            if let Some(dismiss_surface) = dismiss_surface {
                let _ = backend.destroy_surface(dismiss_surface);
            }
            return Err(format!("failed to create panel menu surface: {error}"));
        }
    };
    panel.open_menu(family, true);
    Ok(LiveMenu {
        surface,
        dismiss_surface,
        family,
        shell: ShellState::new(build_family(family, snapshot), config.reduced_motion)?,
        geometry: None,
        scene: None,
        touch_regions: BTreeMap::new(),
        motion: (!config.reduced_motion).then(|| SceneTransitionClock::new(false)),
    })
}

fn activate_panel_target(
    backend: &mut WaylandBackend,
    config: &ShellConfig,
    snapshot: &mut FixtureSnapshot,
    panel: &mut PanelController,
    active_menu: &mut Option<LiveMenu>,
    item_id: &str,
    context: PanelActivationContext<'_>,
) -> Result<(), String> {
    let Some(target) = panel_target_for_id(item_id) else {
        return Ok(());
    };
    if context.policy.execute_provider_actions {
        if let Some(tab) = target.control_center_tab {
            refresh_control_center_tab_snapshot(snapshot, tab);
        }
    }
    replace_live_menu(
        backend,
        config,
        snapshot,
        context.output,
        panel,
        active_menu,
        target.family,
    )?;
    if let Some(menu) = active_menu.as_mut() {
        if let Some(tab) = target.control_center_tab {
            menu.shell.set_control_center_tab(tab, snapshot)?;
        }
        if target.focus_search {
            let _ = menu.shell.apply_input(launcher_search_input());
        }
    }
    Ok(())
}

fn replace_live_menu(
    backend: &mut WaylandBackend,
    config: &ShellConfig,
    snapshot: &FixtureSnapshot,
    output: Option<&nuraloumi_wayland::OutputInfo>,
    panel: &mut PanelController,
    active_menu: &mut Option<LiveMenu>,
    family: MenuFamily,
) -> Result<(), String> {
    close_live_menu(backend, panel, active_menu)?;
    *active_menu = Some(open_live_menu(
        backend, config, snapshot, output, panel, family,
    )?);
    Ok(())
}

fn close_live_menu(
    backend: &mut WaylandBackend,
    panel: &mut PanelController,
    active_menu: &mut Option<LiveMenu>,
) -> Result<(), String> {
    if let Some(menu) = active_menu.take() {
        backend
            .destroy_surface(menu.surface)
            .map_err(|error| format!("failed to destroy panel menu surface: {error}"))?;
        if let Some(dismiss_surface) = menu.dismiss_surface {
            backend
                .destroy_surface(dismiss_surface)
                .map_err(|error| format!("failed to destroy panel dismiss backdrop: {error}"))?;
        }
        backend
            .flush()
            .map_err(|error| format!("failed to flush Wayland connection: {error}"))?;
    }
    panel.close_menu();
    Ok(())
}

fn render_panel_and_present(
    backend: &mut WaylandBackend,
    surface: SurfaceId,
    renderer: &CairoRenderer,
    snapshot: &FixtureSnapshot,
    config: &ShellConfig,
    search: &PanelSearchView,
    geometry: (u32, u32, i32),
) -> Result<Scene, String> {
    let theme = RenderTheme::from(match config.theme {
        ShellTheme::Dark => &DARK_THEME,
        ShellTheme::Light => &LIGHT_THEME,
    });
    let scene = build_panel_scene(snapshot, config, search, geometry, theme);
    present_scene(backend, surface, renderer, &scene)?;
    Ok(scene)
}

fn render_menu_and_present(
    backend: &mut WaylandBackend,
    surface: SurfaceId,
    renderer: &CairoRenderer,
    shell: &ShellState,
    snapshot: &FixtureSnapshot,
    config: &ShellConfig,
    presentation: ((u32, u32, i32), Option<nuraloumi_core::Transition>),
) -> Result<Scene, String> {
    let (geometry, transition) = presentation;
    let (logical_width, logical_height, scale) = geometry;
    let viewport = Viewport::new(
        f64::from(logical_width),
        f64::from(logical_height),
        f64::from(scale.max(1)),
    );
    let theme_tokens = match config.theme {
        ShellTheme::Dark => &DARK_THEME,
        ShellTheme::Light => &LIGHT_THEME,
    };
    let presentation_menu = shell.renderer_presentation_menu();
    let (scene, mut buffer) = render_menu_follow_selection(
        renderer,
        &presentation_menu,
        &shell.state,
        viewport,
        theme_tokens,
        RenderOptions::default(),
        transition,
    )
    .map_err(|error| format!("Cairo menu render failed: {error}"))?;
    paint_window_thumbnail_overlays(&scene, &mut buffer, snapshot)?;
    present_buffer(backend, surface, &mut buffer)?;
    Ok(scene)
}

fn present_scene(
    backend: &mut WaylandBackend,
    surface: SurfaceId,
    renderer: &CairoRenderer,
    scene: &Scene,
) -> Result<(), String> {
    let mut buffer = renderer
        .render_scene(scene)
        .map_err(|error| format!("Cairo panel render failed: {error}"))?;
    present_buffer(backend, surface, &mut buffer)
}

fn present_buffer(
    backend: &mut WaylandBackend,
    surface: SurfaceId,
    buffer: &mut nuraloumi_render_cairo::RenderedBuffer,
) -> Result<(), String> {
    let info = buffer.info();
    let width = u32::try_from(info.width).map_err(|_| "negative rendered width".to_owned())?;
    let height = u32::try_from(info.height).map_err(|_| "negative rendered height".to_owned())?;
    let stride = usize::try_from(info.stride).map_err(|_| "negative rendered stride".to_owned())?;
    let present = buffer
        .with_argb32_bytes(|pixels, _| {
            backend.present(
                surface,
                Frame {
                    width,
                    height,
                    stride,
                    format: PixelFormat::Argb8888,
                    pixels,
                },
            )
        })
        .map_err(|error| format!("failed to borrow rendered pixels: {error}"))?;

    match present {
        Ok(()) | Err(BackendError::WouldBlock) => Ok(()),
        Err(error) => Err(format!("Wayland present failed: {error}")),
    }
}

fn present_transparent_surface(
    backend: &mut WaylandBackend,
    surface: SurfaceId,
    width: u32,
    height: u32,
    scale: i32,
) -> Result<(), String> {
    let scale = u32::try_from(scale.max(1)).map_err(|_| "invalid surface scale".to_owned())?;
    let width = width
        .checked_mul(scale)
        .ok_or_else(|| "dismiss backdrop width overflow".to_owned())?;
    let height = height
        .checked_mul(scale)
        .ok_or_else(|| "dismiss backdrop height overflow".to_owned())?;
    let byte_len = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "dismiss backdrop buffer is too large".to_owned())?;
    let pixels = vec![0_u8; byte_len];
    match backend.present(
        surface,
        Frame::packed(width, height, PixelFormat::Argb8888, &pixels),
    ) {
        Ok(()) | Err(BackendError::WouldBlock) => Ok(()),
        Err(error) => Err(format!("failed to present dismiss backdrop: {error}")),
    }
}

fn build_panel_scene(
    snapshot: &FixtureSnapshot,
    config: &ShellConfig,
    search: &PanelSearchView,
    geometry: (u32, u32, i32),
    theme: RenderTheme,
) -> Scene {
    let (logical_width, logical_height, scale) = geometry;
    let viewport = Viewport::new(
        f64::from(logical_width),
        f64::from(logical_height),
        f64::from(scale.max(1)),
    );
    let panel_rect = viewport.logical_rect();
    let affordances = live_panel_affordances(snapshot, search);
    let horizontal = matches!(
        config.panel_edge,
        ShellPanelEdge::Top | ShellPanelEdge::Bottom
    );
    let slot_rects = panel_affordance_rects(panel_rect, horizontal, &affordances);

    let mut paint = vec![PaintNode::FillRect {
        rect: panel_rect,
        color: theme.raised,
    }];
    let mut hits = Vec::with_capacity(affordances.len());

    for (index, (affordance, rect)) in affordances.iter().zip(slot_rects).enumerate() {
        let index = index as f64;
        if affordance.id == "search" {
            let field = Rect::new(
                rect.x,
                rect.y + 5.0,
                rect.width,
                (rect.height - 10.0).max(1.0),
            );
            paint.push(PaintNode::RoundedRect {
                rect: field,
                radius: 0.0,
                fill: nuraloumi_render_cairo::Fill::Solid(theme.card),
                stroke: Some((
                    if search.focused {
                        theme.accent
                    } else {
                        theme.border
                    },
                    1.0,
                )),
            });
            let text = if search.query.is_empty() {
                "Search…".to_owned()
            } else {
                truncate_chars(
                    &search.query,
                    ((field.width - 28.0) / 7.0).max(3.0) as usize,
                )
            };
            paint.push(PaintNode::Text {
                origin: Point::new(field.x + 14.0, field.y + field.height / 2.0 + 4.0),
                text,
                style: TextStyle {
                    size: 12.0,
                    bold: false,
                },
                color: if search.query.is_empty() && !search.focused {
                    theme.hint
                } else {
                    theme.primary_text
                },
            });
            hits.push(RenderHitRegion {
                item_id: affordance.id.clone(),
                rect,
                actionable: true,
                enabled: true,
                kind: RowKind::Action,
            });
            continue;
        }

        if affordance.id == "power" && horizontal {
            // Draw the power symbol with Cairo primitives: the SL101 does not
            // need a font containing U+23FB to display its shutdown affordance.
            let cx = rect.x + rect.width / 2.0;
            let cy = rect.y + rect.height / 2.0;
            for (x1, y1, x2, y2) in [
                (0.0, -9.0, 0.0, 0.0),
                (-5.0, -6.0, -8.0, -2.0),
                (-8.0, -2.0, -8.0, 3.0),
                (-8.0, 3.0, -5.0, 7.0),
                (-5.0, 7.0, 0.0, 9.0),
                (0.0, 9.0, 5.0, 7.0),
                (5.0, 7.0, 8.0, 3.0),
                (8.0, 3.0, 8.0, -2.0),
                (8.0, -2.0, 5.0, -6.0),
            ] {
                paint.push(PaintNode::Line {
                    from: Point::new(cx + x1, cy + y1),
                    to: Point::new(cx + x2, cy + y2),
                    width: 2.0,
                    color: theme.primary_text,
                });
            }
            paint.push(PaintNode::Line {
                from: Point::new(rect.x, rect.y + 8.0),
                to: Point::new(rect.x, rect.bottom() - 8.0),
                width: 1.0,
                color: theme.separator,
            });
            hits.push(RenderHitRegion {
                item_id: affordance.id.clone(),
                rect,
                actionable: true,
                enabled: true,
                kind: RowKind::Action,
            });
            continue;
        }

        let marker_color = state_color(affordance.state, theme);
        if affordance.id == "battery" && horizontal {
            // Real battery outline + terminal + proportional fill, rather
            // than a generic status dot or a font-dependent Unicode block.
            let (capacity, charging) = battery_presentation(affordance.value.as_deref());
            let battery = Rect::new(rect.x + 8.0, rect.y + rect.height / 2.0 - 6.0, 20.0, 12.0);
            paint.push(PaintNode::RoundedRect {
                rect: battery,
                radius: 0.0,
                fill: nuraloumi_render_cairo::Fill::Solid(theme.raised),
                stroke: Some((marker_color, 1.0)),
            });
            paint.push(PaintNode::FillRect {
                rect: Rect::new(battery.right() + 1.0, battery.y + 4.0, 3.0, 4.0),
                color: marker_color,
            });
            if let Some(capacity) = capacity {
                paint.push(PaintNode::FillRect {
                    rect: Rect::new(
                        battery.x + 2.0,
                        battery.y + 2.0,
                        16.0 * f64::from(capacity) / 100.0,
                        8.0,
                    ),
                    color: if charging { theme.accent } else { marker_color },
                });
            }
        } else if horizontal && matches!(affordance.id.as_str(), "apps" | "network" | "audio") {
            draw_panel_status_icon(
                &mut paint,
                &affordance.id,
                rect,
                marker_color,
                affordance
                    .value
                    .as_deref()
                    .is_some_and(|value| value.to_ascii_lowercase().contains("muted")),
            );
        } else if affordance.id != "power" && affordance.id != "clock" {
            let marker = if horizontal {
                Rect::new(rect.x + 8.0, rect.y + rect.height / 2.0 - 2.5, 5.0, 5.0)
            } else {
                Rect::new(rect.x + 6.0, rect.y + 8.0, 5.0, 5.0)
            };
            paint.push(PaintNode::FillRect {
                rect: marker,
                color: marker_color,
            });
        }

        let available = if horizontal {
            (rect.width
                - if matches!(
                    affordance.id.as_str(),
                    "battery" | "apps" | "network" | "audio"
                ) {
                    44.0
                } else {
                    28.0
                })
            .max(18.0)
        } else {
            (rect.width - 22.0).max(18.0)
        };
        let label = panel_text(affordance, available);
        let baseline = if horizontal {
            rect.y + rect.height / 2.0 + 4.5
        } else {
            rect.y + rect.height / 2.0 + 4.0
        };
        paint.push(PaintNode::Text {
            origin: Point::new(
                rect.x
                    + if horizontal
                        && matches!(
                            affordance.id.as_str(),
                            "battery" | "apps" | "network" | "audio"
                        )
                    {
                        38.0
                    } else if affordance.id == "power" && horizontal {
                        18.0
                    } else {
                        20.0
                    },
                baseline,
            ),
            text: label,
            style: TextStyle {
                size: 12.0,
                bold: affordance.id == "apps",
            },
            color: if affordance.state == ValueState::Unavailable {
                theme.hint
            } else {
                theme.primary_text
            },
        });
        if index > 0.0 {
            let (from, to) = if horizontal {
                (
                    Point::new(rect.x, rect.y + 8.0),
                    Point::new(rect.x, rect.bottom() - 8.0),
                )
            } else {
                (
                    Point::new(rect.x + 8.0, rect.y),
                    Point::new(rect.right() - 8.0, rect.y),
                )
            };
            paint.push(PaintNode::Line {
                from,
                to,
                width: 1.0,
                color: theme.separator,
            });
        }
        hits.push(RenderHitRegion {
            item_id: affordance.id.clone(),
            rect,
            actionable: true,
            enabled: true,
            kind: RowKind::Action,
        });
    }

    let border = if horizontal {
        let y = match config.panel_edge {
            ShellPanelEdge::Bottom => panel_rect.y,
            _ => panel_rect.bottom() - 1.0,
        };
        (
            Point::new(panel_rect.x, y),
            Point::new(panel_rect.right(), y),
        )
    } else {
        let x = match config.panel_edge {
            ShellPanelEdge::Right => panel_rect.x,
            _ => panel_rect.right() - 1.0,
        };
        (
            Point::new(x, panel_rect.y),
            Point::new(x, panel_rect.bottom()),
        )
    };
    paint.push(PaintNode::Line {
        from: border.0,
        to: border.1,
        width: 1.0,
        color: theme.border,
    });

    Scene {
        viewport,
        menu_id: "panel".into(),
        panel_rect,
        paint,
        hits,
        scroll: ScrollWindow {
            viewport: panel_rect,
            content_height: panel_rect.height,
            offset: 0.0,
            max_offset: 0.0,
            clipped: false,
        },
    }
}

fn stroke_panel_icon_path(
    paint: &mut Vec<PaintNode>,
    center: Point,
    path: &[(f64, f64)],
    color: Color,
) {
    for pair in path.windows(2) {
        paint.push(PaintNode::Line {
            from: Point::new(center.x + pair[0].0, center.y + pair[0].1),
            to: Point::new(center.x + pair[1].0, center.y + pair[1].1),
            width: 1.8,
            color,
        });
    }
}

/// Font-independent status icons: missing Nerd Fonts must not make Wi-Fi
/// and volume controls indistinguishable on the ARMv7 software path.
fn draw_panel_status_icon(
    paint: &mut Vec<PaintNode>,
    id: &str,
    rect: Rect,
    color: Color,
    muted: bool,
) {
    let cx = rect.x + 18.0;
    let cy = rect.y + rect.height / 2.0;
    let center = Point::new(cx, cy);
    match id {
        "apps" => {
            for (dx, dy) in [(-8.0, -8.0), (1.0, -8.0), (-8.0, 1.0), (1.0, 1.0)] {
                paint.push(PaintNode::FillRect {
                    rect: Rect::new(cx + dx, cy + dy, 6.0, 6.0),
                    color,
                });
            }
        }
        "network" => {
            stroke_panel_icon_path(
                paint,
                center,
                &[
                    (-11.0, -4.0),
                    (-7.0, -7.0),
                    (-3.0, -9.0),
                    (3.0, -9.0),
                    (7.0, -7.0),
                    (11.0, -4.0),
                ],
                color,
            );
            stroke_panel_icon_path(
                paint,
                center,
                &[
                    (-7.0, 0.0),
                    (-3.0, -3.0),
                    (0.0, -4.0),
                    (3.0, -3.0),
                    (7.0, 0.0),
                ],
                color,
            );
            stroke_panel_icon_path(paint, center, &[(-3.0, 4.0), (0.0, 2.0), (3.0, 4.0)], color);
            paint.push(PaintNode::FillRect {
                rect: Rect::new(cx - 1.5, cy + 7.0, 3.0, 3.0),
                color,
            });
        }
        "audio" => {
            stroke_panel_icon_path(
                paint,
                center,
                &[
                    (-10.0, -4.0),
                    (-6.0, -4.0),
                    (0.0, -9.0),
                    (0.0, 9.0),
                    (-6.0, 4.0),
                    (-10.0, 4.0),
                    (-10.0, -4.0),
                ],
                color,
            );
            if muted {
                stroke_panel_icon_path(paint, center, &[(4.0, -6.0), (12.0, 6.0)], color);
            } else {
                stroke_panel_icon_path(
                    paint,
                    center,
                    &[(4.0, -4.0), (7.0, 0.0), (4.0, 4.0)],
                    color,
                );
                stroke_panel_icon_path(
                    paint,
                    center,
                    &[(7.0, -8.0), (12.0, 0.0), (7.0, 8.0)],
                    color,
                );
            }
        }
        _ => {}
    }
}

fn live_panel_affordances(
    snapshot: &FixtureSnapshot,
    search: &PanelSearchView,
) -> Vec<PanelAffordance> {
    let mut affordances = panel_affordances(snapshot);
    let search_index = affordances
        .iter()
        .position(|item| item.id == "network")
        .map(|index| index + 1)
        .unwrap_or(affordances.len());
    affordances.insert(
        search_index,
        PanelAffordance {
            id: "search".into(),
            label: "Search".into(),
            value: (!search.query.is_empty()).then(|| search.query.clone()),
            state: ValueState::Ready,
        },
    );
    affordances
}

fn panel_affordance_rects(
    panel_rect: Rect,
    horizontal: bool,
    affordances: &[PanelAffordance],
) -> Vec<Rect> {
    if affordances.is_empty() {
        return Vec::new();
    }

    if !horizontal {
        let extent = panel_rect.height / affordances.len() as f64;
        return (0..affordances.len())
            .map(|index| {
                Rect::new(
                    panel_rect.x,
                    panel_rect.y + index as f64 * extent,
                    panel_rect.width,
                    extent,
                )
            })
            .collect();
    }

    fn width_for(id: &str) -> Option<f64> {
        match id {
            "apps" => Some(84.0),
            "network" => Some(180.0),
            "audio" => Some(104.0),
            "battery" => Some(104.0),
            "renderer" => Some(90.0),
            "clock" => Some(72.0),
            "power" => Some(48.0),
            _ => None,
        }
    }

    let known = affordances
        .iter()
        .all(|item| item.id == "search" || width_for(&item.id).is_some());
    let left_total: f64 = affordances
        .iter()
        .filter(|item| matches!(item.id.as_str(), "apps" | "network"))
        .filter_map(|item| width_for(&item.id))
        .sum();
    let right_total: f64 = affordances
        .iter()
        .filter(|item| {
            matches!(
                item.id.as_str(),
                "audio" | "battery" | "renderer" | "clock" | "power"
            )
        })
        .filter_map(|item| width_for(&item.id))
        .sum();
    let center_available = panel_rect.width - left_total - right_total;
    if !known || center_available < 272.0 {
        let extent = panel_rect.width / affordances.len() as f64;
        return (0..affordances.len())
            .map(|index| {
                Rect::new(
                    panel_rect.x + index as f64 * extent,
                    panel_rect.y,
                    extent,
                    panel_rect.height,
                )
            })
            .collect();
    }

    let search_width = (center_available - 32.0).clamp(240.0, 360.0);
    let search_x = panel_rect.x + left_total + (center_available - search_width) / 2.0;
    let mut left_x = panel_rect.x;
    let mut right_x = panel_rect.right() - right_total;

    affordances
        .iter()
        .map(|item| match item.id.as_str() {
            "apps" | "network" => {
                let width = width_for(&item.id).expect("known left panel affordance");
                let rect = Rect::new(left_x, panel_rect.y, width, panel_rect.height);
                left_x += width;
                rect
            }
            "search" => Rect::new(search_x, panel_rect.y, search_width, panel_rect.height),
            _ => {
                let width = width_for(&item.id).expect("known right panel affordance");
                let rect = Rect::new(right_x, panel_rect.y, width, panel_rect.height);
                right_x += width;
                rect
            }
        })
        .collect()
}

fn panel_search_view(active_menu: Option<&LiveMenu>) -> PanelSearchView {
    active_menu
        .filter(|menu| menu.family == MenuFamily::Launcher)
        .map(|menu| PanelSearchView {
            query: menu.shell.state.query.clone(),
            focused: menu.shell.search_focused,
        })
        .unwrap_or_default()
}

fn battery_presentation(value: Option<&str>) -> (Option<u8>, bool) {
    let Some((capacity, status)) = value.and_then(|value| value.split_once('%')) else {
        return (None, false);
    };
    let percent = capacity
        .trim()
        .parse::<u8>()
        .ok()
        .filter(|percent| *percent <= 100);
    let charging = status
        .rsplit('·')
        .next()
        .is_some_and(|status| status.trim().eq_ignore_ascii_case("charging"));
    (percent, charging)
}

fn compact_clock(value: Option<&str>) -> String {
    let time = value.unwrap_or("").trim().as_bytes();
    if time.len() >= 5
        && time[0..2].iter().all(u8::is_ascii_digit)
        && time[2] == b':'
        && time[3..5].iter().all(u8::is_ascii_digit)
        && time[0..2]
            .iter()
            .fold(0_u32, |n, digit| n * 10 + u32::from(*digit - b'0'))
            < 24
        && time[3..5]
            .iter()
            .fold(0_u32, |n, digit| n * 10 + u32::from(*digit - b'0'))
            < 60
    {
        String::from_utf8(time[0..5].to_vec()).expect("validated ASCII time")
    } else {
        "--:--".to_owned()
    }
}

fn panel_text(affordance: &PanelAffordance, available_width: f64) -> String {
    let raw = match (affordance.id.as_str(), affordance.value.as_deref()) {
        ("clock", value) => compact_clock(value),
        ("network" | "audio", Some(value)) => {
            // The icon replaces the category label; detail stays in the
            // controls sheet where it has room for the complete value.
            value.split('·').next().unwrap_or(value).trim().to_owned()
        }
        ("network" | "audio", None) => "—".to_owned(),
        ("battery", value) => {
            let (percent, charging) = battery_presentation(value);
            match percent {
                Some(percent) if charging => format!("{percent}% ⚡"),
                Some(percent) => format!("{percent}%"),
                None => "--%".to_owned(),
            }
        }
        (_, Some(value)) if !value.is_empty() => format!("{} · {value}", affordance.label),
        _ => affordance.label.clone(),
    };
    let approx_chars = (available_width / 7.0).floor().max(3.0) as usize;
    truncate_chars(&raw, approx_chars)
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let count = value.chars().count();
    if count <= max_chars {
        return value.to_owned();
    }
    if max_chars <= 1 {
        return "…".into();
    }
    let mut out: String = value.chars().take(max_chars - 1).collect();
    out.push('…');
    out
}

fn state_color(state: ValueState, theme: RenderTheme) -> Color {
    match state {
        ValueState::Ready => theme.accent,
        ValueState::Stale => theme.warning,
        ValueState::Unavailable => theme.hint,
        ValueState::Error => theme.error,
    }
}

fn panel_target_for_id(id: &str) -> Option<PanelTarget> {
    let (family, focus_search, control_center_tab) = match id {
        "apps" => (MenuFamily::Launcher, false, None),
        "network" => (
            MenuFamily::ControlCenter,
            false,
            Some(ControlCenterTab::Network),
        ),
        "search" => (MenuFamily::Launcher, true, None),
        "audio" => (
            MenuFamily::ControlCenter,
            false,
            Some(ControlCenterTab::Media),
        ),
        "power" => (MenuFamily::Power, false, None),
        "battery" | "renderer" | "clock" => (
            MenuFamily::ControlCenter,
            false,
            Some(ControlCenterTab::System),
        ),
        _ => return None,
    };
    Some(PanelTarget {
        family,
        focus_search,
        control_center_tab,
    })
}

fn wayland_panel_edge(edge: ShellPanelEdge) -> WaylandPanelEdge {
    match edge {
        ShellPanelEdge::Top => WaylandPanelEdge::Top,
        ShellPanelEdge::Bottom => WaylandPanelEdge::Bottom,
        ShellPanelEdge::Left => WaylandPanelEdge::Left,
        ShellPanelEdge::Right => WaylandPanelEdge::Right,
    }
}

fn shell_hit_region(scene: &Scene, x: f64, y: f64) -> Option<ShellHitRegion> {
    scene.hit_test(x, y).map(|item_id| ShellHitRegion {
        region_id: format!("row:{item_id}"),
        item_id: item_id.to_owned(),
    })
}

fn fixture_snapshot_from_probe(probe: &ProbeSnapshot) -> FixtureSnapshot {
    let network_value = if probe.network.connected {
        let mut value = probe
            .network
            .ssid
            .clone()
            .unwrap_or_else(|| "Connected".into());
        if let Some(signal) = probe.network.signal_percent {
            value.push_str(&format!(" · {signal}%"));
        }
        Some(value)
    } else {
        Some("Disconnected".into())
    };
    let wifi_networks = probe
        .network
        .networks
        .iter()
        .map(|network| WifiNetworkEntry {
            ssid: network.ssid.clone(),
            signal_percent: network.signal_percent,
            secured: network.secured,
            connected: network.connected,
        })
        .collect();
    let audio_value = probe.audio.volume_percent.map(|volume| {
        if probe.audio.muted == Some(true) {
            format!("{volume}% · muted")
        } else {
            format!("{volume}% · speaker")
        }
    });
    let preferred_backlight = probe
        .backlight
        .devices
        .iter()
        .find(|device| device.writable)
        .or_else(|| probe.backlight.devices.first());
    let brightness_value = preferred_backlight.map(|device| format!("{}%", device.percent));
    let brightness_writable = preferred_backlight.is_some_and(|device| device.writable);
    let battery_value = probe
        .battery
        .supplies
        .iter()
        .find_map(|supply| {
            supply
                .capacity_percent
                .map(|capacity| (capacity, supply.status.as_deref()))
        })
        .map(|(capacity, status)| match status {
            Some(status) => format!("{capacity}% · {status}"),
            None => format!("{capacity}%"),
        });

    FixtureSnapshot {
        network: probe_value(&probe.network.meta, network_value, &probe.network.issues),
        audio: probe_value(&probe.audio.meta, audio_value, &probe.audio.issues),
        battery: probe_value(&probe.battery.meta, battery_value, &probe.battery.issues),
        clock: probe_value(
            &probe.clock.meta,
            Some(probe.clock.time_label.clone()),
            &probe.clock.issues,
        ),
        brightness: probe_value(
            &probe.backlight.meta,
            brightness_value,
            &probe.backlight.issues,
        ),
        renderer: Some(nuraloumi_providers::renderer::running_renderer().into()),
        renderer_switch_available: std::path::Path::new("/usr/local/sbin/sl101-renderer-switch")
            .exists(),
        brightness_writable,
        bluetooth: probe_value(
            &probe.bluetooth.meta,
            probe.bluetooth.powered.map(|powered| {
                let connected = probe
                    .bluetooth
                    .devices
                    .iter()
                    .filter(|device| device.connected)
                    .count();
                format!(
                    "{} · {connected} connected",
                    if powered { "On" } else { "Off" }
                )
            }),
            &probe.bluetooth.issues,
        ),
        media: ProviderValue {
            state: ValueState::Unavailable,
            value: None,
            message: Some("MPRIS provider not refreshed yet".into()),
        },
        media_player: None,
        notifications: ProviderValue {
            state: ValueState::Unavailable,
            value: None,
            message: Some("Notification history not refreshed yet".into()),
        },
        notification_history: Vec::new(),
        wifi_networks,
        bluetooth_devices: probe
            .bluetooth
            .devices
            .iter()
            .map(|device| BluetoothDeviceEntry {
                id: device.address.clone(),
                label: device.label.clone(),
                paired: device.paired,
                connected: device.connected,
            })
            .collect(),
        tasks: Vec::new(),
        windows: Vec::new(),
        window_thumbnails: Vec::new(),
        window_thumbnails_available: false,
        window_thumbnail_issue: None,
        applications: Vec::new(),
        recent_application_ids: Vec::new(),
        desktops: Vec::new(),
        desktop_capabilities: DesktopControlCapabilities::unavailable(),
    }
}

fn refresh_probe_snapshot(
    snapshot: &mut FixtureSnapshot,
    probe: &ProbeSnapshot,
    enable_renderer_switch: bool,
) {
    let tasks = std::mem::take(&mut snapshot.tasks);
    let applications = std::mem::take(&mut snapshot.applications);
    let recent_application_ids = std::mem::take(&mut snapshot.recent_application_ids);
    let windows = std::mem::take(&mut snapshot.windows);
    let window_thumbnails = std::mem::take(&mut snapshot.window_thumbnails);
    let window_thumbnails_available = snapshot.window_thumbnails_available;
    let window_thumbnail_issue = snapshot.window_thumbnail_issue.clone();
    let desktops = std::mem::take(&mut snapshot.desktops);
    let desktop_capabilities = snapshot.desktop_capabilities;
    let media = snapshot.media.clone();
    let media_player = snapshot.media_player.clone();
    let notifications = snapshot.notifications.clone();
    let notification_history = snapshot.notification_history.clone();
    *snapshot = fixture_snapshot_from_probe(probe);
    snapshot.renderer_switch_available &= enable_renderer_switch;
    snapshot.tasks = tasks;
    snapshot.applications = applications;
    snapshot.recent_application_ids = recent_application_ids;
    snapshot.windows = windows;
    snapshot.window_thumbnails = window_thumbnails;
    snapshot.window_thumbnails_available = window_thumbnails_available;
    snapshot.window_thumbnail_issue = window_thumbnail_issue;
    snapshot.desktops = desktops;
    snapshot.desktop_capabilities = desktop_capabilities;
    snapshot.media = media;
    snapshot.media_player = media_player;
    snapshot.notifications = notifications;
    snapshot.notification_history = notification_history;
}

fn apply_application_snapshot(
    snapshot: &mut FixtureSnapshot,
    applications: Result<ApplicationSnapshot, nuraloumi_providers::ProviderError>,
    preferences: &nuraloumi_shell::LauncherPreferences,
) {
    match applications {
        Ok(applications) => {
            snapshot.applications = applications
                .applications
                .into_iter()
                .map(|application| ShellApplicationEntry {
                    id: application.id,
                    label: application.name,
                    generic_name: application.generic_name,
                    keywords: application.keywords,
                    launchable: application.launchable,
                    pinned: false,
                })
                .collect();
            apply_launcher_preferences(&mut snapshot.applications, preferences);
        }
        Err(error) => {
            eprintln!("nuraloumi-application-provider-error: {error}");
            snapshot.applications.clear();
        }
    }
}

fn apply_process_snapshot(
    snapshot: &mut FixtureSnapshot,
    processes: Result<ProcessSnapshot, nuraloumi_providers::ProviderError>,
) {
    match processes {
        Ok(processes) => {
            snapshot.tasks = processes
                .processes
                .into_iter()
                .map(|process| ShellTaskEntry {
                    id: process.id,
                    label: process.label,
                    state: process.state,
                    cpu_percent: process.cpu_percent,
                    memory_mib: process.memory_mib,
                })
                .collect();
        }
        Err(error) => {
            eprintln!("nuraloumi-process-provider-error: {error}");
            snapshot.tasks.clear();
        }
    }
}

fn refresh_media_snapshot(snapshot: &mut FixtureSnapshot) {
    let mut provider = MediaProvider::system();
    match provider.snapshot() {
        Ok(media) => {
            let current = media.current().cloned();
            let value = current.as_ref().map(|player| {
                let label = player
                    .title
                    .as_deref()
                    .filter(|title| !title.is_empty())
                    .unwrap_or(&player.id);
                format!("{} · {label}", player.status)
            });
            snapshot.media = probe_value(&media.meta, value, &media.issues);
            snapshot.media_player = current.map(|player| ShellMediaPlayerEntry {
                id: player.id,
                status: player.status,
                artist: player.artist,
                title: player.title,
            });
        }
        Err(error) => {
            snapshot.media = ProviderValue {
                state: ValueState::Unavailable,
                value: None,
                message: Some(error.diagnostic()),
            };
            snapshot.media_player = None;
        }
    }
}

fn refresh_notification_snapshot(snapshot: &mut FixtureSnapshot) {
    let mut provider = NotificationProvider::system();
    match provider.snapshot() {
        Ok(notifications) => {
            let value = Some(format!(
                "{} saved notification{}",
                notifications.notifications.len(),
                if notifications.notifications.len() == 1 {
                    ""
                } else {
                    "s"
                }
            ));
            snapshot.notifications = probe_value(&notifications.meta, value, &notifications.issues);
            snapshot.notification_history = notifications
                .notifications
                .into_iter()
                .map(|notification| ShellNotificationHistoryEntry {
                    id: notification.id,
                    app: notification.app,
                    summary: notification.summary,
                    body: notification.body,
                    actions: notification.actions,
                    default_action: notification.default_action,
                })
                .collect();
        }
        Err(error) => {
            snapshot.notifications = ProviderValue {
                state: ValueState::Unavailable,
                value: None,
                message: Some(error.diagnostic()),
            };
            snapshot.notification_history.clear();
        }
    }
}

fn refresh_control_center_tab_snapshot(snapshot: &mut FixtureSnapshot, tab: ControlCenterTab) {
    match tab {
        ControlCenterTab::Media => refresh_media_snapshot(snapshot),
        ControlCenterTab::Notifications => refresh_notification_snapshot(snapshot),
        ControlCenterTab::Network | ControlCenterTab::Display | ControlCenterTab::System => {}
    }
}

fn refresh_lazy_control_action(snapshot: &mut FixtureSnapshot, action: &MenuAction) -> bool {
    match action {
        MenuAction::Custom { kind, .. } if kind.starts_with("media.") => {
            refresh_media_snapshot(snapshot);
            true
        }
        MenuAction::Custom { kind, .. } if kind.starts_with("notification.") => {
            refresh_notification_snapshot(snapshot);
            true
        }
        _ => false,
    }
}

fn refresh_window_snapshot(snapshot: &mut FixtureSnapshot, backend: &WaylandBackend) {
    let capabilities = window_control_capabilities(&backend.capabilities());
    snapshot.windows = window_entries(&backend.toplevels(), capabilities);
    snapshot.window_thumbnails.retain(|thumbnail| {
        snapshot
            .windows
            .iter()
            .any(|window| window.id == thumbnail.window_id)
    });
}

#[derive(Clone, Debug)]
struct ThumbnailHelperRequest {
    key: String,
    title: String,
    app_id: Option<String>,
    protocol_identifier: Option<String>,
}

#[derive(Clone, Copy, Debug, Default)]
struct ThumbnailHelperCapabilities {
    ext_foreign_toplevel_list: bool,
    foreign_toplevel_capture_source: bool,
    image_copy_capture: bool,
    shm: bool,
}

impl ThumbnailHelperCapabilities {
    fn available(self) -> bool {
        self.ext_foreign_toplevel_list
            && self.foreign_toplevel_capture_source
            && self.image_copy_capture
            && self.shm
    }
}

#[derive(Clone, Debug)]
struct ThumbnailHelperThumbnail {
    key: String,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
struct ThumbnailHelperReport {
    capabilities: ThumbnailHelperCapabilities,
    thumbnails: Vec<ThumbnailHelperThumbnail>,
    issues: Vec<String>,
}

const THUMBNAIL_HELPER_REQUEST_MAGIC: &[u8; 8] = b"NLTHRQ01";
const THUMBNAIL_HELPER_RESPONSE_MAGIC: &[u8; 8] = b"NLTHRS01";
const THUMBNAIL_HELPER_MAX_STRING_BYTES: usize = 4096;
const THUMBNAIL_HELPER_MAX_ISSUES: usize = 16;
const THUMBNAIL_HELPER_MAX_THUMBNAILS: usize = 4;
const THUMBNAIL_HELPER_MAX_PIXEL_BYTES: usize = 16 * 1024 * 1024;

fn start_window_thumbnail_probe(
    backend: &WaylandBackend,
) -> Option<std::thread::JoinHandle<Result<ThumbnailHelperReport, String>>> {
    let mut windows = backend.toplevels();
    windows.sort_by_key(|window| !window.state.activated);
    let requests = windows
        .into_iter()
        .take(4)
        .map(|window| ThumbnailHelperRequest {
            key: window.id.to_string(),
            title: window.title,
            app_id: window.app_id,
            protocol_identifier: window.protocol_identifier,
        })
        .collect::<Vec<_>>();
    (!requests.is_empty())
        .then(|| std::thread::spawn(move || run_window_thumbnail_helper(&requests)))
}

fn run_window_thumbnail_helper(
    requests: &[ThumbnailHelperRequest],
) -> Result<ThumbnailHelperReport, String> {
    let request_bytes = encode_thumbnail_helper_request(requests)?;
    let current_exe =
        env::current_exe().map_err(|error| format!("resolve panel executable failed: {error}"))?;
    let parent = current_exe
        .parent()
        .ok_or_else(|| "panel executable has no parent directory".to_owned())?;
    let helper = parent.join("nuraloumi-thumbnail-helper");
    let mut child = Command::new(&helper)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            format!(
                "start thumbnail helper {} failed: {error}",
                helper.display()
            )
        })?;
    child
        .stdin
        .take()
        .ok_or_else(|| "thumbnail helper stdin unavailable".to_owned())?
        .write_all(&request_bytes)
        .map_err(|error| format!("write thumbnail helper request failed: {error}"))?;
    let output = child
        .wait_with_output()
        .map_err(|error| format!("wait for thumbnail helper failed: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "thumbnail helper exited with {}: {}",
            output.status,
            stderr.trim()
        ));
    }
    decode_thumbnail_helper_report(&output.stdout)
}

fn encode_thumbnail_helper_request(requests: &[ThumbnailHelperRequest]) -> Result<Vec<u8>, String> {
    if requests.len() > THUMBNAIL_HELPER_MAX_THUMBNAILS {
        return Err(format!(
            "thumbnail helper accepts at most {THUMBNAIL_HELPER_MAX_THUMBNAILS} requests"
        ));
    }
    let mut out = Vec::new();
    out.extend_from_slice(THUMBNAIL_HELPER_REQUEST_MAGIC);
    push_helper_u32(&mut out, requests.len())?;
    for request in requests {
        push_helper_string(&mut out, &request.key)?;
        push_helper_string(&mut out, &request.title)?;
        push_helper_optional_string(&mut out, request.app_id.as_deref())?;
        push_helper_optional_string(&mut out, request.protocol_identifier.as_deref())?;
    }
    Ok(out)
}

fn decode_thumbnail_helper_report(bytes: &[u8]) -> Result<ThumbnailHelperReport, String> {
    let mut cursor = ThumbnailHelperCursor::new(bytes);
    cursor.expect_magic(THUMBNAIL_HELPER_RESPONSE_MAGIC)?;
    let mask = cursor.read_u8()?;
    let capabilities = ThumbnailHelperCapabilities {
        ext_foreign_toplevel_list: mask & 1 != 0,
        foreign_toplevel_capture_source: mask & 2 != 0,
        image_copy_capture: mask & 4 != 0,
        shm: mask & 8 != 0,
    };
    let issue_count = cursor.read_u32()? as usize;
    if issue_count > THUMBNAIL_HELPER_MAX_ISSUES {
        return Err(format!(
            "thumbnail helper returned {issue_count} issues, limit is {THUMBNAIL_HELPER_MAX_ISSUES}"
        ));
    }
    let mut issues = Vec::with_capacity(issue_count);
    for _ in 0..issue_count {
        issues.push(cursor.read_string()?);
    }
    let thumbnail_count = cursor.read_u32()? as usize;
    if thumbnail_count > THUMBNAIL_HELPER_MAX_THUMBNAILS {
        return Err(format!(
            "thumbnail helper returned {thumbnail_count} thumbnails, limit is {THUMBNAIL_HELPER_MAX_THUMBNAILS}"
        ));
    }
    let mut thumbnails = Vec::with_capacity(thumbnail_count);
    for _ in 0..thumbnail_count {
        let key = cursor.read_string()?;
        let width = cursor.read_u32()?;
        let height = cursor.read_u32()?;
        let pixel_len = cursor.read_u32()? as usize;
        if pixel_len > THUMBNAIL_HELPER_MAX_PIXEL_BYTES {
            return Err(format!(
                "thumbnail helper pixel payload {pixel_len} exceeds {THUMBNAIL_HELPER_MAX_PIXEL_BYTES}"
            ));
        }
        let pixels = cursor.read_bytes(pixel_len)?.to_vec();
        thumbnails.push(ThumbnailHelperThumbnail {
            key,
            width,
            height,
            pixels,
        });
    }
    cursor.expect_end()?;
    Ok(ThumbnailHelperReport {
        capabilities,
        thumbnails,
        issues,
    })
}

fn push_helper_u32(out: &mut Vec<u8>, value: usize) -> Result<(), String> {
    let value = u32::try_from(value).map_err(|_| "thumbnail helper request length exceeds u32")?;
    out.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

fn push_helper_string(out: &mut Vec<u8>, value: &str) -> Result<(), String> {
    if value.len() > THUMBNAIL_HELPER_MAX_STRING_BYTES {
        return Err(format!(
            "thumbnail helper request string is {} bytes, limit is {THUMBNAIL_HELPER_MAX_STRING_BYTES}",
            value.len()
        ));
    }
    push_helper_u32(out, value.len())?;
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn push_helper_optional_string(out: &mut Vec<u8>, value: Option<&str>) -> Result<(), String> {
    match value {
        Some(value) => push_helper_string(out, value),
        None => {
            out.extend_from_slice(&u32::MAX.to_le_bytes());
            Ok(())
        }
    }
}

struct ThumbnailHelperCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> ThumbnailHelperCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn expect_magic(&mut self, expected: &[u8; 8]) -> Result<(), String> {
        if self.read_bytes(expected.len())? != expected {
            return Err("thumbnail helper response magic mismatch".to_owned());
        }
        Ok(())
    }

    fn read_u8(&mut self) -> Result<u8, String> {
        Ok(self.read_bytes(1)?[0])
    }

    fn read_u32(&mut self) -> Result<u32, String> {
        let bytes: [u8; 4] = self
            .read_bytes(4)?
            .try_into()
            .map_err(|_| "thumbnail helper response u32 decode failed")?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn read_string(&mut self) -> Result<String, String> {
        let len = self.read_u32()? as usize;
        if len > THUMBNAIL_HELPER_MAX_STRING_BYTES {
            return Err(format!(
                "thumbnail helper response string length {len} exceeds {THUMBNAIL_HELPER_MAX_STRING_BYTES}"
            ));
        }
        String::from_utf8(self.read_bytes(len)?.to_vec())
            .map_err(|_| "thumbnail helper response string is not UTF-8".to_owned())
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| "thumbnail helper response length overflow".to_owned())?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| "thumbnail helper response truncated".to_owned())?;
        self.offset = end;
        Ok(bytes)
    }

    fn expect_end(&self) -> Result<(), String> {
        if self.offset != self.bytes.len() {
            return Err("thumbnail helper response has trailing bytes".to_owned());
        }
        Ok(())
    }
}

fn apply_window_thumbnail_report(
    snapshot: &mut FixtureSnapshot,
    result: Result<ThumbnailHelperReport, String>,
) {
    match result {
        Ok(report) => {
            snapshot.window_thumbnails_available = report.capabilities.available();
            snapshot.window_thumbnail_issue = report.issues.first().cloned();
            for issue in &report.issues {
                eprintln!("nuraloumi-thumbnail: {issue}");
            }
            snapshot.window_thumbnails = report
                .thumbnails
                .into_iter()
                .map(|thumbnail| WindowThumbnailEntry {
                    window_id: thumbnail.key,
                    width: thumbnail.width,
                    height: thumbnail.height,
                    pixels: thumbnail.pixels,
                })
                .collect();
        }
        Err(error) => {
            eprintln!("nuraloumi-thumbnail: {error}");
            snapshot.window_thumbnails_available = false;
            snapshot.window_thumbnail_issue = Some(error);
            snapshot.window_thumbnails.clear();
        }
    }
}

fn paint_window_thumbnail_overlays(
    scene: &Scene,
    buffer: &mut nuraloumi_render_cairo::RenderedBuffer,
    snapshot: &FixtureSnapshot,
) -> Result<(), String> {
    if scene.menu_id != "launcher" || snapshot.window_thumbnails.is_empty() {
        return Ok(());
    }
    for thumbnail in &snapshot.window_thumbnails {
        let row_id = format!("overview.window.{}", thumbnail.window_id);
        let Some(hit) = scene.hits.iter().find(|hit| hit.item_id == row_id) else {
            continue;
        };
        let preview_width = 72.0_f64.min(hit.rect.width * 0.28);
        let preview = Rect {
            x: hit.rect.right() - preview_width - 8.0,
            y: hit.rect.y + 4.0,
            width: preview_width,
            height: (hit.rect.height - 8.0).max(1.0),
        };
        buffer
            .paint_argb32_preview(
                preview,
                scene.viewport.scale,
                thumbnail.width,
                thumbnail.height,
                &thumbnail.pixels,
            )
            .map_err(|_| "failed to paint window thumbnail preview".to_owned())?;
    }
    Ok(())
}

fn window_control_capabilities(backend: &WaylandCapabilities) -> WindowControlCapabilities {
    let mut capabilities: WindowControlCapabilities = backend.toplevel.into();
    capabilities.focus &= backend.seat;
    capabilities
}

fn successful_compositor_restart(action: &MenuAction, result: &ActionResult) -> bool {
    matches!(
        action,
        MenuAction::Custom { kind, .. } if kind == "renderer.switch"
    ) && result.executed
        && !result.dry_run
}

fn execute_live_action(
    action: &MenuAction,
    enable_power_actions: bool,
    enable_renderer_switch: bool,
) -> Result<Option<ActionResult>, String> {
    let result = match action {
        MenuAction::Custom { kind, payload } if kind == "renderer.switch" => {
            nuraloumi_providers::renderer::request_switch(
                &mut SystemCommandRunner,
                payload,
                enable_renderer_switch,
            )
            .map_err(|error| error.to_string())?
        }
        MenuAction::Custom { kind, payload } if kind == "app.launch" => {
            ApplicationProvider::system()
                .launch_nonblocking(payload)
                .map_err(|error| error.to_string())?
        }
        MenuAction::Custom { kind, payload } if kind == "media.previous" => MediaProvider::system()
            .execute(MediaAction::Previous {
                player: payload.clone(),
            })
            .map_err(|error| error.to_string())?,
        MenuAction::Custom { kind, payload } if kind == "media.play_pause" => {
            MediaProvider::system()
                .execute(MediaAction::PlayPause {
                    player: payload.clone(),
                })
                .map_err(|error| error.to_string())?
        }
        MenuAction::Custom { kind, payload } if kind == "media.next" => MediaProvider::system()
            .execute(MediaAction::Next {
                player: payload.clone(),
            })
            .map_err(|error| error.to_string())?,
        MenuAction::Custom { kind, payload } if kind == "notification.redisplay" => {
            let id = payload
                .parse::<u32>()
                .map_err(|_| format!("invalid notification id {payload:?}"))?;
            NotificationProvider::system()
                .execute(NotificationAction::Redisplay { id })
                .map_err(|error| error.to_string())?
        }
        MenuAction::Custom { kind, payload } if kind == "notification.remove" => {
            let id = payload
                .parse::<u32>()
                .map_err(|_| format!("invalid notification id {payload:?}"))?;
            NotificationProvider::system()
                .execute(NotificationAction::Remove { id })
                .map_err(|error| error.to_string())?
        }
        MenuAction::Custom { kind, .. } if kind == "notification.clear" => {
            NotificationProvider::system()
                .execute(NotificationAction::Clear)
                .map_err(|error| error.to_string())?
        }
        MenuAction::Custom { kind, payload } if kind == "notification.invoke" => {
            let request: NotificationInvokeRequest = serde_json::from_str(payload)
                .map_err(|error| format!("invalid notification action payload: {error}"))?;
            NotificationProvider::system()
                .execute(NotificationAction::Invoke {
                    id: request.id,
                    action: request.action,
                })
                .map_err(|error| error.to_string())?
        }
        MenuAction::Toggle { id } if id == "network.wifi" => {
            NetworkProvider::new(SystemCommandRunner)
                .execute(NetworkAction::ToggleRadio)
                .map_err(|error| error.to_string())?
        }
        MenuAction::Activate { id } if id == "network.rescan" => {
            NetworkProvider::new(SystemCommandRunner)
                .execute(NetworkAction::Rescan { interface: None })
                .map_err(|error| error.to_string())?
        }
        MenuAction::Custom { kind, payload } if kind == "network.connect" => {
            NetworkProvider::new(SystemCommandRunner)
                .execute(NetworkAction::Connect {
                    ssid: payload.clone(),
                    interface: None,
                })
                .map_err(|error| error.to_string())?
        }
        MenuAction::Toggle { id } if id == "bluetooth.radio" => {
            BluetoothProvider::new(SystemCommandRunner)
                .execute(BluetoothAction::TogglePower)
                .map_err(|error| error.to_string())?
        }
        MenuAction::Custom { kind, payload } if kind == "bluetooth.connect" => {
            BluetoothProvider::new(SystemCommandRunner)
                .execute(BluetoothAction::Connect {
                    address: payload.clone(),
                })
                .map_err(|error| error.to_string())?
        }
        MenuAction::Custom { kind, payload } if kind == "bluetooth.disconnect" => {
            BluetoothProvider::new(SystemCommandRunner)
                .execute(BluetoothAction::Disconnect {
                    address: payload.clone(),
                })
                .map_err(|error| error.to_string())?
        }
        MenuAction::Toggle { id } if id == "audio.mute" => AudioProvider::new(SystemCommandRunner)
            .execute(AudioAction::ToggleMute)
            .map_err(|error| error.to_string())?,
        MenuAction::Adjust { id, delta } if id == "audio.volume" => {
            let delta = i16::try_from(*delta)
                .map_err(|_| format!("audio delta {delta} exceeds provider range"))?;
            AudioProvider::new(SystemCommandRunner)
                .execute(AudioAction::AdjustVolume(delta))
                .map_err(|error| error.to_string())?
        }
        MenuAction::Adjust { id, delta } if id == "system.brightness" => {
            let mut provider = BacklightProvider::system();
            let current = provider.snapshot().map_err(|error| error.to_string())?;
            let device = current
                .devices
                .iter()
                .find(|device| device.writable)
                .ok_or_else(|| "no writable backlight device is available".to_owned())?;
            let percent = (i32::from(device.percent) + *delta).clamp(0, 100) as u8;
            provider
                .execute(BacklightAction::SetPercent {
                    device: device.name.clone(),
                    percent,
                })
                .map_err(|error| error.to_string())?
        }
        MenuAction::Activate { id } if id == "system.suspend" => {
            SessionProvider::new(SystemCommandRunner)
                .with_destructive_actions(enable_power_actions)
                .execute(SessionAction::Suspend)
                .map_err(|error| error.to_string())?
        }
        MenuAction::Activate { id } if id == "system.restart" => {
            SessionProvider::new(SystemCommandRunner)
                .with_destructive_actions(enable_power_actions)
                .execute(SessionAction::Reboot)
                .map_err(|error| error.to_string())?
        }
        MenuAction::Activate { id } if id == "system.poweroff" => {
            SessionProvider::new(SystemCommandRunner)
                .with_destructive_actions(enable_power_actions)
                .execute(SessionAction::PowerOff)
                .map_err(|error| error.to_string())?
        }
        MenuAction::Activate { id } if id == "system.end_desktop" => {
            let mut provider = SessionProvider::new(SystemCommandRunner)
                .with_destructive_actions(enable_power_actions);
            if enable_power_actions {
                let owned =
                    OwnedDesktopSession::from_environment().map_err(|error| error.to_string())?;
                provider = provider.with_desktop_session(owned);
            }
            provider
                .execute(SessionAction::EndDesktop)
                .map_err(|error| error.to_string())?
        }
        _ => return Ok(None),
    };
    Ok(Some(result))
}

fn probe_value(meta: &SnapshotMeta, value: Option<String>, issues: &[String]) -> ProviderValue {
    let state = if meta.stale {
        ValueState::Stale
    } else {
        match meta.health {
            Health::Healthy => ValueState::Ready,
            Health::Degraded => ValueState::Stale,
            Health::Unavailable => ValueState::Unavailable,
        }
    };
    ProviderValue {
        state,
        value,
        message: (!issues.is_empty()).then(|| issues.join("; ")),
    }
}

fn parse_args() -> Result<Args, String> {
    let mut parsed = Args::default();
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--headless" => {}
            "--live" => parsed.live = true,
            "--providers" => parsed.providers = Some(next_value(&mut args, "--providers")?.into()),
            "--config" => parsed.config = Some(next_value(&mut args, "--config")?.into()),
            "--dump-keybindings" => parsed.dump_keybindings = true,
            "--open" => parsed.open = Some(next_value(&mut args, "--open")?),
            "--reduced-motion" => parsed.reduced_motion = true,
            "--enable-power-actions" => parsed.enable_power_actions = true,
            "--enable-renderer-switch" => parsed.enable_renderer_switch = true,
            other => return Err(format!("unknown argument {other:?}; use --help")),
        }
    }
    Ok(parsed)
}

fn next_value(args: &mut impl Iterator<Item = String>, option: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{option} requires a value"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_bar_has_non_overlapping_touch_regions_across_sl101_widths() {
        let config = ShellConfig::default();
        let snapshot = FixtureSnapshot::default();
        for width in [800, 1024, 1280] {
            let scene = build_panel_scene(
                &snapshot,
                &config,
                &PanelSearchView::default(),
                (width, config.panel_height, 1),
                RenderTheme::dark(),
            );
            let mut hits = scene.hits.iter().collect::<Vec<_>>();
            hits.sort_by(|left, right| left.rect.x.total_cmp(&right.rect.x));
            assert!(hits.iter().all(|hit| hit.rect.width >= 48.0));
            assert!(
                hits.windows(2)
                    .all(|pair| { pair[0].rect.right() <= pair[1].rect.x + f64::EPSILON }),
                "status slots must not overlap at width {width}"
            );
            assert!(hits.first().unwrap().rect.x >= 0.0);
            assert!(hits.last().unwrap().rect.right() <= f64::from(width));
        }
    }

    #[test]
    fn touch_then_synthetic_pointer_is_one_activation_but_real_mouse_still_works() {
        use std::time::{Duration, Instant};
        let mut guard = SyntheticPointerFilter::default();
        let t0 = Instant::now();
        let apps = TouchPosition {
            surface: None,
            x: 44.0,
            y: 16.0,
        };
        let network = TouchPosition {
            surface: None,
            x: 250.0,
            y: 16.0,
        };
        guard.down(7, apps);
        // Spurious pointer press while the finger is still down.
        assert!(guard.suppress_pointer(apps, true, t0));
        assert!(guard.suppress_pointer(apps, false, t0 + Duration::from_millis(2)));
        guard.up(7, t0 + Duration::from_millis(10));
        // The emulated click following touch-up must not re-open the menu.
        assert!(guard.suppress_pointer(apps, true, t0 + Duration::from_millis(20)));
        assert!(guard.suppress_pointer(apps, false, t0 + Duration::from_millis(21)));
        // Real mouse targeting a *different* widget works immediately.
        assert!(!guard.suppress_pointer(network, true, t0 + Duration::from_millis(22)));
        assert!(!guard.suppress_pointer(network, false, t0 + Duration::from_millis(23)));
        // Even at the same spot the mouse works after the bounded window.
        assert!(!guard.suppress_pointer(apps, true, t0 + Duration::from_millis(220)));
        assert!(!guard.suppress_pointer(apps, false, t0 + Duration::from_millis(221)));
    }

    #[test]
    fn touch_move_cancel_and_focus_transition_are_safe() {
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        let apps = TouchPosition {
            surface: None,
            x: 44.0,
            y: 16.0,
        };
        let mut guard = SyntheticPointerFilter::default();
        guard.down(8, apps);
        guard.motion(8, TouchPosition { x: 150.0, ..apps });
        guard.up(8, t0);
        assert!(!guard.suppress_pointer(apps, true, t0 + Duration::from_millis(10)));
        guard.down(9, apps);
        guard.cancel(&[9]);
        assert!(!guard.suppress_pointer(apps, true, t0 + Duration::from_millis(210)));

        let mut pressed = Some("apps".to_owned());
        update_panel_touch_press(&mut pressed, Some("network"));
        assert!(
            pressed.is_none(),
            "dragging away must cancel the original target"
        );
        assert!(!should_dismiss_for_focus_change(Some("window-a"), None));
        assert!(!should_dismiss_for_focus_change(None, None));
        assert!(!should_dismiss_for_focus_change(
            Some("window-a"),
            Some("window-a")
        ));
        assert!(should_dismiss_for_focus_change(
            Some("window-a"),
            Some("window-b")
        ));

        let mut fingers = BTreeMap::new();
        begin_panel_touch(&mut fingers, 1, Some("apps".to_owned()));
        begin_panel_touch(&mut fingers, 2, Some("apps".to_owned()));
        assert!(fingers.values().all(Option::is_none));
    }

    #[test]
    fn backdrop_touch_survives_surface_destruction_without_reopening_menu() {
        use std::time::{Duration, Instant};
        let now = Instant::now();
        let mut guard = SyntheticPointerFilter::default();
        let backdrop = TouchPosition {
            surface: Some(SurfaceId(2)),
            x: 40.0,
            y: 40.0,
        };
        let panel = TouchPosition {
            surface: Some(SurfaceId(1)),
            x: 40.0,
            y: 8.0,
        };
        guard.down(11, backdrop);
        guard.up(11, now);
        assert!(guard.suppress_pointer(panel, true, now + Duration::from_millis(20)));
        assert!(guard.suppress_pointer(panel, false, now + Duration::from_millis(21)));
        assert!(!guard.suppress_pointer(panel, true, now + Duration::from_millis(200)));
    }

    #[test]
    fn battery_and_clock_present_compact_truthful_panel_values() {
        let display = |id: &str, value: Option<&str>| {
            panel_text(
                &PanelAffordance {
                    id: id.to_owned(),
                    label: if id == "clock" { "Clock" } else { "Battery" }.to_owned(),
                    value: value.map(str::to_owned),
                    state: ValueState::Ready,
                },
                140.0,
            )
        };
        assert_eq!(display("clock", Some("07:05 Europe/Berlin")), "07:05");
        assert_eq!(display("clock", None), "--:--");
        assert_eq!(display("clock", Some("nonsense")), "--:--");
        assert_eq!(display("battery", Some("75% · Discharging")), "75%");
        assert_eq!(display("battery", Some("75% · Charging")), "75% ⚡");
        assert_eq!(display("battery", Some("75% · Not charging")), "75%");
        assert_eq!(display("battery", None), "--%");
        assert_eq!(battery_presentation(Some("101% · Charging")), (None, true));
    }

    #[test]
    fn live_provider_refresh_is_deferred_only_without_fixture_providers() {
        let live = Args {
            live: true,
            ..Args::default()
        };
        assert!(should_defer_live_provider_refresh(&live));

        let fixture_live = Args {
            live: true,
            providers: Some("providers.json".into()),
            ..Args::default()
        };
        assert!(!should_defer_live_provider_refresh(&fixture_live));

        assert!(!should_defer_live_provider_refresh(&Args::default()));
    }

    #[test]
    fn panel_target_mapping_is_stable_and_search_requests_focus() {
        assert_eq!(
            panel_target_for_id("apps"),
            Some(PanelTarget {
                family: MenuFamily::Launcher,
                focus_search: false,
                control_center_tab: None,
            })
        );
        assert_eq!(
            panel_target_for_id("network"),
            Some(PanelTarget {
                family: MenuFamily::ControlCenter,
                focus_search: false,
                control_center_tab: Some(ControlCenterTab::Network),
            })
        );
        assert_eq!(
            panel_target_for_id("search"),
            Some(PanelTarget {
                family: MenuFamily::Launcher,
                focus_search: true,
                control_center_tab: None,
            })
        );
        assert_eq!(
            panel_target_for_id("audio"),
            Some(PanelTarget {
                family: MenuFamily::ControlCenter,
                focus_search: false,
                control_center_tab: Some(ControlCenterTab::Media),
            })
        );
        assert_eq!(
            panel_target_for_id("battery"),
            Some(PanelTarget {
                family: MenuFamily::ControlCenter,
                focus_search: false,
                control_center_tab: Some(ControlCenterTab::System),
            })
        );
        assert_eq!(
            panel_target_for_id("clock"),
            Some(PanelTarget {
                family: MenuFamily::ControlCenter,
                focus_search: false,
                control_center_tab: Some(ControlCenterTab::System),
            })
        );
    }

    #[test]
    fn panel_scene_has_one_hit_per_affordance() {
        let snapshot = FixtureSnapshot::default();
        let config = ShellConfig::default();
        let search = PanelSearchView::default();
        let scene = build_panel_scene(
            &snapshot,
            &config,
            &search,
            (1366, config.panel_height, 1),
            RenderTheme::dark(),
        );
        assert_eq!(scene.hits.len(), panel_affordances(&snapshot).len() + 1);
        assert!(scene.hits.iter().all(|hit| hit.actionable && hit.enabled));
        assert_eq!(scene.panel_rect.width, 1366.0);
        assert_eq!(scene.panel_rect.height, f64::from(config.panel_height));

        let apps = scene.hits.iter().find(|hit| hit.item_id == "apps").unwrap();
        let network = scene
            .hits
            .iter()
            .find(|hit| hit.item_id == "network")
            .unwrap();
        let search = scene
            .hits
            .iter()
            .find(|hit| hit.item_id == "search")
            .unwrap();
        let audio = scene
            .hits
            .iter()
            .find(|hit| hit.item_id == "audio")
            .unwrap();
        let clock = scene
            .hits
            .iter()
            .find(|hit| hit.item_id == "clock")
            .unwrap();
        assert_eq!(apps.rect.width, 84.0);
        assert_eq!(network.rect.width, 180.0);
        assert_eq!(search.rect.width, 360.0);
        assert!(network.rect.right() < search.rect.x);
        assert!(search.rect.right() < audio.rect.x);
        let power = scene
            .hits
            .iter()
            .find(|hit| hit.item_id == "power")
            .unwrap();
        assert!(clock.rect.right() <= power.rect.x);
        assert_eq!(power.rect.right(), scene.panel_rect.right());
    }

    #[test]
    fn battery_fill_and_power_icon_are_deterministic_and_font_independent() {
        let mut snapshot = FixtureSnapshot::default();
        snapshot.battery.value = Some("25% · Charging".to_owned());
        snapshot.battery.state = ValueState::Ready;
        let config = ShellConfig::default();
        let scene = build_panel_scene(
            &snapshot,
            &config,
            &PanelSearchView::default(),
            (1280, config.panel_height, 1),
            RenderTheme::dark(),
        );
        let battery = scene
            .hits
            .iter()
            .find(|hit| hit.item_id == "battery")
            .unwrap();
        let power = scene
            .hits
            .iter()
            .find(|hit| hit.item_id == "power")
            .unwrap();
        assert_eq!(power.rect.width, 48.0);
        assert!(scene.paint.iter().any(|node| matches!(
            node,
            PaintNode::FillRect { rect, .. }
                if (rect.width - 4.0).abs() < f64::EPSILON
                    && rect.x >= battery.rect.x
                    && rect.right() <= battery.rect.right()
        )));
        assert!(!scene.paint.iter().any(|node| matches!(
            node,
            PaintNode::Text { text, .. } if text == "⏻"
        )));
    }

    #[test]
    fn panel_visual_geometry_uses_square_corners() {
        let snapshot = FixtureSnapshot::default();
        let config = ShellConfig::default();
        let scene = build_panel_scene(
            &snapshot,
            &config,
            &PanelSearchView::default(),
            (1280, config.panel_height, 1),
            RenderTheme::dark(),
        );

        assert!(scene.paint.iter().all(|node| match node {
            PaintNode::RoundedRect { radius, .. } => *radius == 0.0,
            _ => true,
        }));
    }

    #[test]
    fn focused_search_query_is_rendered_in_top_bar() {
        let snapshot = FixtureSnapshot::default();
        let config = ShellConfig::default();
        let search = PanelSearchView {
            query: "term".into(),
            focused: true,
        };
        let scene = build_panel_scene(
            &snapshot,
            &config,
            &search,
            (1280, config.panel_height, 1),
            RenderTheme::dark(),
        );
        assert!(scene.paint.iter().any(|node| matches!(
            node,
            PaintNode::Text { text, .. } if text == "term"
        )));
    }

    #[test]
    fn panel_text_truncates_without_splitting_unicode() {
        let affordance = PanelAffordance {
            id: "network".into(),
            label: "Network".into(),
            value: Some("Werkstatt-Gastnetz · 82%".into()),
            state: ValueState::Ready,
        };
        let label = panel_text(&affordance, 70.0);
        assert!(label.ends_with('…'));
        assert!(label.chars().count() <= 10);
    }

    #[test]
    fn status_bar_draws_network_and_audio_icons_without_redundant_labels() {
        let mut snapshot = FixtureSnapshot::default();
        snapshot.network.value = Some("Was Geht Lan · 77%".into());
        snapshot.audio.value = Some("35% · Speakers".into());
        let config = ShellConfig::default();
        let scene = build_panel_scene(
            &snapshot,
            &config,
            &PanelSearchView::default(),
            (1280, config.panel_height, 1),
            RenderTheme::dark(),
        );
        let texts: Vec<&str> = scene
            .paint
            .iter()
            .filter_map(|node| match node {
                PaintNode::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(texts.contains(&"Was Geht Lan"));
        assert!(texts.contains(&"35%"));
        assert!(!texts.iter().any(|text| {
            text.starts_with("Network") || text.starts_with("Audio") || text.starts_with("Clock")
        }));
        for id in ["network", "audio"] {
            let hit = scene.hits.iter().find(|hit| hit.item_id == id).unwrap();
            let strokes = scene
                .paint
                .iter()
                .filter(|node| {
                    matches!(node,
                        PaintNode::Line { from, to, width, .. }
                            if (*width - 1.8).abs() < 0.001
                                && from.x > hit.rect.x && from.x < hit.rect.right()
                                && to.x > hit.rect.x && to.x < hit.rect.right()
                    )
                })
                .count();
            assert!(
                strokes >= 6,
                "{id} should render a recognizable vector icon"
            );
        }
    }

    #[test]
    fn successful_renderer_restart_exits_only_after_real_execution() {
        let renderer = MenuAction::Custom {
            kind: "renderer.switch".into(),
            payload: "grate".into(),
        };
        assert!(successful_compositor_restart(
            &renderer,
            &ActionResult::executed("restarted")
        ));
        assert!(!successful_compositor_restart(
            &renderer,
            &ActionResult::dry_run("not restarted")
        ));

        let unrelated = MenuAction::Custom {
            kind: "audio.adjust".into(),
            payload: "5".into(),
        };
        assert!(!successful_compositor_restart(
            &unrelated,
            &ActionResult::executed("changed")
        ));
    }

    #[test]
    fn destructive_live_action_is_dry_run_without_explicit_enable() {
        let result = execute_live_action(
            &MenuAction::Activate {
                id: "system.poweroff".into(),
            },
            false,
            false,
        )
        .expect("dry-run session action should be valid")
        .expect("poweroff should map to a session action");
        assert!(!result.executed);
        assert!(result.dry_run);
    }

    #[test]
    fn logout_is_dry_run_without_power_permission() {
        let result = execute_live_action(
            &MenuAction::Activate {
                id: "system.end_desktop".into(),
            },
            false,
            false,
        )
        .expect("logout dry run should not inspect local sessions")
        .expect("logout must be wired to SessionProvider");
        assert!(result.dry_run);
        assert!(!result.executed);
    }

    #[test]
    fn sl101_round_bits_matches_rust_round_semantics() {
        for value in [
            -2.75_f64, -2.5, -2.49, -1.5, -0.5, -0.49, -0.0, 0.0, 0.49, 0.5, 1.5, 2.49, 2.5, 2.75,
            1024.5,
        ] {
            assert_eq!(sl101_round_bits(value).to_bits(), value.round().to_bits());
        }
        assert!(sl101_round_bits(f64::NAN).is_nan());
        assert_eq!(sl101_round_bits(f64::INFINITY), f64::INFINITY);
        assert_eq!(sl101_round_bits(f64::NEG_INFINITY), f64::NEG_INFINITY);
    }

    #[test]
    fn panel_wayland_edge_mapping_preserves_orientation() {
        assert_eq!(
            wayland_panel_edge(ShellPanelEdge::Top),
            WaylandPanelEdge::Top
        );
        assert_eq!(
            wayland_panel_edge(ShellPanelEdge::Right),
            WaylandPanelEdge::Right
        );
    }
}
