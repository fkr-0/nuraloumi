//! nuraloumi-viewer [--live] PATH: software Cairo/wl_shm image viewer with file(1) fallback.
//! --fixture PATH renders the same actual scene headlessly; no desktop mutation.
#[path = "../viewer_diagnostics.rs"]
mod viewer_diagnostics;
#[path = "../viewer_image.rs"]
mod viewer_image;
#[path = "../viewer_scene.rs"]
mod viewer_scene;
use nuraloumi_wayland::{
    BackendError, Frame, Key, MenuConfig, PixelFormat, PlatformEvent, SurfaceId, WaylandBackend,
};
use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
    process::ExitCode,
};
use std::{
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
};
use viewer_scene::{Content, Scene};

struct Options {
    path: PathBuf,
    live: bool,
}

fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Option<Options>, String> {
    let mut path = None;
    let mut live = true;
    let mut end_of_options = false;
    for arg in args {
        if path.is_some() {
            return Err("expected only one PATH argument".into());
        }
        if end_of_options {
            path = Some(PathBuf::from(arg));
            continue;
        }
        match arg.to_str() {
            Some("-h" | "--help") => {
                println!("nuraloumi-viewer [--live] PATH\nnuraloumi-viewer --fixture PATH\nEsc/Backspace or touch Back/X closes. Supports bounded PNG/JPEG; other files show file(1).");
                return Ok(None);
            }
            Some("--live") => live = true,
            Some("--fixture") => live = false,
            Some("--") => end_of_options = true,
            _ => path = Some(PathBuf::from(arg)),
        }
    }
    Ok(Some(Options {
        path: path.ok_or("missing PATH")?,
        live,
    }))
}

fn load_scene(path: &Path) -> Scene {
    let name = path.file_name().unwrap_or(path.as_os_str());
    let name = viewer_diagnostics::sanitize(&name.to_string_lossy());
    let content = match viewer_image::load(path) {
        Ok(image) => Content::Image(image),
        Err(error) => Content::Diagnostic(viewer_diagnostics::describe(path, &error)),
    };
    Scene { name, content }
}

fn pending_scene(path: &Path) -> Scene {
    let name = path.file_name().unwrap_or(path.as_os_str());
    Scene {
        name: viewer_diagnostics::sanitize(&name.to_string_lossy()),
        content: Content::Diagnostic(
            "Opening file…\nPNG/JPEG decoding or file identification in progress.".into(),
        ),
    }
}

fn spawn_loader(job: impl FnOnce() -> Scene + Send + 'static) -> Receiver<Scene> {
    // A single bounded handoff. Never mutate Wayland or Cairo from the worker.
    let (tx, rx) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let result = job();
        let _ = tx.send(result); // Receiver is dropped if the user closes early.
    });
    rx
}

/// Multiplex compositor events while a single file job is pending. Once the
/// worker replies, return to blocking Wayland dispatch without periodic polls.
fn dispatch_while_loading(backend: &mut WaylandBackend) -> Result<(), String> {
    backend.flush().map_err(|e| e.to_string())?;
    let mut fd = libc::pollfd {
        fd: backend.poll_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let result = unsafe { libc::poll(&mut fd, 1, 40) };
    if result < 0 {
        let err = std::io::Error::last_os_error();
        if err.kind() != std::io::ErrorKind::Interrupted {
            return Err(format!("polling Wayland while loading: {err}"));
        }
    } else if result > 0 {
        if fd.revents & libc::POLLIN != 0 {
            backend.blocking_dispatch().map_err(|e| e.to_string())?;
        } else if fd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err("Wayland fd closed or failed during loading".into());
        }
    } else {
        backend.dispatch_pending().map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn draw(
    backend: &mut WaylandBackend,
    id: SurfaceId,
    scene: &Scene,
    geometry: (u32, u32, i32),
) -> Result<(), String> {
    let (pixels, stride, w, h) = viewer_scene::render(scene, geometry.0, geometry.1, geometry.2)?;
    match backend.present(
        id,
        Frame {
            width: w,
            height: h,
            stride,
            format: PixelFormat::Argb8888,
            pixels: &pixels,
        },
    ) {
        Ok(()) | Err(BackendError::WouldBlock) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}
fn close(backend: &mut WaylandBackend, id: SurfaceId) {
    let _ = backend.destroy_surface(id);
}

// Linux BTN_LEFT (0x110) only; other mouse buttons must never close the
// viewer. Match both press and release to the same primary-button gesture.
fn pointer_close(held: &mut bool, button: u32, pressed: bool, inside: bool) -> bool {
    if button != 0x110 {
        return false;
    }
    if pressed {
        *held = inside;
        false
    } else {
        let activate = *held && inside;
        *held = false;
        activate
    }
}
fn run_live(path: &Path) -> Result<(), String> {
    let mut backend = WaylandBackend::connect().map_err(|e| e.to_string())?;
    if !backend.capabilities().layer_shell {
        return Err("Wayland layer shell unavailable".into());
    }
    // connect() has already performed registry roundtrips. Do not block
    // waiting for unrelated events before creating a surface: the compositor
    // sends our first Configure only after create_menu() commits.
    let output = backend.outputs().into_iter().next();
    let (width, height) = output
        .as_ref()
        .and_then(|o| o.logical_size())
        .map(|(w, h)| {
            (
                w.saturating_sub(24).clamp(160, 800),
                h.saturating_sub(64).clamp(160, 600),
            )
        })
        .unwrap_or((800, 600));
    let surface = backend
        .create_menu(MenuConfig {
            width,
            height,
            margin_top: 40,
            margin_left: 0,
            output: output.as_ref().map(|o| o.id),
            namespace: "nuraloumi-viewer".into(),
        })
        .map_err(|e| e.to_string())?;
    // Create and configure the UI before any potentially slow read/decode.
    let mut scene = pending_scene(path);
    let worker_path = path.to_path_buf();
    let replies = spawn_loader(move || load_scene(&worker_path));
    let mut busy = true;
    let mut geometry = None::<(u32, u32, i32)>;
    let mut pointer_pressed = false;
    let mut touch_close_id = None::<i32>;
    loop {
        if busy {
            dispatch_while_loading(&mut backend)?;
        } else {
            backend.blocking_dispatch().map_err(|e| e.to_string())?;
        }
        let mut redraw = false;
        if busy {
            match replies.try_recv() {
                Ok(loaded) => {
                    scene = loaded;
                    busy = false;
                    redraw = true;
                }
                Err(TryRecvError::Disconnected) => {
                    scene.content =
                        Content::Diagnostic("File loading worker terminated unexpectedly".into());
                    busy = false;
                    redraw = true;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        for incoming in backend.drain_events().collect::<Vec<_>>() {
            if incoming.surface != Some(surface) {
                continue;
            }
            match incoming.event {
                PlatformEvent::Configure {
                    width,
                    height,
                    scale,
                } => {
                    geometry = Some((width, height, scale));
                    redraw = true;
                }
                PlatformEvent::Close
                | PlatformEvent::Key {
                    key: Key::Escape | Key::Backspace | Key::Left,
                    pressed: true,
                } => {
                    close(&mut backend, surface);
                    return Ok(());
                }
                PlatformEvent::PointerButton {
                    x,
                    y,
                    pressed,
                    button,
                } => {
                    let inside =
                        geometry.is_some_and(|(w, h, _)| viewer_scene::close_hit(x, y, w, h));
                    if pointer_close(&mut pointer_pressed, button, pressed, inside) {
                        close(&mut backend, surface);
                        return Ok(());
                    }
                }
                PlatformEvent::TouchDown { id, x, y } if touch_close_id.is_none() => {
                    if geometry.is_some_and(|(w, h, _)| viewer_scene::close_hit(x, y, w, h)) {
                        touch_close_id = Some(id);
                    }
                }
                PlatformEvent::TouchMotion { id, x, y } if touch_close_id == Some(id) => {
                    if !geometry.is_some_and(|(w, h, _)| viewer_scene::close_hit(x, y, w, h)) {
                        touch_close_id = None;
                    }
                }
                PlatformEvent::TouchUp { id } if touch_close_id == Some(id) => {
                    close(&mut backend, surface);
                    return Ok(());
                }
                PlatformEvent::TouchCancel { ids }
                    if ids.iter().any(|id| Some(*id) == touch_close_id) =>
                {
                    touch_close_id = None;
                }
                _ => {}
            }
        }
        if redraw {
            if let Some(size) = geometry {
                draw(&mut backend, surface, &scene, size)?;
            }
        }
    }
}
fn run(options: Options) -> Result<(), String> {
    if options.live {
        run_live(&options.path)
    } else {
        let scene = load_scene(&options.path);
        let (frame, _, width, height) = viewer_scene::render(&scene, 800, 480, 1)?;
        let nonblank = frame
            .chunks_exact(4)
            .any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0);
        if !nonblank {
            return Err("empty fixture scene".into());
        }
        match &scene.content {
            Content::Image(img) => println!(
                "image {} {}x{} scene={}x{} pixels={}",
                img.format,
                img.width,
                img.height,
                width,
                height,
                frame.len() / 4
            ),
            Content::Diagnostic(info) => println!(
                "diagnostic scene={}x{} pixels={}\n{}",
                width,
                height,
                frame.len() / 4,
                info
            ),
        }
        Ok(())
    }
}
fn main() -> ExitCode {
    match parse(env::args_os().skip(1)).and_then(|opt| opt.map(run).transpose()) {
        Ok(_) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("nuraloumi-viewer: {err}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn loading_scene_is_visible_before_slow_worker_finishes() {
        // Control the worker with channels rather than CPU-speed-sensitive
        // sleeps: the ARMv7/no-NEON test must also be deterministic.
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let replies = spawn_loader(move || {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Scene {
                name: "done.png".into(),
                content: Content::Diagnostic("Ready".into()),
            }
        });
        started_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let pending = pending_scene(Path::new("slow-file.jpg"));
        let (frame, _, _, _) = viewer_scene::render(&pending, 320, 240, 1).unwrap();
        assert!(!frame.is_empty());
        assert!(matches!(replies.try_recv(), Err(TryRecvError::Empty)));
        release_tx.send(()).unwrap();
        let finished = replies.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(finished.name, "done.png");
    }
    #[test]
    fn close_is_only_primary_button_release_inside() {
        let mut held = false;
        assert!(!pointer_close(&mut held, 0x111, true, true));
        assert!(!pointer_close(&mut held, 0x111, false, true));
        assert!(!held);
        assert!(!pointer_close(&mut held, 0x110, true, true));
        assert!(!pointer_close(&mut held, 0x110, false, false));
        assert!(!held);
        assert!(!pointer_close(&mut held, 0x110, true, false));
        assert!(!pointer_close(&mut held, 0x110, false, true));
        assert!(!pointer_close(&mut held, 0x110, true, true));
        assert!(pointer_close(&mut held, 0x110, false, true));
        assert!(!held);
    }
    #[test]
    fn exact_argv_and_leading_hyphen() {
        let opts = parse([OsString::from("--live"), OsString::from("-unsafe.jpeg")])
            .unwrap()
            .unwrap();
        assert_eq!(opts.path, PathBuf::from("-unsafe.jpeg"));
        assert!(opts.live);
        assert!(parse([OsString::from("--live")]).is_err());
        let literal = parse([OsString::from("--"), OsString::from("--fixture")])
            .unwrap()
            .unwrap();
        assert_eq!(literal.path, PathBuf::from("--fixture"));
        assert!(literal.live);
    }
    #[test]
    fn content_sniffing_ignores_extensions_and_uses_a_real_path() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("nuraloumi-viewer-{nonce}.txt"));
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, 1, 1);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[9, 29, 249]).unwrap();
        }
        std::fs::write(&path, &png).unwrap();
        let good = load_scene(&path);
        assert!(matches!(good.content, Content::Image(_)));
        std::fs::write(&path, b"%PDF-1.7\nnot a renderable PNG").unwrap();
        let fallback = load_scene(&path);
        assert!(matches!(fallback.content, Content::Diagnostic(_)));
        std::fs::remove_file(&path).unwrap();
    }
    #[test]
    fn unknown_file_is_real_diagnostic_scene() {
        let path = std::env::temp_dir().join(format!(
            "nuraloumi-viewer-test-{}.missing",
            std::process::id()
        ));
        let scene = load_scene(&path);
        assert!(matches!(scene.content, Content::Diagnostic(_)));
        let (pixels, _, _, _) = viewer_scene::render(&scene, 800, 480, 1).unwrap();
        assert!(pixels.iter().any(|b| *b != 0));
    }
}
