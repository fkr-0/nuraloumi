//! Bounded, software-first activation of the *regular* NuraLoumi WPE bundle.
//!
//! The separate Debian/private-root `,browse` launcher is deliberately not
//! involved. Packaging/CI is responsible for qualifying the real ARM ELF and
//! dlopen closure; this code only owns process lifetime and IPC readiness.

use super::{transport::checked_socket_path, BrowserRpc, NativeBrowser};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    env, fs,
    io::ErrorKind,
    os::{
        unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
        unix::process::ExitStatusExt,
    },
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub const REGULAR_BUNDLE: &str = "/usr/libexec/nuraloumi/browser";
const START_DEADLINE: Duration = Duration::from_secs(12);
const POLL: Duration = Duration::from_millis(50);

/// The process is killed only when we created it. Existing same-UID engines
/// are never adopted, killed, restarted or unlinked by this launcher.
pub struct EngineSession {
    owned: Option<Child>,
}

impl Drop for EngineSession {
    fn drop(&mut self) {
        if let Some(mut child) = self.owned.take() {
            if child.try_wait().ok().flatten().is_none() {
                // A held (unreaped) Child PID cannot be recycled before wait.
                unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
                for _ in 0..20 {
                    if child.try_wait().ok().flatten().is_some() {
                        return;
                    }
                    thread::sleep(POLL);
                }
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    }
}

struct LaunchPlan {
    engine: PathBuf,
    helper_paths: String,
    library_paths: String,
    webprocess_dir: PathBuf,
}

fn regular_file(root: &Path, name: &str, executable: bool) -> Result<PathBuf, String> {
    let relative = Path::new(name);
    if name.is_empty()
        || !relative
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err("Unsafe browser runtime inventory path".into());
    }
    let mut current = root.to_path_buf();
    let mut parts = relative.components().peekable();
    while let Some(component) = parts.next() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current)
            .map_err(|_| format!("Browser runtime file missing: {name}"))?;
        if metadata.file_type().is_symlink() {
            return Err(format!("Browser runtime path is a symlink: {name}"));
        }
        if metadata.permissions().mode() & 0o022 != 0 {
            return Err(format!("Browser runtime path has unsafe mode: {name}"));
        }
        if parts.peek().is_some() && !metadata.file_type().is_dir() {
            return Err(format!("Browser runtime parent is not a directory: {name}"));
        }
    }
    let metadata = fs::symlink_metadata(&current)
        .map_err(|_| format!("Browser runtime file missing: {name}"))?;
    if !metadata.file_type().is_file()
        || metadata.permissions().mode() & 0o022 != 0
        || (executable && metadata.permissions().mode() & 0o111 == 0)
    {
        return Err(format!("Browser runtime file has unsafe mode: {name}"));
    }
    Ok(current)
}

fn launch_plan(root: &Path) -> Result<LaunchPlan, String> {
    let metadata = fs::symlink_metadata(root)
        .map_err(|_| "Qualified regular NuraBrowser bundle is not installed")?;
    if !metadata.file_type().is_dir()
        || metadata.file_type().is_symlink()
        || metadata.permissions().mode() & 0o022 != 0
        || (!cfg!(test) && metadata.uid() != 0)
    {
        return Err("Browser bundle directory is not a trusted installation".into());
    }
    let document = regular_file(root, "runtime.json", false)?;
    let manifest: Value = serde_json::from_slice(
        &fs::read(document).map_err(|_| "Cannot read browser runtime manifest")?,
    )
    .map_err(|_| "Invalid browser runtime manifest")?;
    if manifest["schema"] != "nuraloumi-browser-runtime-v1" {
        return Err("Unqualified browser runtime schema".into());
    }
    let expected = [
        "sandbox",
        "instruction_closure",
        "dlopen_closure",
        "webprocess",
        "review",
    ];
    let qualifications = manifest["qualification"]
        .as_object()
        .ok_or("Browser runtime qualifications missing")?;
    if qualifications.len() != expected.len()
        || expected
            .iter()
            .any(|key| qualifications.get(*key) != Some(&Value::String("PASS".into())))
    {
        return Err("Browser runtime is not qualified for activation".into());
    }
    #[cfg(all(target_arch = "arm", target_env = "musl"))]
    if manifest["target"] != "armv7-unknown-linux-musleabihf" || manifest["abi"] != "musl" {
        return Err("Browser runtime targets the wrong ARMv7 libc".into());
    }
    #[cfg(all(target_arch = "arm", target_env = "gnu"))]
    if manifest["target"] != "armv7-unknown-linux-gnueabihf" || manifest["abi"] != "glibc" {
        return Err("Browser runtime targets the wrong ARMv7 libc".into());
    }
    let entries = manifest["files"]
        .as_array()
        .ok_or("Browser runtime file inventory missing")?;
    let mut engine = None;
    let mut webprocess_dir = None;
    let mut helpers = BTreeSet::new();
    let mut libraries = BTreeSet::new();
    let mut names = BTreeSet::new();
    for entry in entries {
        let name = entry["path"]
            .as_str()
            .ok_or("Invalid browser runtime inventory path")?;
        let role = entry["role"]
            .as_str()
            .ok_or("Invalid browser runtime inventory role")?;
        if !names.insert(name.to_owned()) {
            return Err("Duplicate browser runtime inventory file".into());
        }
        match role {
            "engine" if name == "nurabrowser-core" && engine.is_none() => {
                engine = Some(regular_file(root, name, true)?);
            }
            "webprocess" if name.ends_with("/WPEWebProcess") && webprocess_dir.is_none() => {
                let file = regular_file(root, name, true)?;
                webprocess_dir = file.parent().map(Path::to_path_buf);
            }
            "bwrap" if name.ends_with("/bwrap") => {
                let file = regular_file(root, name, true)?;
                helpers.insert(
                    file.parent()
                        .ok_or("Invalid browser helper path")?
                        .to_path_buf(),
                );
            }
            "xdg-dbus-proxy" if name.ends_with("/xdg-dbus-proxy") => {
                let file = regular_file(root, name, true)?;
                helpers.insert(
                    file.parent()
                        .ok_or("Invalid browser helper path")?
                        .to_path_buf(),
                );
            }
            "library" | "module" => {
                let file = regular_file(root, name, false)?;
                libraries.insert(
                    file.parent()
                        .ok_or("Invalid browser library path")?
                        .to_path_buf(),
                );
            }
            "loader" => {
                regular_file(root, name, true)?;
            }
            _ => return Err("Unsupported or ambiguous browser runtime inventory role".into()),
        }
    }
    let engine = engine.ok_or("Browser engine missing from qualified bundle")?;
    let webprocess_dir = webprocess_dir.ok_or("Browser WebProcess missing")?;
    if helpers.is_empty()
        || libraries.is_empty()
        || !entries.iter().any(|entry| entry["role"] == "bwrap")
        || !entries
            .iter()
            .any(|entry| entry["role"] == "xdg-dbus-proxy")
        || !entries.iter().any(|entry| entry["role"] == "loader")
    {
        return Err("Browser sandbox, loader or library closure missing".into());
    }
    let helper_paths = env::join_paths(helpers)
        .map_err(|_| "Unsafe browser helper path")?
        .to_string_lossy()
        .into_owned();
    let library_paths = env::join_paths(libraries)
        .map_err(|_| "Unsafe browser library path")?
        .to_string_lossy()
        .into_owned();
    Ok(LaunchPlan {
        engine,
        helper_paths,
        library_paths,
        webprocess_dir,
    })
}

fn wayland_socket(runtime: &Path, display: &str) -> Result<PathBuf, String> {
    let requested = Path::new(display);
    let path = if requested.is_absolute() {
        requested.to_path_buf()
    } else if !display.is_empty()
        && requested
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        runtime.join(requested)
    } else {
        return Err("Unsafe WAYLAND_DISPLAY path".into());
    };
    let metadata = fs::symlink_metadata(&path)
        .map_err(|_| "NuraBrowser requires a running Wayland compositor")?;
    if !metadata.file_type().is_socket() || metadata.uid() != unsafe { libc::geteuid() } {
        return Err("Unsafe or foreign Wayland socket".into());
    }
    Ok(path)
}

/// Connect to an existing private socket or start our packaged engine. Never
/// steal a pre-existing socket or start against an unsafe runtime directory.
pub fn connect_or_start(
    root: &Path,
    runtime: &Path,
    display: &str,
    deadline: Duration,
) -> Result<(NativeBrowser, EngineSession), String> {
    if env::var_os("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS").is_some()
        || env::var_os("WEBKIT_USE_SANDBOX").is_some_and(|value| value != "1")
    {
        return Err("WebKit sandbox disabling is forbidden".into());
    }
    let plan = launch_plan(root)?;
    let socket = checked_socket_path(runtime)?;
    wayland_socket(runtime, display)?;
    match fs::symlink_metadata(&socket) {
        Ok(_) => {
            let mut client = NativeBrowser::at_runtime_dir(runtime)?;
            client.request("uri", None)?;
            return Ok((client, EngineSession { owned: None }));
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(_) => return Err("Cannot inspect browser control socket".into()),
    }
    let mut command = Command::new(&plan.engine);
    command
        .arg("--platform=wayland")
        .arg("--url=about:blank")
        .env_clear()
        .env("XDG_RUNTIME_DIR", runtime)
        .env("WAYLAND_DISPLAY", display)
        .env("WPE_PLATFORM", "wayland")
        .env("WEBKIT_USE_SANDBOX", "1")
        // Search paths affect only our native engine and its children. They
        // must never reach NuraLoumi's Cairo shell or private VDE/GStreamer.
        .env("LD_LIBRARY_PATH", &plan.library_paths)
        // Upstream release WebKit normally uses compiled PKGLIBEXECDIR;
        // this override helps developer builds but is not proof of layout.
        .env("WEBKIT_EXEC_PATH", &plan.webprocess_dir)
        .env("PATH", format!("{}:/usr/bin:/bin", plan.helper_paths))
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .env("GALLIUM_DRIVER", "softpipe")
        .env("MESA_LOADER_DRIVER_OVERRIDE", "swrast")
        .stdin(Stdio::null());
    for key in [
        "HOME",
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
        "XDG_STATE_HOME",
        "XDG_DATA_HOME",
        "DBUS_SESSION_BUS_ADDRESS",
        "LANG",
        "LC_ALL",
    ] {
        if let Some(value) = env::var_os(key) {
            command.env(key, value);
        }
    }
    let child = command
        .spawn()
        .map_err(|_| "Could not start qualified native NuraBrowser engine")?;
    let mut session = EngineSession { owned: Some(child) };
    let started = Instant::now();
    loop {
        if let Some(exit) = session
            .owned
            .as_mut()
            .expect("owned engine while starting")
            .try_wait()
            .map_err(|_| "Cannot inspect browser engine process")?
        {
            return Err(format!(
                "NuraBrowser engine exited before IPC readiness ({})",
                exit.signal().map_or_else(
                    || format!("exit code {:?}", exit.code()),
                    |signal| format!("signal {signal}"),
                )
            ));
        }
        match fs::symlink_metadata(&socket) {
            Ok(meta) => {
                if !meta.file_type().is_socket() {
                    return Err("Browser engine created an unsafe IPC socket".into());
                }
                if let Ok(mut client) = NativeBrowser::at_runtime_dir(runtime) {
                    if client.request("uri", None).is_ok() {
                        return Ok((client, session));
                    }
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(_) => return Err("Cannot inspect browser engine socket".into()),
        }
        if started.elapsed() >= deadline {
            return Err("NuraBrowser engine IPC readiness deadline exceeded".into());
        }
        thread::sleep(POLL);
    }
}

pub fn connect_or_start_default() -> Result<(NativeBrowser, EngineSession), String> {
    let runtime = env::var_os("XDG_RUNTIME_DIR")
        .filter(|value| !value.is_empty())
        .ok_or("XDG_RUNTIME_DIR required for native NuraBrowser")?;
    let display = env::var("WAYLAND_DISPLAY")
        .map_err(|_| "WAYLAND_DISPLAY required for native NuraBrowser")?;
    connect_or_start(
        Path::new(REGULAR_BUNDLE),
        Path::new(&runtime),
        &display,
        START_DEADLINE,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        os::unix::{fs::PermissionsExt, net::UnixListener},
        time::{SystemTime, UNIX_EPOCH},
    };

    struct TemporaryTree(PathBuf);

    impl TemporaryTree {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = env::temp_dir().join(format!(
                "nuraloumi-browser-activation-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            Self(path)
        }

        fn directory(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::create_dir(&path).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            path
        }
    }

    impl Drop for TemporaryTree {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn fixture_bundle(root: &Path) {
        let files = [
            ("nurabrowser-core", "engine", true),
            ("libexec/WPEWebProcess", "webprocess", true),
            ("bin/bwrap", "bwrap", true),
            ("bin/xdg-dbus-proxy", "xdg-dbus-proxy", true),
            ("lib/ld-musl-armhf.so.1", "loader", true),
            ("lib/libWPE.so", "library", false),
            ("lib/wpe-platform.so", "module", false),
        ];
        let mut inventory = Vec::new();
        for (path, role, executable) in files {
            let target = root.join(path);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).unwrap();
                fs::set_permissions(parent, fs::Permissions::from_mode(0o755)).unwrap();
            }
            fs::write(&target, b"synthetic fixture, not an ELF").unwrap();
            fs::set_permissions(
                &target,
                fs::Permissions::from_mode(if executable { 0o755 } else { 0o644 }),
            )
            .unwrap();
            inventory.push(serde_json::json!({"path":path,"role":role,"sha256":"0".repeat(64)}));
        }
        fs::write(
            root.join("runtime.json"),
            serde_json::to_vec(&serde_json::json!({
                "schema":"nuraloumi-browser-runtime-v1",
                "target":"armv7-unknown-linux-musleabihf",
                "abi":"musl",
                "source_sha":"a".repeat(64),
                "producer_revision":"b".repeat(64),
                "qualification":{
                    "sandbox":"PASS", "instruction_closure":"PASS",
                    "dlopen_closure":"PASS", "webprocess":"PASS", "review":"PASS"
                },
                "files":inventory
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn activation_requires_complete_qualified_manifest_and_safe_paths() {
        let temp = TemporaryTree::new();
        let root = temp.directory("bundle");
        assert!(launch_plan(&root).is_err());
        fixture_bundle(&root);
        let plan = launch_plan(&root).unwrap();
        assert_eq!(plan.engine, root.join("nurabrowser-core"));
        assert!(regular_file(&root, "../escape", true).is_err());
        assert!(regular_file(&root, "/etc/passwd", false).is_err());
        fs::set_permissions(root.join("bin"), fs::Permissions::from_mode(0o777)).unwrap();
        assert!(launch_plan(&root).is_err());
        fs::set_permissions(root.join("bin"), fs::Permissions::from_mode(0o755)).unwrap();
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(root.join("runtime.json")).unwrap()).unwrap();
        manifest["qualification"]["sandbox"] = Value::String("UNVERIFIED".into());
        fs::write(
            root.join("runtime.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        assert!(launch_plan(&root).is_err());
    }

    #[test]
    fn activation_attaches_only_to_private_same_uid_ipc() {
        let temp = TemporaryTree::new();
        let root = temp.directory("bundle");
        fixture_bundle(&root);
        let runtime = temp.directory("runtime");
        assert!(connect_or_start(&root, &runtime, "missing-wayland", POLL).is_err());
        let _wayland = UnixListener::bind(runtime.join("wayland-0")).unwrap();
        let socket = checked_socket_path(&runtime).unwrap();
        let listener = UnixListener::bind(&socket).unwrap();
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
        let worker = thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut request = [0_u8; 256];
            let len = conn.read(&mut request).unwrap();
            assert!(std::str::from_utf8(&request[..len])
                .unwrap()
                .contains("\"uri\""));
            conn.write_all(b"{\"ok\":true,\"result\":{\"uri\":\"about:blank\"}}\n")
                .unwrap();
        });
        let (mut browser, session) = connect_or_start(&root, &runtime, "wayland-0", POLL).unwrap();
        worker.join().unwrap();
        assert!(session.owned.is_none());
        assert_eq!(
            browser.request("uri", None).unwrap_err(),
            "Cannot connect to local browser engine"
        );
    }
}
