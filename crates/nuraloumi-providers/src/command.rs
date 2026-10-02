use crate::error::{ProviderError, ProviderErrorCategory};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
}

impl CommandSpec {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
        }
    }

    pub fn arg(mut self, value: impl Into<String>) -> Self {
        self.args.push(value.into());
        self
    }

    pub fn args<I, S>(mut self, values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(values.into_iter().map(Into::into));
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandLimits {
    pub timeout: Duration,
    pub max_output_bytes: usize,
}

impl CommandLimits {
    pub const fn new(timeout: Duration, max_output_bytes: usize) -> Self {
        Self {
            timeout,
            max_output_bytes,
        }
    }
}

impl Default for CommandLimits {
    fn default() -> Self {
        Self::new(Duration::from_secs(3), 64 * 1024)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

pub trait CommandRunner {
    fn run(
        &mut self,
        spec: &CommandSpec,
        limits: CommandLimits,
    ) -> Result<CommandOutput, ProviderError>;
}

impl<T> CommandRunner for Box<T>
where
    T: CommandRunner + ?Sized,
{
    fn run(
        &mut self,
        spec: &CommandSpec,
        limits: CommandLimits,
    ) -> Result<CommandOutput, ProviderError> {
        (**self).run(spec, limits)
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(
        &mut self,
        spec: &CommandSpec,
        limits: CommandLimits,
    ) -> Result<CommandOutput, ProviderError> {
        let mut child = Command::new(&spec.program)
            .args(&spec.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                let category = match error.kind() {
                    io::ErrorKind::NotFound => ProviderErrorCategory::Unavailable,
                    io::ErrorKind::PermissionDenied => ProviderErrorCategory::Permission,
                    _ => ProviderErrorCategory::Io,
                };
                ProviderError::new(
                    category,
                    format!("failed to start backend '{}': {error}", spec.program),
                )
            })?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ProviderError::backend("backend stdout capture unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| ProviderError::backend("backend stderr capture unavailable"))?;

        let cap = limits.max_output_bytes.saturating_add(1).max(1);
        let stdout_thread = thread::spawn(move || read_limited(stdout, cap));
        let stderr_thread = thread::spawn(move || read_limited(stderr, cap));

        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    if started.elapsed() >= limits.timeout {
                        let _ = child.kill();
                        let _ = child.wait();
                        let _ = stdout_thread.join();
                        let _ = stderr_thread.join();
                        return Err(ProviderError::timeout(format!(
                            "backend '{}' exceeded deadline",
                            spec.program
                        )));
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stdout_thread.join();
                    let _ = stderr_thread.join();
                    return Err(ProviderError::from_io("waiting for backend", &error));
                }
            }
        };

        let stdout = join_capture(stdout_thread)?;
        let stderr = join_capture(stderr_thread)?;
        if stdout.len().saturating_add(stderr.len()) > limits.max_output_bytes {
            return Err(ProviderError::backend(format!(
                "backend '{}' exceeded output limit",
                spec.program
            )));
        }

        Ok(CommandOutput {
            status: status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        })
    }
}

fn read_limited(mut reader: impl Read, cap: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(cap.min(8192));
    reader
        .by_ref()
        .take(u64::try_from(cap).unwrap_or(u64::MAX))
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn join_capture(handle: thread::JoinHandle<io::Result<Vec<u8>>>) -> Result<Vec<u8>, ProviderError> {
    handle
        .join()
        .map_err(|_| ProviderError::backend("backend capture thread failed"))?
        .map_err(|error| ProviderError::from_io("reading backend output", &error))
}

/// Exact-argv replacement for tests and fixture-mode probing.
///
/// A fixture directory contains numbered files such as 001.argv, 001.stdout,
/// 001.stderr, and 001.status. The first line of the argv file is the program
/// and each following line is one argument. Matching is exact; shell parsing
/// and interpolation never occur.
#[derive(Debug, Default, Clone)]
pub struct FixtureCommandRunner {
    entries: Vec<(CommandSpec, CommandOutput)>,
}

impl FixtureCommandRunner {
    pub fn insert(&mut self, spec: CommandSpec, output: CommandOutput) {
        self.entries.push((spec, output));
    }

    pub fn from_dir(directory: impl AsRef<Path>) -> Result<Self, ProviderError> {
        let directory = directory.as_ref();
        let mut argv_files = match fs::read_dir(directory) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|value| value == "argv"))
                .collect::<Vec<_>>(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(ProviderError::from_io("reading command fixtures", &error)),
        };
        argv_files.sort();

        let mut runner = Self::default();
        for argv_path in argv_files {
            let argv = fs::read_to_string(&argv_path)
                .map_err(|error| ProviderError::from_io("reading argv fixture", &error))?;
            let mut lines = argv.lines();
            let program = lines
                .next()
                .filter(|line| !line.is_empty())
                .ok_or_else(|| ProviderError::parse("fixture argv has no program"))?;
            let spec = CommandSpec {
                program: program.to_owned(),
                args: lines.map(ToOwned::to_owned).collect(),
            };

            let stdout = read_optional_sidecar(&argv_path, "stdout")?;
            let stderr = read_optional_sidecar(&argv_path, "stderr")?;
            let status_text = read_optional_sidecar(&argv_path, "status")?;
            let status = if status_text.trim().is_empty() {
                0
            } else {
                status_text
                    .trim()
                    .parse::<i32>()
                    .map_err(|_| ProviderError::parse("fixture status is not an integer"))?
            };

            runner.insert(
                spec,
                CommandOutput {
                    status,
                    stdout,
                    stderr,
                },
            );
        }
        Ok(runner)
    }
}

fn read_optional_sidecar(argv_path: &Path, extension: &str) -> Result<String, ProviderError> {
    let mut path = PathBuf::from(argv_path);
    path.set_extension(extension);
    match fs::read_to_string(path) {
        Ok(value) => Ok(value),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(ProviderError::from_io("reading command fixture", &error)),
    }
}

impl CommandRunner for FixtureCommandRunner {
    fn run(
        &mut self,
        spec: &CommandSpec,
        limits: CommandLimits,
    ) -> Result<CommandOutput, ProviderError> {
        let Some((_, output)) = self.entries.iter().find(|(candidate, _)| candidate == spec) else {
            return Err(ProviderError::unavailable(format!(
                "no fixture for backend '{}'",
                spec.program
            )));
        };
        if output.stdout.len().saturating_add(output.stderr.len()) > limits.max_output_bytes {
            return Err(ProviderError::backend(format!(
                "fixture backend '{}' exceeded output limit",
                spec.program
            )));
        }
        Ok(output.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_runner_requires_exact_argv() {
        let mut runner = FixtureCommandRunner::default();
        runner.insert(
            CommandSpec::new("tool").args(["literal", "a;touch /tmp/nope"]),
            CommandOutput {
                status: 0,
                stdout: "ok".to_owned(),
                stderr: String::new(),
            },
        );

        let result = runner
            .run(
                &CommandSpec::new("tool").args(["literal", "a;touch /tmp/nope"]),
                CommandLimits::default(),
            )
            .expect("exact argv should match");
        assert_eq!(result.stdout, "ok");

        let error = runner
            .run(
                &CommandSpec::new("tool").args(["literal", "different"]),
                CommandLimits::default(),
            )
            .expect_err("different argv must not match");
        assert_eq!(error.category, ProviderErrorCategory::Unavailable);
    }
}
