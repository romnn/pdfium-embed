//! Loading, measuring, rendering, reading and copying documents.

use color_eyre::eyre;
use googletest::prelude::*;
use pdfium_embed::{DocumentError, Pdfium, TextSegment};

/// A one-page US Letter document, 612 × 864 points.
fn single_page_pdf() -> eyre::Result<Vec<u8>> {
    crate::fixture("origin-rotate-0.pdf")
}

/// A two-page document: page 0 holds twelve lines of prose and three words followed by raised,
/// smaller markers, set like footnote references; page 1 is A4.
fn text_pdf() -> eyre::Result<Vec<u8>> {
    crate::fixture("text.pdf")
}

#[test]
fn can_load_rejects_garbage() -> eyre::Result<()> {
    let pdfium = Pdfium::instance()?;
    assert!(!pdfium.can_load(b"not a pdf"));
    assert!(!pdfium.can_load(b""));
    Ok(())
}

#[test]
fn can_load_accepts_valid_pdf() -> eyre::Result<()> {
    let pdf_bytes = single_page_pdf()?;
    let pdfium = Pdfium::instance()?;
    assert!(pdfium.can_load(&pdf_bytes));
    Ok(())
}

#[test]
fn page_dimensions_are_the_media_box_in_points() -> eyre::Result<()> {
    let pdf_bytes = single_page_pdf()?;
    let pdfium = Pdfium::instance()?;
    let dimensions = pdfium.page_dimensions(&pdf_bytes, 0)?;

    assert_that!(dimensions.width_pts, eq(612.0));
    assert_that!(dimensions.height_pts, eq(864.0));
    Ok(())
}

#[test]
fn page_dimensions_out_of_range_errors() -> eyre::Result<()> {
    let pdf_bytes = single_page_pdf()?;
    let pdfium = Pdfium::instance()?;
    assert!(pdfium.page_dimensions(&pdf_bytes, 999).is_err());
    Ok(())
}

#[test]
fn render_page_produces_image() -> eyre::Result<()> {
    let pdf_bytes = single_page_pdf()?;
    let pdfium = Pdfium::instance()?;
    let rendered = pdfium.render_page(
        &pdf_bytes,
        0,
        72,
        pdfium_embed::DEFAULT_MAX_RENDER_DIMENSION,
    )?;

    // At 72 DPI one point is one pixel.
    assert_that!(rendered.image.dimensions(), eq((612, 864)));
    assert_that!(rendered.width_pts, eq(612.0));
    assert_that!(rendered.height_pts, eq(864.0));
    Ok(())
}

#[test]
fn with_document_multi_page_workflow() -> eyre::Result<()> {
    let pdf_bytes = text_pdf()?;
    let pdfium = Pdfium::instance()?;

    let page_widths = pdfium.with_document(&pdf_bytes, |doc| -> eyre::Result<_> {
        let mut page_widths = Vec::new();
        for page_index in 0..doc.num_pages() {
            page_widths.push(doc.page_dimensions(page_index)?.width_pts);
        }
        Ok(page_widths)
    })?;

    assert_that!(page_widths, elements_are![eq(&612.0), eq(&595.0)]);
    Ok(())
}

#[test]
fn page_text_reads_the_text_layer() -> eyre::Result<()> {
    let pdf_bytes = text_pdf()?;
    let pdfium = Pdfium::instance()?;
    assert_that!(pdfium.page_text(&pdf_bytes, 1)?.as_str(), eq("Second page"));
    Ok(())
}

/// The limit counts the page's characters, checked before the text is copied out of pdfium.
#[test]
fn page_text_limited_allows_up_to_the_limit() -> eyre::Result<()> {
    let pdf_bytes = text_pdf()?;
    let pdfium = Pdfium::instance()?;
    let page_text_limited = |max_chars| {
        pdfium.with_document(&pdf_bytes, |document| {
            document.page_text_limited(1, max_chars)
        })
    };

    // The second page holds exactly the eleven characters of "Second page".
    let page_chars = "Second page".chars().count();
    assert_that!(
        page_text_limited(page_chars - 1),
        err(matches_pattern!(DocumentError::PageTextTooLong {
            page_index: eq(&1),
            max_chars: eq(&(page_chars - 1)),
        }))
    );
    assert_that!(page_text_limited(page_chars), ok(eq("Second page")));
    assert_that!(page_text_limited(page_chars + 1), ok(eq("Second page")));
    Ok(())
}

/// Text is found in raw page coordinates, whatever the page's rotation and crop box origin.
#[test]
fn page_text_reads_rotated_and_offset_pages() -> eyre::Result<()> {
    let pdfium = Pdfium::instance()?;
    for name in [
        "cropped-rotate-90.pdf",
        "cropped-rotate-270.pdf",
        "negative-origin.pdf",
    ] {
        let pdf_bytes = crate::fixture(name)?;
        assert_that!(
            pdfium.page_text(&pdf_bytes, 0)?.as_str(),
            eq("Anchor"),
            "{name}"
        );
    }
    Ok(())
}

#[test]
fn page_text_words_splits_lines_into_word_boxes() -> eyre::Result<()> {
    let pdf_bytes = text_pdf()?;
    let pdfium = Pdfium::instance()?;

    let words = pdfium.page_text_words(&pdf_bytes, 0)?;
    let segments = pdfium.page_text_segments(&pdf_bytes, 0)?;

    // Line-level segments merge every character on a line that shares a font, so only a few of
    // them are single words; word extraction must find many more.
    let whitespace_free_segments = segments
        .iter()
        .filter(|segment| {
            let trimmed = segment.text.trim();
            !trimmed.is_empty() && !trimmed.chars().any(char::is_whitespace)
        })
        .count();

    // The page has twelve lines of eight words each, plus the marked words.
    assert_that!(words, len(gt(90)));
    assert_that!(words, len(gt(whitespace_free_segments)));

    for word in &words {
        assert_that!(word.text.as_str(), not(eq("")));
        assert_that!(word.text.chars().any(char::is_whitespace), is_false());
        // Every emitted word carries a real, non-degenerate glyph box.
        assert_that!(word.bounds.width(), gt(0.0));
        assert_that!(word.bounds.height(), gt(0.0));
    }

    Ok(())
}

/// Returns whether `words` has a `marker` word starting within one line height after a `base`
/// word and overlapping it vertically.
fn has_near_marker_pair(words: &[TextSegment], base: &str, marker: &str) -> bool {
    words.iter().any(|base_word| {
        if base_word.text != base {
            return false;
        }

        words.iter().any(|marker_word| {
            if marker_word.text != marker {
                return false;
            }

            let vertical_overlap = (base_word.bounds.top.min(marker_word.bounds.top)
                - base_word.bounds.bottom.max(marker_word.bounds.bottom))
            .max(0.0);

            marker_word.bounds.left >= base_word.bounds.left
                && marker_word.bounds.left <= base_word.bounds.right + base_word.bounds.height()
                && vertical_overlap > 0.0
        })
    })
}

/// A raised, smaller marker set directly after a word, with no space between them in the text
/// stream, becomes a word of its own.
#[test]
fn page_text_words_splits_off_baseline_superscript_markers() -> eyre::Result<()> {
    let pdf_bytes = text_pdf()?;
    let pdfium = Pdfium::instance()?;

    let words = pdfium.page_text_words(&pdf_bytes, 0)?;
    let texts: Vec<&str> = words.iter().map(|word| word.text.as_str()).collect();

    // No marker stays glued to its base word.
    assert_that!(texts, not(contains(eq(&"Total3"))));
    assert_that!(texts, not(contains(eq(&"Fee4"))));
    assert_that!(texts, not(contains(eq(&"Rate7"))));

    // Each marker sits right after its base word.
    assert_that!(has_near_marker_pair(&words, "Total", "3"), is_true());
    assert_that!(has_near_marker_pair(&words, "Fee", "4"), is_true());
    assert_that!(has_near_marker_pair(&words, "Rate", "7"), is_true());

    Ok(())
}

#[test]
fn copy_page_range_extracts_subset_pdf() -> eyre::Result<()> {
    let pdf_bytes = text_pdf()?;
    let pdfium = Pdfium::instance()?;
    assert_that!(pdfium.num_pages(&pdf_bytes)?, eq(2));

    let subset_bytes = pdfium.copy_page_range(&pdf_bytes, 1, 1)?;
    assert_that!(pdfium.num_pages(&subset_bytes)?, eq(1));

    // The copied page keeps its own A4 size rather than the first page's.
    let source_page_dimensions = pdfium.page_dimensions(&pdf_bytes, 1)?;
    let subset_page_dimensions = pdfium.page_dimensions(&subset_bytes, 0)?;
    assert_that!(
        subset_page_dimensions.width_pts,
        eq(source_page_dimensions.width_pts)
    );
    assert_that!(
        subset_page_dimensions.height_pts,
        eq(source_page_dimensions.height_pts)
    );
    assert_that!(subset_page_dimensions.width_pts, eq(595.0));
    Ok(())
}

#[test]
fn copy_page_range_rejects_invalid_ranges() -> eyre::Result<()> {
    let pdf_bytes = text_pdf()?;
    let pdfium = Pdfium::instance()?;
    assert!(pdfium.copy_page_range(&pdf_bytes, 0, 0).is_err());
    assert!(pdfium.copy_page_range(&pdf_bytes, u32::MAX, 1).is_err());
    Ok(())
}
