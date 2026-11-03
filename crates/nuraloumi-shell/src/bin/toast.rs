//! One-shot NuraLoumi notification toast (not a notification daemon).
//!
//! Caller supplies incoming NotificationEntry fields and whether the notification
//! center is open. The existing dunstctl history provider is not an arrivals
//! subscription: lane 08 must wire the authoritative notification event source.
//!
//! Example: nuraloumi-toast --summary "Sync complete" --body "12 files"
//!          --app "Files" --timeout-ms 4500
//! With --center-open, exit successfully *without* mapping a surface.

#[path = "../toast.rs"]
mod toast;

use nuraloumi_core::{MenuItem, MenuModel, MenuState, DARK_THEME, LIGHT_THEME};
use nuraloumi_providers::notifications::NotificationEntry;
use nuraloumi_render_cairo::{CairoRenderer, RenderOptions, Viewport};
use nuraloumi_shell::{load_config_or_default, render_menu_with_glyphs, Theme};
use nuraloumi_wayland::{Frame, MenuConfig, PixelFormat, PlatformEvent, SurfaceId, WaylandBackend};
use std::env;
use std::os::raw::{c_int, c_short, c_ulong};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;
use toast::{Toast, DEFAULT_TIMEOUT_MS};

const MAX_ARG_BYTES: usize = 4096;
const WIDTH: u32 = 420;
const HEIGHT: u32 = 184;

#[derive(Default)]
struct Options {
    id: u32,
    app: String,
    summary: String,
    body: String,
    timeout_ms: Option<u64>,
    center_open: bool,
    fixture: bool,
    dry_run: bool,
    config: Option<PathBuf>,
    help: bool,
}

fn next_value<I: Iterator<Item = String>>(args: &mut I, flag: &str) -> Result<String, String> {
    let value = args
        .next()
        .ok_or_else(|| format!("{flag} requires a value"))?;
    if value.len() > MAX_ARG_BYTES || value.contains('\0') {
        return Err(format!("{flag} value exceeds safe input limits"));
    }
    Ok(value)
}

fn options(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
    let mut parsed = Options::default();
    let mut args = args.into_iter();
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--id" => {
                parsed.id = next_value(&mut args, &flag)?
                    .parse()
                    .map_err(|_| "invalid --id")?
            }
            "--app" => parsed.app = next_value(&mut args, &flag)?,
            "--summary" => parsed.summary = next_value(&mut args, &flag)?,
            "--body" => parsed.body = next_value(&mut args, &flag)?,
            "--timeout-ms" => {
                parsed.timeout_ms = Some(
                    next_value(&mut args, &flag)?
                        .parse()
                        .map_err(|_| "invalid --timeout-ms")?,
                )
            }
            "--config" => parsed.config = Some(next_value(&mut args, &flag)?.into()),
            "--center-open" => parsed.center_open = true,
            "--fixture" => parsed.fixture = true,
            "--dry-run" => parsed.dry_run = true,
            "--help" | "-h" => parsed.help = true,
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    if !parsed.help && !parsed.center_open && !parsed.fixture && parsed.summary.is_empty() {
        return Err("--summary is required unless --fixture or --center-open is set".into());
    }
    Ok(parsed)
}

fn notification(options: &Options) -> NotificationEntry {
    NotificationEntry {
        id: options.id,
        app: if options.fixture && options.app.is_empty() {
            "NuraLoumi".to_owned()
        } else {
            options.app.clone()
        },
        summary: if options.fixture && options.summary.is_empty() {
            "One-shot demo toast".to_owned()
        } else {
            options.summary.clone()
        },
        body: if options.fixture && options.body.is_empty() {
            "Touch or press Esc to dismiss.".to_owned()
        } else {
            options.body.clone()
        },
        actions: Vec::new(),
        default_action: None,
    }
}

fn model(toast: &Toast) -> MenuModel {
    let notification = &toast.notification;
    let app = if notification.app.is_empty() {
        "NuraLoumi"
    } else {
        notification.app.as_str()
    };
    let mut items = vec![MenuItem::status("summary", &notification.summary)];
    if !notification.body.is_empty() {
        items.push(MenuItem::status("body", &notification.body));
    }
    items.push(MenuItem::status(
        "dismiss-hint",
        "Tap or press Esc to dismiss",
    ));
    MenuModel::new("toast", app, items)
}

fn present(
    backend: &mut WaylandBackend,
    surface: SurfaceId,
    toast: &Toast,
    geometry: (u32, u32, i32),
    theme: Theme,
) -> Result<(), String> {
    let model = model(toast);
    let state = MenuState::new(&model);
    let (width, height, scale) = geometry;
    let viewport = Viewport::new(f64::from(width), f64::from(height), f64::from(scale.max(1)));
    let tokens = if theme == Theme::Dark {
        &DARK_THEME
    } else {
        &LIGHT_THEME
    };
    let (_, mut buffer) = render_menu_with_glyphs(
        &CairoRenderer::default(),
        &model,
        &state,
        viewport,
        tokens,
        RenderOptions::default(),
        None,
        None,
    )
    .map_err(|e| format!("Cairo render: {e}"))?;
    let info = buffer.info();
    let width = u32::try_from(info.width).map_err(|e| e.to_string())?;
    let height = u32::try_from(info.height).map_err(|e| e.to_string())?;
    let stride = usize::try_from(info.stride).map_err(|e| e.to_string())?;
    buffer
        .with_argb32_bytes(|pixels, _| {
            backend.present(
                surface,
                Frame {
                    width,
                    height,
                    stride,
                    pixels,
                    format: PixelFormat::Argb8888,
                },
            )
        })
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("wl_shm present: {e}"))
}

// Linux poll(2): one kernel wait with the remaining monotonic deadline.
// No thread, busy-polling timer, repaint loop, or second notification daemon.
#[repr(C)]
struct PollFd {
    fd: c_int,
    events: c_short,
    revents: c_short,
}

unsafe extern "C" {
    fn poll(fds: *mut PollFd, nfds: c_ulong, timeout: c_int) -> c_int;
}

fn wait_for_wayland(backend: &WaylandBackend, remaining_ms: u64) -> Result<bool, String> {
    const POLLIN: c_short = 0x0001;
    const POLLERR: c_short = 0x0008;
    const POLLHUP: c_short = 0x0010;
    const POLLNVAL: c_short = 0x0020;

    let mut fd = PollFd {
        fd: backend.poll_fd(),
        events: POLLIN,
        revents: 0,
    };
    let timeout = c_int::try_from(remaining_ms).unwrap_or(c_int::MAX);
    // SAFETY: fd points to a valid stack pollfd for the duration of the call,
    // nfds is exactly one, and the WaylandBackend owns the live descriptor.
    let ready = unsafe { poll(&raw mut fd, 1, timeout) };
    if ready < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted {
            return Ok(false);
        }
        return Err(format!("Wayland poll: {error}"));
    }
    if ready == 0 {
        return Ok(false);
    }
    if fd.revents & (POLLERR | POLLHUP | POLLNVAL) != 0 {
        return Err("Wayland socket closed or invalid".into());
    }
    Ok(fd.revents & POLLIN != 0)
}

fn run(options: Options) -> Result<(), String> {
    let ttl = options.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);
    let Some(mut toast) = Toast::new(notification(&options), 0, ttl, options.center_open) else {
        return Ok(());
    };
    if options.dry_run {
        println!(
            "toast id={} summary={:?} timeout_ms={}",
            toast.notification.id,
            toast.notification.summary,
            ttl.clamp(toast::MIN_TIMEOUT_MS, toast::MAX_TIMEOUT_MS)
        );
        return Ok(());
    }

    let config = load_config_or_default(options.config.as_deref())?;
    let start = Instant::now();
    let mut backend = WaylandBackend::connect().map_err(|e| e.to_string())?;
    if !backend.capabilities().layer_shell || !backend.capabilities().argb8888 {
        return Err("layer-shell and ARGB8888 wl_shm are required".into());
    }
    let surface = backend
        .create_menu(MenuConfig {
            width: WIDTH,
            height: HEIGHT,
            margin_top: i32::try_from(config.panel_height).unwrap_or(i32::MAX),
            margin_left: 0,
            output: backend.outputs().first().map(|output| output.id),
            namespace: "nuraloumi-toast".into(),
        })
        .map_err(|e| e.to_string())?;

    let mut drawn = false;
    let mut extent = (WIDTH, HEIGHT);
    loop {
        let elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
        let Some(remaining) = toast.remaining_ms(elapsed_ms) else {
            break;
        };

        // Handles already buffered events without blocking. If none, wait
        // until the Wayland socket is readable or the exact expiry deadline.
        backend.dispatch_pending().map_err(|e| e.to_string())?;
        let events = backend.drain_events().collect::<Vec<_>>();
        if events.is_empty() {
            if wait_for_wayland(&backend, remaining)? {
                backend.blocking_dispatch().map_err(|e| e.to_string())?;
            }
            continue;
        }

        for event in events {
            if event.surface != Some(surface) {
                continue;
            }
            let now_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            match event.event {
                PlatformEvent::Configure {
                    width,
                    height,
                    scale,
                } => {
                    extent = (width, height);
                    if toast.remaining_ms(now_ms).is_none() {
                        break;
                    }
                    // Draw exactly on configure (including a compositor resize),
                    // never on an idle tick or expiry timer.
                    present(
                        &mut backend,
                        surface,
                        &toast,
                        (width, height, scale),
                        config.theme,
                    )?;
                    drawn = true;
                }
                other => {
                    if toast.handle(other, now_ms, extent) {
                        break;
                    }
                }
            }
        }
        if toast
            .remaining_ms(u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX))
            .is_none()
        {
            break;
        }
        if !drawn {
            // A configure will arrive before a wl_shm buffer may be submitted.
            continue;
        }
    }
    backend
        .destroy_surface(surface)
        .map_err(|e| e.to_string())?;
    backend.flush().map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    let args = match options(env::args().skip(1)) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("nuraloumi-toast: {error}");
            return ExitCode::from(2);
        }
    };
    if args.help {
        println!(
            "nuraloumi-toast --summary TEXT [--app TEXT] [--body TEXT] [--id N] \
            [--timeout-ms 4500] [--center-open] [--fixture] [--dry-run] [--config PATH]"
        );
        return ExitCode::SUCCESS;
    }
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("nuraloumi-toast: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_shot_cli_rejects_unbounded_or_missing_inputs() {
        assert!(options(["--summary".into(), "Ready".into()]).is_ok());
        assert!(options(Vec::new()).is_err());
        assert!(options(["--summary".into()]).is_err());
        assert!(options(["--summary".into(), "x".repeat(4097)]).is_err());
        assert!(options(["--timeout-ms".into(), "-5".into(), "--fixture".into()]).is_err());
    }

    #[test]
    fn center_open_does_not_require_summary_or_wayland() {
        let args = options(["--center-open".into()]).unwrap();
        assert!(args.summary.is_empty());
        assert!(run(args).is_ok());
    }

    #[test]
    fn fixture_and_model_are_bounded_and_no_actions_are_exposed() {
        let args = options(["--fixture".into(), "--dry-run".into()]).unwrap();
        let toast = Toast::new(notification(&args), 0, DEFAULT_TIMEOUT_MS, false).unwrap();
        let menu = model(&toast);
        assert_eq!(menu.id, "toast");
        assert_eq!(menu.items.len(), 3);
        assert!(menu.items.iter().all(|item| item.action.is_none()));
    }
}
