//! Bounded, private, same-UID native WPE-engine transport.
//!
//! Not HTTP, TCP, D-Bus, or WebView DOM injection. Commands are allowlisted
//! and the Unix peer is authenticated using kernel SO_PEERCRED.

use super::BrowserRpc;
use serde_json::{json, Value};
use std::{
    env, fs,
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{FileTypeExt, MetadataExt, PermissionsExt},
            net::UnixStream,
        },
    },
    path::{Component, Path, PathBuf},
    time::Duration,
};

const MAX_FRAME: usize = 8192;
const MAX_SOCKET_PATH: usize = 104;
const SOCKET_NAME: &str = "nura-browser.sock";
const TIMEOUT: Duration = Duration::from_millis(450);

pub struct NativeBrowser {
    socket: PathBuf,
}

fn private_runtime_path(dir: &Path) -> Result<PathBuf, String> {
    if !dir.is_absolute() || dir.as_os_str().len() > MAX_SOCKET_PATH {
        return Err("Browser runtime path is invalid".into());
    }
    for component in dir.components() {
        if !matches!(component, Component::RootDir | Component::Normal(_)) {
            return Err("Unsafe browser runtime path".into());
        }
    }
    let mut prefix = PathBuf::from("/");
    for component in dir.components() {
        if let Component::Normal(piece) = component {
            prefix.push(piece);
            let metadata = fs::symlink_metadata(&prefix)
                .map_err(|_| "Browser runtime directory is missing")?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err("Browser runtime directory contains a symlink".into());
            }
        }
    }
    let metadata =
        fs::symlink_metadata(dir).map_err(|_| "Browser runtime directory unavailable")?;
    if metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o777 != 0o700
    {
        return Err("Browser runtime directory must be owned and mode 0700".into());
    }
    let socket = dir.join(SOCKET_NAME);
    if socket.as_os_str().len() > MAX_SOCKET_PATH {
        return Err("Browser socket path exceeds Unix socket limit".into());
    }
    let socket_meta = fs::symlink_metadata(&socket)
        .map_err(|_| "Browser engine socket is not running".to_owned())?;
    if !socket_meta.file_type().is_socket()
        || socket_meta.uid() != unsafe { libc::geteuid() }
        || socket_meta.permissions().mode() & 0o777 != 0o600
    {
        return Err("Browser socket has unsafe type, ownership or mode".into());
    }
    Ok(socket)
}

impl NativeBrowser {
    /// Uses XDG_RUNTIME_DIR, never a /tmp fallback or implicit TCP service.
    pub fn from_environment() -> Result<Self, String> {
        let base = env::var_os("XDG_RUNTIME_DIR")
            .filter(|s| !s.is_empty())
            .ok_or("XDG_RUNTIME_DIR required to contact NuraBrowser")?;
        Self::at_runtime_dir(Path::new(&base))
    }

    pub fn at_runtime_dir(path: &Path) -> Result<Self, String> {
        Ok(Self {
            socket: private_runtime_path(path)?,
        })
    }

    fn connect(&self) -> Result<UnixStream, String> {
        let stream = UnixStream::connect(&self.socket)
            .map_err(|_| "Cannot connect to local browser engine".to_owned())?;
        stream
            .set_read_timeout(Some(TIMEOUT))
            .map_err(|_| "Cannot configure browser read deadline".to_owned())?;
        stream
            .set_write_timeout(Some(TIMEOUT))
            .map_err(|_| "Cannot configure browser write deadline".to_owned())?;
        let mut credentials = libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let mut size = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let status = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&raw mut credentials).cast(),
                &mut size,
            )
        };
        if status != 0
            || size as usize != std::mem::size_of::<libc::ucred>()
            || credentials.uid != unsafe { libc::geteuid() }
        {
            return Err("Browser IPC peer identity mismatch".into());
        }
        Ok(stream)
    }

    fn command_allowed(command: &str, arg: Option<&str>) -> bool {
        match command {
            "open" => arg.is_some_and(|text| {
                !text.is_empty() && text.len() <= 4096 && !text.chars().any(char::is_control)
            }),
            "tab-select" | "tab-close" => arg.is_some_and(|text| {
                text.len() == 1 && text.as_bytes()[0].is_ascii_digit() && text.as_bytes()[0] < b'6'
            }),
            "bookmarks" => arg.is_none_or(|text| {
                text.len() == 1 && text.as_bytes()[0].is_ascii_digit() && text.as_bytes()[0] < b'8'
            }),
            "uri" | "tabs" | "recent" | "tab-new" | "back" | "forward" | "reload" | "stop"
            | "bookmark-toggle" => arg.is_none(),
            _ => false,
        }
    }

    fn read_frame(stream: &mut UnixStream) -> Result<Value, String> {
        let mut line = Vec::with_capacity(512);
        let mut buffer = [0_u8; 1024];
        loop {
            let count = stream
                .read(&mut buffer)
                .map_err(|_| "Browser response timed out or failed".to_owned())?;
            if count == 0 {
                return Err("Browser response closed without newline".into());
            }
            if let Some(index) = buffer[..count].iter().position(|b| *b == b'\n') {
                if index + 1 != count || line.len() + index >= MAX_FRAME {
                    return Err("Invalid browser IPC frame boundary".into());
                }
                line.extend_from_slice(&buffer[..index]);
                break;
            }
            if line.len() + count >= MAX_FRAME || buffer[..count].contains(&0) {
                return Err("Oversized or invalid browser IPC reply".into());
            }
            line.extend_from_slice(&buffer[..count]);
        }
        if line.contains(&0) {
            return Err("Browser IPC response contains NUL".into());
        }
        let reply: Value =
            serde_json::from_slice(&line).map_err(|_| "Invalid browser IPC JSON".to_owned())?;
        if reply.get("ok").and_then(Value::as_bool) != Some(true) {
            return Err("Browser engine refused the requested action".into());
        }
        let result = reply
            .get("result")
            .filter(|value| value.is_object() || value.is_null())
            .ok_or("Browser engine response has no result")?;
        Ok(result.clone())
    }
}

impl BrowserRpc for NativeBrowser {
    fn request(&mut self, command: &str, argument: Option<&str>) -> Result<Value, String> {
        if !Self::command_allowed(command, argument) {
            return Err("Unknown or invalid browser action".into());
        }
        let mut stream = self.connect()?;
        let value = if let Some(argument) = argument {
            json!({ "command": command, "argument": argument })
        } else {
            json!({ "command": command })
        };
        let mut frame =
            serde_json::to_vec(&value).map_err(|_| "Cannot serialize browser action")?;
        frame.push(b'\n');
        if frame.len() >= MAX_FRAME {
            return Err("Browser IPC request too long".into());
        }
        stream
            .write_all(&frame)
            .map_err(|_| "Browser IPC write failed".to_owned())?;
        Self::read_frame(&mut stream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::thread;

    fn private_dir() -> PathBuf {
        let path = env::temp_dir().join(format!(
            "nuraloumi-browser-ipc-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    #[test]
    fn client_validates_private_socket_and_decodes_typed_response() {
        let dir = private_dir();
        let listener = UnixListener::bind(dir.join(SOCKET_NAME)).unwrap();
        fs::set_permissions(dir.join(SOCKET_NAME), fs::Permissions::from_mode(0o600)).unwrap();
        let thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut frame = [0_u8; 512];
            let length = stream.read(&mut frame).unwrap();
            let value: Value = serde_json::from_slice(&frame[..length]).unwrap();
            assert_eq!(value["command"], "tabs");
            stream
                .write_all(b"{\"ok\":true,\"result\":{\"active\":0,\"tabs\":[{}]}}\n")
                .unwrap();
        });
        let mut client = NativeBrowser::at_runtime_dir(&dir).unwrap();
        let value = client.request("tabs", None).unwrap();
        assert_eq!(value["active"], 0);
        assert!(client.request("execute", None).is_err());
        thread.join().unwrap();
        fs::remove_file(dir.join(SOCKET_NAME)).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn insecure_runtime_mode_or_socket_symlink_fail_closed() {
        let dir = private_dir();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(NativeBrowser::at_runtime_dir(&dir).is_err());
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        let target = dir.join("target");
        fs::write(&target, b"not a socket").unwrap();
        std::os::unix::fs::symlink(&target, dir.join(SOCKET_NAME)).unwrap();
        assert!(NativeBrowser::at_runtime_dir(&dir).is_err());
        fs::remove_file(dir.join(SOCKET_NAME)).unwrap();
        fs::remove_file(target).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
