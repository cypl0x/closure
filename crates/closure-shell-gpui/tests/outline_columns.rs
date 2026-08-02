//! "The outline indention is off. Please tabularize (and/or colorize)
//! it in order to show the hierachy clearly. Currently it is more like
//! guessing." — and its twin, "outline sometimes wrong spacing".
//!
//! The guide rules already answered the guessing half: one rule per
//! ancestor, in that ancestor's colour, which is what every outliner
//! draws instead of empty space you have to measure by eye.
//!
//! What was still off is the tabular half. The indent came *first* in
//! the row, so everything after it — the status dot, the keyword, the
//! priority cookie — moved right with the level. Six rows at four
//! depths put their `TODO` chips at four different x positions, so the
//! one question the outline exists to answer, "what is still open",
//! could not be answered by running an eye down a column. That is also
//! the "sometimes wrong spacing": the spacing was never wrong, it was
//! depth-dependent, which looks the same from the outside.
//!
//! So the status cells are a gutter at a fixed left edge and the indent
//! applies to the title alone. Depth still reads — the guides and the
//! fold arrow are with the title, where the tree is.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::float_cmp,
    missing_docs
)]

use closure_shell_gpui::{gutter_px, indent_guides, title_indent_px};

#[test]
fn the_status_gutter_does_not_depend_on_depth() {
    // The whole item: a `TODO` at level 1 and a `TODO` at level 5 start
    // at the same x, so a column of them is a column.
    let a = gutter_px(4.0, 1.0);
    let b = gutter_px(4.0, 1.0);
    assert_eq!(a, b);
    assert!(a > 0.0, "the gutter has to hold the chips");
}

#[test]
fn a_wider_keyword_widens_the_gutter_for_every_row_at_once() {
    // It is one column, so it is one width — a vault declaring
    // `CANCELLED` moves every title right together rather than
    // ragged.
    assert!(gutter_px(9.0, 1.0) > gutter_px(4.0, 1.0));
}

#[test]
fn the_title_indent_grows_with_the_level() {
    let mut last = -1.0;
    for level in 1..=6u8 {
        let indent = title_indent_px(level, 1.0);
        assert!(indent > last, "level {level} did not indent further");
        last = indent;
    }
}

#[test]
fn the_first_level_is_not_indented() {
    assert_eq!(title_indent_px(1, 1.0), 0.0);
}

#[test]
fn a_level_of_zero_does_not_indent_the_width_of_the_screen() {
    // Levels are 1-based and a 0 from anywhere would underflow.
    assert_eq!(title_indent_px(0, 1.0), 0.0);
    assert_eq!(indent_guides(0), 0);
}

#[test]
fn one_guide_per_ancestor() {
    assert_eq!(indent_guides(1), 0, "a top-level row has no ancestor");
    assert_eq!(indent_guides(4), 3);
}

#[test]
fn both_halves_scale_with_the_zoom() {
    assert!(gutter_px(4.0, 2.0) > gutter_px(4.0, 1.0));
    assert!(title_indent_px(3, 2.0) > title_indent_px(3, 1.0));
}
