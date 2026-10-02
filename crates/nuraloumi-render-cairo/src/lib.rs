//! Deterministic software scene/layout and Cairo rendering for NuraLoumi.
//!
//! The crate deliberately keeps menu semantics behind MenuSource. Wave 1's
//! semantic-core crate is being implemented concurrently, so callers can use
//! MenuView immediately and add a tiny local wrapper around the final core
//! model without changing layout or raster code. The canonical core crate is
//! still re-exported as semantic_core to keep dependency direction explicit.
//!
//! Text rendering is backend-selectable. DeterministicText is the default
//! lightweight SL101 path. The optional pangocairo feature exposes
//! PangoCairoText for complex-script shaping with an explicit preferred family.
//! The optional packaged-font feature bypasses family lookup entirely: it opens
//! exact font files with FreeType, shapes against those same faces with
//! HarfBuzz, and sends the returned glyph IDs directly to Cairo FT.

mod layout;
mod model;
#[cfg(all(feature = "packaged-font", unix))]
mod packaged_font;
mod render;
mod theme;

#[cfg(all(feature = "packaged-font", not(unix)))]
compile_error!("the packaged-font backend currently requires a Unix target");

pub use layout::{layout_menu, LayoutMetrics};
pub use model::{
    CoreMenuAdapter, HitRegion, Insets, InteractionState, MenuItemRef, MenuItemView, MenuSource,
    MenuView, PaintNode, Point, Rect, RowKind, Scene, ScrollWindow, TextMetrics, TextStyle,
    Viewport,
};
#[cfg(all(feature = "packaged-font", unix))]
pub use packaged_font::{
    PackagedBidiRun, PackagedFontError, PackagedFontSource, PackagedFontText, StrictRenderError,
};
pub use render::{
    BufferAccessError, BufferInfo, CairoRenderer, DeterministicText, RenderOptions, RenderedBuffer,
    TextBackend, TextBackendCapabilities, TextMeasurer, ToyText,
};
#[cfg(feature = "pangocairo")]
pub use render::{PangoCairoText, PangoTextError};
pub use theme::{Color, Fill, Theme};

pub use nuraloumi_core as semantic_core;

/// Wave-1 renderer contract version.
pub const RENDERER_API_VERSION: u32 = 1;
