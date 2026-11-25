//! Rectangle arithmetic in PDF point coordinates.

use googletest::prelude::*;
use pdfium_embed::Rect;

#[test]
fn does_overlap() {
    let a = Rect::new(0.0, 0.0, 10.0, 10.0);
    let b = Rect::new(5.0, 5.0, 15.0, 15.0);
    let c = Rect::new(20.0, 20.0, 30.0, 30.0);

    assert!(a.does_overlap(&b));
    assert!(b.does_overlap(&a));
    assert!(!a.does_overlap(&c));
    assert!(!c.does_overlap(&a));
}

#[test]
fn expand() {
    let rect = Rect::new(10.0, 10.0, 20.0, 20.0);
    let expanded = rect.expand(5.0);

    assert_that!((expanded.left - 5.0).abs(), lt(f32::EPSILON));
    assert_that!((expanded.bottom - 5.0).abs(), lt(f32::EPSILON));
    assert_that!((expanded.right - 25.0).abs(), lt(f32::EPSILON));
    assert_that!((expanded.top - 25.0).abs(), lt(f32::EPSILON));
}

#[test]
fn width_height() {
    let rect = Rect::new(10.0, 20.0, 50.0, 80.0);
    assert_that!((rect.width() - 40.0).abs(), lt(f32::EPSILON));
    assert_that!((rect.height() - 60.0).abs(), lt(f32::EPSILON));

    // Degenerate rect clamps to zero.
    let degenerate = Rect::new(50.0, 80.0, 10.0, 20.0);
    assert_that!(degenerate.width(), eq(0.0));
    assert_that!(degenerate.height(), eq(0.0));
}
