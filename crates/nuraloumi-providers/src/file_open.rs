//! XDG file opener: argv-only handlers, bounded helpers, no renderer dependency.
//! A successful launch is only an accepted request, not a rendered app frame.
use std::collections::BTreeMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const LIMIT: Duration = Duration::from_secs(4);
const MAX_CAPTURE: usize = 4096;
const MAX_DESKTOP: u64 = 256 * 1024;
const MAX_SCAN: usize = 4096;
const MAX_HANDLERS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopHandler {
    pub id: String,
    pub name: String,
    pub desktop_file: PathBuf,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchReceipt {
    pub handler: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenOutcome {
    Opened {
        handler: String,
    },
    NeedsOpenWith {
        reason: String,
        handlers: Vec<DesktopHandler>,
    },
    NeedsFallback {
        reason: String,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandResult {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}
pub trait OpenCommandRunner {
    fn run(
        &mut self,
        program: &str,
        args: &[OsString],
        capture: bool,
    ) -> Result<CommandResult, String>;
}
#[derive(Default, Clone, Copy)]
pub struct SystemOpenCommandRunner;
// Consume both child pipes directly with nonblocking reads. A GUI helper can
// spawn descendants that inherit stdout/stderr: joining EOF-bound reader
// threads after the launcher exits otherwise hangs forever.
#[cfg(unix)]
fn nonblocking<R: std::os::fd::AsRawFd>(stream: &R) -> Result<(), String> {
    use std::os::raw::c_int;
    unsafe extern "C" {
        fn fcntl(fd: c_int, cmd: c_int, arg: c_int) -> c_int;
    }
    const F_GETFL: c_int = 3;
    const F_SETFL: c_int = 4;
    const O_NONBLOCK: c_int = 0o4000;
    let fd = stream.as_raw_fd();
    let current = unsafe { fcntl(fd, F_GETFL, 0) };
    if current < 0 || unsafe { fcntl(fd, F_SETFL, current | O_NONBLOCK) } < 0 {
        return Err(format!(
            "cannot bound helper pipe: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}
#[cfg(not(unix))]
fn nonblocking<R>(_stream: &R) -> Result<(), String> {
    Err("bounded pipe capture requires Unix".into())
}

fn drain_pipe(reader: &mut impl Read, bytes: &mut Vec<u8>) -> Result<(), String> {
    loop {
        let mut chunk = [0u8; 1024];
        match reader.read(&mut chunk) {
            Ok(0) => return Ok(()),
            Ok(n) => {
                if bytes.len().saturating_add(n) > MAX_CAPTURE {
                    return Err("backend output exceeded cap".into());
                }
                bytes.extend_from_slice(&chunk[..n]);
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(format!("backend pipe read: {err}")),
        }
    }
}

impl OpenCommandRunner for SystemOpenCommandRunner {
    fn run(
        &mut self,
        program: &str,
        args: &[OsString],
        capture: bool,
    ) -> Result<CommandResult, String> {
        // The native file manager is a long-lived GUI, not a short-lived helper.
        if program == "nuraloumi-files" {
            let mut child = Command::new(program)
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("{program}: {e}"))?;
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                return Ok(CommandResult {
                    status: status.code().unwrap_or(-1),
                    stdout: String::new(),
                    stderr: String::new(),
                });
            }
            thread::spawn(move || {
                let _ = child.wait();
            });
            return Ok(CommandResult {
                status: 0,
                stdout: String::new(),
                stderr: String::new(),
            });
        }

        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(if capture {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stderr(if capture {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .spawn()
            .map_err(|e| format!("{program}: {e}"))?;
        let mut out = child.stdout.take();
        let mut err = child.stderr.take();
        if capture {
            let prep = out
                .as_ref()
                .ok_or_else(|| "missing stdout".to_owned())
                .and_then(nonblocking)
                .and_then(|_| {
                    err.as_ref()
                        .ok_or_else(|| "missing stderr".to_owned())
                        .and_then(nonblocking)
                });
            if let Err(e) = prep {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        }
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let started = Instant::now();
        let status = loop {
            if let Err(e) = (|| {
                if let Some(pipe) = out.as_mut() {
                    drain_pipe(pipe, &mut stdout)?;
                }
                if let Some(pipe) = err.as_mut() {
                    drain_pipe(pipe, &mut stderr)?;
                }
                if stdout.len() + stderr.len() > MAX_CAPTURE {
                    return Err("combined helper output exceeded cap".into());
                }
                Ok::<(), String>(())
            })() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed() < LIMIT => thread::sleep(Duration::from_millis(10)),
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("{program}: timeout"));
                }
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("{program}: wait: {e}"));
                }
            }
        };
        // Drain any final buffered output once, without waiting for EOF from
        // unrelated descendants that inherited the pipe descriptors.
        if let Some(pipe) = out.as_mut() {
            drain_pipe(pipe, &mut stdout)?;
        }
        if let Some(pipe) = err.as_mut() {
            drain_pipe(pipe, &mut stderr)?;
        }
        if stdout.len() + stderr.len() > MAX_CAPTURE {
            return Err("combined helper output exceeded cap".into());
        }
        Ok(CommandResult {
            status: status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        })
    }
}

fn checked_path(path: &Path) -> Result<(PathBuf, bool), String> {
    let meta =
        fs::symlink_metadata(path).map_err(|e| format!("inspect {}: {e}", path.display()))?;
    if meta.file_type().is_symlink() {
        return Err("symlinks are not opened implicitly".into());
    }
    if !meta.is_file() && !meta.is_dir() {
        return Err("not a regular file or directory".into());
    }
    Ok((
        fs::canonicalize(path).map_err(|e| format!("resolve path: {e}"))?,
        meta.is_dir(),
    ))
}
fn invoke<R: OpenCommandRunner>(
    r: &mut R,
    cmd: &str,
    argv: Vec<OsString>,
    capture: bool,
) -> Result<CommandResult, String> {
    let result = r.run(cmd, &argv, capture)?;
    if result.status != 0 {
        return Err(format!(
            "{cmd} exited {}: {}",
            result.status,
            result.stderr.trim()
        ));
    }
    Ok(result)
}
fn argv(items: &[&str]) -> Vec<OsString> {
    items.iter().map(OsString::from).collect()
}
fn valid_mime(m: &str) -> bool {
    !m.is_empty()
        && m.len() <= 128
        && m.contains('/')
        && m.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-+_/".contains(&b))
}
fn valid_id(id: &str) -> bool {
    id.len() <= 512
        && id.ends_with(".desktop")
        && id != ".desktop"
        && !id.starts_with('-')
        && !id.contains(['/', '\\', '\0', '\r', '\n'])
}
fn mime_of<R: OpenCommandRunner>(path: &Path, dir: bool, r: &mut R) -> Result<String, String> {
    if dir {
        return Ok("inode/directory".into());
    }
    let mut a = argv(&["query", "filetype"]);
    a.push(path.as_os_str().to_owned());
    if let Ok(result) = invoke(r, "xdg-mime", a, true) {
        let mime = result.stdout.trim();
        if valid_mime(mime) {
            return Ok(mime.into());
        }
    }
    let mut a = argv(&["-b", "--mime-type", "--"]);
    a.push(path.as_os_str().to_owned());
    let result = invoke(r, "file", a, true)?;
    let mime = result.stdout.trim();
    if !valid_mime(mime) {
        return Err("unknown MIME type".into());
    }
    Ok(mime.into())
}
fn default_for<R: OpenCommandRunner>(mime: &str, r: &mut R) -> Option<String> {
    let value = invoke(r, "xdg-mime", argv(&["query", "default", mime]), true)
        .ok()?
        .stdout;
    let id = value.trim();
    valid_id(id).then(|| id.to_owned())
}
pub fn application_roots() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(v) = env::var_os("XDG_DATA_HOME") {
        dirs.push(PathBuf::from(v).join("applications"));
    } else if let Some(v) = env::var_os("HOME") {
        dirs.push(PathBuf::from(v).join(".local/share/applications"));
    }
    let global = env::var_os("XDG_DATA_DIRS")
        .unwrap_or_else(|| OsString::from("/usr/local/share:/usr/share"));
    for d in env::split_paths(&global) {
        dirs.push(d.join("applications"));
    }
    dirs
}
fn scan_dir(
    root: &Path,
    dir: &Path,
    mime: &str,
    depth: usize,
    visited: &mut usize,
    apps: &mut BTreeMap<String, DesktopHandler>,
) {
    if depth > 2 || *visited >= MAX_SCAN {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    // Bound memory before sorting: a directory can contain millions of entries.
    let mut paths: Vec<_> = entries
        .take(MAX_SCAN.saturating_sub(*visited))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    paths.sort();
    for p in paths {
        if *visited >= MAX_SCAN {
            return;
        }
        *visited += 1;
        let Ok(meta) = fs::symlink_metadata(&p) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            scan_dir(root, &p, mime, depth + 1, visited, apps);
            continue;
        }
        if p.extension() != Some(OsStr::new("desktop")) {
            continue;
        }
        if !meta.is_file() || meta.len() > MAX_DESKTOP {
            continue;
        }
        let Ok(content) = fs::read_to_string(&p) else {
            continue;
        };
        let mut in_entry = false;
        let mut fields = BTreeMap::<&str, &str>::new();
        for raw in content.lines() {
            let line = raw.trim();
            if line.starts_with('[') && line.ends_with(']') {
                in_entry = line == "[Desktop Entry]";
                continue;
            }
            if !in_entry || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                fields.entry(k.trim()).or_insert(v.trim());
            }
        }
        if fields.get("Type") != Some(&"Application")
            || fields.get("Hidden") == Some(&"true")
            // NoDisplay hides an app from launcher menus, not from file
            // association resolution. A MIME-capable helper may be hidden.
            || fields.get("Exec").is_none_or(|v| v.is_empty())
            || !fields
                .get("MimeType")
                .is_some_and(|v| v.split(';').any(|m| m == mime))
        {
            continue;
        }
        let Some(name) = fields.get("Name").filter(|v| !v.is_empty()) else {
            continue;
        };
        let Ok(relative) = p.strip_prefix(root) else {
            continue;
        };
        let Some(rel) = relative.to_str() else {
            continue;
        };
        let id = rel.replace('/', "-");
        if valid_id(&id) {
            apps.entry(id.clone()).or_insert(DesktopHandler {
                id,
                name: (*name).to_string(),
                desktop_file: p,
            });
        }
    }
}
fn scan_apps(roots: &[PathBuf], mime: &str) -> Vec<DesktopHandler> {
    let mut visited = 0;
    let mut apps = BTreeMap::new();
    for root in roots {
        scan_dir(root, root, mime, 0, &mut visited, &mut apps);
        if visited >= MAX_SCAN {
            break;
        }
    }
    let mut handlers: Vec<_> = apps.into_values().collect();
    handlers.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.id.cmp(&b.id))
    });
    handlers.truncate(MAX_HANDLERS);
    handlers
}
fn launch<R: OpenCommandRunner>(
    path: &Path,
    app: &DesktopHandler,
    r: &mut R,
) -> Result<LaunchReceipt, String> {
    invoke(
        r,
        "gio",
        vec![
            OsString::from("launch"),
            app.desktop_file.as_os_str().to_owned(),
            path.as_os_str().to_owned(),
        ],
        false,
    )?;
    Ok(LaunchReceipt {
        handler: format!("{} (launch requested)", app.name),
    })
}
pub fn open_default(path: &Path) -> OpenOutcome {
    open_default_with(path, &application_roots(), &mut SystemOpenCommandRunner)
}
pub fn open_default_with<R: OpenCommandRunner>(
    path: &Path,
    roots: &[PathBuf],
    r: &mut R,
) -> OpenOutcome {
    let (path, directory) = match checked_path(path) {
        Ok(v) => v,
        Err(reason) => return OpenOutcome::NeedsFallback { reason },
    };
    let mut reasons = Vec::new();
    if directory {
        match invoke(
            r,
            "nuraloumi-files",
            vec![OsString::from("--live"), path.as_os_str().to_owned()],
            false,
        ) {
            Ok(_) => {
                return OpenOutcome::Opened {
                    handler: "nuraloumi-files (launch requested)".into(),
                }
            }
            Err(e) => reasons.push(e),
        }
    }
    let mime = match mime_of(&path, directory, r) {
        Ok(m) => m,
        Err(e) => return OpenOutcome::NeedsFallback { reason: e },
    };
    let handlers = scan_apps(roots, &mime);
    if let Some(default_id) = default_for(&mime, r) {
        if let Some(handler) = handlers.iter().find(|h| h.id == default_id) {
            match launch(&path, handler, r) {
                Ok(receipt) => {
                    return OpenOutcome::Opened {
                        handler: receipt.handler,
                    }
                }
                Err(e) => reasons.push(e),
            }
        } else {
            // mimeapps.list may associate a desktop app that omits MimeType,
            // or that isn't indexed in our bounded chooser scan. Let GIO
            // resolve the default instead of rejecting a valid association.
            match invoke(
                r,
                "gio",
                vec!["open".into(), path.as_os_str().to_owned()],
                false,
            ) {
                Ok(_) => {
                    return OpenOutcome::Opened {
                        handler: format!("XDG default {default_id} (launch requested)"),
                    }
                }
                Err(e) => reasons.push(format!("default {default_id} unavailable: {e}")),
            }
        }
    } else {
        // GIO may have a default even if xdg-mime itself is missing.
        match invoke(
            r,
            "gio",
            vec!["open".into(), path.as_os_str().to_owned()],
            false,
        ) {
            Ok(_) => {
                return OpenOutcome::Opened {
                    handler: "GIO default (launch requested)".into(),
                }
            }
            Err(e) => reasons.push(format!("no usable XDG default for {mime}: {e}")),
        }
    }
    let reason = reasons.join("; ");
    if handlers.is_empty() {
        OpenOutcome::NeedsFallback { reason }
    } else {
        OpenOutcome::NeedsOpenWith { reason, handlers }
    }
}
pub fn handlers_for_path(path: &Path) -> Vec<DesktopHandler> {
    handlers_for_path_with(path, &application_roots(), &mut SystemOpenCommandRunner)
}
pub fn handlers_for_path_with<R: OpenCommandRunner>(
    path: &Path,
    roots: &[PathBuf],
    r: &mut R,
) -> Vec<DesktopHandler> {
    let Ok((path, dir)) = checked_path(path) else {
        return Vec::new();
    };
    let Ok(mime) = mime_of(&path, dir, r) else {
        return Vec::new();
    };
    scan_apps(roots, &mime)
}
pub fn open_with(path: &Path, id: &str) -> Result<LaunchReceipt, String> {
    open_with_with(path, id, &application_roots(), &mut SystemOpenCommandRunner)
}
pub fn open_with_with<R: OpenCommandRunner>(
    path: &Path,
    id: &str,
    roots: &[PathBuf],
    r: &mut R,
) -> Result<LaunchReceipt, String> {
    if !valid_id(id) {
        return Err("invalid desktop ID".into());
    }
    let (path, dir) = checked_path(path)?;
    let mime = mime_of(&path, dir, r)?;
    let handlers = scan_apps(roots, &mime);
    let app = handlers
        .iter()
        .find(|h| h.id == id)
        .ok_or_else(|| format!("handler {id} is not eligible for {mime}"))?;
    launch(&path, app, r)
}
pub fn describe_file(path: &Path) -> Result<String, String> {
    describe_file_with(path, &mut SystemOpenCommandRunner)
}
pub fn describe_file_with<R: OpenCommandRunner>(path: &Path, r: &mut R) -> Result<String, String> {
    let (path, _) = checked_path(path)?;
    let mut a = argv(&["--"]);
    a.push(path.as_os_str().to_owned());
    let result = invoke(r, "file", a, true)?;
    Ok(result.stdout.trim().to_owned())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::symlink;
    use std::time::{SystemTime, UNIX_EPOCH};
    #[derive(Default)]
    struct Fixture {
        entries: Vec<(String, Vec<OsString>, Result<CommandResult, String>)>,
    }
    impl Fixture {
        fn ok(&mut self, p: &str, a: Vec<OsString>, status: i32, out: &str) {
            self.entries.push((
                p.into(),
                a,
                Ok(CommandResult {
                    status,
                    stdout: out.into(),
                    stderr: String::new(),
                }),
            ));
        }
    }
    impl OpenCommandRunner for Fixture {
        fn run(&mut self, p: &str, a: &[OsString], _: bool) -> Result<CommandResult, String> {
            self.entries
                .iter()
                .find(|(cmd, args, _)| cmd == p && args == a)
                .map(|(_, _, v)| v.clone())
                .unwrap_or_else(|| Err(format!("{p}: missing helper")))
        }
    }
    fn temp() -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = env::temp_dir().join(format!("nura-open-{}-{n}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        dir
    }
    fn desktop(root: &Path, name: &str, mime: &str) -> PathBuf {
        let p = root.join(format!("{name}.desktop"));
        fs::write(
            &p,
            format!(
                "[Desktop Entry]\nType=Application\nName={name}\nExec=view %f\nMimeType={mime};\n"
            ),
        )
        .unwrap();
        p
    }
    fn mime(r: &mut Fixture, p: &Path, kind: &str, default: &str) {
        r.ok(
            "xdg-mime",
            vec!["query".into(), "filetype".into(), p.as_os_str().to_owned()],
            0,
            kind,
        );
        r.ok("xdg-mime", argv(&["query", "default", kind]), 0, default);
    }
    #[test]
    fn default_and_manual_handlers_use_exact_argv() {
        let dir = temp();
        let path = dir.join(" - name 'quotes'; $(oops).txt");
        fs::write(&path, "hi").unwrap();
        let app = desktop(&dir, "view", "text/plain");
        let mut r = Fixture::default();
        mime(&mut r, &path, "text/plain", "view.desktop");
        r.ok(
            "gio",
            vec![
                "launch".into(),
                app.into_os_string(),
                path.clone().into_os_string(),
            ],
            0,
            "",
        );
        assert!(matches!(
            open_default_with(&path, std::slice::from_ref(&dir), &mut r),
            OpenOutcome::Opened { .. }
        ));
        assert!(open_with_with(&path, "view.desktop", std::slice::from_ref(&dir), &mut r).is_ok());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn missing_default_and_process_failure_offer_real_handler() {
        let dir = temp();
        let p = dir.join("a");
        fs::write(&p, "hi").unwrap();
        desktop(&dir, "viewer", "text/plain");
        let mut r = Fixture::default();
        mime(&mut r, &p, "text/plain", "");
        assert!(
            matches!(open_default_with(&p,std::slice::from_ref(&dir),&mut r),OpenOutcome::NeedsOpenWith{handlers,..} if handlers.len()==1)
        );
        let mut r = Fixture::default();
        mime(&mut r, &p, "text/plain", "viewer.desktop");
        assert!(
            matches!(open_default_with(&p,std::slice::from_ref(&dir),&mut r),OpenOutcome::NeedsOpenWith{reason,..} if reason.contains("gio"))
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn no_helper_no_target_and_special_files_are_diagnostics() {
        let dir = temp();
        let p = dir.join("a");
        fs::write(&p, "hi").unwrap();
        assert!(
            matches!(open_default_with(&p,&[],&mut Fixture::default()),OpenOutcome::NeedsFallback{reason} if reason.contains("helper"))
        );
        assert!(matches!(
            open_default_with(&dir.join("absent"), &[], &mut Fixture::default()),
            OpenOutcome::NeedsFallback { .. }
        ));
        symlink(&p, dir.join("symlink")).unwrap();
        assert!(
            matches!(open_default_with(&dir.join("symlink"),&[],&mut Fixture::default()),OpenOutcome::NeedsFallback{reason} if reason.contains("symlink"))
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn directory_file_manager_and_file_description() {
        let dir = temp();
        let p = dir.join("a");
        fs::write(&p, "x").unwrap();
        let mut r = Fixture::default();
        r.ok(
            "nuraloumi-files",
            vec!["--live".into(), dir.clone().into_os_string()],
            0,
            "",
        );
        r.ok(
            "file",
            vec!["--".into(), p.clone().into_os_string()],
            0,
            "a: ASCII text",
        );
        assert!(matches!(
            open_default_with(&dir, &[], &mut r),
            OpenOutcome::Opened { .. }
        ));
        assert_eq!(describe_file_with(&p, &mut r).unwrap(), "a: ASCII text");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn non_utf8_path_preserved_and_untrusted_desktop_rejected() {
        let dir = temp();
        let p = dir.join(OsString::from_vec(vec![b'-', 0xff, b'x']));
        fs::write(&p, "hi").unwrap();
        let mut r = Fixture::default();
        mime(&mut r, &p, "text/plain", "");
        r.ok(
            "file",
            vec!["--".into(), p.clone().into_os_string()],
            0,
            "data",
        );
        assert_eq!(describe_file_with(&p, &mut r).unwrap(), "data");
        desktop(&dir, "wrong", "image/png");
        fs::write(dir.join("hidden.desktop"),
            "[Desktop Entry]\nType=Application\nName=Hidden\nExec=x %f\nMimeType=text/plain;\nHidden=true\n").unwrap();
        assert!(handlers_for_path_with(&p, std::slice::from_ref(&dir), &mut r).is_empty());
        assert!(open_with_with(&p, "wrong.desktop", std::slice::from_ref(&dir), &mut r).is_err());
        assert!(
            open_with_with(&p, "../wrong.desktop", std::slice::from_ref(&dir), &mut r).is_err()
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn gio_default_accepts_association_not_declared_in_desktop_mime() {
        let dir = temp();
        let path = dir.join("ordinary file.dat");
        fs::write(&path, "data").unwrap();
        // mimeapps.list can associate a handler even if its desktop MimeType
        // omits that MIME; GIO is the system authority for resolution.
        let mut r = Fixture::default();
        mime(&mut r, &path, "application/x-test", "other.desktop");
        r.ok(
            "gio",
            vec!["open".into(), path.clone().into_os_string()],
            0,
            "",
        );
        assert!(matches!(
            open_default_with(&path, std::slice::from_ref(&dir), &mut r),
            OpenOutcome::Opened { handler } if handler.contains("other.desktop")
        ));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn explicit_nonzero_launch_status_opens_chooser_not_false_success() {
        let dir = temp();
        let path = dir.join("failed.txt");
        fs::write(&path, "hello").unwrap();
        let desktop_path = desktop(&dir, "notworking", "text/plain");
        let mut runner = Fixture::default();
        mime(&mut runner, &path, "text/plain", "notworking.desktop");
        runner.ok(
            "gio",
            vec![
                "launch".into(),
                desktop_path.into_os_string(),
                path.clone().into_os_string(),
            ],
            2,
            "",
        );
        assert!(matches!(
            open_default_with(&path, std::slice::from_ref(&dir), &mut runner),
            OpenOutcome::NeedsOpenWith { reason, .. } if reason.contains("exited 2")
        ));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn hidden_menu_entry_can_still_be_a_mime_handler() {
        let dir = temp();
        let path = dir.join("n.txt");
        fs::write(&path, "hi").unwrap();
        fs::write(
            dir.join("background.desktop"),
            "[Desktop Entry]\nType=Application\nName=Background Handler\nExec=example %f\nMimeType=text/plain;\nNoDisplay=true\n",
        ).unwrap();
        let mut r = Fixture::default();
        mime(&mut r, &path, "text/plain", "");
        let handlers = handlers_for_path_with(&path, std::slice::from_ref(&dir), &mut r);
        assert_eq!(handlers.len(), 1);
        assert_eq!(handlers[0].id, "background.desktop");
        fs::remove_dir_all(dir).unwrap();
    }
}
