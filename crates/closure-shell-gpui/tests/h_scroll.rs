//! "horizontal scroll in editor view — With long lines I still can
//! type \"in the dark\"" and the [#A] "horizontal scroll for long
//! titles detail preview".
//!
//! The editor already scrolls sideways and already follows the caret.
//! The screenshot shows why that was not enough: a line runs off the
//! right edge, stops mid-word, and the view never moves — so the last
//! stretch of what you type is invisible while you type it.
//!
//! The cause is not the scrolling. It is the *count*: `body_columns`
//! divided the pane by a hardcoded 7.2px glyph advance, which is a
//! guess, and the bundled Maple Mono is wider than that at the
//! editor's 13px. A pane told it has more columns than it has thinks
//! the caret is still inside when the caret has already left, and a
//! caret that is never "past the edge" never asks anything to scroll.
//!
//! So the advance is measured, not assumed, and these tests pin the
//! consequence: too-narrow a guess must never inflate the count.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::{body_columns, body_columns_at, h_scroll_start};

#[test]
fn a_wider_glyph_means_fewer_columns() {
    // The bug, stated: the count has to fall when the font is wider.
    // It could not, because the advance was a constant.
    let narrow = body_columns_at(1000.0, 7.2);
    let wide = body_columns_at(1000.0, 8.4);
    assert!(
        wide < narrow,
        "a wider glyph has to mean fewer columns: {wide} vs {narrow}"
    );
}

#[test]
fn the_count_never_promises_more_than_fits() {
    // The property that actually matters: whatever the count says,
    // that many glyphs must fit in the pane. Overshooting by even a
    // few columns is exactly the reported "typing in the dark" — the
    // caret sits beyond the edge and nothing scrolls, because as far
    // as the arithmetic is concerned it is still on screen.
    for width in [400.0_f32, 800.0, 1280.0, 1920.0] {
        for advance in [6.0_f32, 7.2, 7.8, 8.4, 9.6] {
            let cols = body_columns_at(width, advance);
            // The same chrome the count subtracts: gutter, its margin,
            // the pane padding on both sides, and the scrollbar.
            let usable = width - (34.0 + 8.0 + 16.0 + 10.0);
            #[allow(clippy::cast_precision_loss)]
            let needed = cols as f32 * advance;
            assert!(
                needed <= usable || cols == 8,
                "{cols} cols x {advance}px = {needed} > {usable} available at {width}px"
            );
        }
    }
}

#[test]
fn the_caret_pulls_the_view_along_once_it_reaches_the_edge() {
    // Unchanged behaviour, kept as cover: this half was never broken,
    // which is why the report reads as "scrolling does not work" when
    // scrolling was only ever asked at the wrong moment.
    assert_eq!(h_scroll_start(10, 80), 0, "still inside");
    assert!(h_scroll_start(200, 80) > 0, "past the edge, so it moves");
}

#[test]
fn the_old_default_is_still_what_an_unmeasured_pane_gets() {
    // A pane that has not been laid out yet has no width to divide,
    // and answering zero columns there would make the first frame
    // scroll to the end of every line.
    assert_eq!(body_columns(0.0, 1.0), body_columns(f32::NAN, 1.0));
}

#[test]
fn zoom_still_widens_the_glyph() {
    // Zoom scales the text, so it scales the advance, so it lowers the
    // count. The measured path must not lose that.
    assert!(
        body_columns(1000.0, 2.0) < body_columns(1000.0, 1.0),
        "zoomed in, fewer columns fit"
    );
}
