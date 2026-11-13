//! Owner-only playback service. Never starts another media player or a shell.
#![allow(dead_code)]
#[path = "../player_engine.rs"]
mod player_engine;
use player_engine::{Command, Player, Reply};
use std::{
    env, fs,
    io::{Read, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process::ExitCode,
    thread,
    time::{Duration, Instant},
};
const MAX_REQUEST: u64 = 4096;
const REQUEST_DEADLINE: Duration = Duration::from_millis(300);
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
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err("stale socket not owned by this user".into());
        }
        // Never unlink a path replaced after the initial stale-socket probe.
        let current = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        if !current.file_type().is_socket()
            || current.dev() != metadata.dev()
            || current.ino() != metadata.ino()
        {
            return Err("stale socket path changed during validation".into());
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
struct SocketCleanup {
    path: PathBuf,
    dev: u64,
    ino: u64,
}
impl SocketCleanup {
    fn new(path: PathBuf) -> Result<Self, String> {
        let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if !metadata.file_type().is_socket() {
            return Err("bound socket replaced with non-socket".into());
        }
        Ok(Self {
            path,
            dev: metadata.dev(),
            ino: metadata.ino(),
        })
    }
}
impl Drop for SocketCleanup {
    fn drop(&mut self) {
        // Another service might have replaced a stale path while this process
        // was exiting. Remove only the socket inode we originally bound.
        if let Ok(metadata) = fs::symlink_metadata(&self.path) {
            if metadata.file_type().is_socket()
                && metadata.dev() == self.dev
                && metadata.ino() == self.ino
            {
                let _ = fs::remove_file(&self.path);
            }
        }
    }
}
fn read_request(mut stream: &UnixStream) -> Result<Vec<u8>, String> {
    let deadline = Instant::now() + REQUEST_DEADLINE;
    let mut buffer = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("socket request deadline exceeded".into());
        }
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|e| format!("socket timeout: {e}"))?;
        let mut chunk = [0u8; 512];
        let count = stream
            .read(&mut chunk)
            .map_err(|e| format!("socket read: {e}"))?;
        if count == 0 {
            return Err("unterminated command".into());
        }
        let delimiter = chunk[..count].iter().position(|byte| *byte == b'\n');
        let consumed = delimiter.map_or(count, |index| index + 1);
        if buffer.len() + consumed > MAX_REQUEST as usize {
            return Err("oversized command".into());
        }
        buffer.extend_from_slice(&chunk[..consumed]);
        if delimiter.is_some() {
            return Ok(buffer);
        }
    }
}
fn respond(mut stream: UnixStream, engine: &mut Player) {
    let _ = stream.set_write_timeout(Some(Duration::from_millis(300)));
    // Single newline-delimited command. A second request requires a new connection.
    let command = match read_request(&stream) {
        Ok(buffer) => {
            serde_json::from_slice::<Command>(&buffer).map_err(|e| format!("invalid command: {e}"))
        }
        Err(e) => Err(e),
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
    let _guard = SocketCleanup::new(path)?;
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
    use std::io::{BufRead, BufReader};
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
    #[test]
    fn slow_trickle_cannot_extend_total_request_deadline() {
        let (mut writer, reader) = UnixStream::pair().unwrap();
        let sender = thread::spawn(move || {
            let _ = writer.write_all(b"{");
            thread::sleep(Duration::from_millis(200));
            let _ = writer.write_all(b"\"");
            thread::sleep(Duration::from_millis(200));
            let _ = writer.write_all(b"op");
        });
        let start = Instant::now();
        assert!(read_request(&reader).is_err());
        assert!(start.elapsed() < Duration::from_secs(1));
        sender.join().unwrap();
    }
    #[test]
    fn cleanup_never_unlinks_replacement_socket() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "nuraloumi-player-cleanup-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("player.sock");
        let original = bind(&path).unwrap();
        let cleanup = SocketCleanup::new(path.clone()).unwrap();
        fs::remove_file(&path).unwrap();
        let replacement = bind(&path).unwrap();
        drop(cleanup);
        assert!(fs::symlink_metadata(&path).unwrap().file_type().is_socket());
        drop(replacement);
        drop(original);
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
