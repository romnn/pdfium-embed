//! Integration tests, in one binary because every test binary embeds its own copy of pdfium.
//!
//! Tests that load PDFs need a pdfium library, so they run with the `bundled` feature only.
//! The fixtures under `tests/fixtures` are small, uncompressed PDFs that can be read as text.

mod binding;
mod display_frame;
#[cfg(feature = "bundled")]
mod documents;
mod rect;
#[cfg(feature = "bundled")]
mod thread_safety;

/// Reads a PDF from `tests/fixtures`.
#[cfg(feature = "bundled")]
fn fixture(name: &str) -> color_eyre::eyre::Result<Vec<u8>> {
    use color_eyre::eyre::WrapErr as _;

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read(&path).wrap_err_with(|| format!("read {}", path.display()))
}
