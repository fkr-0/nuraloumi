//! Native Wayland wl_shm/layer-shell backend for NuraLoumi.
//!
//! This crate intentionally has no EGL, GLES, X11, XCB, or GUI-toolkit
//! dependency. Caller-supplied software pixels are copied into two bounded
//! shared-memory buffers per surface and presented through wl_shm.

mod backend;
mod error;
mod foreign_toplevel;
mod shm;
mod thumbnail;
mod types;
mod workspace;

pub use backend::WaylandBackend;
pub use error::{BackendError, Result};
pub use foreign_toplevel::{
    ToplevelCapabilities, ToplevelEvent, ToplevelId, ToplevelInfo, ToplevelSource, ToplevelState,
};
pub use thumbnail::{
    capture_toplevel_thumbnails, capture_toplevel_thumbnails_with_timeout, ToplevelThumbnail,
    ToplevelThumbnailCapabilities, ToplevelThumbnailReport, ToplevelThumbnailRequest,
};
pub use types::{
    normalize_output_point, BackendCapabilities, BackendEvent, DismissBackdropConfig, Frame, Key,
    MenuConfig, OutputId, OutputInfo, OutputTransform, PanelConfig, PanelEdge, PixelFormat,
    PlatformEvent, Point, SurfaceId,
};
pub use workspace::{
    WorkspaceCapabilities, WorkspaceEvent, WorkspaceId, WorkspaceInfo, WorkspaceState,
};
