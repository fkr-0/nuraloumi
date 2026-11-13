//! Deterministic discovery of shell-owned surfaces.
//!
//! Only the panel and its embedded menu families are presumed available.
//! Separately packaged executables must actually exist beside the running
//! binary, while new 0.2 features require an explicit, reviewed registration.
//! This catalog never guesses that a separate process is running.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::Path;

use crate::ipc::SurfaceOpenState;

const EMBEDDED_MENUS: &[&str] = &[
    "launcher",
    "control-center",
    "network",
    "bluetooth",
    "display",
    "audio",
    "power",
    "tasks",
    "windows",
    "system",
];

// These are existing, separately launched shell binaries, not evidence of
// active processes. A missing binary is never advertised as installed.
const PACKAGED_SURFACES: &[(&str, &str)] = &[
    ("browser", "nuraloumi-browser"),
    ("calendar", "nuraloumi-calendar"),
    ("clipboard", "nuraloumi-clipboard"),
    ("editor", "nuraloumi-editor"),
    ("files", "nuraloumi-files"),
    ("music", "nuraloumi-music"),
    ("notifications", "nuraloumi-notifications"),
    ("resources", "nuraloumi-resources"),
    ("run", "nuraloumi-run"),
    ("terminal", "nuraloumi-terminal"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceKind {
    Panel,
    EmbeddedMenu,
    InstalledBinary,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceEntry {
    pub name: String,
    pub kind: SurfaceKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReleasedBinary<'a> {
    pub name: &'a str,
    pub executable: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceObservation {
    pub name: String,
    /// This panel observes panel/embedded menu state; separately launched
    /// binaries have an unknown state even when installed.
    pub open_state: SurfaceOpenState,
    /// Zero means this shell has no knowledge of a running process.
    pub pid: u32,
    pub selected_item: Option<String>,
    pub item_count: usize,
}

#[derive(Clone, Debug, Default)]
pub struct SurfaceCatalog {
    entries: BTreeMap<String, SurfaceKind>,
}

impl SurfaceCatalog {
    /// Discover surfaces without spawning programs or probing other processes.
    ///
    /// Additional (0.2+) capabilities must be supplied by the integration
    /// owner *after* their implementation and tests have been accepted.
    /// Supplying a feature name alone is not enough: its binary must exist.
    pub fn discover(binary_dir: &Path, released: &[ReleasedBinary<'_>]) -> Self {
        let mut entries = BTreeMap::new();
        entries.insert("panel".to_owned(), SurfaceKind::Panel);
        for name in EMBEDDED_MENUS {
            entries.insert((*name).to_owned(), SurfaceKind::EmbeddedMenu);
        }
        for &(name, executable) in PACKAGED_SURFACES {
            if is_executable(binary_dir, executable) {
                entries
                    .entry(name.to_owned())
                    .or_insert(SurfaceKind::InstalledBinary);
            }
        }
        for registration in released {
            if valid_identifier(registration.name)
                && is_executable(binary_dir, registration.executable)
            {
                entries
                    .entry(registration.name.to_owned())
                    .or_insert(SurfaceKind::InstalledBinary);
            }
        }
        Self { entries }
    }

    pub fn contains(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    /// Standalone binaries are discoverable but never observed by this panel.
    /// Do not infer closure (or a PID) from installation, process names or /proc.
    pub fn observe(
        &self,
        active_menu: Option<&str>,
        selected_item: Option<&str>,
        item_count: usize,
        panel_pid: u32,
    ) -> Vec<SurfaceObservation> {
        self.entries
            .iter()
            .map(|(name, kind)| {
                let open_state = match kind {
                    SurfaceKind::Panel => SurfaceOpenState::Open,
                    SurfaceKind::EmbeddedMenu if active_menu == Some(name.as_str()) => {
                        SurfaceOpenState::Open
                    }
                    SurfaceKind::EmbeddedMenu => SurfaceOpenState::Closed,
                    SurfaceKind::InstalledBinary => SurfaceOpenState::Unknown,
                };
                SurfaceObservation {
                    name: name.clone(),
                    open_state,
                    pid: match kind {
                        SurfaceKind::Panel => panel_pid,
                        SurfaceKind::EmbeddedMenu if open_state == SurfaceOpenState::Open => {
                            panel_pid
                        }
                        _ => 0,
                    },
                    selected_item: (*kind == SurfaceKind::EmbeddedMenu
                        && open_state == SurfaceOpenState::Open)
                        .then(|| selected_item.map(str::to_owned))
                        .flatten(),
                    item_count: if *kind == SurfaceKind::EmbeddedMenu
                        && open_state == SurfaceOpenState::Open
                    {
                        item_count
                    } else {
                        0
                    },
                }
            })
            .collect()
    }
}

fn valid_identifier(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn is_executable(binary_dir: &Path, name: &str) -> bool {
    if !valid_identifier(name) {
        return false;
    }
    // Do not follow symlinks: discovering an entry must not escape this
    // installation directory or pretend a launcher alias is a shipping binary.
    fs::symlink_metadata(binary_dir.join(name)).is_ok_and(|metadata| {
        metadata.file_type().is_file() && metadata.permissions().mode() & 0o111 != 0
    })
}

/// Read-only IPC companion for the panel's blocking Wayland dispatch loop.
///
/// The worker never touches Wayland, Cairo, providers, or any live menu state.
/// The panel publishes a tiny immutable observation after event processing.
/// Keeping the socket off the render thread allows it to respond even when
/// Wayland is idle and blocking_dispatch has no pending compositor events.
/// A validated IPC request handoff, consumed only by the Wayland owner.
/// A response is returned on the same IPC connection after the owner commits.
pub struct OwnerReloadRequest {
    pub id: String,
    issued: std::time::Instant,
    reply: std::sync::mpsc::Sender<crate::ipc::IpcResponse>,
}

impl OwnerReloadRequest {
    pub fn expired(&self) -> bool {
        self.issued.elapsed() >= std::time::Duration::from_millis(850)
    }

    pub fn respond(self, response: crate::ipc::IpcResponse) {
        let _ = self.reply.send(response);
    }
}

pub struct IpcRuntime {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    observations: std::sync::Arc<std::sync::Mutex<Vec<SurfaceObservation>>>,
    reload_requests: std::sync::mpsc::Receiver<OwnerReloadRequest>,
    wake_reader: UnixStream,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl IpcRuntime {
    pub fn start(
        socket: &Path,
        initial: Vec<SurfaceObservation>,
        toast_program: std::path::PathBuf,
    ) -> Result<Self, String> {
        use std::sync::{atomic::AtomicBool, Arc, Mutex};

        let mut server = crate::ipc::IpcServer::bind(socket)
            .map_err(|error| format!("cannot bind panel IPC {}: {error}", socket.display()))?;
        let stop = Arc::new(AtomicBool::new(false));
        let observations = Arc::new(Mutex::new(initial));
        // A readable pipe wakes the owner out of Wayland's idle wait without
        // requiring a periodic compositor roundtrip or mutating Wayland here.
        let (wake_reader, mut wake_writer) = UnixStream::pair()
            .map_err(|error| format!("cannot create IPC owner wakeup: {error}"))?;
        wake_reader
            .set_nonblocking(true)
            .map_err(|error| format!("cannot configure IPC owner wakeup: {error}"))?;
        wake_writer
            .set_nonblocking(true)
            .map_err(|error| format!("cannot configure IPC worker wakeup: {error}"))?;
        let (reload_sender, reload_requests) = std::sync::mpsc::channel();
        let stop_worker = Arc::clone(&stop);
        let observations_worker = Arc::clone(&observations);
        let worker = std::thread::Builder::new()
            .name("nuraloumi-ipc".to_owned())
            .spawn(move || {
                let started = std::time::Instant::now();
                let mut toast_child: Option<std::process::Child> = None;
                while !stop_worker.load(std::sync::atomic::Ordering::Relaxed) {
                    // Reap the one-shot child without ever waiting on the Wayland
                    // thread. Do not accumulate overlapping toast processes.
                    if toast_child
                        .as_mut()
                        .is_some_and(|child| child.try_wait().map_or(true, |exit| exit.is_some()))
                    {
                        toast_child.take();
                    }
                    if let Ok(Some(request)) = server.accept_one() {
                        let current = observations_worker
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .clone();
                        let response =
                            if matches!(request.command, crate::ipc::IpcCommand::ReloadConfig(_)) {
                                let (reply, outcome) = std::sync::mpsc::channel();
                                let handoff = OwnerReloadRequest {
                                    id: request.id.clone(),
                                    issued: std::time::Instant::now(),
                                    reply,
                                };
                                let queued = reload_sender.send(handoff).is_ok();
                                let signalled = queued
                                    && match wake_writer.write_all(&[1]) {
                                        Ok(()) => true,
                                        // Full nonblocking wake pipe is already readable.
                                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => true,
                                        Err(_) => false,
                                    };
                                if !signalled {
                                    crate::ipc::IpcResponse::failure(
                                        &request.id, "reload_unavailable",
                                        "Wayland owner is not available for reload",
                                    )
                                } else {
                                    outcome.recv_timeout(std::time::Duration::from_millis(850))
                                        .unwrap_or_else(|_| crate::ipc::IpcResponse::failure(
                                            &request.id, "reload_timeout",
                                            "Wayland owner did not complete reload within IPC budget",
                                        ))
                                }
                            } else if matches!(request.command, crate::ipc::IpcCommand::Notify(_)) {
                                let available = toast_program
                                    .parent()
                                    .is_some_and(|dir| is_executable(dir, "nuraloumi-toast"))
                                    && toast_program
                                        .file_name()
                                        .is_some_and(|name| name == "nuraloumi-toast");
                                let busy = toast_child.is_some();
                                notification_reply(
                                    &request,
                                    &current,
                                    available,
                                    busy,
                                    |notification| {
                                        use std::process::{Command, Stdio};
                                        let child = Command::new(&toast_program)
                                            .arg("--summary")
                                            .arg(&notification.summary)
                                            .arg("--body")
                                            .arg(notification.body.as_deref().unwrap_or(""))
                                            .arg("--app")
                                            .arg(notification.app.as_deref().unwrap_or("NuraLoumi"))
                                            .stdin(Stdio::null())
                                            .stdout(Stdio::null())
                                            .stderr(Stdio::null())
                                            .spawn()
                                            .map_err(|error| error.to_string())?;
                                        toast_child = Some(child);
                                        Ok(())
                                    },
                                )
                            } else {
                                ipc_reply(&request, &current, started.elapsed())
                            };
                        // A disconnected, non-reading, or malformed client cannot
                        // stall the compositor loop: this is a separate worker.
                        let _ = server.respond(&response);
                    }
                    // Existing bounded IPC worker cadence, not a notification
                    // daemon or a new notification-history polling loop.
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                if let Some(mut child) = toast_child {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                let _ = server.cleanup();
            })
            .map_err(|error| format!("cannot start panel IPC worker: {error}"))?;
        Ok(Self {
            stop,
            observations,
            reload_requests,
            wake_reader,
            worker: Some(worker),
        })
    }

    /// Pollable event descriptor for the Wayland-owner event wait.
    pub fn wake_fd(&self) -> RawFd {
        self.wake_reader.as_raw_fd()
    }

    pub fn clear_wakeup(&mut self) {
        let mut bytes = [0u8; 64];
        loop {
            match self.wake_reader.read(&mut bytes) {
                Ok(0) => break,
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
    }

    pub fn take_reload(&self) -> Option<OwnerReloadRequest> {
        self.reload_requests.try_recv().ok()
    }

    /// The UI thread only takes a short lock to replace a bounded snapshot.
    pub fn publish(&self, current: Vec<SurfaceObservation>) {
        *self
            .observations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = current;
    }
}

impl Drop for IpcRuntime {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Authoritative explicit incoming-notification request path. An installed
/// standalone center has UNKNOWN open state, so it cannot be treated as closed.
/// Only a known-closed center permits the one-shot child. The launcher callback
/// is injected to fixture-test all branches without running an external program.
fn notification_reply<F>(
    request: &crate::ipc::IpcRequest,
    surfaces: &[SurfaceObservation],
    executable_available: bool,
    busy: bool,
    launch: F,
) -> crate::ipc::IpcResponse
where
    F: FnOnce(&crate::ipc::Notify) -> Result<(), String>,
{
    use crate::ipc::{IpcCommand, IpcResponse};
    let IpcCommand::Notify(notification) = &request.command else {
        return IpcResponse::failure(&request.id, "invalid_request", "not a notify request");
    };
    if surfaces.iter().any(|entry| {
        (entry.name == "notifications" || entry.name == "control-center")
            && entry.open_state == SurfaceOpenState::Open
    }) {
        return IpcResponse::success(
            &request.id,
            serde_json::json!({
                "accepted": false, "reason": "notification_center_open"
            }),
        );
    }
    if surfaces
        .iter()
        .any(|entry| entry.name == "notifications" && entry.open_state == SurfaceOpenState::Unknown)
    {
        return IpcResponse::failure(
            &request.id,
            "center_state_unknown",
            "standalone notification center is installed but its open state is not observed",
        );
    }
    if !executable_available {
        return IpcResponse::failure(
            &request.id,
            "toast_unavailable",
            "one-shot toast executable is not installed",
        );
    }
    if busy {
        return IpcResponse::failure(
            &request.id,
            "toast_busy",
            "previous one-shot toast is still active",
        );
    }
    match launch(notification) {
        Ok(()) => IpcResponse::success(
            &request.id,
            serde_json::json!({
                "accepted": true, "presentation": "one_shot_started_unverified"
            }),
        ),
        Err(error) => IpcResponse::failure(&request.id, "toast_spawn_failed", error),
    }
}

fn ipc_reply(
    request: &crate::ipc::IpcRequest,
    surfaces: &[SurfaceObservation],
    uptime: std::time::Duration,
) -> crate::ipc::IpcResponse {
    use crate::ipc::{IpcCommand, IpcResponse, PingResult, SurfaceInfo, SurfaceState};
    let id = &request.id;
    match &request.command {
        IpcCommand::Ping(_) => IpcResponse::success(
            id,
            PingResult {
                version: env!("CARGO_PKG_VERSION").to_owned(),
                uptime_seconds: uptime.as_secs(),
            },
        ),
        IpcCommand::ListSurfaces(_) => IpcResponse::success(
            id,
            surfaces
                .iter()
                .map(|entry| SurfaceInfo {
                    name: entry.name.clone(),
                    open: entry.open_state == SurfaceOpenState::Open,
                    open_state: Some(entry.open_state),
                    observation_supported: Some(entry.open_state != SurfaceOpenState::Unknown),
                    pid: entry.pid,
                })
                .collect::<Vec<_>>(),
        ),
        IpcCommand::SurfaceState(query) => match surfaces
            .iter()
            .find(|surface| surface.name == query.surface)
        {
            Some(surface) => IpcResponse::success(
                id,
                SurfaceState {
                    open: surface.open_state == SurfaceOpenState::Open,
                    open_state: Some(surface.open_state),
                    observation_supported: Some(surface.open_state != SurfaceOpenState::Unknown),
                    selected_item: surface.selected_item.clone(),
                    item_count: surface.item_count,
                },
            ),
            None => IpcResponse::failure(
                id,
                "unknown_surface",
                "surface is not registered in this running panel",
            ),
        },
        IpcCommand::ReloadConfig(_) => IpcResponse::failure(
            id,
            "reload_unavailable",
            "reload-config must be handled by the Wayland owner",
        ),
        IpcCommand::Navigate(_) => IpcResponse::failure(
            id,
            "unsupported",
            "remote navigation is not wired to the Wayland event owner",
        ),
        IpcCommand::PushItem(_) | IpcCommand::SetStatus(_) | IpcCommand::Notify(_) => {
            IpcResponse::failure(
                id,
                "unsupported",
                "this command has no live visual consumer in the panel",
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct TestDir(std::path::PathBuf);

    impl TestDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "nuraloumi-catalog-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).expect("fresh per-test directory");
            Self(path)
        }

        fn executable(&self, name: &str, mode: u32) {
            let path = self.0.join(name);
            fs::write(&path, b"fixture").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).expect("remove only test-owned fixture directory");
        }
    }

    #[test]
    fn reload_ipc_handoff_waits_for_owner_acknowledgement() {
        use crate::ipc::{Empty, IpcCommand, IpcRequest};
        let dir = TestDir::new();
        let socket = dir.0.join("panel.sock");
        let mut runtime = IpcRuntime::start(&socket, Vec::new(), dir.0.join("not-installed-toast"))
            .expect("start isolated test IPC runtime");
        let client = std::thread::spawn(move || {
            crate::ipc::request(
                &socket,
                &IpcRequest {
                    id: "reload-1".into(),
                    command: IpcCommand::ReloadConfig(Empty {}),
                },
            )
        });

        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(650);
        let pending = loop {
            if let Some(pending) = runtime.take_reload() {
                break pending;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "no reload delivered to owner"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert_eq!(pending.id, "reload-1");
        assert!(!pending.expired());
        runtime.clear_wakeup();
        pending.respond(crate::ipc::IpcResponse::success(
            "reload-1",
            serde_json::json!({"applied": true}),
        ));
        let response = client.join().expect("client thread").expect("IPC response");
        assert!(response.ok);
        assert_eq!(response.result.unwrap()["applied"], true);
    }

    #[test]
    fn builtins_are_visible_without_external_services() {
        let dir = TestDir::new();
        let catalog = SurfaceCatalog::discover(&dir.0, &[]);
        assert!(catalog.contains("panel"));
        assert!(catalog.contains("launcher"));
        assert!(catalog.contains("system"));
        assert!(!catalog.contains("screenshot"));
        assert!(!catalog.contains("maps"));
        assert!(!catalog.contains("toast"));
        assert!(!catalog.contains("calendar"));
        let rows = catalog.observe(Some("launcher"), Some("apps.first"), 13, 456);
        let panel = rows.iter().find(|row| row.name == "panel").unwrap();
        let launcher = rows.iter().find(|row| row.name == "launcher").unwrap();
        assert_eq!((panel.open_state, panel.pid), (SurfaceOpenState::Open, 456));
        assert_eq!(launcher.selected_item.as_deref(), Some("apps.first"));
        assert_eq!(launcher.item_count, 13);
        assert_eq!(
            (launcher.open_state, launcher.pid),
            (SurfaceOpenState::Open, 456)
        );
        assert_eq!(
            rows.iter()
                .filter(|row| row.open_state == SurfaceOpenState::Open)
                .count(),
            2
        );
        assert_eq!(
            rows.iter()
                .find(|row| row.name == "system")
                .unwrap()
                .open_state,
            SurfaceOpenState::Closed
        );
    }

    #[test]
    fn native_browser_is_discoverable_only_when_installed_and_never_assumed_open() {
        let dir = TestDir::new();
        assert!(!SurfaceCatalog::discover(&dir.0, &[]).contains("browser"));
        dir.executable("nuraloumi-browser", 0o644);
        assert!(!SurfaceCatalog::discover(&dir.0, &[]).contains("browser"));
        fs::set_permissions(
            dir.0.join("nuraloumi-browser"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        let catalog = SurfaceCatalog::discover(&dir.0, &[]);
        assert!(catalog.contains("browser"));
        let observed = catalog
            .observe(None, None, 0, 423)
            .into_iter()
            .find(|row| row.name == "browser")
            .unwrap();
        assert_eq!(
            catalog.entries.get("browser"),
            Some(&SurfaceKind::InstalledBinary)
        );
        assert_eq!(observed.open_state, SurfaceOpenState::Unknown);
        assert_eq!(observed.pid, 0);
    }

    #[test]
    fn packaged_capability_requires_a_real_executable() {
        let dir = TestDir::new();
        dir.executable("nuraloumi-calendar", 0o644);
        assert!(!SurfaceCatalog::discover(&dir.0, &[]).contains("calendar"));
        fs::set_permissions(
            dir.0.join("nuraloumi-calendar"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        let catalog = SurfaceCatalog::discover(&dir.0, &[]);
        let item = catalog
            .observe(None, None, 0, 456)
            .into_iter()
            .find(|row| row.name == "calendar")
            .unwrap();
        assert_eq!(item.open_state, SurfaceOpenState::Unknown);
        assert_eq!(item.pid, 0);
    }

    #[test]
    fn new_capabilities_are_opt_in_even_if_binary_exists() {
        let dir = TestDir::new();
        dir.executable("nuraloumi-screenshot", 0o755);
        assert!(!SurfaceCatalog::discover(&dir.0, &[]).contains("screenshot"));
        let catalog = SurfaceCatalog::discover(
            &dir.0,
            &[ReleasedBinary {
                name: "screenshot",
                executable: "nuraloumi-screenshot",
            }],
        );
        assert!(catalog.contains("screenshot"));
        assert_eq!(
            catalog
                .observe(None, None, 0, 1)
                .iter()
                .find(|row| row.name == "screenshot")
                .unwrap()
                .open_state,
            SurfaceOpenState::Unknown
        );
    }

    #[test]
    fn invalid_names_and_overrides_cannot_escape_or_shadow_builtins() {
        let dir = TestDir::new();
        dir.executable("other", 0o755);
        let catalog = SurfaceCatalog::discover(
            &dir.0,
            &[
                ReleasedBinary {
                    name: "../fake",
                    executable: "other",
                },
                ReleasedBinary {
                    name: "unsafe",
                    executable: "../other",
                },
                ReleasedBinary {
                    name: "panel",
                    executable: "other",
                },
            ],
        );
        assert!(!catalog.contains("../fake"));
        assert!(!catalog.contains("unsafe"));
        let panel = catalog
            .observe(None, None, 0, 71)
            .into_iter()
            .find(|row| row.name == "panel")
            .unwrap();
        assert_eq!(panel.open_state, SurfaceOpenState::Open);
        assert_eq!(panel.pid, 71);
    }

    #[test]
    fn explicit_notify_never_guesses_standalone_center_closed() {
        use crate::ipc::{IpcCommand, IpcRequest, Notify};
        use std::cell::Cell;
        let dir = TestDir::new();
        let request = IpcRequest {
            id: "arrival-1".to_owned(),
            command: IpcCommand::Notify(Notify {
                summary: "Build passed".to_owned(),
                body: Some("Fixture".to_owned()),
                app: None,
            }),
        };
        let launched = Cell::new(0);
        let launch = |_: &crate::ipc::Notify| {
            launched.set(launched.get() + 1);
            Ok(())
        };
        let observations = SurfaceCatalog::discover(&dir.0, &[]).observe(None, None, 0, 51);
        let response = notification_reply(&request, &observations, true, false, launch);
        assert!(response.ok);
        assert_eq!(response.result.unwrap()["accepted"], true);
        assert_eq!(launched.get(), 1);

        let closed = |_: &crate::ipc::Notify| -> Result<(), String> { panic!("must not spawn") };
        let response = notification_reply(&request, &observations, false, false, closed);
        assert_eq!(response.error.unwrap().code, "toast_unavailable");
        let response = notification_reply(&request, &observations, true, true, closed);
        assert_eq!(response.error.unwrap().code, "toast_busy");
        let open =
            SurfaceCatalog::discover(&dir.0, &[]).observe(Some("control-center"), None, 0, 51);
        let response = notification_reply(&request, &open, true, false, closed);
        assert_eq!(
            response.result.unwrap()["reason"],
            "notification_center_open"
        );
        dir.executable("nuraloumi-notifications", 0o755);
        let unknown = SurfaceCatalog::discover(&dir.0, &[]).observe(None, None, 0, 51);
        let response = notification_reply(&request, &unknown, true, false, closed);
        assert_eq!(response.error.unwrap().code, "center_state_unknown");
        assert_eq!(launched.get(), 1);
    }

    #[test]
    fn explicit_notify_spawn_failure_is_not_reported_as_presented() {
        use crate::ipc::{IpcCommand, IpcRequest, Notify};
        let dir = TestDir::new();
        let request = IpcRequest {
            id: "fail".into(),
            command: IpcCommand::Notify(Notify {
                summary: "hi".into(),
                body: None,
                app: None,
            }),
        };
        let rows = SurfaceCatalog::discover(&dir.0, &[]).observe(None, None, 0, 9);
        let response = notification_reply(&request, &rows, true, false, |_| {
            Err("fixture spawn error".into())
        });
        assert_eq!(response.error.unwrap().code, "toast_spawn_failed");
    }

    #[test]
    fn ipc_queries_report_only_observed_state_and_reject_mutations() {
        use crate::ipc::{Empty, IpcCommand, IpcRequest, SurfaceName};
        let dir = TestDir::new();
        let catalog = SurfaceCatalog::discover(&dir.0, &[]);
        let state = catalog.observe(Some("launcher"), Some("apps.first"), 13, 412);
        let request = |command| IpcRequest {
            id: "test".into(),
            command,
        };
        let ping = ipc_reply(
            &request(IpcCommand::Ping(Empty {})),
            &state,
            std::time::Duration::from_secs(19),
        );
        assert!(ping.ok);
        assert_eq!(ping.result.unwrap()["uptime_seconds"], 19);

        let list = ipc_reply(
            &request(IpcCommand::ListSurfaces(Empty {})),
            &state,
            std::time::Duration::ZERO,
        );
        let rows = list.result.unwrap();
        let rows = rows.as_array().unwrap();
        assert_eq!(rows.iter().filter(|row| row["open"] == true).count(), 2);
        assert!(rows
            .iter()
            .any(|row| row["name"] == "panel" && row["pid"] == 412));
        assert!(rows.iter().any(|row| row["name"] == "panel"
            && row["open_state"] == "open"
            && row["observation_supported"] == true));
        assert!(rows.iter().any(|row| row["name"] == "system"
            && row["open_state"] == "closed"
            && row["observation_supported"] == true));

        let current = ipc_reply(
            &request(IpcCommand::SurfaceState(SurfaceName {
                surface: "launcher".into(),
            })),
            &state,
            std::time::Duration::ZERO,
        );
        assert_eq!(current.result.unwrap()["selected_item"], "apps.first");
        let unknown = ipc_reply(
            &request(IpcCommand::SurfaceState(SurfaceName {
                surface: "toast".into(),
            })),
            &state,
            std::time::Duration::ZERO,
        );
        assert_eq!(unknown.error.unwrap().code, "unknown_surface");

        let reload = ipc_reply(
            &request(IpcCommand::ReloadConfig(Empty {})),
            &state,
            std::time::Duration::ZERO,
        );
        assert_eq!(reload.error.unwrap().code, "reload_unavailable");
        let notify = ipc_reply(
            &request(IpcCommand::Notify(crate::ipc::Notify {
                summary: "fixture only".into(),
                body: None,
                app: None,
            })),
            &state,
            std::time::Duration::ZERO,
        );
        assert_eq!(notify.error.unwrap().code, "unsupported");
    }

    #[test]
    fn ipc_distinguishes_known_open_closed_unknown_and_unregistered() {
        use crate::ipc::{Empty, IpcCommand, IpcRequest, SurfaceName};
        let dir = TestDir::new();
        dir.executable("nuraloumi-calendar", 0o755);
        let catalog = SurfaceCatalog::discover(&dir.0, &[]);
        let observations = catalog.observe(Some("launcher"), Some("first"), 2, 982);
        let request = |command| IpcRequest {
            id: "truth".into(),
            command,
        };
        let lookup = |name: &str| {
            ipc_reply(
                &request(IpcCommand::SurfaceState(SurfaceName {
                    surface: name.into(),
                })),
                &observations,
                std::time::Duration::ZERO,
            )
        };
        let open = lookup("launcher").result.unwrap();
        assert_eq!(open["open"], true);
        assert_eq!(open["open_state"], "open");
        assert_eq!(open["observation_supported"], true);
        assert_eq!(open["selected_item"], "first");
        assert_eq!(open["item_count"], 2);

        let closed = lookup("system").result.unwrap();
        assert_eq!(closed["open"], false);
        assert_eq!(closed["open_state"], "closed");
        assert_eq!(closed["observation_supported"], true);
        assert_eq!(closed["item_count"], 0);

        let unknown = lookup("calendar").result.unwrap();
        assert_eq!(unknown["open"], false); // old-client compatibility only
        assert_eq!(unknown["open_state"], "unknown");
        assert_eq!(unknown["observation_supported"], false);
        assert_eq!(unknown["selected_item"], serde_json::Value::Null);

        let list = ipc_reply(
            &request(IpcCommand::ListSurfaces(Empty {})),
            &observations,
            std::time::Duration::ZERO,
        )
        .result
        .unwrap();
        let rows = list.as_array().unwrap();
        let calendar = rows.iter().find(|row| row["name"] == "calendar").unwrap();
        assert_eq!(calendar["pid"], 0);
        assert_eq!(calendar["open_state"], "unknown");
        assert_eq!(calendar["observation_supported"], false);
        assert!(rows.iter().all(|row| row["name"] != "missing"));
        assert_eq!(lookup("missing").error.unwrap().code, "unknown_surface");
    }

    #[test]
    fn ipc_runtime_services_socket_without_compositor_and_cleans_it_up() {
        use crate::ipc::{self, Empty, IpcCommand, IpcRequest, SurfaceName};
        let dir = TestDir::new();
        let socket = dir.0.join("nuraloumi.sock");
        let catalog = SurfaceCatalog::discover(&dir.0, &[]);
        let initial = catalog.observe(None, None, 0, 512);
        let runtime = IpcRuntime::start(&socket, initial, dir.0.join("nuraloumi-toast")).unwrap();

        let ping = ipc::request(
            &socket,
            &IpcRequest {
                id: "ping".into(),
                command: IpcCommand::Ping(Empty {}),
            },
        )
        .unwrap();
        assert!(ping.ok);

        runtime.publish(catalog.observe(Some("system"), Some("volume"), 4, 512));
        let response = ipc::request(
            &socket,
            &IpcRequest {
                id: "state".into(),
                command: IpcCommand::SurfaceState(SurfaceName {
                    surface: "system".into(),
                }),
            },
        )
        .unwrap();
        assert_eq!(response.result.unwrap()["selected_item"], "volume");

        assert!(IpcRuntime::start(&socket, Vec::new(), dir.0.join("nuraloumi-toast")).is_err());
        drop(runtime);
        assert!(!socket.exists());
    }
}
