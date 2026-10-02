use nuraloumi_core::{DARK_THEME, LIGHT_THEME};
use nuraloumi_providers::{
    ActionProvider, ActionResult, AudioAction, AudioProvider, BacklightAction, BacklightProvider,
    BluetoothAction, BluetoothProvider, Health, NetworkAction, NetworkProvider, ProbeSnapshot,
    Provider, SessionAction, SessionProvider, SnapshotMeta, SystemCommandRunner,
};
use nuraloumi_render_cairo::{
    CairoRenderer, Color, HitRegion as RenderHitRegion, PaintNode, Point, Rect, RenderOptions,
    RowKind, Scene, ScrollWindow, TextStyle, Theme as RenderTheme, Viewport,
};
use nuraloumi_shell::{
    build_family, load_config, load_fixture_snapshot, panel_affordances, parse_family,
    BluetoothDeviceEntry, FixtureSnapshot, HitRegion as ShellHitRegion, MenuAction, MenuFamily,
    PanelAffordance, PanelController, PanelEdge as ShellPanelEdge,
    PlatformEvent as ShellPlatformEvent, ProviderValue, SemanticInput, ShellConfig, ShellState,
    Theme as ShellTheme, ValueState, WifiNetworkEntry,
};
use nuraloumi_wayland::{
    BackendError, Frame, Key as WaylandKey, MenuConfig as WaylandMenuConfig,
    PanelConfig as WaylandPanelConfig, PanelEdge as WaylandPanelEdge, PixelFormat,
    PlatformEvent as WaylandEvent, SurfaceId, WaylandBackend,
};
use serde::Serialize;
use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

const HELP: &str = r#"nuraloumi-panel — NuraLoumi top panel

USAGE:
    nuraloumi-panel [OPTIONS]

OPTIONS:
    --headless                 Run without a compositor (default)
    --live                     Open a native Wayland/Cairo wl_shm panel
    --providers <path>         Load deterministic JSON/TOML provider snapshot
    --config <path>            Load JSON/TOML shell geometry/theme config
    --open <family>            Open launcher|network|audio|system initially
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

    let snapshot = if let Some(path) = args.providers.as_ref() {
        load_fixture_snapshot(path)?
    } else if args.live {
        fixture_snapshot_from_probe(&ProbeSnapshot::live())
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
        );
    }

    run_headless(config, snapshot, args.open.as_deref())
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
        affordances: panel_affordances(&snapshot),
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
    let render_theme = RenderTheme::from(match config.theme {
        ShellTheme::Dark => &DARK_THEME,
        ShellTheme::Light => &LIGHT_THEME,
    });
    let mut panel = PanelController::default();
    let mut panel_geometry: Option<(u32, u32, i32)> = None;
    let mut panel_scene: Option<Scene> = None;
    let mut panel_pointer_press: Option<String> = None;
    let mut panel_touch_press: BTreeMap<i32, Option<String>> = BTreeMap::new();
    let mut active_menu: Option<LiveMenu> = None;

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
        backend
            .blocking_dispatch()
            .map_err(|error| format!("Wayland dispatch failed: {error}"))?;
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
                        panel_scene = Some(render_panel_and_present(
                            &mut backend,
                            panel_surface,
                            &renderer,
                            &render_theme,
                            &snapshot,
                            &config,
                            (width, height, scale),
                        )?);
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
                                    if let Some(family) = panel_family_for_id(&id) {
                                        replace_live_menu(
                                            &mut backend,
                                            &config,
                                            &snapshot,
                                            output.as_ref(),
                                            &mut panel,
                                            &mut active_menu,
                                            family,
                                        )?;
                                    }
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
                            if let Some(family) = panel_family_for_id(&item_id) {
                                replace_live_menu(
                                    &mut backend,
                                    &config,
                                    &snapshot,
                                    output.as_ref(),
                                    &mut panel,
                                    &mut active_menu,
                                    family,
                                )?;
                            }
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
                    }
                }

                if policy.execute_provider_actions {
                    if let nuraloumi_shell::ActionReport::Dispatched { action, .. } = &action_report
                    {
                        let is_navigation = matches!(
                            action,
                            MenuAction::Custom { kind, .. } if kind == "menu.open"
                        );
                        if !is_navigation {
                            match execute_live_action(action, policy.enable_power_actions) {
                                Ok(Some(result)) => {
                                    eprintln!(
                                        "nuraloumi-provider-result: executed={} dry_run={} message={}",
                                        result.executed, result.dry_run, result.message
                                    );
                                    snapshot = fixture_snapshot_from_probe(&ProbeSnapshot::live());
                                    menu.shell
                                        .refresh_menu(build_family(menu.family, &snapshot))?;
                                    panel_scene = None;
                                    redraw = true;
                                }
                                Ok(None) => {}
                                Err(error) => {
                                    eprintln!("nuraloumi-provider-error: {error}");
                                    snapshot = fixture_snapshot_from_probe(&ProbeSnapshot::live());
                                    menu.shell
                                        .refresh_menu(build_family(menu.family, &snapshot))?;
                                    panel_scene = None;
                                    redraw = true;
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
                menu.shell.refresh_menu(build_family(family, &snapshot))?;
                redraw = true;
            }

            if close {
                close_live_menu(&mut backend, &mut panel, &mut active_menu)?;
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

        if panel_scene.is_none() {
            if let Some(geometry) = panel_geometry {
                panel_scene = Some(render_panel_and_present(
                    &mut backend,
                    panel_surface,
                    &renderer,
                    &render_theme,
                    &snapshot,
                    &config,
                    geometry,
                )?);
            }
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
                .saturating_sub(config.panel_height.saturating_add(16))
                .clamp(240, 720)
        })
        .unwrap_or(640);

    let margin_top = if config.panel_edge == ShellPanelEdge::Top {
        config.panel_height as i32
    } else {
        0
    };
    let margin_left = if config.panel_edge == ShellPanelEdge::Left {
        config.panel_height as i32
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
    theme: &RenderTheme,
    snapshot: &FixtureSnapshot,
    config: &ShellConfig,
    geometry: (u32, u32, i32),
) -> Result<Scene, String> {
    let scene = build_panel_scene(snapshot, config, geometry, *theme);
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
    let affordances = panel_affordances(snapshot);
    let horizontal = matches!(
        config.panel_edge,
        ShellPanelEdge::Top | ShellPanelEdge::Bottom
    );
    let count = affordances.len().max(1) as f64;
    let segment_extent = if horizontal {
        panel_rect.width / count
    } else {
        panel_rect.height / count
    };

    let mut paint = vec![PaintNode::FillRect {
        rect: panel_rect,
        color: theme.raised,
    }];
    let mut hits = Vec::with_capacity(affordances.len());

    for (index, affordance) in affordances.iter().enumerate() {
        let index = index as f64;
        let rect = if horizontal {
            Rect::new(
                panel_rect.x + index * segment_extent,
                panel_rect.y,
                segment_extent,
                panel_rect.height,
            )
        } else {
            Rect::new(
                panel_rect.x,
                panel_rect.y + index * segment_extent,
                panel_rect.width,
                segment_extent,
            )
        };
        let marker_color = state_color(affordance.state, theme);
        let marker = if horizontal {
            Rect::new(rect.x + 8.0, rect.y + rect.height / 2.0 - 3.0, 6.0, 6.0)
        } else {
            Rect::new(rect.x + 6.0, rect.y + 8.0, 6.0, 6.0)
        };
        paint.push(PaintNode::RoundedRect {
            rect: marker,
            radius: 3.0,
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
            rect.y + rect.height / 2.0 + 5.0
        } else {
            rect.y + rect.height / 2.0 + 4.0
        };
        paint.push(PaintNode::Text {
            origin: Point::new(rect.x + 20.0, baseline),
            text: label,
            style: TextStyle {
                size: if horizontal { 13.0 } else { 12.0 },
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

fn panel_family_for_id(id: &str) -> Option<MenuFamily> {
    match id {
        "apps" => Some(MenuFamily::Launcher),
        "network" => Some(MenuFamily::Network),
        "audio" => Some(MenuFamily::Audio),
        "battery" | "clock" => Some(MenuFamily::System),
        _ => None,
    }
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
    }
}

fn execute_live_action(
    action: &MenuAction,
    enable_power_actions: bool,
) -> Result<Option<ActionResult>, String> {
    let result = match action {
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
    fn panel_family_mapping_is_stable() {
        assert_eq!(panel_family_for_id("apps"), Some(MenuFamily::Launcher));
        assert_eq!(panel_family_for_id("network"), Some(MenuFamily::Network));
        assert_eq!(panel_family_for_id("audio"), Some(MenuFamily::Audio));
        assert_eq!(panel_family_for_id("battery"), Some(MenuFamily::System));
        assert_eq!(panel_family_for_id("clock"), Some(MenuFamily::System));
    }

    #[test]
    fn panel_scene_has_one_hit_per_affordance() {
        let snapshot = FixtureSnapshot::default();
        let config = ShellConfig::default();
        let scene = build_panel_scene(
            &snapshot,
            &config,
            (1366, config.panel_height, 1),
            RenderTheme::dark(),
        );
        assert_eq!(scene.hits.len(), panel_affordances(&snapshot).len());
        assert!(scene.hits.iter().all(|hit| hit.actionable && hit.enabled));
        assert_eq!(scene.panel_rect.width, 1366.0);
        assert_eq!(scene.panel_rect.height, f64::from(config.panel_height));
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
