#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod library;
mod safe;

use pdfium_render::prelude::PdfiumError;
use thiserror::Error;

pub use library::Library;
pub use safe::{
    DisplayRect, Document, PageDimensions, PageDisplayFrame, Pdfium, Rect, RenderedPage,
    TextSegment,
};

/// Default cap for a rendered page's longest pixel edge.
///
/// Every render call takes an explicit bound; this constant is the value to pass when a caller
/// has no stricter requirement.
pub const DEFAULT_MAX_RENDER_DIMENSION: u32 = 4096;

/// An error binding the pdfium library.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum InitError {
    /// The embedded library could not be written to a new temporary directory.
    #[cfg(feature = "bundled")]
    #[error("failed to write the bundled pdfium library to {}", directory.display())]
    WriteBundledLibrary {
        /// The temporary directory, or the system's temporary directory if creating it failed.
        directory: std::path::PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The library could not be loaded, or lacks a symbol pdfium-render needs.
    #[error("failed to bind to pdfium library")]
    BindLibrary(#[source] PdfiumError),
    /// The process already bound a different [`Library`].
    #[error("pdfium is already bound to a different library")]
    AlreadyBound,
}

/// An error loading, reading, rendering or writing a PDF document.
#[derive(Debug, Error)]
pub enum DocumentError {
    /// The bytes are not a PDF pdfium can load.
    #[error("failed to load pdf document")]
    LoadDocument(#[source] PdfiumError),
    /// The page index exceeds the range pdfium addresses pages with.
    #[error("page index does not fit in i32")]
    PageIndexOutOfRange(#[source] std::num::TryFromIntError),
    /// The page does not exist or cannot be read.
    #[error("failed to get page {page_index}")]
    GetPage {
        /// The zero-based index of the page.
        page_index: u32,
        /// The underlying pdfium error.
        #[source]
        source: PdfiumError,
    },
    /// pdfium failed to render the page.
    #[error("failed to render page")]
    RenderPage(#[source] PdfiumError),
    /// The rendered bitmap could not be converted to an image.
    #[error("failed to convert rendered page to image")]
    ConvertRenderedPage(#[source] PdfiumError),
    /// The page's boundaries or rotation could not be read.
    #[error("failed to read pdf page display frame")]
    PageDisplayFrame(#[source] PdfiumError),
    /// The page's visible box is empty or not finite, or its rotation is no quarter turn.
    #[error("invalid pdf page display frame")]
    InvalidDisplayFrame,
    /// The page size, DPI or maximum dimension gives no positive, finite bitmap size.
    #[error("invalid pdf render dimensions")]
    InvalidRenderDimensions,
    /// The page's text could not be read.
    #[error("failed to extract page text")]
    ExtractPageText(#[source] PdfiumError),
    /// A page range of zero pages was requested.
    #[error("pdf page range is empty")]
    EmptyPageRange,
    /// The page range reaches past the document's last page.
    #[error(
        "pdf page range {start_page_index}..={end_page_index} is out of bounds for {total_pages} pages"
    )]
    PageRangeOutOfBounds {
        /// The zero-based index of the first page in the range.
        start_page_index: u32,
        /// The zero-based index of the last page in the range.
        end_page_index: u32,
        /// The number of pages in the document.
        total_pages: u32,
    },
    /// The page range's last index overflows `u32`.
    #[error("pdf page range overflows")]
    PageRangeOverflow,
    /// pdfium failed to create the destination document.
    #[error("failed to create pdf document")]
    CreateDocument(#[source] PdfiumError),
    /// pdfium failed to copy the pages into the destination document.
    #[error("failed to copy pdf page range")]
    CopyPageRange(#[source] PdfiumError),
    /// pdfium failed to serialize the destination document.
    #[error("failed to save pdf document")]
    SaveDocument(#[source] PdfiumError),
}
