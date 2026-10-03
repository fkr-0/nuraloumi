use nuraloumi_core::{DARK_THEME, LIGHT_THEME};
use nuraloumi_providers::{
    ActionProvider, ActionResult, ApplicationAction, ApplicationProvider, AudioAction,
    AudioProvider, BacklightAction, BacklightProvider, BluetoothAction, BluetoothProvider, Health,
    MediaAction, MediaProvider, NetworkAction, NetworkProvider, NotificationAction,
    NotificationProvider, ProbeSnapshot, ProcessProvider, Provider, SessionAction, SessionProvider,
    SnapshotMeta, SystemCommandRunner,
};
use nuraloumi_render_cairo::{CairoRenderer, RenderOptions, Scene, Viewport};
use nuraloumi_shell::{
    apply_launcher_preferences, blank_press_requests_dismiss, build_family,
    dismiss_backdrop_margins, dispatch_keybinding, execute_window_command, launcher_search_input,
    load_config_or_default, load_fixture_snapshot, load_menu, parse_desktop_command, parse_family,
    parse_window_command, render_menu_follow_selection, resolve_app_activation,
    successful_action_requests_dismiss, window_entries, ActionReport, AppActivation,
    ApplicationEntry as ShellApplicationEntry, BluetoothDeviceEntry, ControlCenterTab,
    DesktopCommand, DesktopControlCapabilities, DesktopEntry, FixtureSnapshot,
    HitRegion as ShellHitRegion, KeybindingRegistry, MediaPlayerEntry as ShellMediaPlayerEntry,
    MenuAction, MenuFamily, NotificationHistoryEntry as ShellNotificationHistoryEntry,
    OverviewMode, PlatformEvent as ShellPlatformEvent, ProviderValue, SceneTransitionClock,
    SemanticInput, ShellConfig, ShellInput, ShellState, TaskEntry as ShellTaskEntry,
    Theme as ShellTheme, ValueState, WifiNetworkEntry, WindowCommand, WindowControlCapabilities,
    WindowEntry, WindowThumbnailEntry,
};
use nuraloumi_wayland::{
    capture_toplevel_thumbnails_with_timeout, BackendCapabilities as WaylandCapabilities,
    BackendError, DismissBackdropConfig, Frame, MenuConfig as WaylandMenuConfig, PixelFormat,
    PlatformEvent as WaylandEvent, SurfaceId, ToplevelThumbnailReport, ToplevelThumbnailRequest,
    WaylandBackend, WorkspaceId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

const HELP: &str = r#"nuraloumi-menu — NuraLoumi menu runner

USAGE:
    nuraloumi-menu [OPTIONS]

OPTIONS:
    --headless                 Render no surface; print deterministic JSON (default)
    --live                     Open a native Wayland/Cairo software-rendered menu sheet
    --probe-toplevels          Print compositor toplevel capabilities/windows as JSON and exit
    --probe-workspaces         Print compositor workspace capabilities/workspaces as JSON and exit
    --family <name>            launcher|control-center|wifi|bluetooth|display|audio|power|tasks|windows|system
    --fixture <path>           Load a JSON/TOML MenuModel instead of a built-in family
    --providers <path>         Load deterministic JSON/TOML provider snapshot
    --config <path>            Load JSON/TOML shell geometry/theme config
    --dump-keybindings         Print effective keybinding registry as JSON and exit
    --input <steps>            Initial semantic input, e.g. down,enter,search,text:term
    --reduced-motion           Force reduced-motion state
    --enable-power-actions     Allow confirmed reboot/poweroff in live mode
    --enable-unsafe-suspend    Also allow confirmed suspend; requires --enable-power-actions
    -h, --help                 Show this help

HEADLESS OUTPUT:
    JSON containing the menu, semantic state, action reports and hit regions.

LIVE MODE:
    Uses wl_shm + layer-shell + Cairo only; no EGL/XWayland. Without --providers,
    status values and ordinary controls use bounded provider adapters. Reboot/poweroff
    remain dry-run unless --enable-power-actions is supplied after UI confirmation.
    Suspend remains dry-run unless --enable-unsafe-suspend is supplied as a second opt-in.
"#;

#[derive(Debug, Default)]
struct Args {
    live: bool,
    probe_toplevels: bool,
    probe_workspaces: bool,
    family: Option<String>,
    fixture: Option<PathBuf>,
    providers: Option<PathBuf>,
    config: Option<PathBuf>,
    dump_keybindings: bool,
    input: Option<String>,
    reduced_motion: bool,
    enable_power_actions: bool,
    enable_unsafe_suspend: bool,
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

#[derive(Clone, Copy)]
struct LivePolicy {
    execute_provider_actions: bool,
    enable_power_actions: bool,
    enable_unsafe_suspend: bool,
}

#[derive(Debug, Deserialize)]
struct NotificationInvokeRequest {
    id: u32,
    action: String,
}

#[derive(Debug, Serialize)]
struct Output<'a> {
    mode: &'static str,
    family: MenuFamily,
    config: &'a ShellConfig,
    shell: &'a ShellState,
    reports: &'a [ActionReport],
    hit_regions: Vec<ShellHitRegion>,
}

#[derive(Debug, Serialize)]
struct ToplevelProbeOutput {
    capabilities: WindowControlCapabilities,
    windows: Vec<WindowEntry>,
}

#[derive(Debug, Serialize)]
struct WorkspaceProbeOutput {
    capabilities: DesktopControlCapabilities,
    desktops: Vec<DesktopEntry>,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("nuraloumi-menu: {error}");
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
    if args.enable_unsafe_suspend && (!args.live || !args.enable_power_actions) {
        return Err("--enable-unsafe-suspend requires --live and --enable-power-actions".into());
    }
    if args.probe_toplevels {
        return run_toplevel_probe();
    }
    if args.probe_workspaces {
        return run_workspace_probe();
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

    let mut snapshot = if let Some(path) = args.providers.as_ref() {
        load_fixture_snapshot(path)?
    } else if args.live {
        fixture_snapshot_from_probe(&ProbeSnapshot::live())
    } else {
        FixtureSnapshot::default()
    };
    apply_launcher_preferences(&mut snapshot.applications, &config.launcher);
    let family = parse_family(args.family.as_deref().unwrap_or("launcher"))?;
    if args.live && args.providers.is_none() {
        refresh_application_snapshot(&mut snapshot, &config.launcher);
        if matches!(family, MenuFamily::Launcher | MenuFamily::Tasks) {
            refresh_process_snapshot(&mut snapshot);
        }
    }
    let menu = if let Some(path) = args.fixture.as_ref() {
        load_menu(path)?
    } else {
        build_family(family, &snapshot)
    };
    let mut shell = ShellState::new(menu, config.reduced_motion)?;

    if args.live {
        return run_live(
            shell,
            config,
            snapshot,
            keybindings,
            LivePolicy {
                execute_provider_actions: args.providers.is_none(),
                enable_power_actions: args.enable_power_actions,
                enable_unsafe_suspend: args.enable_unsafe_suspend,
            },
            args.input.as_deref(),
        );
    }

    let mut reports = Vec::new();
    if let Some(input) = args.input.as_deref() {
        for step in input.split(',').filter(|step| !step.is_empty()) {
            let report = shell.apply_input(parse_input(step)?);
            if matches!(report, ActionReport::SearchChanged { .. }) {
                let _ = refresh_builtin_menu(&mut shell, &snapshot)?;
            }
            reports.push(report);
        }
    }

    let output = Output {
        mode: "headless",
        family,
        config: &config,
        hit_regions: shell.hit_regions(),
        shell: &shell,
        reports: &reports,
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&output)
            .map_err(|err| format!("failed to serialize headless output: {err}"))?
    );
    Ok(())
}

fn run_toplevel_probe() -> Result<(), String> {
    let mut backend =
        WaylandBackend::connect().map_err(|error| format!("Wayland connection failed: {error}"))?;
    backend
        .roundtrip()
        .map_err(|error| format!("Wayland toplevel roundtrip failed: {error}"))?;
    let capabilities = window_control_capabilities(&backend.capabilities());
    let output = ToplevelProbeOutput {
        windows: window_entries(&backend.toplevels(), capabilities),
        capabilities,
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&output)
            .map_err(|error| format!("failed to serialize toplevel probe: {error}"))?
    );
    Ok(())
}

fn run_workspace_probe() -> Result<(), String> {
    let mut backend =
        WaylandBackend::connect().map_err(|error| format!("Wayland connection failed: {error}"))?;
    backend
        .roundtrip()
        .map_err(|error| format!("Wayland workspace roundtrip failed: {error}"))?;
    let mut snapshot = FixtureSnapshot::default();
    refresh_workspace_snapshot(&mut snapshot, &backend);
    let output = WorkspaceProbeOutput {
        capabilities: snapshot.desktop_capabilities,
        desktops: snapshot.desktops,
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&output)
            .map_err(|error| format!("failed to serialize workspace probe: {error}"))?
    );
    Ok(())
}

fn run_live(
    mut shell: ShellState,
    config: ShellConfig,
    mut snapshot: FixtureSnapshot,
    keybindings: KeybindingRegistry,
    policy: LivePolicy,
    initial_input: Option<&str>,
) -> Result<(), String> {
    let mut backend =
        WaylandBackend::connect().map_err(|error| format!("Wayland connection failed: {error}"))?;
    let capabilities = backend.capabilities();
    if !capabilities.layer_shell {
        return Err("compositor does not advertise zwlr_layer_shell_v1".into());
    }
    if policy.execute_provider_actions {
        refresh_window_snapshot(&mut snapshot, &backend);
        refresh_workspace_snapshot(&mut snapshot, &backend);
        refresh_builtin_menu(&mut shell, &snapshot)?;
    }

    if let Some(input) = initial_input {
        for step in input.split(',').filter(|step| !step.is_empty()) {
            let report = shell.apply_input(parse_input(step)?);
            if handle_live_report(
                &report,
                &mut shell,
                &mut snapshot,
                &mut backend,
                &config,
                policy,
            )? {
                return Ok(());
            }
        }
    }

    let output = backend.outputs().into_iter().next();
    let requested_width = output
        .as_ref()
        .and_then(|output| {
            let scale = output.scale.max(1) as u32;
            (output.mode_width > 0).then_some(output.mode_width as u32 / scale)
        })
        .map(|width| config.menu_width.min(width.saturating_sub(16).max(240)))
        .unwrap_or(config.menu_width);
    let requested_height = output
        .as_ref()
        .and_then(|output| {
            let scale = output.scale.max(1) as u32;
            (output.mode_height > 0).then_some(output.mode_height as u32 / scale)
        })
        .map(|height| {
            height
                .saturating_sub(config.panel_height.saturating_add(16))
                .clamp(240, 720)
        })
        .unwrap_or(640);

    let dismiss_surface = if config.menu_dismissal.outside_press {
        let (margin_top, margin_right, margin_bottom, margin_left) =
            dismiss_backdrop_margins(config.panel_edge, config.panel_height);
        Some(
            backend
                .create_dismiss_backdrop(DismissBackdropConfig {
                    margin_top,
                    margin_right,
                    margin_bottom,
                    margin_left,
                    output: output.as_ref().map(|output| output.id),
                    namespace: "nuraloumi-menu-dismiss".into(),
                })
                .map_err(|error| format!("failed to create menu dismiss backdrop: {error}"))?,
        )
    } else {
        None
    };
    let surface = match backend.create_menu(WaylandMenuConfig {
        width: requested_width,
        height: requested_height,
        margin_top: config.panel_height as i32,
        margin_left: 0,
        output: output.as_ref().map(|output| output.id),
        namespace: "nuraloumi-menu".into(),
    }) {
        Ok(surface) => surface,
        Err(error) => {
            if let Some(dismiss_surface) = dismiss_surface {
                let _ = backend.destroy_surface(dismiss_surface);
            }
            return Err(format!("failed to create menu surface: {error}"));
        }
    };

    let renderer = CairoRenderer::default();
    let theme_tokens = match config.theme {
        ShellTheme::Dark => &DARK_THEME,
        ShellTheme::Light => &LIGHT_THEME,
    };
    let mut geometry: Option<(u32, u32, i32)> = None;
    let mut scene: Option<Scene> = None;
    let mut touch_regions: BTreeMap<i32, Option<ShellHitRegion>> = BTreeMap::new();
    let mut initial_control_refresh_pending =
        policy.execute_provider_actions && shell.menu.id == "control-center";
    let mut motion = (!config.reduced_motion).then(|| SceneTransitionClock::new(false));
    let mut thumbnail_probe: Option<
        std::thread::JoinHandle<Result<ToplevelThumbnailReport, String>>,
    > = None;
    let mut thumbnail_probe_started = false;

    loop {
        if policy.execute_provider_actions
            && shell.menu.id == "launcher"
            && !thumbnail_probe_started
        {
            thumbnail_probe = start_window_thumbnail_probe(&backend);
            thumbnail_probe_started = true;
        } else if shell.menu.id != "launcher" {
            thumbnail_probe_started = false;
        }

        if motion.is_some() || thumbnail_probe.is_some() {
            std::thread::sleep(SceneTransitionClock::frame_interval());
            backend
                .roundtrip()
                .map_err(|error| format!("Wayland animation roundtrip failed: {error}"))?;
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
            if refresh_builtin_menu(&mut shell, &snapshot)? {
                if let Some(geometry) = geometry {
                    scene = Some(render_and_present(
                        &mut backend,
                        surface,
                        &renderer,
                        theme_tokens,
                        &shell,
                        &snapshot,
                        (
                            geometry,
                            motion.as_ref().map(|clock| clock.sample().transition),
                        ),
                    )?);
                }
            }
        }
        let events: Vec<_> = backend.drain_events().collect();

        for event in events {
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
                            close_live_surfaces(&mut backend, surface, Some(dismiss_surface))?;
                            return Ok(());
                        }
                        WaylandEvent::Close => {
                            close_live_surfaces(&mut backend, surface, Some(dismiss_surface))?;
                            return Ok(());
                        }
                        _ => {}
                    }
                    continue;
                }
            }
            if event.surface.is_some() && event.surface != Some(surface) {
                continue;
            }

            let mut redraw = false;
            let mut report: Option<ActionReport> = None;

            match event.event {
                WaylandEvent::Configure {
                    width,
                    height,
                    scale,
                } if event.surface == Some(surface) => {
                    geometry = Some((width, height, scale));
                    redraw = true;
                }
                WaylandEvent::PointerButton { x, y, pressed, .. } => {
                    let input_enabled = motion
                        .as_ref()
                        .is_none_or(|clock| !clock.blocks_hit_testing());
                    let region = input_enabled
                        .then(|| {
                            scene
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
                        close_live_surfaces(&mut backend, surface, dismiss_surface)?;
                        return Ok(());
                    }
                    report = shell.handle_platform_event(ShellPlatformEvent::PointerButton {
                        region,
                        pressed,
                    });
                    redraw = true;
                }
                WaylandEvent::TouchDown { id, x, y } => {
                    let input_enabled = motion
                        .as_ref()
                        .is_none_or(|clock| !clock.blocks_hit_testing());
                    let region = input_enabled
                        .then(|| {
                            scene
                                .as_ref()
                                .and_then(|scene| shell_hit_region(scene, x, y))
                        })
                        .flatten();
                    if blank_press_requests_dismiss(
                        config.menu_dismissal,
                        input_enabled,
                        region.is_some(),
                    ) {
                        close_live_surfaces(&mut backend, surface, dismiss_surface)?;
                        return Ok(());
                    }
                    touch_regions.insert(id, region.clone());
                    report =
                        shell.handle_platform_event(ShellPlatformEvent::TouchDown { id, region });
                    redraw = true;
                }
                WaylandEvent::TouchMotion { id, x, y } => {
                    let input_enabled = motion
                        .as_ref()
                        .is_none_or(|clock| !clock.blocks_hit_testing());
                    let region = input_enabled
                        .then(|| {
                            scene
                                .as_ref()
                                .and_then(|scene| shell_hit_region(scene, x, y))
                        })
                        .flatten();
                    touch_regions.insert(id, region);
                }
                WaylandEvent::TouchUp { id } => {
                    let region = touch_regions.remove(&id).flatten();
                    report =
                        shell.handle_platform_event(ShellPlatformEvent::TouchUp { id, region });
                    redraw = true;
                }
                WaylandEvent::TouchCancel { ids } => {
                    for id in ids {
                        touch_regions.remove(&id);
                        let _ = shell.handle_platform_event(ShellPlatformEvent::TouchUp {
                            id,
                            region: None,
                        });
                    }
                    redraw = true;
                }
                WaylandEvent::Key { key, pressed: true } => {
                    if let Some(key_report) = dispatch_keybinding(&keybindings, &mut shell, &key) {
                        report = Some(key_report);
                        redraw = true;
                    }
                }
                WaylandEvent::Close => {
                    close_live_surfaces(&mut backend, surface, dismiss_surface)?;
                    return Ok(());
                }
                _ => {}
            }

            if let Some(report) = report {
                let scene_changed = matches!(
                    &report,
                    ActionReport::Dispatched {
                        action: MenuAction::Custom { kind, .. },
                        ..
                    } if kind == "menu.open" || kind == "overview.mode" || kind == "control.tab"
                );
                if handle_live_report(
                    &report,
                    &mut shell,
                    &mut snapshot,
                    &mut backend,
                    &config,
                    policy,
                )? {
                    close_live_surfaces(&mut backend, surface, dismiss_surface)?;
                    return Ok(());
                }
                if scene_changed {
                    motion = (!config.reduced_motion).then(|| SceneTransitionClock::new(false));
                }
                redraw = true;
            }

            if shell.closed {
                close_live_surfaces(&mut backend, surface, dismiss_surface)?;
                return Ok(());
            }

            if redraw {
                if let Some(geometry) = geometry {
                    scene = Some(render_and_present(
                        &mut backend,
                        surface,
                        &renderer,
                        theme_tokens,
                        &shell,
                        &snapshot,
                        (
                            geometry,
                            motion.as_ref().map(|clock| clock.sample().transition),
                        ),
                    )?);
                    if initial_control_refresh_pending {
                        initial_control_refresh_pending = false;
                        refresh_control_center_tab_snapshot(
                            &mut snapshot,
                            shell.control_center_tab,
                        );
                        shell.refresh_family(MenuFamily::ControlCenter, &snapshot)?;
                        scene = Some(render_and_present(
                            &mut backend,
                            surface,
                            &renderer,
                            theme_tokens,
                            &shell,
                            &snapshot,
                            (
                                geometry,
                                motion.as_ref().map(|clock| clock.sample().transition),
                            ),
                        )?);
                    }
                }
            }
        }

        if let (Some(clock), Some(geometry)) = (motion.as_ref(), geometry) {
            let sample = clock.sample();
            scene = Some(render_and_present(
                &mut backend,
                surface,
                &renderer,
                theme_tokens,
                &shell,
                &snapshot,
                (geometry, Some(sample.transition)),
            )?);
            if sample.complete {
                motion = None;
            }
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
            if shell.menu.id == "launcher" {
                apply_window_thumbnail_report(&mut snapshot, result);
                shell.refresh_family(MenuFamily::Launcher, &snapshot)?;
                if let Some(geometry) = geometry {
                    scene = Some(render_and_present(
                        &mut backend,
                        surface,
                        &renderer,
                        theme_tokens,
                        &shell,
                        &snapshot,
                        (
                            geometry,
                            motion.as_ref().map(|clock| clock.sample().transition),
                        ),
                    )?);
                }
            }
        }
    }
}

fn render_and_present(
    backend: &mut WaylandBackend,
    surface: SurfaceId,
    renderer: &CairoRenderer,
    tokens: &nuraloumi_core::ThemeTokens,
    shell: &ShellState,
    snapshot: &FixtureSnapshot,
    presentation: ((u32, u32, i32), Option<nuraloumi_core::Transition>),
) -> Result<Scene, String> {
    let (geometry, transition) = presentation;
    let (logical_width, logical_height, scale) = geometry;
    let viewport = Viewport::new(
        f64::from(logical_width),
        f64::from(logical_height),
        f64::from(scale.max(1)),
    );
    let (scene, mut buffer) = render_menu_follow_selection(
        renderer,
        &shell.menu,
        &shell.state,
        viewport,
        tokens,
        RenderOptions::default(),
        transition,
    )
    .map_err(|error| format!("Cairo render failed: {error}"))?;
    paint_window_thumbnail_overlays(&scene, &mut buffer, snapshot)?;

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
        Ok(()) | Err(BackendError::WouldBlock) => Ok(scene),
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

fn close_live_surfaces(
    backend: &mut WaylandBackend,
    surface: SurfaceId,
    dismiss_surface: Option<SurfaceId>,
) -> Result<(), String> {
    backend
        .destroy_surface(surface)
        .map_err(|error| format!("failed to destroy menu surface: {error}"))?;
    if let Some(dismiss_surface) = dismiss_surface {
        backend
            .destroy_surface(dismiss_surface)
            .map_err(|error| format!("failed to destroy menu dismiss backdrop: {error}"))?;
    }
    backend
        .flush()
        .map_err(|error| format!("failed to flush Wayland connection: {error}"))
}

fn shell_hit_region(scene: &Scene, x: f64, y: f64) -> Option<ShellHitRegion> {
    scene.hit_test(x, y).map(|item_id| ShellHitRegion {
        region_id: format!("row:{item_id}"),
        item_id: item_id.to_owned(),
    })
}

fn handle_live_report(
    report: &ActionReport,
    shell: &mut ShellState,
    snapshot: &mut FixtureSnapshot,
    backend: &mut WaylandBackend,
    config: &ShellConfig,
    policy: LivePolicy,
) -> Result<bool, String> {
    log_live_report(report)?;

    if matches!(report, ActionReport::SearchChanged { .. }) {
        let _ = refresh_builtin_menu(shell, snapshot)?;
    }

    if let ActionReport::Dispatched {
        action: MenuAction::Custom { kind, payload },
        ..
    } = report
    {
        if kind == "menu.open" {
            let family = parse_family(payload)?;
            if policy.execute_provider_actions && family == MenuFamily::ControlCenter {
                refresh_control_center_tab_snapshot(snapshot, ControlCenterTab::Media);
            }
            shell.refresh_family(family, snapshot)?;
            return Ok(false);
        }
        if kind == "overview.mode" {
            shell.set_overview_mode(OverviewMode::parse(payload)?, snapshot)?;
            return Ok(false);
        }
        if kind == "control.tab" {
            let tab = ControlCenterTab::parse(payload)?;
            if policy.execute_provider_actions {
                refresh_control_center_tab_snapshot(snapshot, tab);
            }
            shell.set_control_center_tab(tab, snapshot)?;
            return Ok(false);
        }
    }

    if policy.execute_provider_actions {
        if let ActionReport::Dispatched { action, .. } = report {
            let close_after_success =
                successful_action_requests_dismiss(config.menu_dismissal, action);
            if let Some(command) = parse_desktop_command(action, snapshot)? {
                match command {
                    DesktopCommand::Switch { desktop_id } => {
                        let id = parse_workspace_id(&desktop_id)?;
                        backend
                            .activate_workspace(id)
                            .map_err(|error| error.to_string())?;
                        if !close_after_success {
                            refresh_workspace_snapshot(snapshot, backend);
                            refresh_builtin_menu(shell, snapshot)?;
                        }
                    }
                    DesktopCommand::MoveWindow { .. } => {
                        return Err(
                            "move-window-to-desktop is not exposed by the active Wayland protocols"
                                .into(),
                        );
                    }
                }
                return Ok(close_after_success);
            }
            if let Some(command) = parse_window_command(action, &snapshot.windows)? {
                let focused = matches!(command, WindowCommand::Focus(_));
                execute_window_command(backend, command)?;
                if focused {
                    backend
                        .roundtrip()
                        .map_err(|error| format!("toplevel focus roundtrip failed: {error}"))?;
                }
                if !close_after_success {
                    refresh_window_snapshot(snapshot, backend);
                    refresh_builtin_menu(shell, snapshot)?;
                }
                return Ok(close_after_success);
            }
            if let MenuAction::Custom { kind, payload } = action {
                if kind == "app.launch" {
                    match resolve_app_activation(payload, &snapshot.windows, &config.launcher) {
                        AppActivation::AlreadyFocused => return Ok(close_after_success),
                        AppActivation::FocusWindow(window_id) => {
                            let focus = MenuAction::Custom {
                                kind: "window.focus".into(),
                                payload: window_id,
                            };
                            let command = parse_window_command(&focus, &snapshot.windows)?
                                .ok_or_else(|| {
                                    "resolved app focus did not produce a window command".to_owned()
                                })?;
                            execute_window_command(backend, command)?;
                            backend.roundtrip().map_err(|error| {
                                format!("toplevel focus roundtrip failed: {error}")
                            })?;
                            if !close_after_success {
                                refresh_window_snapshot(snapshot, backend);
                                refresh_builtin_menu(shell, snapshot)?;
                            }
                            return Ok(close_after_success);
                        }
                        AppActivation::Launch => {}
                    }
                }
            }
            match execute_live_action(
                action,
                policy.enable_power_actions,
                policy.enable_unsafe_suspend,
            ) {
                Ok(Some(result)) => {
                    eprintln!(
                        "nuraloumi-provider-result: executed={} dry_run={} message={}",
                        result.executed, result.dry_run, result.message
                    );
                    if close_after_success && result.executed {
                        return Ok(true);
                    }
                    if refresh_lazy_control_action(snapshot, action) {
                        refresh_builtin_menu(shell, snapshot)?;
                    } else {
                        refresh_probe_snapshot(snapshot, &ProbeSnapshot::live());
                        refresh_window_snapshot(snapshot, backend);
                        refresh_workspace_snapshot(snapshot, backend);
                        refresh_builtin_menu(shell, snapshot)?;
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    eprintln!("nuraloumi-provider-error: {error}");
                    if refresh_lazy_control_action(snapshot, action) {
                        refresh_builtin_menu(shell, snapshot)?;
                    } else {
                        refresh_probe_snapshot(snapshot, &ProbeSnapshot::live());
                        refresh_window_snapshot(snapshot, backend);
                        refresh_workspace_snapshot(snapshot, backend);
                        refresh_builtin_menu(shell, snapshot)?;
                    }
                }
            }
        }
    }

    Ok(matches!(report, ActionReport::SurfaceClosed))
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

fn start_window_thumbnail_probe(
    backend: &WaylandBackend,
) -> Option<std::thread::JoinHandle<Result<ToplevelThumbnailReport, String>>> {
    let mut windows = backend.toplevels();
    windows.sort_by_key(|window| !window.state.activated);
    let requests = windows
        .into_iter()
        .take(4)
        .map(|window| ToplevelThumbnailRequest {
            key: window.id.to_string(),
            title: window.title,
            app_id: window.app_id,
            protocol_identifier: window.protocol_identifier,
        })
        .collect::<Vec<_>>();
    (!requests.is_empty()).then(|| {
        std::thread::spawn(move || {
            capture_toplevel_thumbnails_with_timeout(
                &requests,
                std::time::Duration::from_millis(350),
            )
        })
    })
}

fn apply_window_thumbnail_report(
    snapshot: &mut FixtureSnapshot,
    result: Result<ToplevelThumbnailReport, String>,
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
        let preview = nuraloumi_render_cairo::Rect {
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
    // zwlr_foreign_toplevel_handle_v1.activate requires a wl_seat. The
    // protocol may be present while seat access is restricted, so advertise
    // focus only when both capabilities exist.
    capabilities.focus &= backend.seat;
    capabilities
}

fn refresh_builtin_menu(
    shell: &mut ShellState,
    snapshot: &FixtureSnapshot,
) -> Result<bool, String> {
    let Ok(family) = parse_family(&shell.menu.id) else {
        return Ok(false);
    };
    shell.refresh_family(family, snapshot)?;
    Ok(true)
}

fn execute_live_action(
    action: &MenuAction,
    enable_power_actions: bool,
    enable_unsafe_suspend: bool,
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
                .with_suspend_actions(enable_unsafe_suspend)
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

fn log_live_report(report: &ActionReport) -> Result<(), String> {
    let encoded = serde_json::to_string(report)
        .map_err(|error| format!("failed to serialize live action report: {error}"))?;
    eprintln!("nuraloumi-live-action: {encoded}");
    Ok(())
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
        renderer: None,
        renderer_switch_available: false,
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

fn refresh_application_snapshot(
    snapshot: &mut FixtureSnapshot,
    preferences: &nuraloumi_shell::LauncherPreferences,
) {
    match ApplicationProvider::system().snapshot() {
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

fn refresh_process_snapshot(snapshot: &mut FixtureSnapshot) {
    let mut provider = ProcessProvider::system();
    match provider.snapshot() {
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
            "--probe-toplevels" => parsed.probe_toplevels = true,
            "--probe-workspaces" => parsed.probe_workspaces = true,
            "--family" => parsed.family = Some(next_value(&mut args, "--family")?),
            "--fixture" => parsed.fixture = Some(next_value(&mut args, "--fixture")?.into()),
            "--providers" => parsed.providers = Some(next_value(&mut args, "--providers")?.into()),
            "--config" => parsed.config = Some(next_value(&mut args, "--config")?.into()),
            "--dump-keybindings" => parsed.dump_keybindings = true,
            "--input" => parsed.input = Some(next_value(&mut args, "--input")?),
            "--reduced-motion" => parsed.reduced_motion = true,
            "--enable-power-actions" => parsed.enable_power_actions = true,
            "--enable-unsafe-suspend" => parsed.enable_unsafe_suspend = true,
            other => return Err(format!("unknown argument {other:?}; use --help")),
        }
    }
    Ok(parsed)
}

fn next_value(args: &mut impl Iterator<Item = String>, option: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{option} requires a value"))
}

fn parse_input(step: &str) -> Result<ShellInput, String> {
    let input = match step {
        "up" => ShellInput::Semantic(SemanticInput::Up),
        "down" => ShellInput::Semantic(SemanticInput::Down),
        "left" => ShellInput::Semantic(SemanticInput::Left),
        "right" => ShellInput::Semantic(SemanticInput::Right),
        "enter" | "activate" => ShellInput::Semantic(SemanticInput::Activate),
        "esc" | "escape" | "back" => ShellInput::Semantic(SemanticInput::Back),
        "backspace" => ShellInput::Semantic(SemanticInput::Backspace),
        "search" => launcher_search_input(),
        "blur" => ShellInput::SearchFocus(false),
        other if other.starts_with("text:") => {
            ShellInput::Semantic(SemanticInput::Text(other["text:".len()..].to_owned()))
        }
        other => return Err(format!("unknown input step {other:?}")),
    };
    Ok(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_input_uses_canonical_launcher_search_semantics() {
        assert_eq!(parse_input("search").unwrap(), launcher_search_input());
    }

    #[test]
    fn live_probe_snapshot_preserves_scan_results() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/providers/basic");
        let snapshot = fixture_snapshot_from_probe(&ProbeSnapshot::fixture(root));
        assert!(snapshot.wifi_networks.len() >= 2);
        assert!(snapshot
            .wifi_networks
            .iter()
            .any(|network| network.connected && network.signal_percent == Some(82)));
    }

    #[test]
    fn window_activation_capability_requires_a_wayland_seat() {
        let toplevel = nuraloumi_wayland::ToplevelCapabilities {
            list: true,
            state: true,
            activate: true,
            fullscreen: true,
            close: true,
        };
        let without_seat = WaylandCapabilities {
            seat: false,
            toplevel,
            ..WaylandCapabilities::default()
        };
        let capabilities = window_control_capabilities(&without_seat);
        assert!(capabilities.list);
        assert!(!capabilities.focus);
        assert!(capabilities.fullscreen);
        assert!(capabilities.close);

        let with_seat = WaylandCapabilities {
            seat: true,
            toplevel,
            ..WaylandCapabilities::default()
        };
        assert!(window_control_capabilities(&with_seat).focus);
    }

    #[test]
    fn renderer_consumes_canonical_core_model_directly() {
        let snapshot = FixtureSnapshot::default();
        let model = build_family(MenuFamily::Display, &snapshot);
        let state = nuraloumi_core::MenuState::new(&model);
        let scene = CairoRenderer::default().build_core_scene(
            &model,
            &state,
            Viewport::new(480.0, 640.0, 1.0),
            &DARK_THEME,
            RenderOptions::default(),
        );
        assert_eq!(scene.menu_id, "display");
        assert!(scene
            .hits
            .iter()
            .any(|hit| hit.item_id == "display.fullscreen" && hit.enabled && hit.actionable));
    }
}
