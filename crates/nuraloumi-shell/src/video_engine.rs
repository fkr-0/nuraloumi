//! In-process native video bridge for the single NuraLoumi player daemon.
//!
//! This module dynamically loads the *internal library*, never spawns a second
//! player or a shell. Cairo-only host builds need no EGL/Grate dependency.
//! Hardware with an incompatible or missing native library fails closed.
use std::{
    ffi::{c_char, c_int, c_void, CStr, CString},
    path::Path,
};

#[repr(C)]
#[derive(Clone, Copy)]
struct VideoStatus {
    state: i32,
    position_ms: u64,
    duration_ms: u64,
    frames: u64,
    error: [c_char; 192],
}

impl Default for VideoStatus {
    fn default() -> Self {
        Self {
            state: 0,
            position_ms: 0,
            duration_ms: 0,
            frames: 0,
            error: [0; 192],
        }
    }
}

type FnOpen = unsafe extern "C" fn(*const c_char, f64, c_int) -> *mut c_void;
type FnCommand = unsafe extern "C" fn(*mut c_void, c_int, i64) -> c_int;
type FnStatus = unsafe extern "C" fn(*mut c_void, *mut VideoStatus) -> c_int;
type FnClose = unsafe extern "C" fn(*mut c_void);

#[derive(Debug, PartialEq, Eq)]
pub enum VideoPoll {
    Starting,
    Playing { position_ms: u64, duration_ms: u64 },
    Paused { position_ms: u64, duration_ms: u64 },
    Eos,
}

pub struct VideoEngine {
    module: *mut c_void,
    session: *mut c_void,
    command: FnCommand,
    status: FnStatus,
    close: FnClose,
}

impl VideoEngine {
    pub fn open(path: &str, volume: f64, muted: bool) -> Result<Self, String> {
        if !Path::new(path).is_absolute() || path.contains('\0') {
            return Err("video requires a canonical absolute file path".into());
        }
        if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
            return Err("invalid video volume".into());
        }
        let path = CString::new(path).map_err(|e| e.to_string())?;
        // One internal library, one playerd process. The loader searches the
        // release runtime path; there is no subprocess or alternate video UI.
        let module = unsafe {
            libc::dlopen(
                c"libnuraloumi-video.so".as_ptr(),
                libc::RTLD_NOW | libc::RTLD_LOCAL,
            )
        };
        if module.is_null() {
            return Err("qualified native VDE/Grate library is not installed".into());
        }
        let symbols = (|| unsafe {
            let open: FnOpen = lookup(module, c"nura_video_open")?;
            let command: FnCommand = lookup(module, c"nura_video_command")?;
            let status: FnStatus = lookup(module, c"nura_video_status")?;
            let close: FnClose = lookup(module, c"nura_video_close")?;
            Ok::<_, String>((open, command, status, close))
        })();
        let (open, command, status, close) = match symbols {
            Ok(symbols) => symbols,
            Err(e) => {
                unsafe { libc::dlclose(module) };
                return Err(e);
            }
        };
        let session = unsafe { open(path.as_ptr(), volume, i32::from(muted)) };
        if session.is_null() {
            unsafe { libc::dlclose(module) };
            return Err("native VDE/GPU session rejected the media path".into());
        }
        Ok(Self {
            module,
            session,
            command,
            status,
            close,
        })
    }

    pub fn play(&mut self) -> Result<(), String> {
        self.send(1, 0)
    }
    pub fn pause(&mut self) -> Result<(), String> {
        self.send(2, 0)
    }
    pub fn seek(&mut self, position_ms: u64) -> Result<(), String> {
        let value = i64::try_from(position_ms).map_err(|_| "seek time overflow")?;
        self.send(4, value)
    }
    pub fn set_volume(&mut self, value: f64) -> Result<(), String> {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err("volume out of range".into());
        }
        self.send(5, (value * 1000.0).round() as i64)
    }
    pub fn set_mute(&mut self, muted: bool) -> Result<(), String> {
        self.send(6, i64::from(muted))
    }
    fn send(&mut self, op: c_int, value: i64) -> Result<(), String> {
        if unsafe { (self.command)(self.session, op, value) } == 0 {
            return Err("native VDE/GPU control rejected the command".into());
        }
        Ok(())
    }
    pub fn poll(&self) -> Result<VideoPoll, String> {
        let mut current = VideoStatus::default();
        if unsafe { (self.status)(self.session, &mut current) } == 0 {
            return Err("native VDE/GPU status unavailable".into());
        }
        match current.state {
            0 => Ok(VideoPoll::Starting),
            1 => Ok(VideoPoll::Playing {
                position_ms: current.position_ms,
                duration_ms: current.duration_ms,
            }),
            2 => Ok(VideoPoll::Paused {
                position_ms: current.position_ms,
                duration_ms: current.duration_ms,
            }),
            3 => Ok(VideoPoll::Eos),
            _ => {
                let details = unsafe { CStr::from_ptr(current.error.as_ptr()) }.to_string_lossy();
                Err(format!("native VDE/GPU: {details}"))
            }
        }
    }
}

unsafe fn lookup<T: Copy>(module: *mut c_void, name: &CStr) -> Result<T, String> {
    let address = libc::dlsym(module, name.as_ptr());
    if address.is_null() {
        return Err(format!(
            "missing VDE/GPU ABI symbol {}",
            name.to_string_lossy()
        ));
    }
    Ok(std::mem::transmute_copy(&address))
}

impl Drop for VideoEngine {
    fn drop(&mut self) {
        // Join the render thread before unloading code or releasing its
        // EGL/GStreamer dependencies, including on Stop/Next/playerd exit.
        unsafe {
            (self.close)(self.session);
            libc::dlclose(self.module);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_abi_state_is_stable() {
        assert_eq!(std::mem::size_of::<VideoStatus>(), 224);
        assert_eq!(VideoStatus::default().state, 0);
        assert_eq!(std::mem::size_of::<FnOpen>(), std::mem::size_of::<usize>());
    }
    #[test]
    fn malformed_path_fails_without_loading_a_driver() {
        assert!(VideoEngine::open("relative.mp4", 0.5, false).is_err());
        assert!(VideoEngine::open("/tmp/a\0.mp4", 0.5, false).is_err());
        assert!(VideoEngine::open("/tmp/a.mp4", f64::NAN, false).is_err());
    }
}
