use std::{
    env, fs,
    path::PathBuf,
    process::{Command, ExitCode, Stdio},
};
fn config(path: Option<PathBuf>) -> toml::Value {
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
fn argv(input: &str) -> Vec<String> {
    // Commands from configuration are whitespace-delimited argv, never shell executed.
    input.split_whitespace().map(str::to_owned).collect()
}
fn terminal_editor(editor: &[String]) -> bool {
    let bin = editor
        .first()
        .and_then(|s| std::path::Path::new(s).file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("");
    matches!(
        bin,
        "nvim" | "vim" | "vi" | "nano" | "hx" | "helix" | "micro"
    ) || bin == "emacs"
        && editor
            .iter()
            .any(|s| s == "-nw" || s == "--no-window-system")
}
fn run() -> Result<(), String> {
    let mut config_path = None;
    let mut file = None;
    let mut fixture = false;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                config_path = Some(PathBuf::from(args.next().ok_or("missing --config value")?))
            }
            "--file" => file = Some(args.next().ok_or("missing --file value")?),
            "--fixture" => fixture = true,
            "-h" | "--help" => {
                println!("nuraloumi-editor [--config PATH] [--file PATH] [--fixture]");
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }
    let cfg = config(config_path);
    let name = cfg
        .get("providers")
        .and_then(|p| p.get("editor"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .or_else(|| env::var("EDITOR").ok())
        .unwrap_or_else(|| "nvim".into());
    let mut editor = argv(&name);
    if editor.is_empty() {
        return Err("empty editor command".into());
    }
    if let Some(file) = file {
        editor.push(file);
    }
    let graphical = cfg
        .get("surface")
        .and_then(|s| s.get("editor"))
        .and_then(|s| s.get("graphical"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(!terminal_editor(&editor));
    let mut command = if graphical {
        editor
    } else {
        let terminal = cfg
            .get("providers")
            .and_then(|p| p.get("terminal"))
            .and_then(toml::Value::as_str)
            .unwrap_or("foot");
        let mut terminal = argv(terminal);
        if terminal.is_empty() {
            return Err("empty terminal command".into());
        }
        let base = std::path::Path::new(&terminal[0])
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let launch_args = cfg
            .get("surface")
            .and_then(|s| s.get("terminal"))
            .and_then(|s| s.get("launch_args"))
            .and_then(toml::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(toml::Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| {
                vec![
                    if matches!(base, "foot" | "xterm" | "kitty" | "alacritty") {
                        "-e"
                    } else {
                        "--"
                    }
                    .into(),
                ]
            });
        terminal.extend(launch_args);
        terminal.extend(editor);
        terminal
    };
    let program = command.remove(0);
    if fixture {
        println!("{program} {}", command.join(" "));
        return Ok(());
    }
    Command::new(&program)
        .args(&command)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not start editor '{program}': {e}"))
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("nuraloumi-editor: {e}");
            ExitCode::FAILURE
        }
    }
}
#[cfg(test)]
mod tests {
    use super::terminal_editor;
    #[test]
    fn terminal_heuristic() {
        assert!(terminal_editor(&["nvim".into()]));
        assert!(terminal_editor(&[
            "emacs".into(),
            "-Q".into(),
            "-nw".into()
        ]));
        assert!(!terminal_editor(&["emacs".into()]));
    }
}
