use nuraloumi_core::{DARK_THEME, LIGHT_THEME};
use nuraloumi_providers::{
    ActionProvider, ActionResult, AudioAction, AudioProvider, BacklightAction, BacklightProvider,
    BluetoothAction, BluetoothProvider, Health, NetworkAction, NetworkProvider, ProbeSnapshot,
    Provider, SessionAction, SessionProvider, SnapshotMeta, SystemCommandRunner,
};
use nuraloumi_render_cairo::{CairoRenderer, RenderOptions, Scene, Viewport};
use nuraloumi_shell::{
    build_family, load_config, load_fixture_snapshot, load_menu, parse_family, ActionReport,
    BluetoothDeviceEntry, FixtureSnapshot, HitRegion as ShellHitRegion, MenuAction, MenuFamily,
    PlatformEvent as ShellPlatformEvent, ProviderValue, SemanticInput, ShellConfig, ShellInput,
    ShellState, Theme as ShellTheme, ValueState, WifiNetworkEntry,
};
use nuraloumi_wayland::{
    BackendError, Frame, Key as WaylandKey, MenuConfig as WaylandMenuConfig, PixelFormat,
    PlatformEvent as WaylandEvent, SurfaceId, WaylandBackend,
};
use serde::Serialize;
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
    --family <name>            launcher|wifi|bluetooth|display|audio|power|tasks|windows|system
    --fixture <path>           Load a JSON/TOML MenuModel instead of a built-in family
    --providers <path>         Load deterministic JSON/TOML provider snapshot
    --config <path>            Load JSON/TOML shell geometry/theme config
    --input <steps>            Initial semantic input, e.g. down,enter,search,text:term
    --reduced-motion           Force reduced-motion state
    --enable-power-actions     Allow confirmed suspend/reboot/poweroff in live mode
    -h, --help                 Show this help

HEADLESS OUTPUT:
    JSON containing the menu, semantic state, action reports and hit regions.

LIVE MODE:
    Uses wl_shm + layer-shell + Cairo only; no EGL/XWayland. Without --providers,
    status values and ordinary controls use bounded provider adapters. Power actions
    remain dry-run unless --enable-power-actions is supplied after UI confirmation.
"#;

#[derive(Debug, Default)]
struct Args {
    live: bool,
    family: Option<String>,
    fixture: Option<PathBuf>,
    providers: Option<PathBuf>,
    config: Option<PathBuf>,
    input: Option<String>,
    reduced_motion: bool,
    enable_power_actions: bool,
}

#[derive(Clone, Copy)]
struct LivePolicy {
    execute_provider_actions: bool,
    enable_power_actions: bool,
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
    let family = parse_family(args.family.as_deref().unwrap_or("launcher"))?;
    let menu = if let Some(path) = args.fixture.as_ref() {
        load_menu(path)?
    } else {
        build_family(family, &snapshot)
    };
    let mut shell = ShellState::new(menu, config.reduced_motion)?;
    let mut reports = Vec::new();
    if let Some(input) = args.input.as_deref() {
        for step in input.split(',').filter(|step| !step.is_empty()) {
            reports.push(shell.apply_input(parse_input(step)?));
        }
    }

    if args.live {
        for report in &reports {
            log_live_report(report)?;
        }
        return run_live(
            shell,
            config,
            snapshot,
            LivePolicy {
                execute_provider_actions: args.providers.is_none(),
                enable_power_actions: args.enable_power_actions,
            },
        );
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

fn run_live(
    mut shell: ShellState,
    config: ShellConfig,
    mut snapshot: FixtureSnapshot,
    policy: LivePolicy,
) -> Result<(), String> {
    let mut backend =
        WaylandBackend::connect().map_err(|error| format!("Wayland connection failed: {error}"))?;
    let capabilities = backend.capabilities();
    if !capabilities.layer_shell {
        return Err("compositor does not advertise zwlr_layer_shell_v1".into());
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

    let surface = backend
        .create_menu(WaylandMenuConfig {
            width: requested_width,
            height: requested_height,
            margin_top: config.panel_height as i32,
            margin_left: 0,
            output: output.as_ref().map(|output| output.id),
            namespace: "nuraloumi-menu".into(),
        })
        .map_err(|error| format!("failed to create menu surface: {error}"))?;

    let renderer = CairoRenderer::default();
    let theme_tokens = match config.theme {
        ShellTheme::Dark => &DARK_THEME,
        ShellTheme::Light => &LIGHT_THEME,
    };
    let mut geometry: Option<(u32, u32, i32)> = None;
    let mut scene: Option<Scene> = None;
    let mut touch_regions: BTreeMap<i32, Option<ShellHitRegion>> = BTreeMap::new();

    loop {
        backend
            .blocking_dispatch()
            .map_err(|error| format!("Wayland dispatch failed: {error}"))?;
        let events: Vec<_> = backend.drain_events().collect();

        for event in events {
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
                    let region = scene
                        .as_ref()
                        .and_then(|scene| shell_hit_region(scene, x, y));
                    report = shell.handle_platform_event(ShellPlatformEvent::PointerButton {
                        region,
                        pressed,
                    });
                    redraw = true;
                }
                WaylandEvent::TouchDown { id, x, y } => {
                    let region = scene
                        .as_ref()
                        .and_then(|scene| shell_hit_region(scene, x, y));
                    touch_regions.insert(id, region.clone());
                    report =
                        shell.handle_platform_event(ShellPlatformEvent::TouchDown { id, region });
                    redraw = true;
                }
                WaylandEvent::TouchMotion { id, x, y } => {
                    let region = scene
                        .as_ref()
                        .and_then(|scene| shell_hit_region(scene, x, y));
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
                    if let Some(input) = semantic_from_wayland_key(key) {
                        report = Some(shell.apply_semantic(input));
                        redraw = true;
                    }
                }
                WaylandEvent::Close => {
                    backend
                        .destroy_surface(surface)
                        .map_err(|error| format!("failed to destroy menu surface: {error}"))?;
                    backend
                        .flush()
                        .map_err(|error| format!("failed to flush Wayland connection: {error}"))?;
                    return Ok(());
                }
                _ => {}
            }

            if let Some(report) = report {
                if handle_live_report(&report, &mut shell, &mut snapshot, policy)? {
                    backend
                        .destroy_surface(surface)
                        .map_err(|error| format!("failed to destroy menu surface: {error}"))?;
                    backend
                        .flush()
                        .map_err(|error| format!("failed to flush Wayland connection: {error}"))?;
                    return Ok(());
                }
                redraw = true;
            }

            if shell.closed {
                backend
                    .destroy_surface(surface)
                    .map_err(|error| format!("failed to destroy menu surface: {error}"))?;
                backend
                    .flush()
                    .map_err(|error| format!("failed to flush Wayland connection: {error}"))?;
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
                        geometry,
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
    geometry: (u32, u32, i32),
) -> Result<Scene, String> {
    let (logical_width, logical_height, scale) = geometry;
    let viewport = Viewport::new(
        f64::from(logical_width),
        f64::from(logical_height),
        f64::from(scale.max(1)),
    );
    let (scene, mut buffer) = renderer
        .render_core(
            &shell.menu,
            &shell.state,
            viewport,
            tokens,
            RenderOptions::default(),
        )
        .map_err(|error| format!("Cairo render failed: {error}"))?;

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

fn handle_live_report(
    report: &ActionReport,
    shell: &mut ShellState,
    snapshot: &mut FixtureSnapshot,
    policy: LivePolicy,
) -> Result<bool, String> {
    log_live_report(report)?;

    if let ActionReport::Dispatched {
        action: MenuAction::Custom { kind, payload },
        ..
    } = report
    {
        if kind == "menu.open" {
            let family = parse_family(payload)?;
            shell.refresh_menu(build_family(family, snapshot))?;
            return Ok(false);
        }
    }

    if policy.execute_provider_actions {
        if let ActionReport::Dispatched { action, .. } = report {
            match execute_live_action(action, policy.enable_power_actions) {
                Ok(Some(result)) => {
                    eprintln!(
                        "nuraloumi-provider-result: executed={} dry_run={} message={}",
                        result.executed, result.dry_run, result.message
                    );
                    *snapshot = fixture_snapshot_from_probe(&ProbeSnapshot::live());
                    if let Ok(family) = parse_family(&shell.menu.id) {
                        shell.refresh_menu(build_family(family, snapshot))?;
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    eprintln!("nuraloumi-provider-error: {error}");
                    *snapshot = fixture_snapshot_from_probe(&ProbeSnapshot::live());
                    if let Ok(family) = parse_family(&shell.menu.id) {
                        shell.refresh_menu(build_family(family, snapshot))?;
                    }
                }
            }
        }
    }

    Ok(matches!(report, ActionReport::SurfaceClosed))
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
            "--family" => parsed.family = Some(next_value(&mut args, "--family")?),
            "--fixture" => parsed.fixture = Some(next_value(&mut args, "--fixture")?.into()),
            "--providers" => parsed.providers = Some(next_value(&mut args, "--providers")?.into()),
            "--config" => parsed.config = Some(next_value(&mut args, "--config")?.into()),
            "--input" => parsed.input = Some(next_value(&mut args, "--input")?),
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

fn parse_input(step: &str) -> Result<ShellInput, String> {
    let input = match step {
        "up" => ShellInput::Semantic(SemanticInput::Up),
        "down" => ShellInput::Semantic(SemanticInput::Down),
        "left" => ShellInput::Semantic(SemanticInput::Left),
        "right" => ShellInput::Semantic(SemanticInput::Right),
        "enter" | "activate" => ShellInput::Semantic(SemanticInput::Activate),
        "esc" | "escape" | "back" => ShellInput::Semantic(SemanticInput::Back),
        "backspace" => ShellInput::Semantic(SemanticInput::Backspace),
        "search" => ShellInput::SearchFocus(true),
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

    #[test]
    fn wayland_key_mapping_keeps_semantic_navigation() {
        assert_eq!(
            semantic_from_wayland_key(WaylandKey::Down),
            Some(SemanticInput::Down)
        );
        assert_eq!(
            semantic_from_wayland_key(WaylandKey::Escape),
            Some(SemanticInput::Back)
        );
        assert_eq!(semantic_from_wayland_key(WaylandKey::Raw(30)), None);
    }
}
