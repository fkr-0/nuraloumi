//! Future Sway xdg_toplevel entry point.
//! Until lane 02/15 registers a real xdg-shell Wayland surface, this program
//! deliberately refuses to advertise a simulated graphical file manager.
#[path = "../file_manager_window.rs"]
mod file_manager_window;

fn main() -> std::process::ExitCode {
    let path = std::env::args_os()
        .skip(1)
        .find(|arg| arg != "--probe")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from))
        .unwrap_or_else(|| std::path::PathBuf::from("/"));
    // A headless probe is useful for host fixtures and fails closed on errors.
    if std::env::args_os().any(|arg| arg == "--probe") {
        let mut details = file_manager_window::Details::new(path);
        return match details.refresh() {
            Ok(()) => {
                println!(
                    "{} visible entries; truncated={}",
                    details.rows.len(),
                    details.has_more
                );
                std::process::ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("file manager probe failed: {error}");
                std::process::ExitCode::FAILURE
            }
        };
    }
    eprintln!("nuraloumi-file-manager: xdg_toplevel support has not been integrated; refusing layer-shell window fallback");
    std::process::ExitCode::FAILURE
}
