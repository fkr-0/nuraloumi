//! Private, same-user transport for the player's exported XR24 DMA-BUFs.
use std::{
    fs::{self, File, OpenOptions},
    io,
    mem::{size_of, size_of_val, zeroed},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        unix::{
            ffi::OsStrExt,
            fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        },
    },
    path::{Path, PathBuf},
};

const MAGIC: u32 = 0x4e565631;

pub struct Frame {
    pub fd: OwnedFd,
    pub sequence: u32,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
}

pub enum Packet {
    Frame(Frame),
    Detach,
}

pub struct Receiver {
    listener: OwnedFd,
    peer: Option<OwnedFd>,
    path: PathBuf,
    _lock: File,
}

fn result(value: libc::c_int) -> io::Result<libc::c_int> {
    if value < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(value)
    }
}

impl Receiver {
    pub fn bind(dir: &Path) -> io::Result<Self> {
        let uid = unsafe { libc::geteuid() };
        let dir_info = fs::symlink_metadata(dir)?;
        if !dir.is_absolute()
            || !dir_info.is_dir()
            || dir_info.uid() != uid
            || dir_info.mode() & 0o077 != 0
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "video needs a private runtime directory",
            ));
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(dir.join("nuraloumi-video.lock"))?;
        result(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) })?;
        let path = dir.join("nuraloumi-video.sock");
        let name = path.as_os_str().as_bytes();
        let mut addr: libc::sockaddr_un = unsafe { zeroed() };
        if name.len() >= addr.sun_path.len() || name.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "video socket path too long",
            ));
        }
        if let Ok(old) = fs::symlink_metadata(&path) {
            if !old.file_type().is_socket() || old.uid() != uid {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "unsafe video socket path",
                ));
            }
            fs::remove_file(&path)?;
        }
        addr.sun_family = libc::AF_UNIX as _;
        for (slot, byte) in addr.sun_path.iter_mut().zip(name) {
            *slot = *byte as _;
        }
        let listener = unsafe {
            OwnedFd::from_raw_fd(result(libc::socket(
                libc::AF_UNIX,
                libc::SOCK_SEQPACKET | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
                0,
            ))?)
        };
        result(unsafe {
            libc::bind(
                listener.as_raw_fd(),
                (&raw const addr).cast(),
                size_of::<libc::sockaddr_un>() as _,
            )
        })?;
        let receiver = Self {
            listener,
            peer: None,
            path,
            _lock: lock,
        };
        fs::set_permissions(&receiver.path, fs::Permissions::from_mode(0o600))?;
        result(unsafe { libc::listen(receiver.listener_fd(), 1) })?;
        Ok(receiver)
    }

    pub fn listener_fd(&self) -> RawFd {
        self.listener.as_raw_fd()
    }
    pub fn peer_fd(&self) -> RawFd {
        self.peer.as_ref().map_or(-1, AsRawFd::as_raw_fd)
    }
    pub fn disconnect(&mut self) {
        self.peer = None;
    }

    pub fn accept(&mut self) -> io::Result<()> {
        let fd = unsafe {
            OwnedFd::from_raw_fd(result(libc::accept4(
                self.listener_fd(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
            ))?)
        };
        let mut peer: libc::ucred = unsafe { zeroed() };
        let mut len = size_of::<libc::ucred>() as libc::socklen_t;
        result(unsafe {
            libc::getsockopt(
                fd.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&raw mut peer).cast(),
                &mut len,
            )
        })?;
        if len as usize == size_of::<libc::ucred>()
            && peer.uid == unsafe { libc::geteuid() }
            && self.peer.is_none()
        {
            self.peer = Some(fd);
        }
        Ok(())
    }

    pub fn read(&self) -> io::Result<Option<Packet>> {
        read_packet(self.peer_fd())
    }

    pub fn acknowledge(&self, kind: u32, seq: u32) -> io::Result<()> {
        let data = [kind.to_le_bytes(), seq.to_le_bytes()];
        let len = size_of_val(&data);
        let written = unsafe {
            libc::send(
                self.peer_fd(),
                data.as_ptr().cast(),
                len,
                libc::MSG_NOSIGNAL,
            )
        };
        if written < 0 {
            return Err(io::Error::last_os_error());
        }
        if written as usize != len {
            return Err(io::Error::from(io::ErrorKind::WriteZero));
        }
        Ok(())
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[allow(clippy::unnecessary_cast)] // Linux musl and glibc use different ancillary length types.
fn read_packet(peer: RawFd) -> io::Result<Option<Packet>> {
    let mut words = [0u32; 5];
    let mut control = [0usize; 8];
    let mut iov = libc::iovec {
        iov_base: words.as_mut_ptr().cast(),
        iov_len: size_of_val(&words),
    };
    let mut msg: libc::msghdr = unsafe { zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = size_of_val(&control) as _;
    let len = unsafe { libc::recvmsg(peer, &mut msg, libc::MSG_CMSG_CLOEXEC) };
    if len < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut fds = Vec::new();
    unsafe {
        let mut cmsg = libc::CMSG_FIRSTHDR(&msg);
        while !cmsg.is_null() {
            if (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
                let bytes = (*cmsg).cmsg_len.saturating_sub(libc::CMSG_LEN(0) as _) as usize;
                let data = libc::CMSG_DATA(cmsg).cast::<RawFd>();
                for i in 0..bytes / size_of::<RawFd>() {
                    fds.push(OwnedFd::from_raw_fd(data.add(i).read_unaligned()));
                }
            }
            cmsg = libc::CMSG_NXTHDR(&msg, cmsg);
        }
    }
    if len == 0 && fds.is_empty() && msg.msg_flags & libc::MSG_CTRUNC == 0 {
        return Ok(None);
    }
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid player DMA-BUF packet");
    if len as usize != size_of_val(&words)
        || msg.msg_flags & (libc::MSG_TRUNC | libc::MSG_CTRUNC) != 0
    {
        return Err(invalid());
    }
    let [magic, sequence, width, height, stride] = words.map(u32::from_le);
    if magic != MAGIC {
        return Err(invalid());
    }
    if [sequence, width, height, stride] == [0; 4] && fds.is_empty() {
        return Ok(Some(Packet::Detach));
    }
    if fds.len() != 1
        || width == 0
        || height == 0
        || width > 4096
        || height > 4096
        || stride < width * 4
        || stride > 32768
    {
        return Err(invalid());
    }
    Ok(Some(Packet::Frame(Frame {
        fd: fds.pop().ok_or_else(invalid)?,
        sequence,
        width,
        height,
        stride,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn received_dma_buf_is_owned_cloexec_and_detach_has_no_fd() {
        let mut descriptors = [-1; 2];
        assert_eq!(
            unsafe {
                libc::socketpair(
                    libc::AF_UNIX,
                    libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
                    0,
                    descriptors.as_mut_ptr(),
                )
            },
            0
        );
        let send = unsafe { OwnedFd::from_raw_fd(descriptors[0]) };
        let recv = unsafe { OwnedFd::from_raw_fd(descriptors[1]) };
        let buf = File::open("/dev/null").unwrap();
        let mut words = [MAGIC, 7, 854, 480, 3584].map(u32::to_le);
        let mut control = [0usize; 8];
        let mut iov = libc::iovec {
            iov_base: words.as_mut_ptr().cast(),
            iov_len: size_of_val(&words),
        };
        let mut msg: libc::msghdr = unsafe { zeroed() };
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = control.as_mut_ptr().cast();
        msg.msg_controllen = unsafe { libc::CMSG_SPACE(size_of::<RawFd>() as _) } as _;
        unsafe {
            let cmsg = libc::CMSG_FIRSTHDR(&msg);
            (*cmsg).cmsg_level = libc::SOL_SOCKET;
            (*cmsg).cmsg_type = libc::SCM_RIGHTS;
            (*cmsg).cmsg_len = libc::CMSG_LEN(size_of::<RawFd>() as _) as _;
            libc::CMSG_DATA(cmsg)
                .cast::<RawFd>()
                .write_unaligned(buf.as_raw_fd());
            assert_eq!(libc::sendmsg(send.as_raw_fd(), &msg, 0), 20);
        }
        let Some(Packet::Frame(frame)) = read_packet(recv.as_raw_fd()).unwrap() else {
            panic!("expected a frame");
        };
        assert_eq!(
            (frame.sequence, frame.width, frame.height, frame.stride),
            (7, 854, 480, 3584)
        );
        assert_ne!(
            unsafe { libc::fcntl(frame.fd.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
            0
        );
        assert_eq!(
            unsafe {
                libc::send(
                    send.as_raw_fd(),
                    words.as_ptr().cast(),
                    size_of_val(&words),
                    0,
                )
            },
            20
        );
        assert_eq!(
            read_packet(recv.as_raw_fd()).err().unwrap().kind(),
            io::ErrorKind::InvalidData
        );
        let detach = [MAGIC.to_le(), 0, 0, 0, 0];
        assert_eq!(
            unsafe {
                libc::send(
                    send.as_raw_fd(),
                    detach.as_ptr().cast(),
                    size_of_val(&detach),
                    0,
                )
            },
            20
        );
        assert!(matches!(
            read_packet(recv.as_raw_fd()).unwrap(),
            Some(Packet::Detach)
        ));
    }
}
