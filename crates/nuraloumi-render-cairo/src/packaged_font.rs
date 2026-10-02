use std::{
    cell::RefCell,
    error::Error as StdError,
    ffi::{c_char, c_int, c_long, c_uint, c_void, CString, NulError},
    fmt, fs, io,
    os::unix::ffi::OsStrExt,
    path::PathBuf,
    rc::Rc,
    slice,
};

use cairo::{Context, FontFace, Format, Glyph, ImageSurface, UserDataKey};

use crate::{
    CairoRenderer, InteractionState, MenuSource, RenderOptions, RenderedBuffer, Scene, TextBackend,
    TextBackendCapabilities, TextMeasurer, TextMetrics, TextStyle, Theme, Viewport,
};

type FtError = c_int;
type FtLibrary = *mut FtLibraryRec;
type FtFace = *mut FtFaceRec;

#[repr(C)]
struct FtLibraryRec {
    _private: [u8; 0],
}

#[repr(C)]
struct FtFaceRec {
    _private: [u8; 0],
}

#[repr(C)]
struct HbFont {
    _private: [u8; 0],
}

#[repr(C)]
struct HbBuffer {
    _private: [u8; 0],
}

#[repr(C)]
union HbVarInt {
    u32_value: u32,
    i32_value: i32,
}

#[repr(C)]
struct HbGlyphInfo {
    codepoint: u32,
    _mask: u32,
    cluster: u32,
    _var1: HbVarInt,
    _var2: HbVarInt,
}

#[repr(C)]
struct HbGlyphPosition {
    x_advance: i32,
    y_advance: i32,
    x_offset: i32,
    y_offset: i32,
    _var: HbVarInt,
}

type FriBidiChar = u32;
type FriBidiStrIndex = c_int;
type FriBidiLevel = i8;
type FriBidiParType = u32;

const FRIBIDI_PAR_ON: FriBidiParType = 64;
const HB_DIRECTION_LTR: c_int = 4;
const HB_DIRECTION_RTL: c_int = 5;

#[link(name = "freetype")]
unsafe extern "C" {
    fn FT_Init_FreeType(library: *mut FtLibrary) -> FtError;
    fn FT_Done_FreeType(library: FtLibrary) -> FtError;
    fn FT_New_Face(
        library: FtLibrary,
        filepathname: *const c_char,
        face_index: c_long,
        aface: *mut FtFace,
    ) -> FtError;
    fn FT_Done_Face(face: FtFace) -> FtError;
    fn FT_Set_Char_Size(
        face: FtFace,
        char_width: c_long,
        char_height: c_long,
        horizontal_resolution: c_uint,
        vertical_resolution: c_uint,
    ) -> FtError;
}

#[link(name = "harfbuzz")]
unsafe extern "C" {
    fn hb_ft_font_create_referenced(face: FtFace) -> *mut HbFont;
    fn hb_font_destroy(font: *mut HbFont);
    fn hb_buffer_create() -> *mut HbBuffer;
    fn hb_buffer_destroy(buffer: *mut HbBuffer);
    fn hb_buffer_add_utf8(
        buffer: *mut HbBuffer,
        text: *const c_char,
        text_length: c_int,
        item_offset: c_uint,
        item_length: c_int,
    );
    fn hb_buffer_guess_segment_properties(buffer: *mut HbBuffer);
    fn hb_buffer_set_direction(buffer: *mut HbBuffer, direction: c_int);
    fn hb_shape(
        font: *mut HbFont,
        buffer: *mut HbBuffer,
        features: *const c_void,
        feature_count: c_uint,
    );
    fn hb_buffer_get_glyph_infos(buffer: *mut HbBuffer, length: *mut c_uint) -> *mut HbGlyphInfo;
    fn hb_buffer_get_glyph_positions(
        buffer: *mut HbBuffer,
        length: *mut c_uint,
    ) -> *mut HbGlyphPosition;
}

#[link(name = "fribidi")]
unsafe extern "C" {
    fn fribidi_log2vis(
        logical: *const FriBidiChar,
        length: FriBidiStrIndex,
        base_direction: *mut FriBidiParType,
        visual: *mut FriBidiChar,
        logical_to_visual: *mut FriBidiStrIndex,
        visual_to_logical: *mut FriBidiStrIndex,
        embedding_levels: *mut FriBidiLevel,
    ) -> FriBidiLevel;
}

#[link(name = "cairo")]
unsafe extern "C" {
    fn cairo_ft_font_face_create_for_ft_face(
        face: FtFace,
        load_flags: c_int,
    ) -> *mut cairo::ffi::cairo_font_face_t;
}

struct FtOwner {
    library: FtLibrary,
    face: FtFace,
}

impl Drop for FtOwner {
    fn drop(&mut self) {
        // SAFETY: both handles were created by FreeType and this owner is kept
        // alive by the Cairo font-face user-data reference.
        unsafe {
            if !self.face.is_null() {
                let _ = FT_Done_Face(self.face);
            }
            if !self.library.is_null() {
                let _ = FT_Done_FreeType(self.library);
            }
        }
    }
}

static FT_OWNER_KEY: UserDataKey<FtOwner> = UserDataKey::new();

struct ExactFace {
    source: PackagedFontSource,
    owner: Rc<FtOwner>,
    cairo_face: FontFace,
}

impl ExactFace {
    fn open(source: PackagedFontSource) -> Result<Self, PackagedFontError> {
        let canonical =
            fs::canonicalize(&source.path).map_err(|source_error| PackagedFontError::Io {
                path: source.path.clone(),
                source: source_error,
            })?;
        let metadata = fs::metadata(&canonical).map_err(|source_error| PackagedFontError::Io {
            path: canonical.clone(),
            source: source_error,
        })?;
        if !metadata.is_file() {
            return Err(PackagedFontError::NotRegularFile(canonical));
        }
        let path_c = CString::new(canonical.as_os_str().as_bytes())
            .map_err(PackagedFontError::InvalidPath)?;

        let mut library: FtLibrary = std::ptr::null_mut();
        // SAFETY: the out pointer is valid and checked after the call.
        let init_code = unsafe { FT_Init_FreeType(&mut library) };
        if init_code != 0 || library.is_null() {
            return Err(PackagedFontError::FreeType {
                operation: "FT_Init_FreeType",
                code: init_code,
            });
        }

        let face_index = c_long::try_from(source.face_index)
            .map_err(|_| PackagedFontError::InvalidFaceIndex(source.face_index))?;
        let mut face: FtFace = std::ptr::null_mut();
        // SAFETY: library is live, C path is NUL-terminated, and out pointer is valid.
        let open_code = unsafe { FT_New_Face(library, path_c.as_ptr(), face_index, &mut face) };
        if open_code != 0 || face.is_null() {
            // SAFETY: library was initialized above and no face owns it yet.
            unsafe {
                let _ = FT_Done_FreeType(library);
            }
            return Err(PackagedFontError::FreeType {
                operation: "FT_New_Face",
                code: open_code,
            });
        }

        let owner = Rc::new(FtOwner { library, face });
        // SAFETY: face remains alive through owner and is attached to the
        // returned Cairo FontFace as user data before construction returns.
        let raw_cairo_face = unsafe { cairo_ft_font_face_create_for_ft_face(face, 0) };
        if raw_cairo_face.is_null() {
            return Err(PackagedFontError::Cairo(cairo::Error::NoMemory));
        }
        // SAFETY: raw_cairo_face is a newly-created full reference.
        let cairo_face = unsafe { FontFace::from_raw_full(raw_cairo_face) };
        cairo_face.set_user_data(&FT_OWNER_KEY, Rc::clone(&owner))?;
        cairo_face.status()?;

        Ok(Self {
            source: PackagedFontSource {
                path: canonical,
                face_index: source.face_index,
            },
            owner,
            cairo_face,
        })
    }

    fn set_size(&mut self, style: TextStyle) -> Result<(), PackagedFontError> {
        if !style.size.is_finite() || style.size <= 0.0 {
            return Err(PackagedFontError::InvalidSize(style.size));
        }
        // `f64::round()` may lower to a target libm helper. The SL101's
        // ARMv7/VFPv3-D16 runtime has been observed executing that helper with
        // d16+ registers, which SIGILLs on Tegra20. Text sizes are strictly
        // positive here, so half-up rounding is equivalent and stays inline.
        let fixed = style.size * 64.0;
        if !fixed.is_finite() || fixed > (c_long::MAX as f64 - 0.5) {
            return Err(PackagedFontError::InvalidSize(style.size));
        }
        let fixed = (fixed + 0.5) as c_long;
        // 72 dpi makes one point equal one logical pixel for this renderer.
        // SAFETY: the exact FT_Face is live for this borrow.
        let code = unsafe { FT_Set_Char_Size(self.owner.face, 0, fixed, 72, 72) };
        if code != 0 {
            return Err(PackagedFontError::FreeType {
                operation: "FT_Set_Char_Size",
                code,
            });
        }
        Ok(())
    }

    fn shape_segment(
        &self,
        font: *mut HbFont,
        text: &str,
        rtl: bool,
        cluster_base: usize,
    ) -> Result<ShapedRun, PackagedFontError> {
        let text_len = c_int::try_from(text.len()).map_err(|_| PackagedFontError::TextTooLong)?;

        // SAFETY: no arguments; returned allocation is checked.
        let buffer = unsafe { hb_buffer_create() };
        if buffer.is_null() {
            return Err(PackagedFontError::HarfBuzzAllocation("hb_buffer_create"));
        }
        let buffer = HbBufferGuard(buffer);

        // SAFETY: text pointer is valid for text_len bytes and buffer/font are live.
        unsafe {
            hb_buffer_add_utf8(buffer.0, text.as_ptr().cast(), text_len, 0, text_len);
            hb_buffer_set_direction(
                buffer.0,
                if rtl {
                    HB_DIRECTION_RTL
                } else {
                    HB_DIRECTION_LTR
                },
            );
            // Direction is already resolved by FriBidi. This fills only the
            // remaining unset segment properties, notably script/language.
            hb_buffer_guess_segment_properties(buffer.0);
            hb_shape(font, buffer.0, std::ptr::null(), 0);
        }

        let mut info_len = 0_u32;
        let mut position_len = 0_u32;
        // SAFETY: buffer remains live during both returned slice borrows.
        let infos_ptr = unsafe { hb_buffer_get_glyph_infos(buffer.0, &mut info_len) };
        let positions_ptr = unsafe { hb_buffer_get_glyph_positions(buffer.0, &mut position_len) };
        if info_len != position_len {
            return Err(PackagedFontError::HarfBuzzCorruptRun);
        }
        if info_len == 0 {
            return Ok(ShapedRun {
                glyphs: Vec::new(),
                advance_x: 0.0,
            });
        }
        if infos_ptr.is_null() || positions_ptr.is_null() {
            return Err(PackagedFontError::HarfBuzzCorruptRun);
        }
        // SAFETY: pointers reference arrays of the lengths returned above and
        // remain valid until buffer is destroyed at function exit.
        let infos = unsafe { slice::from_raw_parts(infos_ptr, info_len as usize) };
        let positions = unsafe { slice::from_raw_parts(positions_ptr, position_len as usize) };

        let mut pen_x = 0.0;
        let mut pen_y = 0.0;
        let mut glyphs = Vec::with_capacity(info_len as usize);
        for (info, position) in infos.iter().zip(positions) {
            if info.codepoint == 0 {
                return Err(PackagedFontError::MissingGlyph {
                    cluster: cluster_base.saturating_add(info.cluster as usize),
                });
            }
            let x = pen_x + f64::from(position.x_offset) / 64.0;
            let y = -pen_y - f64::from(position.y_offset) / 64.0;
            glyphs.push(Glyph::new(info.codepoint as _, x, y));
            pen_x += f64::from(position.x_advance) / 64.0;
            pen_y += f64::from(position.y_advance) / 64.0;
        }

        Ok(ShapedRun {
            glyphs,
            advance_x: pen_x,
        })
    }

    fn shape(
        &mut self,
        text: &str,
        style: TextStyle,
        origin_x: f64,
        origin_y: f64,
    ) -> Result<ShapedRun, PackagedFontError> {
        self.set_size(style)?;
        let bidi_runs = resolve_bidi_runs(text)?;

        // SAFETY: FT_Face is live and has an active size from set_size.
        let font = unsafe { hb_ft_font_create_referenced(self.owner.face) };
        if font.is_null() {
            return Err(PackagedFontError::HarfBuzzAllocation(
                "hb_ft_font_create_referenced",
            ));
        }
        let font = HbFontGuard(font);

        let mut visual_x = 0.0;
        let mut glyphs = Vec::new();
        for bidi in bidi_runs {
            let segment = &text[bidi.byte_start..bidi.byte_end];
            let mut shaped = self.shape_segment(font.0, segment, bidi.rtl, bidi.byte_start)?;
            let segment_width = shaped.advance_x.abs();
            let segment_origin = visual_x
                + if shaped.advance_x.is_sign_negative() {
                    segment_width
                } else {
                    0.0
                };
            for glyph in &mut shaped.glyphs {
                glyph.set_x(origin_x + segment_origin + glyph.x());
                glyph.set_y(origin_y + glyph.y());
            }
            visual_x += segment_width;
            glyphs.extend(shaped.glyphs);
        }

        Ok(ShapedRun {
            glyphs,
            advance_x: visual_x,
        })
    }
}

struct HbFontGuard(*mut HbFont);

impl Drop for HbFontGuard {
    fn drop(&mut self) {
        // SAFETY: pointer came from hb_ft_font_create_referenced.
        unsafe { hb_font_destroy(self.0) };
    }
}

struct HbBufferGuard(*mut HbBuffer);

impl Drop for HbBufferGuard {
    fn drop(&mut self) {
        // SAFETY: pointer came from hb_buffer_create.
        unsafe { hb_buffer_destroy(self.0) };
    }
}

struct ShapedRun {
    glyphs: Vec<Glyph>,
    advance_x: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackagedBidiRun {
    pub byte_start: usize,
    pub byte_end: usize,
    pub embedding_level: u8,
    pub rtl: bool,
    /// Left-to-right visual run ordinal after UAX #9 reordering.
    pub visual_index: usize,
}

fn resolve_bidi_runs(text: &str) -> Result<Vec<PackagedBidiRun>, PackagedFontError> {
    if text.is_empty() {
        return Ok(Vec::new());
    }

    let mut logical = Vec::new();
    let mut byte_offsets = Vec::new();
    for (byte_offset, ch) in text.char_indices() {
        byte_offsets.push(byte_offset);
        logical.push(ch as u32);
    }
    byte_offsets.push(text.len());

    let len =
        FriBidiStrIndex::try_from(logical.len()).map_err(|_| PackagedFontError::TextTooLong)?;
    let mut visual = vec![0_u32; logical.len()];
    let mut logical_to_visual = vec![0_i32; logical.len()];
    let mut visual_to_logical = vec![0_i32; logical.len()];
    let mut levels = vec![0_i8; logical.len()];
    let mut base_direction = FRIBIDI_PAR_ON;

    // SAFETY: all slices are allocated for exactly len Unicode scalar values.
    let max_level_plus_one = unsafe {
        fribidi_log2vis(
            logical.as_ptr(),
            len,
            &mut base_direction,
            visual.as_mut_ptr(),
            logical_to_visual.as_mut_ptr(),
            visual_to_logical.as_mut_ptr(),
            levels.as_mut_ptr(),
        )
    };
    if max_level_plus_one == 0 {
        return Err(PackagedFontError::FriBidi);
    }

    let mut logical_runs = Vec::new();
    let mut start = 0_usize;
    while start < levels.len() {
        let level = levels[start];
        let mut end = start + 1;
        while end < levels.len() && levels[end] == level {
            end += 1;
        }
        let visual_start = logical_to_visual[start..end]
            .iter()
            .copied()
            .filter(|index| *index >= 0)
            .min()
            .ok_or(PackagedFontError::FriBidi)?;
        logical_runs.push((
            visual_start,
            PackagedBidiRun {
                byte_start: byte_offsets[start],
                byte_end: byte_offsets[end],
                embedding_level: level as u8,
                rtl: level & 1 != 0,
                visual_index: 0,
            },
        ));
        start = end;
    }

    logical_runs.sort_by_key(|(visual_start, run)| (*visual_start, run.byte_start));
    for (visual_index, (_, run)) in logical_runs.iter_mut().enumerate() {
        run.visual_index = visual_index;
    }
    Ok(logical_runs.into_iter().map(|(_, run)| run).collect())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackagedFontSource {
    pub path: PathBuf,
    pub face_index: isize,
}

impl PackagedFontSource {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            face_index: 0,
        }
    }

    pub fn with_face_index(mut self, face_index: isize) -> Self {
        self.face_index = face_index;
        self
    }
}

#[derive(Debug)]
pub enum PackagedFontError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    NotRegularFile(PathBuf),
    InvalidPath(NulError),
    InvalidFaceIndex(isize),
    InvalidSize(f64),
    FreeType {
        operation: &'static str,
        code: FtError,
    },
    HarfBuzzAllocation(&'static str),
    HarfBuzzCorruptRun,
    FriBidi,
    MissingGlyph {
        cluster: usize,
    },
    TextTooLong,
    Cairo(cairo::Error),
}

impl fmt::Display for PackagedFontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "font file {}: {source}", path.display()),
            Self::NotRegularFile(path) => {
                write!(f, "font path is not a regular file: {}", path.display())
            }
            Self::InvalidPath(error) => write!(f, "font path contains NUL: {error}"),
            Self::InvalidFaceIndex(index) => write!(f, "font face index is out of range: {index}"),
            Self::InvalidSize(size) => write!(f, "invalid font size: {size}"),
            Self::FreeType { operation, code } => {
                write!(f, "{operation} failed with FreeType error {code}")
            }
            Self::HarfBuzzAllocation(operation) => {
                write!(f, "{operation} returned a null HarfBuzz object")
            }
            Self::HarfBuzzCorruptRun => f.write_str("HarfBuzz returned inconsistent glyph arrays"),
            Self::FriBidi => f.write_str("FriBidi failed to resolve text embedding levels"),
            Self::MissingGlyph { cluster } => {
                write!(
                    f,
                    "packaged font has no glyph for text cluster byte offset {cluster}"
                )
            }
            Self::TextTooLong => f.write_str("text run is too long for HarfBuzz"),
            Self::Cairo(error) => write!(f, "Cairo font rendering failed: {error}"),
        }
    }
}

impl StdError for PackagedFontError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::InvalidPath(error) => Some(error),
            Self::Cairo(error) => Some(error),
            _ => None,
        }
    }
}

impl From<cairo::Error> for PackagedFontError {
    fn from(value: cairo::Error) -> Self {
        Self::Cairo(value)
    }
}

#[derive(Debug)]
pub enum StrictRenderError {
    Font(PackagedFontError),
    Cairo(cairo::Error),
}

impl fmt::Display for StrictRenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Font(error) => error.fmt(f),
            Self::Cairo(error) => error.fmt(f),
        }
    }
}

impl StdError for StrictRenderError {}

/// FriBidi + HarfBuzz + FreeType backend bound to exact font files.
///
/// No fontconfig lookup, family resolution, or glyph fallback occurs. If the
/// selected file has no glyph for a shaped cluster, strict APIs return
/// MissingGlyph and the generic TextBackend adapter returns Cairo InvalidString.
///
/// FriBidi resolves Unicode embedding levels and visual run order first.
/// HarfBuzz then shapes every resolved run with an explicit LTR/RTL direction
/// against the same exact FreeType face.
pub struct PackagedFontText {
    scratch_context: Context,
    _scratch_surface: ImageSurface,
    regular: RefCell<ExactFace>,
    bold: Option<RefCell<ExactFace>>,
}

impl PackagedFontText {
    /// Use one exact font face for both normal and bold styles.
    pub fn new(source: PackagedFontSource) -> Result<Self, PackagedFontError> {
        Self::from_sources(source, None)
    }

    /// Bind distinct exact files for normal and bold styles.
    pub fn with_bold(
        regular: PackagedFontSource,
        bold: PackagedFontSource,
    ) -> Result<Self, PackagedFontError> {
        Self::from_sources(regular, Some(bold))
    }

    fn from_sources(
        regular: PackagedFontSource,
        bold: Option<PackagedFontSource>,
    ) -> Result<Self, PackagedFontError> {
        let regular = ExactFace::open(regular)?;
        let bold = bold.map(ExactFace::open).transpose()?;
        let surface = ImageSurface::create(Format::ARgb32, 8, 8)?;
        let context = Context::new(&surface)?;
        Ok(Self {
            scratch_context: context,
            _scratch_surface: surface,
            regular: RefCell::new(regular),
            bold: bold.map(RefCell::new),
        })
    }

    pub fn regular_source(&self) -> PackagedFontSource {
        self.regular.borrow().source.clone()
    }

    pub fn bold_source(&self) -> PackagedFontSource {
        self.bold
            .as_ref()
            .map(|face| face.borrow().source.clone())
            .unwrap_or_else(|| self.regular_source())
    }

    fn selected_face(&self, bold: bool) -> &RefCell<ExactFace> {
        if bold {
            self.bold.as_ref().unwrap_or(&self.regular)
        } else {
            &self.regular
        }
    }

    pub fn try_measure(
        &self,
        text: &str,
        style: TextStyle,
    ) -> Result<TextMetrics, PackagedFontError> {
        let mut face = self.selected_face(style.bold).borrow_mut();
        let run = face.shape(text, style, 0.0, 0.0)?;
        self.scratch_context.set_font_face(&face.cairo_face);
        self.scratch_context.set_font_size(style.size);
        let extents = self.scratch_context.font_extents()?;
        Ok(TextMetrics {
            width: run.advance_x,
            height: extents.height(),
            ascent: extents.ascent(),
        })
    }

    pub fn try_draw(
        &self,
        context: &Context,
        origin: crate::Point,
        text: &str,
        style: TextStyle,
    ) -> Result<f64, PackagedFontError> {
        let mut face = self.selected_face(style.bold).borrow_mut();
        let run = face.shape(text, style, origin.x, origin.y)?;
        context.set_font_face(&face.cairo_face);
        context.set_font_size(style.size);
        context.show_glyphs(&run.glyphs)?;
        Ok(origin.x + run.advance_x)
    }

    pub fn validate_text(&self, text: &str, style: TextStyle) -> Result<(), PackagedFontError> {
        self.try_measure(text, style).map(|_| ())
    }

    pub fn bidi_runs(&self, text: &str) -> Result<Vec<PackagedBidiRun>, PackagedFontError> {
        resolve_bidi_runs(text)
    }
}

impl TextMeasurer for PackagedFontText {
    fn measure(&self, text: &str, style: TextStyle) -> TextMetrics {
        self.try_measure(text, style).unwrap_or_default()
    }
}

impl TextBackend for PackagedFontText {
    fn draw(
        &self,
        context: &Context,
        origin: crate::Point,
        text: &str,
        style: TextStyle,
    ) -> Result<f64, cairo::Error> {
        self.try_draw(context, origin, text, style)
            .map_err(|error| match error {
                PackagedFontError::Cairo(error) => error,
                _ => cairo::Error::InvalidString,
            })
    }

    fn capabilities(&self) -> TextBackendCapabilities {
        TextBackendCapabilities {
            shaping: true,
            deterministic_metrics: false,
            explicit_font_family: false,
            exact_font_file: true,
        }
    }
}

impl CairoRenderer<PackagedFontText> {
    pub fn validate_scene_text(&self, scene: &Scene) -> Result<(), PackagedFontError> {
        for node in &scene.paint {
            if let crate::PaintNode::Text { text, style, .. } = node {
                self.text_backend().validate_text(text, *style)?;
            }
        }
        Ok(())
    }

    pub fn render_strict_scene(&self, scene: &Scene) -> Result<RenderedBuffer, StrictRenderError> {
        self.validate_scene_text(scene)
            .map_err(StrictRenderError::Font)?;
        self.render_scene(scene).map_err(StrictRenderError::Cairo)
    }

    pub fn render_strict<M: MenuSource>(
        &self,
        menu: &M,
        interaction: &InteractionState,
        viewport: Viewport,
        theme: &Theme,
        options: RenderOptions,
    ) -> Result<(Scene, RenderedBuffer), StrictRenderError> {
        let scene = self.build_scene(menu, interaction, viewport, theme, options);
        let buffer = self.render_strict_scene(&scene)?;
        Ok((scene, buffer))
    }
}
