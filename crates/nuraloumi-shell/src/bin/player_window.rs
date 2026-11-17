//! Full player entrypoint staging seam. Cargo registration + xdg backend:
//! lane 15 / lane 02. Never substitute an oversized layer-shell popup.
#[path = "../player_window.rs"]
mod player_window;

use player_window::{require_managed_window, WindowBackend};

fn main() -> std::process::ExitCode {
    if std::env::args().any(|arg| arg == "--help") {
        println!("nuraloumi-player-window [--help] [--probe]");
        return std::process::ExitCode::SUCCESS;
    }
    // nuraloumi-wayland currently implements only layer-shell roles.
    // Fail before connecting to or mutating the shared playback daemon.
    if let Err(err) = require_managed_window(WindowBackend::LayerShellOnly) {
        eprintln!("nuraloumi-player-window: {err}");
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}
