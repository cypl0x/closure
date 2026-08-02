//! "Caret gets hidden at the bottom of the file … In the Screenshot you
//! can see how the blue caret is almost hidden. This soudln't be
//! possible."
//!
//! The editor counted its viewport in `BODY_LINE_H` units and then
//! painted each row with `min_h(BODY_LINE_H)` — a *minimum*, which a
//! glyph box slightly taller than the constant quietly exceeds. Thirty
//! rows of "a little taller" is a row and a half, so the pane clipped
//! its own last line and the core, which had been told it owned that
//! many whole rows, scrolled the cursor onto it. The caret ended up in
//! the sliver below the last readable line.
//!
//! The count and the paint have to come from one number. `body_row_h`
//! is that number, and these are the arithmetic that follows from it:
//! whatever the pane's height, the rows the viewport claims must fit
//! inside it.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::{BODY_CHROME, body_row_h, body_viewport_lines};

/// Every row the viewport claims fits in the pane, with the chrome off
/// the top — the property whose absence hid the caret.
fn fits(pane: f32, zoom: f32) -> bool {
    let row = body_row_h(zoom);
    let rows = body_viewport_lines(pane, row, BODY_CHROME);
    #[allow(clippy::cast_precision_loss)]
    let painted = rows as f32 * row;
    painted <= pane - BODY_CHROME
}

#[test]
fn the_claimed_rows_fit_the_pane() {
    for pane in [200.0, 400.0, 617.0, 720.0, 1080.0, 1440.0] {
        assert!(fits(pane, 1.0), "pane {pane}");
    }
}

#[test]
fn they_fit_at_every_zoom_step() {
    // Zoom scales the row; a count taken at one scale and painted at
    // another is the same bug in a different disguise.
    for zoom in [0.8_f32, 1.0, 1.25, 1.5, 2.0] {
        for pane in [400.0, 617.0, 900.0] {
            assert!(fits(pane, zoom), "zoom {zoom} pane {pane}");
        }
    }
}

#[test]
fn a_taller_pane_never_claims_fewer_rows() {
    let row = body_row_h(1.0);
    let mut last = 0;
    for pane in (200..1400).step_by(7) {
        #[allow(clippy::cast_precision_loss)]
        let rows = body_viewport_lines(pane as f32, row, BODY_CHROME);
        assert!(rows >= last, "pane {pane} went backwards");
        last = rows;
    }
}

#[test]
fn the_row_height_scales_with_the_zoom() {
    assert!(body_row_h(2.0) > body_row_h(1.0));
    assert!((body_row_h(1.0) - body_row_h(1.0)).abs() < f32::EPSILON);
}

#[test]
fn a_row_is_tall_enough_for_its_glyphs() {
    // The other direction: forcing rows shorter than the text would
    // clip descenders instead of the caret, which is not a fix.
    const BODY_TEXT: f32 = 13.0;
    assert!(
        body_row_h(1.0) >= BODY_TEXT * 1.2,
        "a 13px line needs its leading: {}",
        body_row_h(1.0)
    );
}

#[test]
fn an_unmeasured_pane_still_claims_something() {
    assert!(body_viewport_lines(0.0, body_row_h(1.0), BODY_CHROME) >= 4);
}

// ---- horizontal ---------------------------------------------------

use closure_shell_gpui::{body_columns, h_scroll_start};

#[test]
fn a_cursor_past_the_edge_pulls_the_view_with_it() {
    // "editor horizontal scroll. Typing will go out of the view and I
    // don't have the option to view where I am typing if the line is to
    // loong". The columns must come from the pane that clips, not from
    // the text: measured against the text, a long line makes the pane
    // look as wide as the line and nothing ever scrolls.
    let cols = body_columns(1030.0, 1.0);
    assert!(cols > 8 && cols < 200, "a 1030px pane holds {cols} columns");
    // The margin means the view starts moving two columns before the
    // caret would actually leave the pane.
    assert_eq!(
        h_scroll_start(cols - 3, cols),
        0,
        "still comfortably on screen"
    );
    assert!(h_scroll_start(cols, cols) > 0, "past the edge scrolls");
    assert!(
        h_scroll_start(200, cols) > 0,
        "column 200 of a long line is not visible in {cols} columns"
    );
}

#[test]
fn the_column_count_shrinks_as_the_text_grows() {
    let plain = body_columns(1030.0, 1.0);
    let zoomed = body_columns(1030.0, 2.0);
    assert!(zoomed < plain, "{zoomed} vs {plain}");
}

#[test]
fn an_unmeasured_pane_assumes_a_usable_line() {
    assert!(body_columns(0.0, 1.0) >= 8);
    assert!(body_columns(f32::NAN, 1.0) >= 8);
}

#[test]
fn the_caret_is_never_flush_against_the_right_edge() {
    // Scrolling so the caret lands in the *last* column leaves it a
    // two-pixel bar inside the pane's padding, which is the half of the
    // report that scrolling alone does not answer.
    let cols = body_columns(1030.0, 1.0);
    let start = h_scroll_start(200, cols);
    let last_visible = start + cols - 1;
    assert!(
        200 < last_visible,
        "caret at 200, window {start}..={last_visible}"
    );
}
