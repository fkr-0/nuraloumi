//! Deterministic discovery of shell-owned surfaces.
//!
//! Only the panel and its embedded menu families are presumed available.
//! Separately packaged executables must actually exist beside the running
//! binary, while new 0.2 features require an explicit, reviewed registration.
//! This catalog never guesses that a separate process is running.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
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
pub struct IpcRuntime {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    observations: std::sync::Arc<std::sync::Mutex<Vec<SurfaceObservation>>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl IpcRuntime {
    pub fn start(socket: &Path, initial: Vec<SurfaceObservation>) -> Result<Self, String> {
        use std::sync::{atomic::AtomicBool, Arc, Mutex};

        let mut server = crate::ipc::IpcServer::bind(socket)
            .map_err(|error| format!("cannot bind panel IPC {}: {error}", socket.display()))?;
        let stop = Arc::new(AtomicBool::new(false));
        let observations = Arc::new(Mutex::new(initial));
        let stop_worker = Arc::clone(&stop);
        let observations_worker = Arc::clone(&observations);
        let worker = std::thread::Builder::new()
            .name("nuraloumi-ipc".to_owned())
            .spawn(move || {
                let started = std::time::Instant::now();
                while !stop_worker.load(std::sync::atomic::Ordering::Relaxed) {
                    if let Ok(Some(request)) = server.accept_one() {
                        let current = observations_worker
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .clone();
                        let response = ipc_reply(&request, &current, started.elapsed());
                        // A disconnected, non-reading, or malformed client cannot
                        // stall the compositor loop: this is a separate worker.
                        let _ = server.respond(&response);
                    }
                    // Bound processing to <= 50 requests/s per panel. This also
                    // gives idle and malformed connections a finite CPU cost.
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                let _ = server.cleanup();
            })
            .map_err(|error| format!("cannot start panel IPC worker: {error}"))?;
        Ok(Self {
            stop,
            observations,
            worker: Some(worker),
        })
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
            "restart_required",
            "config is validated at startup; live reload is not implemented",
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
        assert_eq!(reload.error.unwrap().code, "restart_required");
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
        let runtime = IpcRuntime::start(&socket, initial).unwrap();

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

        assert!(IpcRuntime::start(&socket, Vec::new()).is_err());
        drop(runtime);
        assert!(!socket.exists());
    }
}
