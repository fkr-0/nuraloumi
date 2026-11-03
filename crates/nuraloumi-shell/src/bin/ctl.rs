//! Command-line client for NuraLoumi's line-delimited Unix socket protocol.
#[allow(dead_code)] // Server-facing API is compiled here until lib.rs integration.
#[path = "../ipc.rs"]
mod ipc;
use ipc::*;
use std::path::PathBuf;

fn help() {
    println!("nuraloumi-ctl [--json] [--socket PATH] <command> [options]\n\nCommands:\n  push-item --surface NAME --label TEXT [--glyph TEXT] [--subtitle TEXT] [--ttl-seconds N]\n  set-status --slot NAME --text TEXT [--glyph TEXT]\n  notify --summary TEXT [--body TEXT] [--app TEXT]\n  surface-state --surface NAME\n  navigate --action open|close|up|down|select|back\n  reload-config\n  list-surfaces\n  ping\n\nOptions:\n  --json          Print raw JSON response\n  --socket PATH   Override $XDG_RUNTIME_DIR/nuraloumi.sock (fallback /tmp/nuraloumi-$UID.sock)\n  --help          Show this help");
}
fn argument_error(message: impl Into<String>) -> ! {
    eprintln!(
        "{}",
        serde_json::json!({"ok":false,"error":{"code":"invalid_arguments","message":message.into()}})
    );
    std::process::exit(1)
}
fn main() {
    let mut args = std::env::args().skip(1);
    let mut json = false;
    let mut socket: Option<PathBuf> = None;
    let mut command = None;
    let mut opts = std::collections::HashMap::<String, String>::new();
    while let Some(arg) = args.next() {
        if arg == "--help" || arg == "-h" {
            help();
            return;
        }
        if arg == "--json" {
            json = true;
            continue;
        }
        if arg == "--socket" {
            socket = Some(PathBuf::from(
                args.next()
                    .unwrap_or_else(|| argument_error("--socket requires PATH")),
            ));
            continue;
        }
        if arg.starts_with("--") {
            let key = arg.trim_start_matches("--").to_string();
            if opts.contains_key(&key) {
                argument_error(format!("duplicate option {arg}"));
            }
            opts.insert(
                key,
                args.next()
                    .unwrap_or_else(|| argument_error(format!("{arg} requires a value"))),
            );
        } else if command.replace(arg).is_some() {
            argument_error("only one command may be specified");
        }
    }
    let command = command.unwrap_or_else(|| {
        help();
        argument_error("missing command")
    });
    let required = |key: &str| {
        opts.get(key)
            .cloned()
            .unwrap_or_else(|| argument_error(format!("missing --{key}")))
    };
    let optional = |key: &str| opts.get(key).cloned();
    let action = match command.as_str() {
        "push-item" => IpcCommand::PushItem(PushItem {
            surface: required("surface"),
            label: required("label"),
            glyph: optional("glyph"),
            subtitle: optional("subtitle"),
            ttl_seconds: optional("ttl-seconds")
                .map(|s| {
                    s.parse()
                        .unwrap_or_else(|_| argument_error("invalid --ttl-seconds"))
                })
                .unwrap_or(DEFAULT_TTL_SECONDS),
        }),
        "set-status" => IpcCommand::SetStatus(SetStatus {
            slot: required("slot"),
            text: required("text"),
            glyph: optional("glyph"),
        }),
        "notify" => IpcCommand::Notify(Notify {
            summary: required("summary"),
            body: optional("body"),
            app: optional("app"),
        }),
        "surface-state" => IpcCommand::SurfaceState(SurfaceName {
            surface: required("surface"),
        }),
        "navigate" => IpcCommand::Navigate(Navigate {
            action: match required("action").as_str() {
                "open" => NavigationAction::Open,
                "close" => NavigationAction::Close,
                "up" => NavigationAction::Up,
                "down" => NavigationAction::Down,
                "select" => NavigationAction::Select,
                "back" => NavigationAction::Back,
                _ => argument_error("action must be open|close|up|down|select|back"),
            },
        }),
        "reload-config" => IpcCommand::ReloadConfig(Empty {}),
        "list-surfaces" => IpcCommand::ListSurfaces(Empty {}),
        "ping" => IpcCommand::Ping(Empty {}),
        _ => argument_error(format!("unknown command {command}")),
    };
    let allowed: &[&str] = match command.as_str() {
        "push-item" => &["surface", "label", "glyph", "subtitle", "ttl-seconds"],
        "set-status" => &["slot", "text", "glyph"],
        "notify" => &["summary", "body", "app"],
        "surface-state" => &["surface"],
        "navigate" => &["action"],
        _ => &[],
    };
    for key in opts.keys() {
        if !allowed.contains(&key.as_str()) {
            argument_error(format!("unknown option --{key}"));
        }
    }
    let req = IpcRequest {
        id: format!("ctl-{}", std::process::id()),
        command: action,
    };
    if let Err(e) = parse_request(&serde_json::to_vec(&req).unwrap()) {
        argument_error(e.message);
    }
    let path = socket.unwrap_or_else(default_socket_path);
    let response = match request(&path, &req) {
        Ok(response) => response,
        Err(e) => {
            let connection = matches!(
                e.kind(),
                std::io::ErrorKind::NotFound
                    | std::io::ErrorKind::ConnectionRefused
                    | std::io::ErrorKind::PermissionDenied
                    | std::io::ErrorKind::NotConnected
            );
            eprintln!(
                "{}",
                serde_json::json!({"ok":false,"error":{"code":if connection {"connection_failed"} else {"transport_error"},"message":e.to_string()}})
            );
            std::process::exit(if connection { 2 } else { 1 });
        }
    };
    if response.ok {
        if json {
            println!("{}", serde_json::to_string(&response).unwrap());
        } else {
            println!(
                "{}",
                serde_json::to_string_pretty(&response.result).unwrap()
            );
        }
    } else {
        eprintln!("{}", serde_json::to_string(&response).unwrap());
        std::process::exit(1);
    }
}
