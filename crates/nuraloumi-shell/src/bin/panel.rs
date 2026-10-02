use nuraloumi_core::{DARK_THEME, LIGHT_THEME};
use nuraloumi_providers::{
    ActionProvider, ActionResult, ApplicationAction, ApplicationProvider, ApplicationSnapshot,
    AudioAction, AudioProvider, BacklightAction, BacklightProvider, BluetoothAction,
    BluetoothProvider, Health, NetworkAction, NetworkProvider, ProbeSnapshot, Provider,
    SessionAction, SessionProvider, SnapshotMeta, SystemCommandRunner,
};
use nuraloumi_render_cairo::{
    CairoRenderer, Color, HitRegion as RenderHitRegion, PaintNode, Point, Rect, RenderOptions,
    RowKind, Scene, ScrollWindow, TextStyle, Theme as RenderTheme, Viewport,
};
use nuraloumi_shell::{
    build_family, execute_window_command, launcher_search_input, load_config,
    load_fixture_snapshot, panel_affordances, parse_desktop_command, parse_family,
    parse_window_command, window_entries, ActionReport, ApplicationEntry as ShellApplicationEntry,
    BluetoothDeviceEntry, ControlCenterTab, DesktopCommand, DesktopControlCapabilities,
    DesktopEntry, FixtureSnapshot, HitRegion as ShellHitRegion, MenuAction, MenuFamily,
    OverviewMode, PanelAffordance, PanelController, PanelEdge as ShellPanelEdge,
    PlatformEvent as ShellPlatformEvent, ProviderValue, SemanticInput, ShellConfig, ShellState,
    Theme as ShellTheme, ValueState, WifiNetworkEntry, WindowControlCapabilities,
};
use nuraloumi_wayland::{
    BackendCapabilities as WaylandCapabilities, BackendError, Frame, Key as WaylandKey,
    MenuConfig as WaylandMenuConfig, PanelConfig as WaylandPanelConfig,
    PanelEdge as WaylandPanelEdge, PixelFormat, PlatformEvent as WaylandEvent, SurfaceId,
    WaylandBackend, WorkspaceId,
};
use serde::Serialize;
use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

const PANEL_MENU_GAP: u32 = 6;

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
}

#[derive(Clone, Copy)]
struct LivePolicy {
    execute_provider_actions: bool,
    enable_power_actions: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PanelTarget {
    family: MenuFamily,
    focus_search: bool,
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
    let mut config = if let Some(path) = args.config.as_ref() {
        load_config(path)?
    } else {
        ShellConfig::default()
    };
    if args.reduced_motion {
        config.reduced_motion = true;
    }
    config.validate()?;

    let defer_live_provider_refresh = should_defer_live_provider_refresh(&args);
    let snapshot = if let Some(path) = args.providers.as_ref() {
        load_fixture_snapshot(path)?
    } else {
        FixtureSnapshot::default()
    };

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
            (probe, applications)
        })
    });

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
        if panel_first_frame_presented && live_provider_probe.is_some() {
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
            }
            if workspace_changed {
                refresh_workspace_snapshot(&mut snapshot, &backend);
            }
            if let Some(menu) = active_menu.as_mut() {
                menu.shell.refresh_family(menu.family, &snapshot)?;
                if let Some(menu_geometry) = menu.geometry {
                    menu.scene = Some(render_menu_and_present(
                        &mut backend,
                        menu.surface,
                        &renderer,
                        &menu.shell,
                        &config,
                        menu_geometry,
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
                                        &snapshot,
                                        output.as_ref(),
                                        &mut panel,
                                        &mut active_menu,
                                        &id,
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
                                &snapshot,
                                output.as_ref(),
                                &mut panel,
                                &mut active_menu,
                                &item_id,
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
                    let region = menu
                        .scene
                        .as_ref()
                        .and_then(|scene| shell_hit_region(scene, x, y));
                    report = menu
                        .shell
                        .handle_platform_event(ShellPlatformEvent::PointerButton {
                            region,
                            pressed,
                        });
                    redraw = true;
                }
                WaylandEvent::TouchDown { id, x, y } => {
                    let region = menu
                        .scene
                        .as_ref()
                        .and_then(|scene| shell_hit_region(scene, x, y));
                    menu.touch_regions.insert(id, region.clone());
                    report = menu
                        .shell
                        .handle_platform_event(ShellPlatformEvent::TouchDown { id, region });
                    redraw = true;
                }
                WaylandEvent::TouchMotion { id, x, y } => {
                    let region = menu
                        .scene
                        .as_ref()
                        .and_then(|scene| shell_hit_region(scene, x, y));
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
                        panel_scene = None;
                        redraw = true;
                    } else if kind == "control.tab" {
                        menu.shell
                            .set_control_center_tab(ControlCenterTab::parse(payload)?, &snapshot)?;
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
                                execute_window_command(&mut backend, command)?;
                                redraw = true;
                            } else {
                                let app_launch = matches!(
                                    action,
                                    MenuAction::Custom { kind, .. } if kind == "app.launch"
                                );
                                match execute_live_action(action, policy.enable_power_actions) {
                                    Ok(Some(result)) => {
                                        eprintln!(
                                        "nuraloumi-provider-result: executed={} dry_run={} message={}",
                                        result.executed, result.dry_run, result.message
                                    );
                                        if !app_launch {
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
                                    Ok(None) => {}
                                    Err(error) => {
                                        eprintln!("nuraloumi-provider-error: {error}");
                                        if !app_launch {
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
                menu.shell.refresh_family(family, &snapshot)?;
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
                        &config,
                        geometry,
                    )?);
                }
            }
        }

        if panel_first_frame_presented
            && live_provider_probe
                .as_ref()
                .is_some_and(|probe| probe.is_finished())
        {
            let (probe, applications) = live_provider_probe
                .take()
                .expect("finished live provider probe must exist")
                .join()
                .map_err(|_| "initial live provider probe panicked".to_owned())?;
            refresh_probe_snapshot(&mut snapshot, &probe);
            apply_application_snapshot(&mut snapshot, applications);
            if policy.execute_provider_actions {
                refresh_window_snapshot(&mut snapshot, &backend);
                refresh_workspace_snapshot(&mut snapshot, &backend);
            }
            if let Some(menu) = active_menu.as_mut() {
                menu.shell.refresh_family(menu.family, &snapshot)?;
                if let Some(menu_geometry) = menu.geometry {
                    menu.scene = Some(render_menu_and_present(
                        &mut backend,
                        menu.surface,
                        &renderer,
                        &menu.shell,
                        &config,
                        menu_geometry,
                    )?);
                } else {
                    menu.scene = None;
                }
            }
            panel_scene = None;
            eprintln!("nuraloumi-panel-provider-refresh: initial live snapshot ready");
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
    })
}

fn activate_panel_target(
    backend: &mut WaylandBackend,
    config: &ShellConfig,
    snapshot: &FixtureSnapshot,
    output: Option<&nuraloumi_wayland::OutputInfo>,
    panel: &mut PanelController,
    active_menu: &mut Option<LiveMenu>,
    item_id: &str,
) -> Result<(), String> {
    let Some(target) = panel_target_for_id(item_id) else {
        return Ok(());
    };
    replace_live_menu(
        backend,
        config,
        snapshot,
        output,
        panel,
        active_menu,
        target.family,
    )?;
    if target.focus_search {
        if let Some(menu) = active_menu.as_mut() {
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
    config: &ShellConfig,
    geometry: (u32, u32, i32),
) -> Result<Scene, String> {
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
    let (scene, mut buffer) = renderer
        .render_core(
            &shell.menu,
            &shell.state,
            viewport,
            theme_tokens,
            RenderOptions::default(),
        )
        .map_err(|error| format!("Cairo menu render failed: {error}"))?;
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

    for (index, (affordance, rect)) in affordances.iter().zip(slot_rects.into_iter()).enumerate() {
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
    let (family, focus_search) = match id {
        "apps" => (MenuFamily::Launcher, false),
        "network" => (MenuFamily::Network, false),
        "search" => (MenuFamily::Launcher, true),
        "audio" => (MenuFamily::Audio, false),
        "battery" | "clock" => (MenuFamily::ControlCenter, false),
        _ => return None,
    };
    Some(PanelTarget {
        family,
        focus_search,
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
        applications: Vec::new(),
        desktops: Vec::new(),
        desktop_capabilities: DesktopControlCapabilities::unavailable(),
    }
}

fn refresh_probe_snapshot(snapshot: &mut FixtureSnapshot, probe: &ProbeSnapshot) {
    let applications = std::mem::take(&mut snapshot.applications);
    let windows = std::mem::take(&mut snapshot.windows);
    let desktops = std::mem::take(&mut snapshot.desktops);
    let desktop_capabilities = snapshot.desktop_capabilities;
    *snapshot = fixture_snapshot_from_probe(probe);
    snapshot.applications = applications;
    snapshot.windows = windows;
    snapshot.desktops = desktops;
    snapshot.desktop_capabilities = desktop_capabilities;
}

fn apply_application_snapshot(
    snapshot: &mut FixtureSnapshot,
    applications: Result<ApplicationSnapshot, nuraloumi_providers::ProviderError>,
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
        }
        Err(error) => {
            eprintln!("nuraloumi-application-provider-error: {error}");
            snapshot.applications.clear();
        }
    }
}

fn refresh_window_snapshot(snapshot: &mut FixtureSnapshot, backend: &WaylandBackend) {
    let capabilities = window_control_capabilities(&backend.capabilities());
    snapshot.windows = window_entries(&backend.toplevels(), capabilities);
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
            })
        );
        assert_eq!(
            panel_target_for_id("network"),
            Some(PanelTarget {
                family: MenuFamily::Network,
                focus_search: false,
            })
        );
        assert_eq!(
            panel_target_for_id("search"),
            Some(PanelTarget {
                family: MenuFamily::Launcher,
                focus_search: true,
            })
        );
        assert_eq!(
            panel_target_for_id("audio"),
            Some(PanelTarget {
                family: MenuFamily::Audio,
                focus_search: false,
            })
        );
        assert_eq!(
            panel_target_for_id("battery"),
            Some(PanelTarget {
                family: MenuFamily::ControlCenter,
                focus_search: false,
            })
        );
        assert_eq!(
            panel_target_for_id("clock"),
            Some(PanelTarget {
                family: MenuFamily::ControlCenter,
                focus_search: false,
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
