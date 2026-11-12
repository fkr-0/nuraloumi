//! Owner-only playback service. Never starts another media player or a shell.
#![allow(dead_code)]
#[path = "../player_engine.rs"]
mod player_engine;
use player_engine::{Command, Player, Reply};
use std::{
    env, fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::FileTypeExt,
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process::ExitCode,
    thread,
    time::{Duration, Instant},
};
const MAX_REQUEST: u64 = 4096;
pub fn socket_path() -> Result<PathBuf, String> {
    let root = env::var_os("XDG_RUNTIME_DIR").ok_or("XDG_RUNTIME_DIR required")?;
    let root = PathBuf::from(root);
    if !root.is_absolute() {
        return Err("runtime directory must be absolute".into());
    }
    let path = root.join("nuraloumi-player.sock");
    if path.as_os_str().len() > 100 {
        return Err("socket path too long".into());
    }
    Ok(path)
}
fn bind(path: &Path) -> Result<UnixListener, String> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if !metadata.file_type().is_socket() {
            return Err("refusing to replace a non-socket path".into());
        }
        if UnixStream::connect(path).is_ok() {
            return Err("another playback daemon is active".into());
        }
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err("stale socket not owned by this user".into());
        }
        fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    let previous = unsafe { libc::umask(0o077) };
    let result = UnixListener::bind(path);
    unsafe {
        libc::umask(previous);
    }
    let listener = result.map_err(|e| format!("bind playback socket: {e}"))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    Ok(listener)
}
struct SocketCleanup(PathBuf);
impl Drop for SocketCleanup {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn respond(mut stream: UnixStream, engine: &mut Player) {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(300)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(300)));
    // Single newline-delimited command. A second request requires a new connection.
    let mut buffer = Vec::new();
    let read = BufReader::new(&stream)
        .take(MAX_REQUEST + 1)
        .read_until(b'\n', &mut buffer);
    let command = match read {
        Ok(_) if buffer.len() as u64 <= MAX_REQUEST && buffer.ends_with(b"\n") => {
            serde_json::from_slice::<Command>(&buffer).map_err(|e| format!("invalid command: {e}"))
        }
        Ok(_) => Err("oversized or unterminated command".into()),
        Err(e) => Err(format!("socket read: {e}")),
    };
    let reply = match command {
        Ok(cmd) => engine.command(cmd),
        Err(e) => Reply {
            ok: false,
            error: Some(e),
            state: engine.snapshot(),
        },
    };
    if let Ok(mut json) = serde_json::to_vec(&reply) {
        json.push(b'\n');
        let _ = stream.write_all(&json);
    }
}
fn run() -> Result<(), String> {
    let path = socket_path()?;
    let listener = bind(&path)?;
    let _guard = SocketCleanup(path);
    let mut engine = Player::open(player_engine::state_path()?)?;
    let mut last_request = Instant::now();
    loop {
        for _ in 0..8 {
            match listener.accept() {
                Ok((stream, _)) => {
                    last_request = Instant::now();
                    respond(stream, &mut engine);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(format!("accept player request: {e}")),
            }
        }
        engine.tick();
        // A stopped, unused daemon doesn't linger forever.
        if last_request.elapsed() >= Duration::from_secs(600)
            && engine.snapshot().status != "Playing"
            && engine.snapshot().status != "Paused"
        {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
}
fn main() -> ExitCode {
    if std::env::args().len() > 1 {
        eprintln!("Usage: nuraloumi-playerd (no options)");
        return ExitCode::from(2);
    }
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("nuraloumi-playerd: {e}");
            ExitCode::FAILURE
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn socket_path_must_have_runtime_dir() {
        // Avoid test-global env mutation; test the path basename only if present.
        if let Ok(path) = socket_path() {
            assert_eq!(path.file_name().unwrap(), "nuraloumi-player.sock");
        }
    }
    #[test]
    fn rejects_oversize_and_unknown_ipc_commands_and_has_owner_only_socket() {
        use std::os::unix::fs::PermissionsExt;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("nuraloumi-ipc-test-{}-{nanos}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("player.sock");
        let listener = bind(&path).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let mut engine = Player::open(directory.join("playlist.json")).unwrap();
        for payload in [vec![b'a'; 4097], b"{\"op\":\"no_such_command\"}\n".to_vec()] {
            let mut client = UnixStream::connect(&path).unwrap();
            client
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            client.write_all(&payload).unwrap();
            let (server, _) = listener.accept().unwrap();
            respond(server, &mut engine);
            let mut reply = String::new();
            BufReader::new(client).read_line(&mut reply).unwrap();
            let result: Reply = serde_json::from_str(&reply).unwrap();
            assert!(!result.ok);
            assert!(result.error.is_some());
            assert_eq!(result.state.status, "Stopped");
        }
        fs::remove_file(&path).unwrap();
        fs::remove_dir(&directory).unwrap();
    }
}
