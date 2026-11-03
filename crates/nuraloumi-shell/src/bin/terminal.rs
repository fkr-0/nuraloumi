use std::{
    env, fs,
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Command, ExitCode, Stdio},
};

fn configuration(path: Option<PathBuf>) -> toml::Value {
    let path = path.or_else(|| {
        env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|p| p.join("nuraloumi/config.toml"))
    });
    path.and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(toml::Value::Table(Default::default()))
}

fn words(command: &str) -> Result<Vec<String>, String> {
    // Configuration is an argv string, never evaluated by a shell.
    let mut args = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;
    for ch in command.chars() {
        if escaped {
            word.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' if quote != Some('\'') => escaped = true,
            '\'' | '"' if quote == Some(ch) => quote = None,
            '\'' | '"' if quote.is_none() => quote = Some(ch),
            c if c.is_whitespace() && quote.is_none() => {
                if !word.is_empty() {
                    args.push(std::mem::take(&mut word));
                }
            }
            c => word.push(c),
        }
    }
    if escaped || quote.is_some() {
        return Err("unterminated command quoting".into());
    }
    if !word.is_empty() {
        args.push(word);
    }
    if args.is_empty() {
        return Err("empty terminal command".into());
    }
    Ok(args)
}

fn launch(program: &str, args: &[String]) -> Result<(), String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Double fork: avoid a zombie owned by this short-lived launcher.
    unsafe {
        command.pre_exec(|| {
            if libc_setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Terminal '{program}' not found or failed: {e}"))
}

unsafe extern "C" {
    fn setsid() -> i32;
}
fn libc_setsid() -> i32 {
    unsafe { setsid() }
}

fn show_error(message: &str, seconds: u64) {
    use nuraloumi_core::{MenuItem, MenuModel, MenuState, DARK_THEME};
    use nuraloumi_render_cairo::{CairoRenderer, RenderOptions, Viewport};
    use nuraloumi_shell::render_menu_with_glyphs;
    use nuraloumi_wayland::{
        BackendError, Frame, MenuConfig, PixelFormat, PlatformEvent, WaylandBackend,
    };
    let Ok(mut backend) = WaylandBackend::connect() else {
        return;
    };
    if !backend.capabilities().layer_shell {
        return;
    }
    if backend.blocking_dispatch().is_err() {
        return;
    }
    let Ok(surface) = backend.create_menu(MenuConfig {
        width: 480,
        height: 90,
        margin_top: 40,
        margin_left: 0,
        output: backend.outputs().first().map(|o| o.id),
        namespace: "nuraloumi-terminal-error".into(),
    }) else {
        return;
    };
    if backend.blocking_dispatch().is_err() {
        return;
    }
    let model = MenuModel::new(
        "terminal-error",
        "Terminal error",
        vec![MenuItem::status("error", message.to_owned())],
    );
    let state = MenuState::new(&model);
    let renderer = CairoRenderer::default();
    for event in backend.drain_events().collect::<Vec<_>>() {
        if let PlatformEvent::Configure {
            width,
            height,
            scale,
        } = event.event
        {
            if event.surface != Some(surface) {
                continue;
            }
            let viewport =
                Viewport::new(f64::from(width), f64::from(height), f64::from(scale.max(1)));
            if let Ok((_, mut buffer)) = render_menu_with_glyphs(
                &renderer,
                &model,
                &state,
                viewport,
                &DARK_THEME,
                RenderOptions::default(),
                None,
                None,
            ) {
                let info = buffer.info();
                if let (Ok(w), Ok(h), Ok(stride)) = (
                    u32::try_from(info.width),
                    u32::try_from(info.height),
                    usize::try_from(info.stride),
                ) {
                    let _ = buffer
                        .with_argb32_bytes(|pixels, _| {
                            backend.present(
                                surface,
                                Frame {
                                    width: w,
                                    height: h,
                                    stride,
                                    format: PixelFormat::Argb8888,
                                    pixels,
                                },
                            )
                        })
                        .map(|r| match r {
                            Ok(()) | Err(BackendError::WouldBlock) => (),
                            Err(_) => (),
                        });
                }
            }
            break;
        }
    }
    std::thread::sleep(std::time::Duration::from_secs(seconds));
    let _ = backend.destroy_surface(surface);
}

fn run() -> Result<(), String> {
    let mut config = None;
    let mut cmd = None;
    let mut font_size = None;
    let mut fixture = false;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                config = Some(PathBuf::from(args.next().ok_or("missing --config value")?))
            }
            "--command" => cmd = Some(args.next().ok_or("missing --command value")?),
            "--font-size" => {
                font_size = Some(
                    args.next()
                        .ok_or("missing --font-size value")?
                        .parse::<u16>()
                        .map_err(|e| e.to_string())?,
                )
            }
            "--fixture" => fixture = true,
            "-h" | "--help" => {
                println!("nuraloumi-terminal [--config PATH] [--command CMD] [--font-size N] [--fixture]");
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }
    let cfg = configuration(config);
    let terminal = cfg
        .get("providers")
        .and_then(|p| p.get("terminal"))
        .and_then(toml::Value::as_str)
        .unwrap_or("foot");
    let mut argv = words(terminal)?;
    let program = argv.remove(0);
    let base = std::path::Path::new(&program)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if base == "foot" {
        let size = font_size
            .or_else(|| {
                cfg.get("surface")
                    .and_then(|s| s.get("terminal"))
                    .and_then(|t| t.get("font_size"))
                    .and_then(toml::Value::as_integer)
                    .and_then(|n| u16::try_from(n).ok())
            })
            .unwrap_or(11);
        argv.push("--font".into());
        argv.push(format!("monospace:size={size}"));
    }
    if let Some(cmd) = cmd {
        let mut parts = words(&cmd)?;
        let launch_args: Vec<String> = cfg
            .get("surface")
            .and_then(|s| s.get("terminal"))
            .and_then(|s| s.get("launch_args"))
            .and_then(toml::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(toml::Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_else(|| {
                vec![
                    if matches!(base, "foot" | "alacritty" | "kitty" | "xterm") {
                        "-e"
                    } else {
                        "--"
                    }
                    .into(),
                ]
            });
        argv.extend(launch_args);
        argv.append(&mut parts);
    }
    if fixture {
        println!("{program} {}", argv.join(" "));
        return Ok(());
    }
    match launch(&program, &argv) {
        Ok(()) => Ok(()),
        Err(message) => {
            show_error(&message, 3);
            Err(message)
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("nuraloumi-terminal: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::words;
    #[test]
    fn parses_quoted_command() {
        assert_eq!(
            words("foot -e 'echo hello'").unwrap(),
            ["foot", "-e", "echo hello"]
        );
        assert!(words("foot 'broken").is_err());
    }
}
