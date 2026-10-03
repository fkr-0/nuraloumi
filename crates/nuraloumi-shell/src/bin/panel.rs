use nuraloumi_core::{DARK_THEME, LIGHT_THEME};
use nuraloumi_providers::{
    ActionProvider, ActionResult, ApplicationAction, ApplicationProvider, ApplicationSnapshot,
    AudioAction, AudioProvider, BacklightAction, BacklightProvider, BluetoothAction,
    BluetoothProvider, Health, MediaAction, MediaProvider, NetworkAction, NetworkProvider,
    NotificationAction, NotificationProvider, ProbeSnapshot, ProcessProvider, ProcessSnapshot,
    Provider, SessionAction, SessionProvider, SnapshotMeta, SystemCommandRunner,
};
use nuraloumi_render_cairo::{
    CairoRenderer, Color, HitRegion as RenderHitRegion, PaintNode, Point, Rect, RenderOptions,
    RowKind, Scene, ScrollWindow, TextStyle, Theme as RenderTheme, Viewport,
};
use nuraloumi_shell::{
    apply_launcher_preferences, build_family, execute_window_command, launcher_search_input,
    load_config_or_default, load_fixture_snapshot, panel_affordances, parse_desktop_command,
    parse_family, parse_window_command, render_menu_follow_selection, resolve_app_activation,
    window_entries, ActionReport, AppActivation, ApplicationEntry as ShellApplicationEntry,
    BluetoothDeviceEntry, ControlCenterTab, DesktopCommand, DesktopControlCapabilities,
    DesktopEntry, FixtureSnapshot, HitRegion as ShellHitRegion,
    MediaPlayerEntry as ShellMediaPlayerEntry, MenuAction, MenuFamily,
    NotificationHistoryEntry as ShellNotificationHistoryEntry, OverviewMode, PanelAffordance,
    PanelController, PanelEdge as ShellPanelEdge, PlatformEvent as ShellPlatformEvent,
    ProviderValue, SceneTransitionClock, SemanticInput, ShellConfig, ShellState,
    TaskEntry as ShellTaskEntry, Theme as ShellTheme, ValueState, WifiNetworkEntry, WindowCommand,
    WindowControlCapabilities, WindowThumbnailEntry,
};
use nuraloumi_wayland::{
    BackendCapabilities as WaylandCapabilities, BackendError, Frame, Key as WaylandKey,
    MenuConfig as WaylandMenuConfig, PanelConfig as WaylandPanelConfig,
    PanelEdge as WaylandPanelEdge, PixelFormat, PlatformEvent as WaylandEvent, SurfaceId,
    WaylandBackend, WorkspaceId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::io::Write;
use std::path::PathBuf;
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
    --open <family>            Open launcher|control-center|network|audio|system initially
    --reduced-motion           Force reduced-motion state
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
    open: Option<String>,
    reduced_motion: bool,
    enable_power_actions: bool,
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
    family: MenuFamily,
    shell: ShellState,
    geometry: Option<(u32, u32, i32)>,
    scene: Option<Scene>,
    touch_regions: BTreeMap<i32, Option<ShellHitRegion>>,
    motion: Option<SceneTransitionClock>,
}

#[derive(Clone, Copy)]
struct LivePolicy {
    execute_provider_actions: bool,
    enable_power_actions: bool,
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
    let mut config = load_config_or_default(args.config.as_deref())?;
    if args.reduced_motion {
        config.reduced_motion = true;
    }
    config.validate()?;

    let defer_live_provider_refresh = should_defer_live_provider_refresh(&args);
    let mut snapshot = if let Some(path) = args.providers.as_ref() {
        load_fixture_snapshot(path)?
    } else {
        FixtureSnapshot::default()
    };
    apply_launcher_preferences(&mut snapshot.applications, &config.launcher);

    if args.live {
        let initial_family = args.open.as_deref().map(parse_family).transpose()?;
        return run_live(
            config,
            snapshot,
            initial_family,
            LivePolicy {
                execute_provider_actions: args.providers.is_none(),
                enable_power_actions: args.enable_power_actions,
            },
            defer_live_provider_refresh,
        );
    }

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

fn run_live(
    config: ShellConfig,
    mut snapshot: FixtureSnapshot,
    initial_family: Option<MenuFamily>,
    policy: LivePolicy,
    defer_live_provider_refresh: bool,
) -> Result<(), String> {
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
        } else {
            backend
                .blocking_dispatch()
                .map_err(|error| format!("Wayland dispatch failed: {error}"))?;
        }
        let toplevel_changed = backend.drain_toplevel_events().count() > 0;
        let workspace_changed = backend.drain_workspace_events().count() > 0;
        if policy.execute_provider_actions && (toplevel_changed || workspace_changed) {
            if toplevel_changed {
                refresh_window_snapshot(&mut snapshot, &backend);
                thumbnail_probe_started = false;
            }
            if workspace_changed {
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
        }
        let events: Vec<_> = backend.drain_events().collect();

        for event in events {
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
                        panel_touch_press.insert(id, hit);
                    }
                    WaylandEvent::TouchUp { id } => {
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
                    report = menu
                        .shell
                        .handle_platform_event(ShellPlatformEvent::PointerButton {
                            region,
                            pressed,
                        });
                    redraw = true;
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
                    menu.touch_regions.insert(id, region.clone());
                    report = menu
                        .shell
                        .handle_platform_event(ShellPlatformEvent::TouchDown { id, region });
                    redraw = true;
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
                    if let Some(input) = semantic_from_wayland_key(key) {
                        report = Some(menu.shell.apply_semantic(input));
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
                        let is_navigation = matches!(
                            action,
                            MenuAction::Custom { kind, .. }
                                if kind == "menu.open"
                                    || kind == "overview.mode"
                                    || kind == "control.tab"
                        );
                        if !is_navigation {
                            if let Some(command) = parse_desktop_command(action, &snapshot)? {
                                match command {
                                    DesktopCommand::Switch { desktop_id } => {
                                        let id = parse_workspace_id(&desktop_id)?;
                                        backend
                                            .activate_workspace(id)
                                            .map_err(|error| error.to_string())?;
                                        redraw = true;
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
                                let close_after_focus = matches!(command, WindowCommand::Focus(_));
                                execute_window_command(&mut backend, command)?;
                                if close_after_focus {
                                    backend.roundtrip().map_err(|error| {
                                        format!("toplevel focus roundtrip failed: {error}")
                                    })?;
                                    close = true;
                                } else {
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
                                                close = true;
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
                                                backend.roundtrip().map_err(|error| {
                                                    format!(
                                                        "toplevel focus roundtrip failed: {error}"
                                                    )
                                                })?;
                                                close = true;
                                                app_reused = true;
                                            }
                                            AppActivation::Launch => {}
                                        }
                                    }
                                }
                                if !app_reused {
                                    match execute_live_action(action, policy.enable_power_actions) {
                                        Ok(Some(result)) => {
                                            eprintln!(
                                            "nuraloumi-provider-result: executed={} dry_run={} message={}",
                                            result.executed, result.dry_run, result.message
                                        );
                                            if app_launch && result.executed {
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
            refresh_probe_snapshot(&mut snapshot, &probe);
            apply_application_snapshot(&mut snapshot, applications, &config.launcher);
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
    let surface = backend
        .create_menu(WaylandMenuConfig {
            width: requested_width,
            height: requested_height,
            margin_top,
            margin_left,
            output: output.map(|output| output.id),
            namespace: "nuraloumi-panel-menu".into(),
        })
        .map_err(|error| format!("failed to create panel menu surface: {error}"))?;
    panel.open_menu(family, true);
    Ok(LiveMenu {
        surface,
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
    let (scene, mut buffer) = render_menu_follow_selection(
        renderer,
        &shell.menu,
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
                radius: 8.0,
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

        let marker_color = state_color(affordance.state, theme);
        let marker = if horizontal {
            Rect::new(rect.x + 8.0, rect.y + rect.height / 2.0 - 2.5, 5.0, 5.0)
        } else {
            Rect::new(rect.x + 6.0, rect.y + 8.0, 5.0, 5.0)
        };
        paint.push(PaintNode::RoundedRect {
            rect: marker,
            radius: 2.5,
            fill: nuraloumi_render_cairo::Fill::Solid(marker_color),
            stroke: None,
        });

        let available = if horizontal {
            (rect.width - 28.0).max(18.0)
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
            origin: Point::new(rect.x + 20.0, baseline),
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
            "apps" => Some(96.0),
            "network" => Some(310.0),
            "audio" => Some(140.0),
            "battery" => Some(180.0),
            "clock" => Some(118.0),
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
        .filter(|item| matches!(item.id.as_str(), "audio" | "battery" | "clock"))
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

fn panel_text(affordance: &PanelAffordance, available_width: f64) -> String {
    let raw = match affordance.value.as_deref() {
        Some(value) if !value.is_empty() => format!("{} · {value}", affordance.label),
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
        "battery" | "clock" => (
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

fn semantic_from_wayland_key(key: WaylandKey) -> Option<SemanticInput> {
    match key {
        WaylandKey::Up => Some(SemanticInput::Up),
        WaylandKey::Down => Some(SemanticInput::Down),
        WaylandKey::Left => Some(SemanticInput::Left),
        WaylandKey::Right => Some(SemanticInput::Right),
        WaylandKey::Enter => Some(SemanticInput::Activate),
        WaylandKey::Escape => Some(SemanticInput::Back),
        WaylandKey::Backspace => Some(SemanticInput::Backspace),
        WaylandKey::Text(text) => Some(SemanticInput::Text(text)),
        WaylandKey::Raw(_) => None,
    }
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
        desktops: Vec::new(),
        desktop_capabilities: DesktopControlCapabilities::unavailable(),
    }
}

fn refresh_probe_snapshot(snapshot: &mut FixtureSnapshot, probe: &ProbeSnapshot) {
    let tasks = std::mem::take(&mut snapshot.tasks);
    let applications = std::mem::take(&mut snapshot.applications);
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
    snapshot.tasks = tasks;
    snapshot.applications = applications;
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

fn execute_live_action(
    action: &MenuAction,
    enable_power_actions: bool,
) -> Result<Option<ActionResult>, String> {
    let result = match action {
        MenuAction::Custom { kind, payload } if kind == "app.launch" => {
            ApplicationProvider::system()
                .execute(ApplicationAction::Launch {
                    id: payload.clone(),
                })
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
            "--open" => parsed.open = Some(next_value(&mut args, "--open")?),
            "--reduced-motion" => parsed.reduced_motion = true,
            "--enable-power-actions" => parsed.enable_power_actions = true,
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
        assert_eq!(apps.rect.width, 96.0);
        assert_eq!(network.rect.width, 310.0);
        assert_eq!(search.rect.width, 360.0);
        assert!(network.rect.right() < search.rect.x);
        assert!(search.rect.right() < audio.rect.x);
        assert_eq!(clock.rect.right(), scene.panel_rect.right());
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
            value: Some("Werkstatt · 82%".into()),
            state: ValueState::Ready,
        };
        let label = panel_text(&affordance, 70.0);
        assert!(label.ends_with('…'));
        assert!(label.chars().count() <= 10);
    }

    #[test]
    fn destructive_live_action_is_dry_run_without_explicit_enable() {
        let result = execute_live_action(
            &MenuAction::Activate {
                id: "system.poweroff".into(),
            },
            false,
        )
        .expect("dry-run session action should be valid")
        .expect("poweroff should map to a session action");
        assert!(!result.executed);
        assert!(result.dry_run);
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
