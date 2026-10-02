use std::{error::Error as StdError, fmt, fs::File, path::Path};

use cairo::{Context, FontSlant, FontWeight, Format, ImageSurface, LinearGradient};

use crate::{
    layout_menu,
    model::{
        CoreMenuAdapter, MenuSource, PaintNode, Rect, Scene, TextMetrics, TextStyle, Viewport,
    },
    theme::{Color, Fill, Theme},
    InteractionState,
};

pub trait TextMeasurer {
    fn measure(&self, text: &str, style: TextStyle) -> TextMetrics;

    fn ellipsize(&self, text: &str, max_width: f64, style: TextStyle) -> String {
        if max_width <= 0.0 {
            return String::new();
        }
        if self.measure(text, style).width <= max_width {
            return text.to_owned();
        }

        const ELLIPSIS: &str = "…";
        let ellipsis_width = self.measure(ELLIPSIS, style).width;
        if ellipsis_width > max_width {
            return String::new();
        }

        let mut fitted = String::new();
        for cluster in text_clusters(text) {
            let mut candidate =
                String::with_capacity(fitted.len() + cluster.len() + ELLIPSIS.len());
            candidate.push_str(&fitted);
            candidate.push_str(cluster);
            candidate.push_str(ELLIPSIS);
            if self.measure(&candidate, style).width > max_width {
                break;
            }
            fitted.push_str(cluster);
        }
        fitted.push('…');
        fitted
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextBackendCapabilities {
    pub shaping: bool,
    pub deterministic_metrics: bool,
    pub explicit_font_family: bool,
    pub exact_font_file: bool,
}

/// Combined layout-measurement and Cairo-raster backend.
///
/// CairoRenderer is generic over this contract so the zero-shaping,
/// deterministic backend stays the default while shaped or fixed-font
/// implementations can plug in without changing semantic/layout code.
pub trait TextBackend: TextMeasurer {
    fn draw(
        &self,
        context: &Context,
        origin: crate::Point,
        text: &str,
        style: TextStyle,
    ) -> Result<f64, cairo::Error>;

    fn capabilities(&self) -> TextBackendCapabilities;
}

/// Fixed logical text metrics plus deterministic Cairo glyph placement.
///
/// The selected Cairo toy-font face still comes from the host font backend, so
/// glyph outlines are not promised to be byte-identical across distributions.
/// Width, baseline metrics, and every glyph origin are independent of Cairo's
/// font extents, kerning, hinting, and output scale.
#[derive(Clone, Copy, Debug, Default)]
pub struct DeterministicText;

impl DeterministicText {
    const BOLD_ADVANCE_SCALE: f64 = 1.04;
    const HEIGHT_EM: f64 = 1.20;
    const ASCENT_EM: f64 = 0.86;

    /// Deterministic logical advance for one Unicode scalar in em units.
    pub fn advance_em(ch: char) -> f64 {
        if is_zero_advance(ch) {
            0.0
        } else if ch == '\t' {
            1.32
        } else if ch.is_ascii_whitespace() {
            0.33
        } else if matches!(ch, '.' | ',' | ':' | ';' | '!' | '\'' | '|') {
            0.28
        } else if matches!(ch, '(' | ')' | '[' | ']' | '{' | '}' | '"') {
            0.38
        } else if matches!(ch, '-' | '_' | '/' | '\\') {
            0.42
        } else if matches!(ch, '@' | '%' | '#' | '&') {
            0.72
        } else if ch.is_ascii_uppercase() {
            match ch {
                'I' => 0.30,
                'J' => 0.48,
                'M' | 'W' => 0.78,
                _ => 0.61,
            }
        } else if ch.is_ascii_lowercase() {
            match ch {
                'i' | 'l' => 0.27,
                'f' | 'j' | 'r' | 't' => 0.38,
                'm' | 'w' => 0.76,
                _ => 0.52,
            }
        } else if ch.is_ascii_digit() {
            0.54
        } else if is_wide(ch) {
            1.0
        } else {
            0.58
        }
    }

    pub fn advance_for_char(ch: char, style: TextStyle) -> f64 {
        let weight = if style.bold {
            Self::BOLD_ADVANCE_SCALE
        } else {
            1.0
        };
        Self::advance_em(ch) * style.size * weight
    }

    pub fn run_width(text: &str, style: TextStyle) -> f64 {
        text.chars()
            .map(|ch| Self::advance_for_char(ch, style))
            .sum()
    }

    /// Draw a single-line run with deterministic logical glyph origins.
    ///
    /// Cairo is used for glyph rasterization only. Kerning/font-provided
    /// advances are deliberately ignored by resetting the current point for
    /// every scalar. The returned value is the deterministic logical x
    /// endpoint and equals origin.x + measure(text).width.
    pub fn draw_to_cairo(
        &self,
        context: &Context,
        origin: crate::Point,
        text: &str,
        style: TextStyle,
    ) -> Result<f64, cairo::Error> {
        context.select_font_face(
            "Sans",
            FontSlant::Normal,
            if style.bold {
                FontWeight::Bold
            } else {
                FontWeight::Normal
            },
        );
        context.set_font_size(style.size);

        let mut x = origin.x;
        let mut last_base_x = origin.x;
        for ch in text.chars() {
            if ch == '\n' || ch == '\r' {
                continue;
            }

            let advance = Self::advance_for_char(ch, style);
            if is_combining_mark(ch) {
                context.move_to(last_base_x, origin.y);
                let mut encoded = [0_u8; 4];
                context.show_text(ch.encode_utf8(&mut encoded))?;
                continue;
            }
            if is_format_scalar(ch) {
                continue;
            }
            if ch == '\t' {
                x += advance;
                continue;
            }

            last_base_x = x;
            context.move_to(x, origin.y);
            let mut encoded = [0_u8; 4];
            context.show_text(ch.encode_utf8(&mut encoded))?;
            x += advance;
        }
        Ok(x)
    }
}

impl TextMeasurer for DeterministicText {
    fn measure(&self, text: &str, style: TextStyle) -> TextMetrics {
        TextMetrics {
            width: Self::run_width(text, style),
            height: style.size * Self::HEIGHT_EM,
            ascent: style.size * Self::ASCENT_EM,
        }
    }

    fn ellipsize(&self, text: &str, max_width: f64, style: TextStyle) -> String {
        if max_width <= 0.0 {
            return String::new();
        }
        let full_width = Self::run_width(text, style);
        if full_width <= max_width {
            return text.to_owned();
        }

        let ellipsis_width = Self::advance_for_char('…', style);
        if ellipsis_width > max_width {
            return String::new();
        }

        let mut fitted = String::with_capacity(text.len().min(64));
        let mut width = 0.0;
        for cluster in text_clusters(text) {
            let cluster_width = Self::run_width(cluster, style);
            if width + cluster_width + ellipsis_width > max_width {
                break;
            }
            fitted.push_str(cluster);
            width += cluster_width;
        }
        fitted.push('…');
        fitted
    }
}

impl TextBackend for DeterministicText {
    fn draw(
        &self,
        context: &Context,
        origin: crate::Point,
        text: &str,
        style: TextStyle,
    ) -> Result<f64, cairo::Error> {
        self.draw_to_cairo(context, origin, text, style)
    }

    fn capabilities(&self) -> TextBackendCapabilities {
        TextBackendCapabilities {
            shaping: false,
            deterministic_metrics: true,
            explicit_font_family: false,
            exact_font_file: false,
        }
    }
}

/// Backward-compatible Wave-1 name for the deterministic text engine.
#[derive(Clone, Copy, Debug, Default)]
pub struct ToyText;

impl TextMeasurer for ToyText {
    fn measure(&self, text: &str, style: TextStyle) -> TextMetrics {
        DeterministicText.measure(text, style)
    }
}

fn is_combining_mark(ch: char) -> bool {
    matches!(
        ch as u32,
        0x0300..=0x036f
            | 0x1ab0..=0x1aff
            | 0x1dc0..=0x1dff
            | 0x20d0..=0x20ff
            | 0xfe20..=0xfe2f
            | 0x1f3fb..=0x1f3ff
    )
}

fn text_clusters(text: &str) -> impl Iterator<Item = &str> {
    let mut starts = Vec::with_capacity(text.len().min(32) + 1);
    starts.push(0);
    let mut previous_was_zwj = false;
    for (index, ch) in text.char_indices().skip(1) {
        let extends =
            is_combining_mark(ch) || is_format_scalar(ch) || previous_was_zwj || ch == '\u{200d}';
        if !extends {
            starts.push(index);
        }
        previous_was_zwj = ch == '\u{200d}';
    }
    starts.push(text.len());
    starts
        .windows(2)
        .map(move |window| &text[window[0]..window[1]])
        .collect::<Vec<_>>()
        .into_iter()
}

#[cfg(feature = "pangocairo")]
mod pango_backend {
    use std::{
        ffi::{c_char, c_int, c_void, CStr, CString, NulError},
        fmt, ptr,
    };

    use cairo::{Context, Format, ImageSurface};

    use super::{DeterministicText, TextBackend, TextBackendCapabilities, TextMeasurer};
    use crate::{Point, TextMetrics, TextStyle};

    const PANGO_SCALE: f64 = 1024.0;
    const PANGO_WEIGHT_NORMAL: c_int = 400;
    const PANGO_WEIGHT_BOLD: c_int = 700;

    #[repr(C)]
    struct PangoLayout {
        _private: [u8; 0],
    }

    #[repr(C)]
    struct PangoFontDescription {
        _private: [u8; 0],
    }

    #[repr(C)]
    struct PangoContext {
        _private: [u8; 0],
    }

    #[repr(C)]
    #[derive(Default)]
    struct PangoRectangle {
        x: c_int,
        y: c_int,
        width: c_int,
        height: c_int,
    }

    #[link(name = "pangocairo-1.0")]
    unsafe extern "C" {
        fn pango_cairo_create_layout(cr: *mut cairo::ffi::cairo_t) -> *mut PangoLayout;
        fn pango_cairo_show_layout(cr: *mut cairo::ffi::cairo_t, layout: *mut PangoLayout);
    }

    #[link(name = "pango-1.0")]
    unsafe extern "C" {
        fn pango_layout_set_text(layout: *mut PangoLayout, text: *const c_char, length: c_int);
        fn pango_layout_set_font_description(
            layout: *mut PangoLayout,
            description: *const PangoFontDescription,
        );
        fn pango_layout_set_single_paragraph_mode(layout: *mut PangoLayout, setting: c_int);
        fn pango_layout_get_context(layout: *mut PangoLayout) -> *mut PangoContext;
        fn pango_layout_get_extents(
            layout: *mut PangoLayout,
            ink_rect: *mut PangoRectangle,
            logical_rect: *mut PangoRectangle,
        );
        fn pango_layout_get_baseline(layout: *mut PangoLayout) -> c_int;
        fn pango_font_description_new() -> *mut PangoFontDescription;
        fn pango_font_description_free(description: *mut PangoFontDescription);
        fn pango_font_description_set_family(
            description: *mut PangoFontDescription,
            family: *const c_char,
        );
        fn pango_font_description_set_absolute_size(
            description: *mut PangoFontDescription,
            size: f64,
        );
        fn pango_font_description_set_weight(description: *mut PangoFontDescription, weight: c_int);
        fn pango_context_set_round_glyph_positions(
            context: *mut PangoContext,
            round_positions: c_int,
        );
    }

    #[link(name = "gobject-2.0")]
    unsafe extern "C" {
        fn g_object_unref(object: *mut c_void);
    }

    struct LayoutGuard(*mut PangoLayout);

    impl Drop for LayoutGuard {
        fn drop(&mut self) {
            // SAFETY: PangoCairo returns an owned GObject reference.
            unsafe { g_object_unref(self.0.cast()) };
        }
    }

    struct FontDescriptionGuard(*mut PangoFontDescription);

    impl Drop for FontDescriptionGuard {
        fn drop(&mut self) {
            // SAFETY: pointer was returned by pango_font_description_new.
            unsafe { pango_font_description_free(self.0) };
        }
    }

    #[derive(Debug)]
    pub enum PangoTextError {
        InvalidFamily(NulError),
        Cairo(cairo::Error),
    }

    impl fmt::Display for PangoTextError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::InvalidFamily(error) => write!(f, "invalid Pango font family: {error}"),
                Self::Cairo(error) => {
                    write!(f, "failed to create Pango measurement surface: {error}")
                }
            }
        }
    }

    impl std::error::Error for PangoTextError {}

    impl From<cairo::Error> for PangoTextError {
        fn from(value: cairo::Error) -> Self {
            Self::Cairo(value)
        }
    }

    /// Optional complex-script backend using the system PangoCairo shaper.
    ///
    /// The family is an explicit preferred family. Pango may still fall back
    /// for missing glyphs. A future packaged-font backend can implement
    /// TextBackend to enforce a particular font file without renderer changes.
    pub struct PangoCairoText {
        family: CString,
        _scratch_surface: ImageSurface,
        scratch_context: Context,
        fallback: DeterministicText,
    }

    impl PangoCairoText {
        pub fn new(family: &str) -> Result<Self, PangoTextError> {
            let family = CString::new(family).map_err(PangoTextError::InvalidFamily)?;
            let surface = ImageSurface::create(Format::ARgb32, 8, 8)?;
            let scratch_context = Context::new(&surface)?;
            Ok(Self {
                family,
                _scratch_surface: surface,
                scratch_context,
                fallback: DeterministicText,
            })
        }

        pub fn family(&self) -> &CStr {
            &self.family
        }

        fn layout_for(
            &self,
            context: &Context,
            text: &str,
            style: TextStyle,
        ) -> Option<LayoutGuard> {
            let text_len = c_int::try_from(text.len()).ok()?;
            // SAFETY: context is live; returned pointers are checked and
            // wrapped in Drop guards with the matching ownership function.
            unsafe {
                let layout = pango_cairo_create_layout(context.to_raw_none());
                if layout.is_null() {
                    return None;
                }
                let layout = LayoutGuard(layout);
                let pango_context = pango_layout_get_context(layout.0);
                if !pango_context.is_null() {
                    pango_context_set_round_glyph_positions(pango_context, 0);
                }
                let description = pango_font_description_new();
                if description.is_null() {
                    return None;
                }
                let description = FontDescriptionGuard(description);
                pango_font_description_set_family(description.0, self.family.as_ptr());
                pango_font_description_set_absolute_size(description.0, style.size * PANGO_SCALE);
                pango_font_description_set_weight(
                    description.0,
                    if style.bold {
                        PANGO_WEIGHT_BOLD
                    } else {
                        PANGO_WEIGHT_NORMAL
                    },
                );
                pango_layout_set_font_description(layout.0, description.0);
                pango_layout_set_single_paragraph_mode(layout.0, 1);
                pango_layout_set_text(layout.0, text.as_ptr().cast(), text_len);
                Some(layout)
            }
        }

        fn metrics_for(
            &self,
            context: &Context,
            text: &str,
            style: TextStyle,
        ) -> Option<TextMetrics> {
            let layout = self.layout_for(context, text, style)?;
            // SAFETY: layout remains alive and logical is writable storage.
            unsafe {
                let mut logical = PangoRectangle::default();
                pango_layout_get_extents(layout.0, ptr::null_mut(), &mut logical);
                let baseline = pango_layout_get_baseline(layout.0);
                Some(TextMetrics {
                    width: f64::from(logical.width.max(0)) / PANGO_SCALE,
                    height: f64::from(logical.height.max(0)) / PANGO_SCALE,
                    ascent: f64::from(baseline.max(0)) / PANGO_SCALE,
                })
            }
        }
    }

    impl TextMeasurer for PangoCairoText {
        fn measure(&self, text: &str, style: TextStyle) -> TextMetrics {
            self.metrics_for(&self.scratch_context, text, style)
                .unwrap_or_else(|| self.fallback.measure(text, style))
        }
    }

    impl TextBackend for PangoCairoText {
        fn draw(
            &self,
            context: &Context,
            origin: Point,
            text: &str,
            style: TextStyle,
        ) -> Result<f64, cairo::Error> {
            let Some(layout) = self.layout_for(context, text, style) else {
                return self.fallback.draw(context, origin, text, style);
            };
            // SAFETY: layout is alive and logical is writable storage.
            let (logical_width, baseline) = unsafe {
                let mut logical = PangoRectangle::default();
                pango_layout_get_extents(layout.0, ptr::null_mut(), &mut logical);
                (
                    f64::from(logical.width.max(0)) / PANGO_SCALE,
                    f64::from(pango_layout_get_baseline(layout.0).max(0)) / PANGO_SCALE,
                )
            };
            context.move_to(origin.x, origin.y - baseline);
            // SAFETY: context and layout remain valid for this call.
            unsafe { pango_cairo_show_layout(context.to_raw_none(), layout.0) };
            Ok(origin.x + logical_width)
        }

        fn capabilities(&self) -> TextBackendCapabilities {
            TextBackendCapabilities {
                shaping: true,
                deterministic_metrics: false,
                explicit_font_family: true,
                exact_font_file: false,
            }
        }
    }
}

#[cfg(feature = "pangocairo")]
pub use pango_backend::{PangoCairoText, PangoTextError};

fn is_format_scalar(ch: char) -> bool {
    matches!(
        ch as u32,
        0x200b..=0x200f | 0x202a..=0x202e | 0x2060..=0x206f | 0xfe00..=0xfe0f
    )
}

fn is_zero_advance(ch: char) -> bool {
    is_combining_mark(ch) || is_format_scalar(ch)
}

fn is_wide(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1100..=0x115f
            | 0x2329..=0x232a
            | 0x2e80..=0xa4cf
            | 0xac00..=0xd7a3
            | 0xf900..=0xfaff
            | 0xfe10..=0xfe19
            | 0xfe30..=0xfe6f
            | 0xff00..=0xff60
            | 0xffe0..=0xffe6
            | 0x1f300..=0x1faff
            | 0x20000..=0x3fffd
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferInfo {
    pub width: i32,
    pub height: i32,
    pub stride: i32,
}

/// Failure to borrow Cairo image bytes, usually due to an outstanding borrow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferAccessError;

impl fmt::Display for BufferAccessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Cairo image surface bytes are currently borrowed")
    }
}

impl StdError for BufferAccessError {}

pub struct RenderedBuffer {
    surface: ImageSurface,
    info: BufferInfo,
}

impl RenderedBuffer {
    pub fn info(&self) -> BufferInfo {
        self.info
    }

    pub fn surface(&self) -> &ImageSurface {
        &self.surface
    }

    /// Borrow native-endian Cairo FORMAT_ARGB32 bytes for a bounded callback.
    ///
    /// Pixels are premultiplied ARGB32 words in native endian. On the SL101's
    /// little-endian ARMv7 target memory bytes are B, G, R, A, matching the
    /// memory representation expected for wl_shm ARGB8888 words.
    pub fn with_argb32_bytes<R>(
        &mut self,
        callback: impl FnOnce(&[u8], BufferInfo) -> R,
    ) -> Result<R, BufferAccessError> {
        self.surface.flush();
        let data = self.surface.data().map_err(|_| BufferAccessError)?;
        Ok(callback(&data, self.info))
    }

    pub fn copy_argb32_bytes(&mut self) -> Result<Vec<u8>, BufferAccessError> {
        self.with_argb32_bytes(|bytes, _| bytes.to_vec())
    }

    pub fn write_png(&self, path: impl AsRef<Path>) -> Result<(), Box<dyn StdError>> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = File::create(path)?;
        self.surface.write_to_png(&mut file)?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderOptions {
    pub constrained: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct CairoRenderer<B = DeterministicText> {
    text: B,
}

impl Default for CairoRenderer<DeterministicText> {
    fn default() -> Self {
        Self {
            text: DeterministicText,
        }
    }
}

impl<B> CairoRenderer<B> {
    pub const fn with_text_backend(text: B) -> Self {
        Self { text }
    }

    pub const fn text_backend(&self) -> &B {
        &self.text
    }
}

impl<B: TextBackend> CairoRenderer<B> {
    /// Build a scene directly from the canonical semantic-core model/state.
    pub fn build_core_scene(
        &self,
        model: &nuraloumi_core::MenuModel,
        state: &nuraloumi_core::MenuState,
        viewport: Viewport,
        tokens: &nuraloumi_core::ThemeTokens,
        options: RenderOptions,
    ) -> Scene {
        let adapter = CoreMenuAdapter::new(model, state);
        let interaction = InteractionState::from(state);
        let theme = Theme::from(tokens);
        self.build_scene(&adapter, &interaction, viewport, &theme, options)
    }

    /// Rasterize the canonical semantic-core model/state into ARGB32.
    pub fn render_core(
        &self,
        model: &nuraloumi_core::MenuModel,
        state: &nuraloumi_core::MenuState,
        viewport: Viewport,
        tokens: &nuraloumi_core::ThemeTokens,
        options: RenderOptions,
    ) -> Result<(Scene, RenderedBuffer), cairo::Error> {
        let scene = self.build_core_scene(model, state, viewport, tokens, options);
        let buffer = self.render_scene(&scene)?;
        Ok((scene, buffer))
    }

    /// Rasterize a canonical semantic scene through one finite presentation
    /// transition. Semantic layout and hit geometry stay at their settled
    /// coordinates; callers should suppress activation until the transition
    /// settles when spatial motion is non-zero.
    pub fn render_core_transition(
        &self,
        model: &nuraloumi_core::MenuModel,
        state: &nuraloumi_core::MenuState,
        viewport: Viewport,
        tokens: &nuraloumi_core::ThemeTokens,
        options: RenderOptions,
        transition: nuraloumi_core::Transition,
    ) -> Result<(Scene, RenderedBuffer), cairo::Error> {
        let scene = self.build_core_scene(model, state, viewport, tokens, options);
        let buffer = self.render_scene_transition(&scene, transition)?;
        Ok((scene, buffer))
    }

    pub fn build_scene<M: MenuSource>(
        &self,
        menu: &M,
        interaction: &InteractionState,
        viewport: Viewport,
        theme: &Theme,
        options: RenderOptions,
    ) -> Scene {
        layout_menu(
            menu,
            interaction,
            viewport,
            theme,
            &self.text,
            options.constrained,
        )
    }

    pub fn render_scene(&self, scene: &Scene) -> Result<RenderedBuffer, cairo::Error> {
        self.render_scene_inner(scene, None)
    }

    pub fn render_scene_transition(
        &self,
        scene: &Scene,
        transition: nuraloumi_core::Transition,
    ) -> Result<RenderedBuffer, cairo::Error> {
        self.render_scene_inner(scene, Some(transition))
    }

    fn render_scene_inner(
        &self,
        scene: &Scene,
        transition: Option<nuraloumi_core::Transition>,
    ) -> Result<RenderedBuffer, cairo::Error> {
        let (width, height) = scene.viewport.device_size();
        let surface = ImageSurface::create(Format::ARgb32, width, height)?;
        let context = Context::new(&surface)?;
        context.scale(scene.viewport.scale, scene.viewport.scale);

        if let Some(transition) = transition {
            let opacity = f64::from(transition.opacity.clamp(0.0, 1.0));
            let scale = f64::from(transition.scale.max(0.01));
            let center_x = scene.panel_rect.x + scene.panel_rect.width / 2.0;
            let center_y = scene.panel_rect.y + scene.panel_rect.height / 2.0;

            context.save()?;
            context.translate(center_x, center_y + f64::from(transition.translate_y));
            context.scale(scale, scale);
            context.translate(-center_x, -center_y);
            context.push_group();
            for node in &scene.paint {
                draw_node(&context, node, &self.text)?;
            }
            context.pop_group_to_source()?;
            context.paint_with_alpha(opacity)?;
            context.restore()?;
        } else {
            for node in &scene.paint {
                draw_node(&context, node, &self.text)?;
            }
        }

        surface.flush();
        let info = BufferInfo {
            width,
            height,
            stride: surface.stride(),
        };
        Ok(RenderedBuffer { surface, info })
    }

    pub fn render<M: MenuSource>(
        &self,
        menu: &M,
        interaction: &InteractionState,
        viewport: Viewport,
        theme: &Theme,
        options: RenderOptions,
    ) -> Result<(Scene, RenderedBuffer), cairo::Error> {
        let scene = self.build_scene(menu, interaction, viewport, theme, options);
        let buffer = self.render_scene(&scene)?;
        Ok((scene, buffer))
    }

    pub fn render_png<M: MenuSource>(
        &self,
        menu: &M,
        interaction: &InteractionState,
        viewport: Viewport,
        theme: &Theme,
        options: RenderOptions,
        path: impl AsRef<Path>,
    ) -> Result<Scene, Box<dyn StdError>> {
        let (scene, buffer) = self.render(menu, interaction, viewport, theme, options)?;
        buffer.write_png(path)?;
        Ok(scene)
    }
}

fn set_color(context: &Context, color: Color) {
    let (r, g, b, a) = color.components();
    context.set_source_rgba(r, g, b, a);
}

fn rounded_path(context: &Context, rect: Rect, radius: f64) {
    let radius = radius.max(0.0).min(rect.width / 2.0).min(rect.height / 2.0);
    let right = rect.right();
    let bottom = rect.bottom();
    context.new_sub_path();
    context.arc(
        right - radius,
        rect.y + radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        0.0,
    );
    context.arc(
        right - radius,
        bottom - radius,
        radius,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    context.arc(
        rect.x + radius,
        bottom - radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    context.arc(
        rect.x + radius,
        rect.y + radius,
        radius,
        std::f64::consts::PI,
        std::f64::consts::PI * 1.5,
    );
    context.close_path();
}

fn set_fill(context: &Context, rect: Rect, fill: Fill) -> Result<(), cairo::Error> {
    match fill {
        Fill::Solid(color) => set_color(context, color),
        Fill::VerticalGradient { top, bottom } => {
            let gradient = LinearGradient::new(rect.x, rect.y, rect.x, rect.bottom());
            let (r, g, b, a) = top.components();
            gradient.add_color_stop_rgba(0.0, r, g, b, a);
            let (r, g, b, a) = bottom.components();
            gradient.add_color_stop_rgba(1.0, r, g, b, a);
            context.set_source(&gradient)?;
        }
    }
    Ok(())
}

fn draw_node<B: TextBackend>(
    context: &Context,
    node: &PaintNode,
    text_backend: &B,
) -> Result<(), cairo::Error> {
    match node {
        PaintNode::FillRect { rect, color } => {
            set_color(context, *color);
            context.rectangle(rect.x, rect.y, rect.width, rect.height);
            context.fill()?;
        }
        PaintNode::RoundedRect {
            rect,
            radius,
            fill,
            stroke,
        } => {
            rounded_path(context, *rect, *radius);
            set_fill(context, *rect, *fill)?;
            if let Some((color, width)) = stroke {
                context.fill_preserve()?;
                set_color(context, *color);
                context.set_line_width(*width);
                context.stroke()?;
            } else {
                context.fill()?;
            }
        }
        PaintNode::Line {
            from,
            to,
            width,
            color,
        } => {
            set_color(context, *color);
            context.set_line_width(*width);
            context.move_to(from.x, from.y);
            context.line_to(to.x, to.y);
            context.stroke()?;
        }
        PaintNode::Text {
            origin,
            text,
            style,
            color,
        } => {
            set_color(context, *color);
            text_backend.draw(context, *origin, text, *style)?;
        }
        PaintNode::Chevron { rect, color } => {
            set_color(context, *color);
            context.set_line_width(2.0);
            context.move_to(rect.x + 2.0, rect.y + 2.0);
            context.line_to(rect.right() - 2.0, rect.y + rect.height / 2.0);
            context.line_to(rect.x + 2.0, rect.bottom() - 2.0);
            context.stroke()?;
        }
        PaintNode::CheckMark {
            rect,
            checked,
            color,
            accent,
        } => {
            rounded_path(context, *rect, 5.0);
            set_color(
                context,
                if *checked {
                    *accent
                } else {
                    Color::rgba(color.r, color.g, color.b, 24)
                },
            );
            context.fill_preserve()?;
            set_color(context, *color);
            context.set_line_width(1.0);
            context.stroke()?;
            if *checked {
                set_color(context, Color::rgb(0x11, 0x12, 0x18));
                context.set_line_width(2.0);
                context.move_to(rect.x + 5.0, rect.y + 10.0);
                context.line_to(rect.x + 9.0, rect.y + 14.0);
                context.line_to(rect.x + 16.0, rect.y + 6.0);
                context.stroke()?;
            }
        }
        PaintNode::ClipPush(rect) => {
            context.save()?;
            context.rectangle(rect.x, rect.y, rect.width, rect.height);
            context.clip();
        }
        PaintNode::ClipPop => context.restore()?,
    }
    Ok(())
}
