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
