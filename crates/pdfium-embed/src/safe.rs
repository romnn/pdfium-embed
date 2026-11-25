//! Thread-safe wrapper around the pdfium C library.
//!
//! The pdfium C library is **not** thread-safe — its global parser state and
//! error codes are shared and unprotected. The `pdfium-render` crate's
//! `thread_safe` feature only adds `unsafe impl Send + Sync` to the Rust
//! wrapper types; it does **not** serialise FFI calls.
//!
//! This module provides a safe, idiomatic API that serialises all access
//! through a process-wide [`parking_lot::Mutex`]. Callers never touch raw
//! pdfium-render types.

use crate::Library;
use pdfium_render::prelude as pr;
use std::cell::Cell;
use std::marker::PhantomData;
use std::sync::OnceLock;

/// Bounding rectangle in PDF point coordinates.
///
/// PDF points are 1/72 inch. The coordinate origin is at the bottom-left
/// of the page; `top > bottom` and `right > left`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Left edge, in points.
    pub left: f32,
    /// Bottom edge, in points.
    pub bottom: f32,
    /// Right edge, in points.
    pub right: f32,
    /// Top edge, in points.
    pub top: f32,
}

impl Rect {
    /// Creates a rectangle from its edges, without checking their order.
    #[must_use]
    pub fn new(left: f32, bottom: f32, right: f32, top: f32) -> Self {
        Self {
            left,
            bottom,
            right,
            top,
        }
    }

    /// Returns the width in points, or zero when `right` is left of `left`.
    #[must_use]
    pub fn width(&self) -> f32 {
        (self.right - self.left).max(0.0)
    }

    /// Returns the height in points, or zero when `top` is below `bottom`.
    #[must_use]
    pub fn height(&self) -> f32 {
        (self.top - self.bottom).max(0.0)
    }

    /// Returns `true` if this rectangle overlaps `other` (shares nonzero area).
    #[must_use]
    pub fn does_overlap(&self, other: &Rect) -> bool {
        self.left < other.right
            && self.right > other.left
            && self.bottom < other.top
            && self.top > other.bottom
    }

    /// Returns the smallest rectangle that contains both `self` and `other`.
    #[must_use]
    pub fn union(&self, other: Rect) -> Rect {
        Rect {
            left: self.left.min(other.left),
            bottom: self.bottom.min(other.bottom),
            right: self.right.max(other.right),
            top: self.top.max(other.top),
        }
    }

    /// Returns a new rectangle expanded by `margin` on every side.
    #[must_use]
    pub fn expand(&self, margin: f32) -> Rect {
        Rect {
            left: self.left - margin,
            bottom: self.bottom - margin,
            right: self.right + margin,
            top: self.top + margin,
        }
    }

    fn from_pdfium(rect: pr::PdfRect) -> Self {
        Self {
            left: rect.left().value,
            bottom: rect.bottom().value,
            right: rect.right().value,
            top: rect.top().value,
        }
    }
}

/// A rectangle on the displayed page, normalized to `[0, 1]` with a top-left origin.
///
/// `y` grows downward, so `top` is always less than `bottom`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplayRect {
    /// Left edge, as a fraction of the displayed page width.
    pub left: f32,
    /// Top edge, as a fraction of the displayed page height.
    pub top: f32,
    /// Right edge, as a fraction of the displayed page width.
    pub right: f32,
    /// Bottom edge, as a fraction of the displayed page height.
    pub bottom: f32,
}

/// Validated visible PDF box and intrinsic clockwise page rotation.
///
/// The box remains in unrotated PDF coordinates; normalized output uses the displayed page's
/// top-left origin and downward y axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageDisplayFrame {
    visible_box: Rect,
    rotation_degrees: u16,
}

impl PageDisplayFrame {
    /// Validates finite, nonempty bounds and a supported quarter-turn rotation.
    #[must_use]
    pub fn new(visible_box: Rect, rotation_degrees: u16) -> Option<Self> {
        let Rect {
            left,
            bottom,
            right,
            top,
        } = visible_box;
        if ![left, bottom, right, top, right - left, top - bottom]
            .into_iter()
            .all(f32::is_finite)
            || right <= left
            || top <= bottom
            || !matches!(rotation_degrees, 0 | 90 | 180 | 270)
        {
            return None;
        }
        Some(Self {
            visible_box,
            rotation_degrees,
        })
    }

    /// Returns the effective crop/media intersection in raw PDF coordinates.
    #[must_use]
    pub fn visible_box(self) -> Rect {
        self.visible_box
    }

    /// Returns intrinsic clockwise rotation, in degrees.
    #[must_use]
    pub fn rotation_degrees(self) -> u16 {
        self.rotation_degrees
    }

    /// Maps a raw PDF point into the normalized displayed page frame.
    #[must_use]
    pub fn normalized_point(self, raw_x: f32, raw_y: f32) -> Option<(f32, f32)> {
        let bounds = self.visible_box;
        let u = (raw_x - bounds.left) / (bounds.right - bounds.left);
        let v = (bounds.top - raw_y) / (bounds.top - bounds.bottom);
        if !u.is_finite() || !v.is_finite() {
            return None;
        }
        Some(match self.rotation_degrees {
            90 => (1.0 - v, u),
            180 => (1.0 - u, 1.0 - v),
            270 => (v, 1.0 - u),
            _ => (u, v),
        })
    }

    /// Clips a raw rectangle to the visible page and transforms all corners.
    ///
    /// Returns `None` for non-finite, inverted, or wholly invisible rectangles.
    #[must_use]
    pub fn normalized_rect(self, rect: Rect) -> Option<DisplayRect> {
        if ![rect.left, rect.bottom, rect.right, rect.top]
            .into_iter()
            .all(f32::is_finite)
            || rect.left >= rect.right
            || rect.bottom >= rect.top
        {
            return None;
        }
        let b = self.visible_box;
        let left = rect.left.max(b.left);
        let right = rect.right.min(b.right);
        let bottom = rect.bottom.max(b.bottom);
        let top = rect.top.min(b.top);
        if left >= right || bottom >= top {
            return None;
        }
        let mut output = DisplayRect {
            left: f32::INFINITY,
            top: f32::INFINITY,
            right: f32::NEG_INFINITY,
            bottom: f32::NEG_INFINITY,
        };
        for (x, y) in [(left, bottom), (left, top), (right, bottom), (right, top)] {
            let (x, y) = self.normalized_point(x, y)?;
            output.left = output.left.min(x);
            output.right = output.right.max(x);
            output.top = output.top.min(y);
            output.bottom = output.bottom.max(y);
        }
        Some(output)
    }
}

/// Page dimensions in PDF points (1 pt = 1/72 inch).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageDimensions {
    /// Displayed page width, in points.
    pub width_pts: f32,
    /// Displayed page height, in points.
    pub height_pts: f32,
}

/// A text segment extracted from a PDF page.
#[derive(Debug, Clone)]
pub struct TextSegment {
    /// The segment's text.
    pub text: String,
    /// The segment's bounding box on the page.
    pub bounds: Rect,
}

/// A rendered PDF page as an RGBA image.
pub struct RenderedPage {
    /// The page as displayed: its visible box, with its intrinsic rotation applied.
    pub image: image::RgbaImage,
    /// Displayed page width, in points.
    pub width_pts: f32,
    /// Displayed page height, in points.
    pub height_pts: f32,
}

impl std::fmt::Debug for RenderedPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderedPage")
            .field(
                "image",
                &format_args!("{}x{}", self.image.width(), self.image.height()),
            )
            .field("width_pts", &self.width_pts)
            .field("height_pts", &self.height_pts)
            .finish()
    }
}

/// Thread-safe entry point to the pdfium C library.
///
/// Every method acquires a process-wide mutex before calling into pdfium,
/// so concurrent use from many threads or async tasks is safe.
///
/// Obtain the singleton via [`Pdfium::instance`] or [`Pdfium::bind`].
pub struct Pdfium {
    mutex: parking_lot::Mutex<()>,
}

/// The library a process bound, and the instance bound to it.
struct Bound {
    library: Library,
    pdfium: Pdfium,
}

/// Set once a library is bound: pdfium-render keeps its bindings in a process-wide cell that can
/// be set only once.
static BOUND: OnceLock<Bound> = OnceLock::new();

/// Serializes binding attempts, so that only one thread loads a library.
static BINDING: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

impl Pdfium {
    /// Returns the process-global [`Pdfium`] instance, binding the default [`Library`] when
    /// none is bound yet.
    ///
    /// When [`Pdfium::bind`] already bound a library, this returns the instance it bound.
    ///
    /// # Errors
    ///
    /// Returns an error if the default library cannot be loaded or bound.
    /// A failed attempt binds nothing, so a later call tries again.
    pub fn instance() -> Result<&'static Pdfium, crate::InitError> {
        Ok(&Self::bound_or_bind(&Library::default())?.pdfium)
    }

    /// Returns the process-global [`Pdfium`] instance, binding `library` when none is bound yet.
    ///
    /// Call it before any [`Pdfium::instance`] call to pick a library other than the default.
    ///
    /// # Errors
    ///
    /// Returns [`InitError::AlreadyBound`](crate::InitError::AlreadyBound) if the process
    /// already bound another library, and otherwise the error binding `library` gave.
    /// A failed attempt binds nothing, so a later call tries again.
    pub fn bind(library: &Library) -> Result<&'static Pdfium, crate::InitError> {
        let bound = Self::bound_or_bind(library)?;
        if bound.library != *library {
            return Err(crate::InitError::AlreadyBound);
        }
        Ok(&bound.pdfium)
    }

    /// Returns the bound library, binding `library` when none is bound yet.
    fn bound_or_bind(library: &Library) -> Result<&'static Bound, crate::InitError> {
        if let Some(bound) = BOUND.get() {
            return Ok(bound);
        }
        let _binding = BINDING.lock();
        if let Some(bound) = BOUND.get() {
            return Ok(bound);
        }
        crate::library::bind(library)?;
        Ok(BOUND.get_or_init(|| Bound {
            library: library.clone(),
            pdfium: Pdfium {
                mutex: parking_lot::Mutex::new(()),
            },
        }))
    }

    /// Returns the number of pages in a PDF.
    ///
    /// # Errors
    ///
    /// Returns an error if the PDF cannot be loaded.
    pub fn num_pages(&self, pdf_bytes: &[u8]) -> Result<u32, crate::DocumentError> {
        self.with_document(pdf_bytes, |doc| Ok(doc.num_pages()))
    }

    /// Returns `true` if pdfium can successfully load the given bytes as a PDF.
    #[must_use]
    pub fn can_load(&self, pdf_bytes: &[u8]) -> bool {
        self.with_document(pdf_bytes, |_doc| Ok::<(), crate::DocumentError>(()))
            .is_ok()
    }

    /// Returns the dimensions of a single page.
    ///
    /// # Errors
    ///
    /// Returns an error if the PDF cannot be loaded or the page index is out of range.
    pub fn page_dimensions(
        &self,
        pdf_bytes: &[u8],
        page_index: u32,
    ) -> Result<PageDimensions, crate::DocumentError> {
        self.with_document(pdf_bytes, |doc| doc.page_dimensions(page_index))
    }

    /// Renders a single page to an RGBA image at the given DPI, limiting its
    /// longest pixel edge to `max_dimension`.
    ///
    /// The bound is mandatory so a PDF with an oversized declared page size
    /// cannot allocate an unbounded bitmap; pass
    /// [`crate::DEFAULT_MAX_RENDER_DIMENSION`] when no stricter cap applies.
    ///
    /// # Errors
    ///
    /// Returns an error if the PDF cannot be loaded, the page index is out of
    /// range, the dimensions are invalid, or rendering fails.
    pub fn render_page(
        &self,
        pdf_bytes: &[u8],
        page_index: u32,
        dpi: u16,
        max_dimension: u32,
    ) -> Result<RenderedPage, crate::DocumentError> {
        self.with_document(pdf_bytes, |doc| {
            doc.render_page(page_index, dpi, max_dimension)
        })
    }

    /// Extracts the text on the visible part of a page as a single string.
    ///
    /// It allocates as much text as the page holds; [`Document::page_text_limited`] bounds it.
    ///
    /// # Errors
    ///
    /// Returns an error if the PDF cannot be loaded or text extraction fails.
    pub fn page_text(
        &self,
        pdf_bytes: &[u8],
        page_index: u32,
    ) -> Result<String, crate::DocumentError> {
        self.with_document(pdf_bytes, |doc| doc.page_text(page_index))
    }

    /// Extracts text segments (text + bounding box) from a page.
    ///
    /// # Errors
    ///
    /// Returns an error if the PDF cannot be loaded or text extraction fails.
    pub fn page_text_segments(
        &self,
        pdf_bytes: &[u8],
        page_index: u32,
    ) -> Result<Vec<TextSegment>, crate::DocumentError> {
        self.with_document(pdf_bytes, |doc| doc.page_text_segments(page_index))
    }

    /// Extracts visual words (text + tight bounding box) from a page.
    ///
    /// See [`Document::page_text_words`] for why this differs from
    /// [`page_text_segments`](Self::page_text_segments).
    ///
    /// # Errors
    ///
    /// Returns an error if the PDF cannot be loaded or text extraction fails.
    pub fn page_text_words(
        &self,
        pdf_bytes: &[u8],
        page_index: u32,
    ) -> Result<Vec<TextSegment>, crate::DocumentError> {
        self.with_document(pdf_bytes, |doc| doc.page_text_words(page_index))
    }

    /// Copies a contiguous page range into a new PDF and returns the new document bytes.
    ///
    /// Page indices are zero-based. The copied pages preserve their original PDF content rather
    /// than rendering pages to images.
    ///
    /// # Errors
    ///
    /// Returns an error if the PDF cannot be loaded, the page range is invalid, or the copied
    /// document cannot be saved.
    pub fn copy_page_range(
        &self,
        pdf_bytes: &[u8],
        start_page_index: u32,
        page_count: u32,
    ) -> Result<Vec<u8>, crate::DocumentError> {
        let _guard = self.mutex.lock();
        let pdfium = pr::Pdfium::default();
        let source_document = load_document(&pdfium, pdf_bytes, None)?;

        if page_count == 0 {
            return Err(crate::DocumentError::EmptyPageRange);
        }

        let total_pages_i32 = source_document.pages().len();
        let total_pages = u32::try_from(total_pages_i32).unwrap_or(u32::MAX);
        let end_page_index = start_page_index
            .checked_add(page_count)
            .and_then(|exclusive_end| exclusive_end.checked_sub(1))
            .ok_or(crate::DocumentError::PageRangeOverflow)?;

        if end_page_index >= total_pages {
            return Err(crate::DocumentError::PageRangeOutOfBounds {
                start_page_index,
                end_page_index,
                total_pages,
            });
        }

        let start_page_index_i32 =
            i32::try_from(start_page_index).map_err(crate::DocumentError::PageIndexOutOfRange)?;
        let end_page_index_i32 =
            i32::try_from(end_page_index).map_err(crate::DocumentError::PageIndexOutOfRange)?;

        let mut destination_document = pdfium
            .create_new_pdf()
            .map_err(crate::DocumentError::CreateDocument)?;
        destination_document
            .pages_mut()
            .copy_page_range_from_document(
                &source_document,
                start_page_index_i32..=end_page_index_i32,
                0,
            )
            .map_err(crate::DocumentError::CopyPageRange)?;

        destination_document
            .save_to_bytes()
            .map_err(crate::DocumentError::SaveDocument)
    }

    /// Opens a PDF and passes a [`Document`] handle to the closure.
    ///
    /// The mutex is held for the closure's entire duration. Use this for
    /// multi-page workflows that should not re-load the PDF per page.
    ///
    /// The closure must do its pdfium work through the [`Document`]: calling another [`Pdfium`]
    /// method from it, or waiting on a thread that does, deadlocks on the held mutex.
    ///
    /// # Errors
    ///
    /// Returns [`DocumentError::Encrypted`](crate::DocumentError::Encrypted) if the PDF needs a
    /// password, another error if it cannot be loaded, and the closure's error if it fails.
    pub fn with_document<F, R, E>(&self, pdf_bytes: &[u8], f: F) -> Result<R, E>
    where
        F: FnOnce(&Document<'_>) -> Result<R, E>,
        E: From<crate::DocumentError>,
    {
        self.open_document(pdf_bytes, None, f)
    }

    /// Opens an encrypted PDF with `password` and passes a [`Document`] handle to the closure.
    ///
    /// It works as [`with_document`](Self::with_document) does; either the user or the owner
    /// password opens the document, and a PDF that is not encrypted ignores it.
    ///
    /// # Errors
    ///
    /// Returns [`DocumentError::Encrypted`](crate::DocumentError::Encrypted) if the password is
    /// wrong, another error if the PDF cannot be loaded, and the closure's error if it fails.
    pub fn with_document_password<F, R, E>(
        &self,
        pdf_bytes: &[u8],
        password: &str,
        f: F,
    ) -> Result<R, E>
    where
        F: FnOnce(&Document<'_>) -> Result<R, E>,
        E: From<crate::DocumentError>,
    {
        self.open_document(pdf_bytes, Some(password), f)
    }

    fn open_document<F, R, E>(&self, pdf_bytes: &[u8], password: Option<&str>, f: F) -> Result<R, E>
    where
        F: FnOnce(&Document<'_>) -> Result<R, E>,
        E: From<crate::DocumentError>,
    {
        let _guard = self.mutex.lock();
        let pdfium = pr::Pdfium::default();
        let inner = load_document(&pdfium, pdf_bytes, password)?;
        let doc = Document {
            inner,
            not_sync: PhantomData,
        };
        f(&doc)
    }
}

/// Loads a PDF, telling an encrypted document pdfium cannot open from any other failure.
fn load_document<'a>(
    pdfium: &'a pr::Pdfium,
    pdf_bytes: &'a [u8],
    password: Option<&str>,
) -> Result<pr::PdfDocument<'a>, crate::DocumentError> {
    use pr::PdfiumInternalError::{PasswordError, SecurityError};

    pdfium
        .load_pdf_from_byte_slice(pdf_bytes, password)
        .map_err(|source| {
            if matches!(
                source,
                pr::PdfiumError::PdfiumLibraryInternalError(PasswordError | SecurityError)
            ) {
                crate::DocumentError::Encrypted(source)
            } else {
                crate::DocumentError::LoadDocument(source)
            }
        })
}

/// A loaded PDF document, only accessible inside [`Pdfium::with_document`]
/// closures. The process-wide mutex is held for its lifetime.
///
/// It is not [`Sync`], so threads the closure spawns cannot call into pdfium through it while
/// the closure's thread does:
///
/// ```compile_fail
/// fn assert_sync<T: Sync>() {}
/// assert_sync::<pdfium_embed::Document<'static>>();
/// ```
pub struct Document<'a> {
    inner: pr::PdfDocument<'a>,
    /// pdfium-render marks `PdfDocument` `Sync`, which would let scoped threads share the
    /// document and call into pdfium at the same time.
    not_sync: PhantomData<Cell<()>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RenderDimensions {
    width: i32,
    height: i32,
}

fn render_dimensions(
    width_pts: f32,
    height_pts: f32,
    dpi: u16,
    max_dimension: u32,
) -> Result<RenderDimensions, crate::DocumentError> {
    if !width_pts.is_finite()
        || !height_pts.is_finite()
        || width_pts <= 0.0
        || height_pts <= 0.0
        || dpi == 0
        || max_dimension == 0
    {
        return Err(crate::DocumentError::InvalidRenderDimensions);
    }

    let width = f64::from(width_pts) * f64::from(dpi) / 72.0;
    let height = f64::from(height_pts) * f64::from(dpi) / 72.0;
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return Err(crate::DocumentError::InvalidRenderDimensions);
    }

    let maximum = f64::from(max_dimension).min(f64::from(i32::MAX));
    let scale = (maximum / width.max(height)).min(1.0);

    #[expect(
        clippy::cast_possible_truncation,
        reason = "the finite value is rounded and clamped to the positive i32 pixel range"
    )]
    let width = (width * scale).round().clamp(1.0, maximum) as i32;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the finite value is rounded and clamped to the positive i32 pixel range"
    )]
    let height = (height * scale).round().clamp(1.0, maximum) as i32;

    Ok(RenderDimensions { width, height })
}

impl Document<'_> {
    /// Returns the number of pages, saturating at `u32::MAX`.
    #[must_use]
    pub fn num_pages(&self) -> u32 {
        self.inner.pages().len().try_into().unwrap_or(u32::MAX)
    }

    /// Returns whether the document is encrypted, including when it opened without a password.
    ///
    /// A PDF with an empty user password opens without one, yet its owner can still have
    /// restricted what readers may do with it, such as copying its text.
    #[must_use]
    pub fn is_encrypted(&self) -> bool {
        // pdfium reports no security handler only for a document without an encryption
        // dictionary; pdfium-render maps revisions 2 to 4 and reports any other one, such as
        // AES-256's 5 and 6, as an error, which still means the document is encrypted.
        !matches!(
            self.inner.permissions().security_handler_revision(),
            Ok(pr::PdfSecurityHandlerRevision::Unprotected)
        )
    }

    /// Returns the dimensions of one page in PDF points.
    ///
    /// # Errors
    ///
    /// Returns an error if the page index is out of range.
    pub fn page_dimensions(&self, page_index: u32) -> Result<PageDimensions, crate::DocumentError> {
        let page = self.get_page(page_index)?;
        Ok(PageDimensions {
            width_pts: page.width().value,
            height_pts: page.height().value,
        })
    }

    /// Resolves inherited crop/media bounds and intrinsic page rotation.
    ///
    /// # Errors
    ///
    /// Returns an error if the page cannot be read or its visible frame is invalid.
    pub fn page_display_frame(
        &self,
        page_index: u32,
    ) -> Result<PageDisplayFrame, crate::DocumentError> {
        use pr::PdfPageRenderRotation;

        let page = self.get_page(page_index)?;
        let bounds = page
            .boundaries()
            .bounding()
            .map_err(crate::DocumentError::PageDisplayFrame)?;
        let rotation = page
            .rotation()
            .map_err(crate::DocumentError::PageDisplayFrame)?;
        let degrees = match rotation {
            PdfPageRenderRotation::None => 0,
            PdfPageRenderRotation::Degrees90 => 90,
            PdfPageRenderRotation::Degrees180 => 180,
            PdfPageRenderRotation::Degrees270 => 270,
        };
        PageDisplayFrame::new(Rect::from_pdfium(bounds.bounds), degrees)
            .ok_or(crate::DocumentError::InvalidDisplayFrame)
    }

    /// Renders one page, limiting its longest pixel edge to `max_dimension`.
    ///
    /// # Errors
    ///
    /// Returns an error if the page index is out of range, the dimensions are
    /// invalid, or rendering fails.
    pub fn render_page(
        &self,
        page_index: u32,
        dpi: u16,
        max_dimension: u32,
    ) -> Result<RenderedPage, crate::DocumentError> {
        let page = self.get_page(page_index)?;
        let width_pts = page.width().value;
        let height_pts = page.height().value;
        let dimensions = render_dimensions(width_pts, height_pts, dpi, max_dimension)?;

        // A fixed size renders exactly the bounded dimensions; target sizes would make
        // pdfium-render recompute them in `f32`, which overflows on extremely narrow pages.
        let config = pr::PdfRenderConfig::new()
            .set_fixed_size(dimensions.width, dimensions.height)
            .render_form_data(true);

        let rendered = page
            .render_with_config(&config)
            .map_err(crate::DocumentError::RenderPage)?
            .as_image()
            .map_err(crate::DocumentError::ConvertRenderedPage)?
            .into_rgba8();

        Ok(RenderedPage {
            image: rendered,
            width_pts,
            height_pts,
        })
    }

    /// Extracts the text on the visible part of one page.
    ///
    /// It allocates as much text as the page holds; [`page_text_limited`](Self::page_text_limited)
    /// bounds it.
    ///
    /// # Errors
    ///
    /// Returns an error if the page index is out of range, its boundaries cannot be read, or
    /// text extraction fails.
    pub fn page_text(&self, page_index: u32) -> Result<String, crate::DocumentError> {
        self.page_text_limited(page_index, usize::MAX)
    }

    /// Extracts the text on the visible part of one page, if the page holds at most `max_chars`
    /// characters.
    ///
    /// The limit applies to every character pdfium finds on the page, the ones outside the
    /// visible box included, and is checked before the text is copied out of pdfium.
    /// The returned text then has at most `max_chars` characters as well.
    ///
    /// # Errors
    ///
    /// Returns [`DocumentError::PageTextTooLong`](crate::DocumentError::PageTextTooLong) if the
    /// page holds more than `max_chars` characters, and another error if the page index is out
    /// of range, its boundaries cannot be read, or text extraction fails.
    pub fn page_text_limited(
        &self,
        page_index: u32,
        max_chars: usize,
    ) -> Result<String, crate::DocumentError> {
        let too_long = crate::DocumentError::PageTextTooLong {
            page_index,
            max_chars,
        };
        let page = self.get_page(page_index)?;
        // pdfium-render's `PdfPageText::all` asks for the text in a rectangle at the origin with
        // the displayed page size, which misses text on rotated or offset pages; the visible box
        // is in the raw coordinates pdfium's text lookup uses.
        let visible_box = page
            .boundaries()
            .bounding()
            .map_err(crate::DocumentError::ExtractPageText)?
            .bounds;
        let text = page.text().map_err(crate::DocumentError::ExtractPageText)?;

        // `inside_rect` allocates whatever length pdfium reports for the box, and no API reports
        // it without that allocation.
        // pdfium's text for the box holds each character inside it as one or two UTF-16 units,
        // plus at most a space or one line break for each character outside it, so checking the
        // page's character count first bounds the allocation to two units per allowed character.
        let page_chars = usize::try_from(text.len()).unwrap_or(usize::MAX);
        if page_chars > max_chars {
            return Err(too_long);
        }
        let visible_text = text.inside_rect(visible_box);
        if visible_text.chars().count() > max_chars {
            return Err(too_long);
        }
        Ok(visible_text)
    }

    /// Extracts the positioned text segments of one page.
    ///
    /// # Errors
    ///
    /// Returns an error if the page index is out of range or text extraction fails.
    pub fn page_text_segments(
        &self,
        page_index: u32,
    ) -> Result<Vec<TextSegment>, crate::DocumentError> {
        let page = self.get_page(page_index)?;
        let text = page.text().map_err(crate::DocumentError::ExtractPageText)?;
        let segments = text
            .segments()
            .iter()
            .map(|segment| TextSegment {
                text: segment.text(),
                bounds: Rect::from_pdfium(segment.bounds()),
            })
            .collect();
        Ok(segments)
    }

    /// Extracts visual words (text + tight bounding box) from a page.
    ///
    /// [`page_text_segments`](Self::page_text_segments) returns the line-level runs
    /// produced by pdfium's `FPDFText_CountRects`: a single rectangle merges every
    /// character on a line that shares the same font, so its text routinely contains
    /// spaces and can span the whole line.
    /// This instead returns one box per visual word: it walks individual characters
    /// (`FPDFText_GetCharBox`) and groups adjacent glyphs into words, unioning
    /// their tight glyph bounds.
    /// Whitespace always starts a new word.
    /// A noticeably smaller glyph raised or lowered next to a word, such as a footnote
    /// marker, also starts one even when the PDF text stream stores it next to the base
    /// word without whitespace; this is a heuristic over glyph sizes and positions.
    ///
    /// A word is only emitted once at least one of its characters contributed a
    /// bounding box.
    ///
    /// # Errors
    ///
    /// Returns an error if the page index is out of range or text extraction fails.
    pub fn page_text_words(
        &self,
        page_index: u32,
    ) -> Result<Vec<TextSegment>, crate::DocumentError> {
        let page = self.get_page(page_index)?;
        let text = page.text().map_err(crate::DocumentError::ExtractPageText)?;

        let mut words: Vec<TextSegment> = Vec::new();
        let mut current_text = String::new();
        let mut current_bounds: Option<Rect> = None;
        let mut previous_glyph_bounds: Option<Rect> = None;
        let mut previous_glyph: Option<char> = None;

        for character in text.chars().iter() {
            let unicode = character.unicode_char();
            let is_separator = unicode.is_none_or(char::is_whitespace);

            if is_separator {
                push_current_word(
                    &mut words,
                    &mut current_text,
                    &mut current_bounds,
                    &mut previous_glyph_bounds,
                    &mut previous_glyph,
                );
                continue;
            }

            let bounds = character.tight_bounds().ok().map(Rect::from_pdfium);

            if let (Some(previous_bounds), Some(previous), Some(next_bounds), Some(next)) =
                (previous_glyph_bounds, previous_glyph, bounds, unicode)
                && should_start_new_text_word(previous_bounds, previous, next_bounds, next)
            {
                push_current_word(
                    &mut words,
                    &mut current_text,
                    &mut current_bounds,
                    &mut previous_glyph_bounds,
                    &mut previous_glyph,
                );
            }

            if let Some(value) = unicode {
                current_text.push(value);
            }
            if let Some(rect) = bounds {
                current_bounds = Some(match current_bounds {
                    Some(existing) => existing.union(rect),
                    None => rect,
                });
                if unicode.is_some_and(char::is_alphanumeric) {
                    previous_glyph_bounds = Some(rect);
                    previous_glyph = unicode;
                }
            }
        }

        push_current_word(
            &mut words,
            &mut current_text,
            &mut current_bounds,
            &mut previous_glyph_bounds,
            &mut previous_glyph,
        );

        Ok(words)
    }

    fn get_page(&self, page_index: u32) -> Result<pr::PdfPage<'_>, crate::DocumentError> {
        let page_index_i32 =
            i32::try_from(page_index).map_err(crate::DocumentError::PageIndexOutOfRange)?;
        self.inner
            .pages()
            .get(page_index_i32)
            .map_err(|source| crate::DocumentError::GetPage { page_index, source })
    }
}

fn push_current_word(
    words: &mut Vec<TextSegment>,
    current_text: &mut String,
    current_bounds: &mut Option<Rect>,
    previous_glyph_bounds: &mut Option<Rect>,
    previous_glyph: &mut Option<char>,
) {
    if let Some(bounds) = current_bounds.take() {
        words.push(TextSegment {
            text: std::mem::take(current_text),
            bounds,
        });
    } else {
        current_text.clear();
    }
    *previous_glyph_bounds = None;
    *previous_glyph = None;
}

fn should_start_new_text_word(
    previous_bounds: Rect,
    previous: char,
    next_bounds: Rect,
    next: char,
) -> bool {
    const MARKER_MAX_SIZE_RATIO: f32 = 0.85;
    const MARKER_MIN_BASELINE_OFFSET_RATIO: f32 = 0.28;
    const MARKER_MIN_CENTER_OFFSET_RATIO: f32 = 0.22;
    const MARKER_MIN_VERTICAL_OVERLAP_RATIO: f32 = 0.10;
    const MARKER_MIN_GAP_RATIO: f32 = -0.35;
    const MARKER_MAX_GAP_RATIO: f32 = 1.00;

    if !previous.is_alphanumeric() || !next.is_alphanumeric() {
        return false;
    }

    let previous_height = previous_bounds.height();
    let next_height = next_bounds.height();
    let max_height = previous_height.max(next_height);
    let min_height = previous_height.min(next_height);
    if max_height <= f32::EPSILON {
        return false;
    }

    let size_ratio = min_height / max_height;
    if size_ratio > MARKER_MAX_SIZE_RATIO {
        return false;
    }

    let previous_center_y = f32::midpoint(previous_bounds.bottom, previous_bounds.top);
    let next_center_y = f32::midpoint(next_bounds.bottom, next_bounds.top);
    let baseline_offset = (previous_bounds.bottom - next_bounds.bottom).abs() / max_height;
    let center_offset = (previous_center_y - next_center_y).abs() / max_height;
    if baseline_offset < MARKER_MIN_BASELINE_OFFSET_RATIO
        || center_offset < MARKER_MIN_CENTER_OFFSET_RATIO
    {
        return false;
    }

    let vertical_overlap = (previous_bounds.top.min(next_bounds.top)
        - previous_bounds.bottom.max(next_bounds.bottom))
    .max(0.0)
        / min_height.max(f32::EPSILON);
    if vertical_overlap < MARKER_MIN_VERTICAL_OVERLAP_RATIO {
        return false;
    }

    let gap_ratio = (next_bounds.left - previous_bounds.right) / max_height;
    (MARKER_MIN_GAP_RATIO..=MARKER_MAX_GAP_RATIO).contains(&gap_ratio)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_dimensions_below_bound() -> Result<(), crate::DocumentError> {
        let dimensions = render_dimensions(612.0, 792.0, 300, 4096)?;

        assert_eq!(
            dimensions,
            RenderDimensions {
                width: 2550,
                height: 3300
            }
        );
        Ok(())
    }

    #[test]
    fn clamps_longest_edge_before_rendering() -> Result<(), crate::DocumentError> {
        let dimensions = render_dimensions(72_000.0, 36_000.0, 300, 4096)?;

        assert_eq!(
            dimensions,
            RenderDimensions {
                width: 4096,
                height: 2048
            }
        );
        Ok(())
    }

    #[test]
    fn rejects_invalid_dimensions() {
        assert!(render_dimensions(f32::NAN, 792.0, 300, 4096).is_err());
        assert!(render_dimensions(612.0, 792.0, 0, 4096).is_err());
        assert!(render_dimensions(612.0, 792.0, 300, 0).is_err());
    }

    #[cfg(feature = "bundled")]
    mod display_frame_oracle {
        use super::*;
        use color_eyre::eyre;
        use std::path::PathBuf;

        /// The normalized frame agrees with `PDFium`'s own page-to-device mapping for every quarter
        /// turn.
        #[test]
        fn frame_matches_pdfium_page_to_device_with_no_additional_rotation() -> eyre::Result<()> {
            let pdfium = Pdfium::instance()?;
            for rotation in [0, 90, 180, 270] {
                let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join(format!("tests/fixtures/cropped-rotate-{rotation}.pdf"));
                let bytes = std::fs::read(path)?;
                pdfium.with_document(&bytes, |document| -> eyre::Result<()> {
                    let frame = document.page_display_frame(0)?;
                    let page = document.get_page(0)?;
                    let (width, height) = if rotation == 90 || rotation == 270 {
                        (1440, 1080)
                    } else {
                        (1080, 1440)
                    };
                    let config = pr::PdfRenderConfig::new()
                        .set_target_width(width)
                        .set_target_height(height)
                        .rotate(pr::PdfPageRenderRotation::None, true);
                    for (x, y) in [(90.0, 684.0), (198.0, 612.0), (36.0, 792.0)] {
                        let (device_x, device_y) = page.points_to_pixels(
                            pr::PdfPoints::new(x),
                            pr::PdfPoints::new(y),
                            &config,
                        )?;
                        let (normalized_x, normalized_y) = frame
                            .normalized_point(x, y)
                            .ok_or_else(|| eyre::eyre!("fixture point rejected"))?;
                        assert!(
                            (f64::from(device_x) - f64::from(normalized_x) * f64::from(width))
                                .abs()
                                <= 1.0
                        );
                        assert!(
                            (f64::from(device_y) - f64::from(normalized_y) * f64::from(height))
                                .abs()
                                <= 1.0
                        );
                    }
                    Ok(())
                })?;
            }
            Ok(())
        }
    }
}
