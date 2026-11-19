//! Crash-safe engine lifecycle, independent of UI integration.
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
use std::{
    io,
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub tabs: Vec<String>,
    pub active: usize,
}
impl Session {
    fn validate(&self) -> io::Result<()> {
        if self.tabs.is_empty()
            || self.tabs.len() > 6
            || self.active >= self.tabs.len()
            || self.tabs.iter().any(|url| {
                url.len() > 4096
                    || url.chars().any(char::is_control)
                    || !(url == "about:blank"
                        || url.starts_with("https://")
                        || url.starts_with("http://"))
            })
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid tab session",
            ));
        }
        Ok(())
    }
    pub fn load(path: &Path) -> io::Result<Self> {
        let bytes = fs::read(path)?;
        if bytes.len() > 25_000 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "session too large",
            ));
        }
        let session: Self = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        session.validate()?;
        Ok(session)
    }
    pub fn save(&self, path: &Path) -> io::Result<()> {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
        self.validate()?;
        let dir = path
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing directory"))?;
        let meta = fs::symlink_metadata(dir)?;
        if !meta.is_dir()
            || meta.file_type().is_symlink()
            || meta.uid() != unsafe { libc::geteuid() }
            || meta.permissions().mode() & 0o777 != 0o700
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "session directory not private",
            ));
        }
        let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        let result = (|| {
            serde_json::to_writer(&mut output, self).map_err(io::Error::other)?;
            output.sync_all()?;
            fs::rename(&tmp, path)?;
            fs::File::open(dir)?.sync_all()
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Stopped,
    Starting,
    Ready,
    Backoff,
    Failed(String),
}

pub struct Supervisor {
    binary: PathBuf,
    args: Vec<String>,
    child: Option<Child>,
    state: State,
    started: Instant,
    next_attempt: Instant,
    attempts: u32,
    max_restarts: u32,
    startup_timeout: Duration,
    stop_timeout: Duration,
    retry_delay: Duration,
}
impl Supervisor {
    pub fn new(binary: PathBuf, args: Vec<String>) -> Self {
        let now = Instant::now();
        Self {
            binary,
            args,
            child: None,
            state: State::Stopped,
            started: now,
            next_attempt: now,
            attempts: 0,
            max_restarts: 2,
            startup_timeout: Duration::from_secs(8),
            stop_timeout: Duration::from_secs(2),
            retry_delay: Duration::from_millis(500),
        }
    }
    pub fn configure(
        &mut self,
        max_restarts: u32,
        startup: Duration,
        stop: Duration,
        retry: Duration,
    ) {
        self.max_restarts = max_restarts;
        self.startup_timeout = startup;
        self.stop_timeout = stop;
        self.retry_delay = retry;
    }
    pub fn state(&self) -> &State {
        &self.state
    }
    pub fn start(&mut self) -> io::Result<()> {
        if self.child.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "engine running",
            ));
        }
        let mut cmd = Command::new(&self.binary);
        cmd.args(&self.args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        self.child = Some(cmd.spawn()?);
        self.started = Instant::now();
        self.state = State::Starting;
        Ok(())
    }
    pub fn poll(&mut self, ready: impl FnOnce() -> bool) {
        if matches!(self.state, State::Backoff) {
            if Instant::now() >= self.next_attempt {
                if let Err(e) = self.start() {
                    self.state = State::Failed(format!("spawn: {e}"));
                }
            }
            return;
        }
        let Some(child) = self.child.as_mut() else {
            return;
        };
        match child.try_wait() {
            Ok(Some(status)) => self.fail(format!("engine exited: {status}")),
            Err(e) => self.fail(format!("wait failed: {e}")),
            Ok(None) if matches!(self.state, State::Starting) && ready() => {
                self.state = State::Ready
            }
            Ok(None)
                if matches!(self.state, State::Starting)
                    && self.started.elapsed() >= self.startup_timeout =>
            {
                self.fail("startup readiness timeout".into())
            }
            Ok(None) => (),
        }
    }
    fn fail(&mut self, reason: String) {
        self.stop_child();
        if self.attempts < self.max_restarts {
            self.attempts += 1;
            self.next_attempt = Instant::now() + self.retry_delay;
            self.state = State::Backoff;
        } else {
            self.state = State::Failed(reason);
        }
    }
    fn stop_child(&mut self) {
        if let Some(mut child) = self.child.take() {
            let group = -(child.id() as i32);
            unsafe {
                libc::kill(group, libc::SIGTERM);
            }
            let deadline = Instant::now() + self.stop_timeout;
            while Instant::now() < deadline {
                if child.try_wait().ok().flatten().is_some() {
                    // The leader may exit before WebProcess helpers: sweep
                    // its dedicated group even after leader reaping.
                    unsafe {
                        libc::kill(group, libc::SIGKILL);
                    }
                    return;
                }
                thread::sleep(Duration::from_millis(10));
            }
            unsafe {
                libc::kill(group, libc::SIGKILL);
            }
            let _ = child.wait();
        }
    }
    pub fn stop(&mut self) {
        self.stop_child();
        self.state = State::Stopped;
    }
}
impl Drop for Supervisor {
    fn drop(&mut self) {
        self.stop();
    }
}
