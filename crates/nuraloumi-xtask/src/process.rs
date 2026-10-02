use std::path::Path;
use std::process::{Command, ExitStatus};

#[derive(Debug)]
pub struct RunOutput {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

pub fn run_capture(program: &str, args: &[&str], cwd: &Path) -> Result<RunOutput, String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|error| format!("failed to start {program}: {error}"))?;

    Ok(RunOutput {
        status: output.status,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

pub fn run_checked(program: &str, args: &[&str], cwd: &Path) -> Result<RunOutput, String> {
    let output = run_capture(program, args, cwd)?;
    if output.status.success() {
        return Ok(output);
    }

    Err(format!(
        "{program} {} failed with status {}\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        output.status,
        output.stdout.trim_end(),
        output.stderr.trim_end()
    ))
}

pub fn command_available(program: &str, cwd: &Path) -> bool {
    Command::new(program)
        .arg("--version")
        .current_dir(cwd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}
