use std::{error::Error, fmt};

pub type Result<T> = std::result::Result<T, BackendError>;

#[derive(Debug)]
pub enum BackendError {
    Connect(String),
    Dispatch(String),
    MissingGlobal(&'static str),
    UnsupportedShmFormat(&'static str),
    UnknownSurface(u32),
    SurfaceClosed(u32),
    SurfaceNotConfigured(u32),
    InvalidSurfaceConfig(String),
    UnsupportedBufferScale { surface_version: u32, scale: i32 },
    WouldBlock,
    InvalidFrame(String),
    Allocation(std::io::Error),
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect(message) => write!(f, "failed to connect to Wayland: {message}"),
            Self::Dispatch(message) => write!(f, "Wayland dispatch failed: {message}"),
            Self::MissingGlobal(name) => {
                write!(f, "required Wayland global is unavailable: {name}")
            }
            Self::UnsupportedShmFormat(name) => {
                write!(f, "required wl_shm format is unavailable: {name}")
            }
            Self::UnknownSurface(id) => write!(f, "unknown NuraLoumi surface id {id}"),
            Self::SurfaceClosed(id) => write!(f, "surface {id} was closed by the compositor"),
            Self::SurfaceNotConfigured(id) => {
                write!(f, "surface {id} has not received a layer-shell configure")
            }
            Self::InvalidSurfaceConfig(message) => {
                write!(f, "invalid layer-shell surface configuration: {message}")
            }
            Self::UnsupportedBufferScale {
                surface_version,
                scale,
            } => write!(
                f,
                "wl_surface v{surface_version} cannot apply buffer scale {scale}; v3+ is required"
            ),
            Self::WouldBlock => write!(
                f,
                "all shm buffers are busy; wait for wl_buffer.release before presenting again"
            ),
            Self::InvalidFrame(message) => write!(f, "invalid software frame: {message}"),
            Self::Allocation(error) => write!(f, "wl_shm allocation failed: {error}"),
        }
    }
}

impl Error for BackendError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Allocation(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for BackendError {
    fn from(value: std::io::Error) -> Self {
        Self::Allocation(value)
    }
}
