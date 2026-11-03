//! Unix-domain, line-delimited JSON IPC transport.
//! This module is intentionally standalone until the surface-binary integration lane.
//! Poll accept_one without blocking; call respond after application dispatch.
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const MAX_REQUEST_BYTES: usize = 4096;
pub const MAX_PUSHED_ITEMS: usize = 64;
pub const DEFAULT_TTL_SECONDS: u64 = 300;
pub const MAX_TTL_SECONDS: u64 = 86_400;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IpcRequest {
    pub id: String,
    #[serde(flatten)]
    pub command: IpcCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "method", content = "params", rename_all = "kebab-case")]
pub enum IpcCommand {
    PushItem(PushItem),
    SetStatus(SetStatus),
    Notify(Notify),
    SurfaceState(SurfaceName),
    Navigate(Navigate),
    ReloadConfig(Empty),
    ListSurfaces(Empty),
    Ping(Empty),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Empty {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PushItem {
    pub surface: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glyph: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(default = "default_ttl")]
    pub ttl_seconds: u64,
}
const fn default_ttl() -> u64 {
    DEFAULT_TTL_SECONDS
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SetStatus {
    pub slot: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glyph: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Notify {
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SurfaceName {
    pub surface: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum NavigationAction {
    Open,
    Close,
    Up,
    Down,
    Select,
    Back,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Navigate {
    pub action: NavigationAction,
}

/// Whether this panel actually knows a surface's open/closed state.
///
/// An installed standalone executable is discoverable but not observed:
/// its state is Unknown, not Closed. This is independent of installation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SurfaceOpenState {
    Open,
    Closed,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SurfaceState {
    /// Legacy compatibility flag. Only authoritative if observation_supported
    /// is Some(true); older peers may omit the new fields altogether.
    pub open: bool,
    /// None means an older server did not provide explicit state semantics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_state: Option<SurfaceOpenState>,
    /// Whether this panel can actually observe this surface's running state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_supported: Option<bool>,
    pub selected_item: Option<String>,
    pub item_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SurfaceInfo {
    pub name: String,
    /// Legacy compatibility flag; false is not proof of closure when
    /// observation_supported is Some(false).
    pub open: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_state: Option<SurfaceOpenState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_supported: Option<bool>,
    pub pid: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PingResult {
    pub version: String,
    pub uptime_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IpcError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IpcResponse {
    pub id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<IpcError>,
}
impl IpcResponse {
    pub fn success(id: impl Into<String>, result: impl Serialize) -> Self {
        Self {
            id: id.into(),
            ok: true,
            result: Some(serde_json::to_value(result).unwrap_or(serde_json::Value::Null)),
            error: None,
        }
    }
    pub fn failure(
        id: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            ok: false,
            result: None,
            error: Some(IpcError {
                code: code.into(),
                message: message.into(),
            }),
        }
    }
}

/// Use an explicitly owned user runtime directory or a UID-scoped /tmp path.
pub fn default_socket_path() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        if !dir.is_empty() {
            let path = PathBuf::from(dir);
            if path.is_absolute() {
                return path.join("nuraloumi.sock");
            }
        }
    }
    // getuid is provided by the POSIX procfs status on Linux, no libc dependency.
    let uid = fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Uid:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .map(str::to_owned)
        })
        .unwrap_or_else(|| std::env::var("UID").unwrap_or_else(|_| "unknown".into()));
    PathBuf::from(format!("/tmp/nuraloumi-{uid}.sock"))
}

pub fn parse_request(line: &[u8]) -> Result<IpcRequest, IpcError> {
    if line.len() > MAX_REQUEST_BYTES {
        return Err(IpcError {
            code: "request_too_large".into(),
            message: "request exceeds 4096 bytes".into(),
        });
    }
    let req: IpcRequest = serde_json::from_slice(line).map_err(|e| IpcError {
        code: "invalid_request".into(),
        message: e.to_string(),
    })?;
    if req.id.is_empty() || req.id.len() > 128 || req.id.contains(['\r', '\n', '\0']) {
        return Err(IpcError {
            code: "invalid_request".into(),
            message: "invalid request id".into(),
        });
    }
    let valid = |s: &str| !s.trim().is_empty() && !s.contains(['\0', '\r', '\n']);
    let good = match &req.command {
        IpcCommand::PushItem(p) => {
            valid(&p.surface) && valid(&p.label) && (1..=MAX_TTL_SECONDS).contains(&p.ttl_seconds)
        }
        IpcCommand::SetStatus(p) => valid(&p.slot) && valid(&p.text),
        IpcCommand::Notify(p) => valid(&p.summary),
        IpcCommand::SurfaceState(p) => valid(&p.surface),
        _ => true,
    };
    if !good {
        return Err(IpcError {
            code: "invalid_params".into(),
            message: "required values must be nonempty; ttl_seconds must be 1..=86400".into(),
        });
    }
    Ok(req)
}

struct Pending {
    stream: UnixStream,
    bytes: Vec<u8>,
    since: Instant,
}

/// One client is active at a time. Nonblocking reads can span event-loop polls.
pub struct IpcServer {
    listener: UnixListener,
    path: PathBuf,
    socket_identity: (u64, u64),
    pending: Option<Pending>,
    responding: Option<UnixStream>,
}
impl IpcServer {
    pub fn bind(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        // Refuse to remove an existing file, including someone else's socket.
        let listener = UnixListener::bind(path)?;
        if let Err(err) = fs::set_permissions(path, fs::Permissions::from_mode(0o600)) {
            drop(listener);
            let _ = fs::remove_file(path);
            return Err(err);
        }
        listener.set_nonblocking(true)?;
        let metadata = fs::symlink_metadata(path)?;
        let socket_identity = (metadata.dev(), metadata.ino());
        Ok(Self {
            listener,
            path: path.into(),
            socket_identity,
            pending: None,
            responding: None,
        })
    }

    pub fn accept_one(&mut self) -> io::Result<Option<IpcRequest>> {
        if self.responding.is_some() {
            return Ok(None);
        }
        if self.pending.is_none() {
            let (stream, _) = match self.listener.accept() {
                Ok(pair) => pair,
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => return Ok(None),
                Err(err) => return Err(err),
            };
            stream.set_nonblocking(true)?;
            self.pending = Some(Pending {
                stream,
                bytes: Vec::new(),
                since: Instant::now(),
            });
        }
        let pending = self.pending.as_mut().expect("pending initialized");
        if pending.since.elapsed() >= REQUEST_TIMEOUT {
            self.pending = None;
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "request read timed out",
            ));
        }
        let mut chunk = [0u8; 512];
        loop {
            match pending.stream.read(&mut chunk) {
                Ok(0) => {
                    self.pending = None;
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "connection closed before newline",
                    ));
                }
                Ok(n) => {
                    pending.bytes.extend_from_slice(&chunk[..n]);
                    if let Some(pos) = pending.bytes.iter().position(|&b| b == b'\n') {
                        if pos > MAX_REQUEST_BYTES {
                            self.pending = None;
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "request too large",
                            ));
                        }
                        let line = pending.bytes[..pos].to_vec();
                        let parsed = parse_request(&line).map_err(|e| {
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                format!("{}: {}", e.code, e.message),
                            )
                        });
                        let pending = self.pending.take().expect("pending initialized");
                        // Invalid requests must not hold the response slot forever.
                        if parsed.is_ok() {
                            self.responding = Some(pending.stream);
                        }
                        return parsed.map(Some);
                    }
                    if pending.bytes.len() > MAX_REQUEST_BYTES {
                        self.pending = None;
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "request too large",
                        ));
                    }
                }
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => return Ok(None),
                Err(err) => {
                    self.pending = None;
                    return Err(err);
                }
            }
        }
    }

    pub fn respond(&mut self, response: &IpcResponse) -> io::Result<()> {
        let mut stream = self
            .responding
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "no accepted request"))?;
        let mut bytes = serde_json::to_vec(response)?;
        bytes.push(b'\n');
        // Never wait for an unresponsive reader on the rendering thread.
        stream.write_all(&bytes)
    }

    pub fn cleanup(&mut self) -> io::Result<()> {
        self.pending = None;
        self.responding = None;
        // Never unlink a socket replaced after bind by another process.
        match self.path.symlink_metadata() {
            Ok(metadata)
                if metadata.file_type().is_socket()
                    && (metadata.dev(), metadata.ino()) == self.socket_identity =>
            {
                fs::remove_file(&self.path)?;
            }
            Ok(_) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
        Ok(())
    }
}

// Cleanup also runs when the IPC worker exits unexpectedly or thread creation
// fails after socket bind. The inode guard in cleanup prevents removing a
// replacement socket installed by another process.
impl Drop for IpcServer {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

// Linux AF_UNIX connect can block if the listener backlog is full. Create the
// socket nonblocking *before* connect, then poll all I/O against one deadline.
// No additional Rust dependency or thread that could outlive a timed-out call.
mod client_socket {
    use super::*;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;

    const AF_UNIX: i32 = 1;
    const SOCK_STREAM: i32 = 1;
    const SOCK_NONBLOCK: i32 = 0x800;
    const SOCK_CLOEXEC: i32 = 0x80000;
    const EINTR: i32 = 4;
    const EAGAIN: i32 = 11;
    const EINPROGRESS: i32 = 115;
    const SOL_SOCKET: i32 = 1;
    const SO_ERROR: i32 = 4;
    pub const READ: i16 = 0x001;
    pub const WRITE: i16 = 0x004;

    #[repr(C)]
    struct SockAddrUn {
        family: u16,
        path: [std::ffi::c_char; 108],
    }

    #[repr(C)]
    struct PollFd {
        fd: i32,
        events: i16,
        revents: i16,
    }

    unsafe extern "C" {
        fn socket(domain: i32, kind: i32, protocol: i32) -> i32;
        fn connect(fd: i32, addr: *const SockAddrUn, len: u32) -> i32;
        fn poll(fds: *mut PollFd, nfds: usize, timeout: i32) -> i32;
        fn getsockopt(fd: i32, level: i32, name: i32, value: *mut i32, len: *mut u32) -> i32;
    }

    pub fn wait(stream: &UnixStream, events: i16, deadline: Instant) -> io::Result<()> {
        wait_fd(stream.as_raw_fd(), events, deadline)
    }

    fn wait_fd(fd: i32, events: i16, deadline: Instant) -> io::Result<()> {
        loop {
            let left = remaining(deadline)?;
            // poll's timeout is in whole milliseconds; round up and re-check
            // Instant on return, even if the descriptor is continuously ready.
            let ms = left.as_millis().saturating_add(1).min(i32::MAX as u128) as i32;
            let mut pfd = PollFd {
                fd,
                events,
                revents: 0,
            };
            // SAFETY: pfd is initialized and lives for the whole poll call.
            let result = unsafe { poll(&mut pfd, 1, ms) };
            match result {
                n if n > 0 => return remaining(deadline).map(|_| ()),
                0 => continue,
                _ if io::Error::last_os_error().raw_os_error() == Some(EINTR) => continue,
                _ => return Err(io::Error::last_os_error()),
            }
        }
    }

    pub fn connect_until(path: &Path, deadline: Instant) -> io::Result<UnixStream> {
        let name = path.as_os_str().as_bytes();
        if name.is_empty() || name.len() >= 108 || name.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid IPC socket path",
            ));
        }
        remaining(deadline)?;
        // SOCK_NONBLOCK|SOCK_CLOEXEC are Linux flags on both ARMv7 and x86_64.
        // SAFETY: a new AF_UNIX socket is owned by the resulting OwnedFd.
        let raw = unsafe { socket(AF_UNIX, SOCK_STREAM | SOCK_NONBLOCK | SOCK_CLOEXEC, 0) };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: raw was returned as an owned descriptor by socket above.
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let mut addr = SockAddrUn {
            family: AF_UNIX as u16,
            path: [0; 108],
        };
        for (slot, byte) in addr.path.iter_mut().zip(name) {
            *slot = *byte as std::ffi::c_char;
        }
        let addr_len = (std::mem::size_of::<u16>() + name.len() + 1) as u32;
        loop {
            remaining(deadline)?;
            // SAFETY: addr is a valid pathname sockaddr_un, including its NUL.
            let result = unsafe { connect(fd.as_raw_fd(), &addr, addr_len) };
            if result == 0 {
                return Ok(fd.into());
            }
            let err = io::Error::last_os_error();
            match err.raw_os_error() {
                Some(EAGAIN) => {
                    // Linux can report EAGAIN if the Unix listener backlog is
                    // full. Poll, then use a tiny capped backoff if the socket
                    // stays writable but the server queue cannot accept us.
                    wait_fd(fd.as_raw_fd(), WRITE, deadline)?;
                    std::thread::sleep(remaining(deadline)?.min(Duration::from_millis(2)));
                }
                Some(EINTR) | Some(EINPROGRESS) => {
                    wait_fd(fd.as_raw_fd(), WRITE, deadline)?;
                    let mut socket_error = 0i32;
                    let mut len = std::mem::size_of::<i32>() as u32;
                    // SAFETY: the output pointer and its length are valid.
                    if unsafe {
                        getsockopt(
                            fd.as_raw_fd(),
                            SOL_SOCKET,
                            SO_ERROR,
                            &mut socket_error,
                            &mut len,
                        )
                    } < 0
                    {
                        return Err(io::Error::last_os_error());
                    }
                    if socket_error != 0 {
                        return Err(io::Error::from_raw_os_error(socket_error));
                    }
                    return remaining(deadline).map(|_| fd.into());
                }
                _ => return Err(err),
            }
        }
    }
}

fn deadline_expired() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "IPC roundtrip deadline exceeded")
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        Err(deadline_expired())
    } else {
        Ok(left)
    }
}

/// Client-side single round-trip. A monotonic deadline covers connect,
/// request writes, response reads, decoding and response-id validation.
pub fn request(path: &Path, req: &IpcRequest) -> io::Result<IpcResponse> {
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    let mut bytes = serde_json::to_vec(req)?;
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "request too large",
        ));
    }
    bytes.push(b'\n');
    let mut stream = client_socket::connect_until(path, deadline)?;
    let mut pending = bytes.as_slice();
    while !pending.is_empty() {
        remaining(deadline)?;
        match stream.write(pending) {
            Ok(0) => return Err(io::Error::from(io::ErrorKind::WriteZero)),
            Ok(n) => pending = &pending[n..],
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                client_socket::wait(&stream, client_socket::WRITE, deadline)?;
            }
            Err(e) => return Err(e),
        }
    }

    let mut response = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        remaining(deadline)?;
        match stream.read(&mut chunk) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "no complete response",
                ))
            }
            Ok(n) => {
                if let Some(end) = chunk[..n].iter().position(|&b| b == b'\n') {
                    if response.len() + end > 65536 {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "response too large",
                        ));
                    }
                    response.extend_from_slice(&chunk[..end]);
                    break;
                }
                if response.len() + n > 65536 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "response too large",
                    ));
                }
                response.extend_from_slice(&chunk[..n]);
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                client_socket::wait(&stream, client_socket::READ, deadline)?;
            }
            Err(e) => return Err(e),
        }
    }
    remaining(deadline)?;
    let parsed: IpcResponse = serde_json::from_slice(&response)?;
    if parsed.id != req.id {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "response id mismatch",
        ));
    }
    remaining(deadline)?;
    Ok(parsed)
}

/// Standalone TTL collection for later surface integration. Capacity is global
/// across all surfaces, not per arbitrary user-provided surface identifier.
#[derive(Default)]
pub struct PushedItems {
    items: VecDeque<(Instant, PushItem)>,
}
impl PushedItems {
    pub fn push(&mut self, item: PushItem) {
        let now = Instant::now();
        self.items.retain(|(expires, _)| *expires > now);
        while self.items.len() >= MAX_PUSHED_ITEMS {
            self.items.pop_front();
        }
        // Clamp even for direct callers that bypass parse_request.
        let expires = now + Duration::from_secs(item.ttl_seconds.min(MAX_TTL_SECONDS));
        self.items.push_back((expires, item));
    }
    pub fn items(&mut self, surface: &str) -> Vec<PushItem> {
        let now = Instant::now();
        self.items.retain(|(expires, _)| *expires > now);
        self.items
            .iter()
            .filter(|(_, item)| item.surface == surface)
            .map(|(_, item)| item.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_methods_roundtrip() {
        let methods = vec![
            IpcCommand::PushItem(PushItem {
                surface: "panel".into(),
                label: "VPN".into(),
                glyph: None,
                subtitle: None,
                ttl_seconds: 300,
            }),
            IpcCommand::SetStatus(SetStatus {
                slot: "vpn".into(),
                text: "Connected".into(),
                glyph: None,
            }),
            IpcCommand::Notify(Notify {
                summary: "Build passed".into(),
                body: None,
                app: None,
            }),
            IpcCommand::SurfaceState(SurfaceName {
                surface: "launcher".into(),
            }),
            IpcCommand::Navigate(Navigate {
                action: NavigationAction::Close,
            }),
            IpcCommand::ReloadConfig(Empty {}),
            IpcCommand::ListSurfaces(Empty {}),
            IpcCommand::Ping(Empty {}),
        ];
        for (n, command) in methods.into_iter().enumerate() {
            let request = IpcRequest {
                id: format!("req-{n}"),
                command,
            };
            let wire = serde_json::to_vec(&request).unwrap();
            assert_eq!(parse_request(&wire).unwrap(), request);
            let reply = IpcResponse::success(&request.id, serde_json::json!({"ok": true}));
            let decoded: IpcResponse =
                serde_json::from_slice(&serde_json::to_vec(&reply).unwrap()).unwrap();
            assert_eq!(reply, decoded);
        }
    }
    #[test]
    fn surface_wire_extends_old_contract_without_requiring_new_fields() {
        let old_info = r#"{"name":"calendar","open":false,"pid":0}"#;
        let parsed: SurfaceInfo = serde_json::from_str(old_info).unwrap();
        assert_eq!(parsed.open_state, None);
        assert_eq!(parsed.observation_supported, None);
        assert_eq!(serde_json::to_string(&parsed).unwrap(), old_info);

        let old_state = r#"{"open":true,"selected_item":null,"item_count":0}"#;
        let parsed: SurfaceState = serde_json::from_str(old_state).unwrap();
        assert_eq!(parsed.open_state, None);
        assert_eq!(parsed.observation_supported, None);
        assert_eq!(serde_json::to_string(&parsed).unwrap(), old_state);

        let modern = SurfaceState {
            open: false,
            open_state: Some(SurfaceOpenState::Unknown),
            observation_supported: Some(false),
            selected_item: None,
            item_count: 0,
        };
        let wire = serde_json::to_value(&modern).unwrap();
        assert_eq!(wire["open"], false);
        assert_eq!(wire["open_state"], "unknown");
        assert_eq!(wire["observation_supported"], false);
        assert_eq!(
            serde_json::from_value::<SurfaceState>(wire).unwrap(),
            modern
        );
    }

    #[test]
    fn bounds_and_evict() {
        assert!(parse_request(&vec![b'x'; 4097]).is_err());
        let mut items = PushedItems::default();
        for n in 0..66 {
            items.push(PushItem {
                surface: "panel".into(),
                label: n.to_string(),
                glyph: None,
                subtitle: None,
                ttl_seconds: 300,
            });
        }
        let entries = items.items("panel");
        assert_eq!(entries.len(), 64);
        assert_eq!(entries[0].label, "2");
    }
    #[test]
    fn oversized_ttl_is_rejected_and_cross_surface_capacity_is_global() {
        let wire = br#"{"id":"1","method":"push-item","params":{"surface":"panel","label":"x","ttl_seconds":18446744073709551615}}"#;
        assert_eq!(parse_request(wire).unwrap_err().code, "invalid_params");
        let mut items = PushedItems::default();
        for n in 0..80 {
            items.push(PushItem {
                surface: format!("surface-{n}"),
                label: "x".into(),
                glyph: None,
                subtitle: None,
                ttl_seconds: u64::MAX,
            });
        }
        assert!(items.items("surface-0").is_empty());
        assert_eq!(items.items("surface-79").len(), 1);
        assert_eq!(items.items.len(), MAX_PUSHED_ITEMS);
    }

    #[test]
    fn invalid_request_does_not_block_next_client() {
        let path =
            std::env::temp_dir().join(format!("nuraloumi-invalid-{}.sock", std::process::id()));
        let mut server = IpcServer::bind(&path).unwrap();
        let mut bad = UnixStream::connect(&path).unwrap();
        bad.write_all(b"{invalid-json}\n").unwrap();
        let started = Instant::now();
        loop {
            match server.accept_one() {
                Err(err) if err.kind() == io::ErrorKind::InvalidData => break,
                Ok(None) => {}
                other => panic!("unexpected malformed-request result: {other:?}"),
            }
            assert!(started.elapsed() < Duration::from_secs(2));
            std::thread::yield_now();
        }
        let good = UnixStream::connect(&path).unwrap();
        good.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let mut good = good;
        good.write_all(b"{\"id\":\"ok\",\"method\":\"ping\",\"params\":{}}\n")
            .unwrap();
        let started = Instant::now();
        loop {
            match server.accept_one().unwrap() {
                Some(req) => {
                    assert_eq!(req.id, "ok");
                    server.respond(&IpcResponse::success(req.id, true)).unwrap();
                    break;
                }
                None => {
                    assert!(started.elapsed() < Duration::from_secs(2));
                    std::thread::yield_now();
                }
            }
        }
        let mut result = String::new();
        good.read_to_string(&mut result).unwrap_or_default();
        assert!(result.contains("\"ok\":true"));
        server.cleanup().unwrap();
    }

    #[test]
    fn server_drop_cleans_only_its_own_socket() {
        let path = std::env::temp_dir().join(format!(
            "nuraloumi-drop-{}-{:?}.sock",
            std::process::id(),
            std::thread::current().id()
        ));
        {
            let _server = IpcServer::bind(&path).unwrap();
            assert!(path.exists());
        }
        assert!(!path.exists());
    }

    #[test]
    fn cleanup_preserves_replacement_socket() {
        let path =
            std::env::temp_dir().join(format!("nuraloumi-replace-{}.sock", std::process::id()));
        let mut server = IpcServer::bind(&path).unwrap();
        fs::remove_file(&path).unwrap();
        let replacement = UnixListener::bind(&path).unwrap();
        server.cleanup().unwrap();
        assert!(path.exists());
        drop(replacement);
        fs::remove_file(&path).unwrap();
    }

    // Each test owns a unique local socket; panics and early returns unlink it.
    struct TestSocket(PathBuf);
    impl Drop for TestSocket {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn slow_trickle_cannot_extend_client_roundtrip_deadline() {
        let socket = TestSocket(std::env::temp_dir().join(format!(
            "nuraloumi-deadline-{}-{:?}.sock",
            std::process::id(),
            std::thread::current().id()
        )));
        let listener = UnixListener::bind(&socket.0).unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let started = Instant::now();
            let mut peer = loop {
                match listener.accept() {
                    Ok((peer, _)) => break peer,
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        assert!(
                            started.elapsed() < Duration::from_secs(2),
                            "no client connected"
                        );
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(e) => panic!("accept failed: {e}"),
                }
            };
            peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut request_bytes = Vec::new();
            let mut buf = [0u8; 256];
            while !request_bytes.contains(&b'\n') {
                let n = peer.read(&mut buf).unwrap();
                assert!(n > 0, "client closed before sending request");
                request_bytes.extend_from_slice(&buf[..n]);
                assert!(request_bytes.len() <= MAX_REQUEST_BYTES + 1);
            }
            assert_eq!(
                parse_request(request_bytes.split(|b| *b == b'\n').next().unwrap())
                    .unwrap()
                    .id,
                "slow"
            );

            // The old per-read one-second timeout accepts this complete
            // response after >1 second because each byte arrives <1 second
            // after the previous one. The new overall deadline cannot.
            for byte in b"{\"id\":\"slow\",\"ok\":true}\n" {
                if peer.write_all(&[*byte]).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(70));
            }
        });
        let started = Instant::now();
        let result = request(
            &socket.0,
            &IpcRequest {
                id: "slow".into(),
                command: IpcCommand::Ping(Empty {}),
            },
        );
        let elapsed = started.elapsed();
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert!(
            elapsed >= Duration::from_millis(800),
            "premature timeout: {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_millis(1700),
            "deadline slipped: {elapsed:?}"
        );
        server.join().unwrap();
    }

    #[test]
    fn client_rejects_mismatched_response_id() {
        let socket = TestSocket(std::env::temp_dir().join(format!(
            "nuraloumi-response-id-{}-{:?}.sock",
            std::process::id(),
            std::thread::current().id()
        )));
        let listener = UnixListener::bind(&socket.0).unwrap();
        let server = std::thread::spawn(move || {
            listener.set_nonblocking(true).unwrap();
            let started = Instant::now();
            let mut peer = loop {
                match listener.accept() {
                    Ok((peer, _)) => break peer,
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        assert!(started.elapsed() < Duration::from_secs(2));
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(e) => panic!("accept failed: {e}"),
                }
            };
            peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut chunk = [0u8; 128];
            assert!(peer.read(&mut chunk).unwrap() > 0);
            peer.write_all(b"{\"id\":\"wrong\",\"ok\":true}\n").unwrap();
        });
        let err = request(
            &socket.0,
            &IpcRequest {
                id: "expected".into(),
                command: IpcCommand::Ping(Empty {}),
            },
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        server.join().unwrap();
    }

    #[test]
    fn socket_roundtrip_and_permission() {
        let path = std::env::temp_dir().join(format!(
            "nuraloumi-ipc-test-{}-{:?}.sock",
            std::process::id(),
            std::thread::current().id()
        ));
        let mut server = IpcServer::bind(&path).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let send_path = path.clone();
        let handle = std::thread::spawn(move || {
            request(
                &send_path,
                &IpcRequest {
                    id: "1".into(),
                    command: IpcCommand::Ping(Empty {}),
                },
            )
            .unwrap()
        });
        let started = Instant::now();
        loop {
            if let Some(req) = server.accept_one().unwrap() {
                server
                    .respond(&IpcResponse::success(
                        req.id,
                        PingResult {
                            version: "test".into(),
                            uptime_seconds: 1,
                        },
                    ))
                    .unwrap();
                break;
            }
            assert!(started.elapsed() < Duration::from_secs(2));
            std::thread::yield_now();
        }
        assert!(handle.join().unwrap().ok);
        server.cleanup().unwrap();
        assert!(!path.exists());
    }
}
