//! Owned, opt-in Sway display idle timer. Never suspends the machine.
//! The panel alone owns the child; no daemon, system service or config edits.
use nuraloumi_providers::command::{
    CommandLimits, CommandRunner, CommandSpec, SystemCommandRunner,
};
use std::{
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};

const OFF: &str = "swaymsg \"output * power off\"";
const ON: &str = "swaymsg \"output * power on\"";
const TIMEOUT_SECONDS: &str = "600";

pub fn swayidle_arguments() -> [&'static str; 6] {
    ["-w", "timeout", TIMEOUT_SECONDS, OFF, "resume", ON]
}

fn restore_display() {
    // One bounded recovery if swayidle unexpectedly skipped its resume hook.
    let mut runner = SystemCommandRunner;
    let result = runner.run(
        &CommandSpec::new("swaymsg").args(["output", "*", "power", "on"]),
        CommandLimits::new(Duration::from_secs(2), 1024),
    );
    if !matches!(result, Ok(output) if output.status == 0) {
        eprintln!("nuraloumi-idle: bounded display-on recovery unavailable");
    }
}

/// Temporary 10-minute screen blanking, switched on only by explicit user
/// action. A missing swayidle binary is an error, not a pretend state update.
#[derive(Default)]
pub struct DisplayIdleController {
    child: Option<Child>,
}

impl DisplayIdleController {
    pub fn always_on(&mut self) -> bool {
        if let Some(child) = self.child.as_mut() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    eprintln!(
                        "nuraloumi-idle: swayidle exited with {status}; mode reverted to always-on"
                    );
                    self.child.take();
                    restore_display();
                }
                Ok(None) => {}
                Err(error) => {
                    eprintln!("nuraloumi-idle: child status error: {error}");
                    self.stop();
                }
            }
        }
        self.child.is_none()
    }

    pub fn toggle(&mut self) -> Result<bool, String> {
        if self.always_on() {
            let mut child = Command::new("swayidle")
                .args(swayidle_arguments())
                .spawn()
                .map_err(|error| format!("swayidle is unavailable: {error}"))?;
            match child.try_wait() {
                Ok(Some(status)) => return Err(format!("swayidle failed to start: {status}")),
                Err(error) => {
                    let _ = child.kill();
                    return Err(format!("cannot supervise swayidle: {error}"));
                }
                Ok(None) => {}
            }
            self.child = Some(child);
            Ok(false)
        } else {
            self.stop();
            Ok(true)
        }
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            // Swayidle documents SIGTERM as a graceful exit that runs pending
            // resume handlers. Killing it immediately could strand a dark panel.
            #[cfg(unix)]
            unsafe {
                libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
            }
            let deadline = Instant::now() + Duration::from_millis(800);
            while Instant::now() < deadline {
                if let Ok(Some(_)) = child.try_wait() {
                    return;
                }
                thread::sleep(Duration::from_millis(20));
            }
            let _ = child.kill();
            let _ = child.try_wait();
            restore_display();
        }
    }
}

impl Drop for DisplayIdleController {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_minute_timer_only_blanks_outputs_and_resumes() {
        let args = swayidle_arguments();
        assert_eq!(args[1], "timeout");
        assert_eq!(args[2], "600");
        assert!(args[3].contains("power off"));
        assert_eq!(args[4], "resume");
        assert!(args[5].contains("power on"));
        assert!(!args.join(" ").contains("suspend"));
        assert!(!args.join(" ").contains("poweroff"));
        let mut controller = DisplayIdleController::default();
        assert!(controller.always_on());
    }
}
