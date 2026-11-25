//! Mapping PDF page rectangles into the normalized frame of the displayed page.

use color_eyre::eyre;
#[cfg(feature = "bundled")]
use pdfium_embed::Pdfium;
use pdfium_embed::{PageDisplayFrame, Rect};

/// Asserts that `frame` maps `raw` to `expected`, given as left, top, right and bottom fractions.
fn assert_normalized_rect(
    frame: PageDisplayFrame,
    raw: Rect,
    expected: [f32; 4],
) -> eyre::Result<()> {
    let result = frame
        .normalized_rect(raw)
        .ok_or_else(|| eyre::eyre!("visible fixture rectangle rejected"))?;
    for (actual, expected) in [result.left, result.top, result.right, result.bottom]
        .into_iter()
        .zip(expected)
    {
        assert!(
            (actual - expected).abs() < 0.000_001,
            "rotation {}: actual {actual}, expected {expected} for {raw:?}",
            frame.rotation_degrees()
        );
    }
    Ok(())
}

/// Crop clipping precedes each quarter turn and rejects invalid or invisible rectangles.
#[test]
fn quarter_turns_match_independent_arithmetic() -> eyre::Result<()> {
    for (rotation, expected) in [
        (0, [0.10, 0.15, 0.30, 0.25]),
        (90, [0.75, 0.10, 0.85, 0.30]),
        (180, [0.70, 0.75, 0.90, 0.85]),
        (270, [0.15, 0.70, 0.25, 0.90]),
    ] {
        let frame = PageDisplayFrame::new(Rect::new(36.0, 72.0, 576.0, 792.0), rotation)
            .ok_or_else(|| eyre::eyre!("valid fixture frame rejected"))?;
        assert_normalized_rect(frame, Rect::new(90.0, 612.0, 198.0, 684.0), expected)?;

        // Rectangles outside the crop, with a NaN corner, or inverted have no visible frame.
        assert!(
            frame
                .normalized_rect(Rect::new(0.0, 0.0, 20.0, 20.0))
                .is_none()
        );
        assert!(
            frame
                .normalized_rect(Rect::new(f32::NAN, 0.0, 50.0, 100.0))
                .is_none()
        );
        assert!(
            frame
                .normalized_rect(Rect::new(50.0, 100.0, 40.0, 90.0))
                .is_none()
        );
        // The crop removes six PDF points from the left edge before rotation.
        let partially_clipped = match rotation {
            90 => [7.0 / 180.0, 0.0, 1.0 / 15.0, 7.0 / 270.0],
            180 => [263.0 / 270.0, 7.0 / 180.0, 1.0, 1.0 / 15.0],
            270 => [14.0 / 15.0, 263.0 / 270.0, 173.0 / 180.0, 1.0],
            _ => [0.0, 14.0 / 15.0, 7.0 / 270.0, 173.0 / 180.0],
        };
        assert_normalized_rect(
            frame,
            Rect::new(30.0, 100.0, 50.0, 120.0),
            partially_clipped,
        )?;
    }
    // An unbounded box and a rotation that is no quarter turn are refused.
    assert!(PageDisplayFrame::new(Rect::new(0.0, 0.0, f32::INFINITY, 10.0), 0).is_none());
    assert!(PageDisplayFrame::new(Rect::new(0.0, 0.0, 10.0, 10.0), 45).is_none());
    Ok(())
}

/// `PDFium` resolves intrinsic rotation and effective crop and media box inheritance from real PDF
/// metadata.
#[cfg(feature = "bundled")]
#[test]
fn real_pdf_metadata_resolves_crop_and_rotation() -> eyre::Result<()> {
    let pdfium = Pdfium::instance()?;
    for rotation in [0, 90, 180, 270] {
        let bytes = crate::fixture(&format!("cropped-rotate-{rotation}.pdf"))?;
        pdfium.with_document(&bytes, |document| -> eyre::Result<()> {
            let frame = document.page_display_frame(0)?;
            assert_eq!(frame.visible_box(), Rect::new(36.0, 72.0, 576.0, 792.0));
            assert_eq!(frame.rotation_degrees(), rotation);
            let rendered = document.render_page(0, 144, 4096)?;
            let expected = if rotation == 90 || rotation == 270 {
                (1440, 1080)
            } else {
                (1080, 1440)
            };
            assert_eq!(rendered.image.dimensions(), expected);
            Ok(())
        })?;
    }
    let bytes = crate::fixture("inherited-crop-rotation.pdf")?;
    pdfium.with_document(&bytes, |document| -> eyre::Result<()> {
        assert_eq!(document.page_display_frame(0)?.rotation_degrees(), 90);
        assert_eq!(
            document.page_display_frame(0)?.visible_box(),
            Rect::new(36.0, 72.0, 576.0, 792.0)
        );
        Ok(())
    })?;
    let bytes = crate::fixture("crop-outside-media.pdf")?;
    pdfium.with_document(&bytes, |document| -> eyre::Result<()> {
        assert_eq!(
            document.page_display_frame(0)?.visible_box(),
            Rect::new(0.0, 0.0, 612.0, 864.0)
        );
        Ok(())
    })?;
    Ok(())
}

/// Uncropped rotated pages use `MediaBox` coordinates and the corresponding displayed raster size.
#[cfg(feature = "bundled")]
#[test]
fn uncropped_rotated_pdf_rectangles_use_the_media_box() -> eyre::Result<()> {
    let pdfium = Pdfium::instance()?;
    for (rotation, expected) in [
        (0, [5.0 / 34.0, 5.0 / 24.0, 11.0 / 34.0, 7.0 / 24.0]),
        (90, [17.0 / 24.0, 5.0 / 34.0, 19.0 / 24.0, 11.0 / 34.0]),
        (180, [23.0 / 34.0, 17.0 / 24.0, 29.0 / 34.0, 19.0 / 24.0]),
        (270, [5.0 / 24.0, 23.0 / 34.0, 7.0 / 24.0, 29.0 / 34.0]),
    ] {
        let bytes = crate::fixture(&format!("origin-rotate-{rotation}.pdf"))?;
        pdfium.with_document(&bytes, |document| -> eyre::Result<()> {
            let frame = document.page_display_frame(0)?;
            assert_eq!(frame.visible_box(), Rect::new(0.0, 0.0, 612.0, 864.0));
            assert_eq!(frame.rotation_degrees(), rotation);
            assert_normalized_rect(frame, Rect::new(90.0, 612.0, 198.0, 684.0), expected)?;
            let rendered = document.render_page(0, 144, 4096)?;
            let dimensions = if rotation == 90 || rotation == 270 {
                (1728, 1224)
            } else {
                (1224, 1728)
            };
            assert_eq!(rendered.image.dimensions(), dimensions);
            Ok(())
        })?;
    }
    Ok(())
}

/// Negative-origin pages translate their anchors and clip both edges before the intrinsic rotation.
#[cfg(feature = "bundled")]
#[test]
fn negative_origin_pdf_rectangles_translate_and_clip_before_rotation() -> eyre::Result<()> {
    let bytes = crate::fixture("negative-origin.pdf")?;
    Pdfium::instance()?.with_document(&bytes, |document| -> eyre::Result<()> {
        let frame = document.page_display_frame(0)?;
        assert_eq!(frame.visible_box(), Rect::new(-18.0, -36.0, 540.0, 720.0));
        assert_eq!(frame.rotation_degrees(), 270);
        assert_normalized_rect(
            frame,
            Rect::new(90.0, 612.0, 198.0, 684.0),
            [1.0 / 21.0, 19.0 / 31.0, 1.0 / 7.0, 25.0 / 31.0],
        )?;
        // Both negative edges are clipped; only the visible eighteen-point square survives.
        assert_normalized_rect(
            frame,
            Rect::new(-30.0, -48.0, 0.0, -18.0),
            [41.0 / 42.0, 30.0 / 31.0, 1.0, 1.0],
        )?;
        let rendered = document.render_page(0, 144, 4096)?;
        assert_eq!(rendered.image.dimensions(), (1512, 1116));
        Ok(())
    })
}
