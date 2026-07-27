//! The block cursor: one cell, always visible, whatever it sits on.
//!
//! Three things were wrong with it. The cursor was suppressed whenever a
//! VISUAL selection existed, so the moment you started selecting you
//! could no longer see which end you were moving. Past the last glyph —
//! an empty line, or `$` on a short line — it was drawn as a hardcoded
//! 8×18px rectangle, which is only the right size at one font size and
//! one line height. And the INSERT bar was hardcoded the same way.
//!
//! The fix is to make the cursor a *cell of text* rather than a
//! rectangle: the line is padded with a space when the cursor sits past
//! its end, and the cursor is a mark over that cell like any other, so
//! the font decides how big it is.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::{Emphasis, cursor_cell, cursor_mark, styled_runs};

// === the cell the cursor covers ===

#[test]
fn the_cursor_covers_the_glyph_it_is_on() {
    let (text, range) = cursor_cell("abc", 1);
    assert_eq!(text, "abc", "nothing to pad: the cursor is on a glyph");
    assert_eq!(range, 1..2, "one cell, the 'b'");
}

#[test]
fn a_cursor_past_the_last_glyph_gets_a_cell_to_sit_on() {
    // `$` on the last column, or INSERT-then-Esc at the end of a line:
    // there is no glyph to invert, and a cursor you cannot see is worse
    // than one drawn over a space.
    let (text, range) = cursor_cell("ab", 2);
    assert_eq!(text, "ab ", "one space, laid out by the same font");
    assert_eq!(range, 2..3);
}

#[test]
fn an_empty_line_still_shows_the_cursor() {
    let (text, range) = cursor_cell("", 0);
    assert_eq!(text, " ");
    assert_eq!(range, 0..1);
}

#[test]
fn a_multibyte_glyph_is_one_cell_not_one_byte() {
    // The range is bytes, the column is characters: a cursor on `é` that
    // covered one byte would split the glyph and paint half a cell.
    let (text, range) = cursor_cell("héllo", 1);
    assert_eq!(text, "héllo");
    assert_eq!(range, 1..3, "the whole two-byte 'é'");
}

#[test]
fn a_column_far_past_the_end_still_lands_on_the_pad() {
    // The editor clamps, but a stale column from a resize must not panic
    // or point outside the string it is about to slice.
    let (text, range) = cursor_cell("ab", 99);
    assert_eq!(text, "ab ");
    assert_eq!(range, 2..3);
}

// === the mark, and what it beats ===

#[test]
fn the_cursor_is_a_mark_in_every_mode_but_insert() {
    assert_eq!(cursor_mark("abc", 1, false), Some((1..2, Emphasis::Cursor)));
    assert_eq!(
        cursor_mark("abc", 1, true),
        None,
        "INSERT draws a bar between glyphs instead"
    );
}

#[test]
fn the_cursor_wins_over_the_selection_it_sits_in() {
    // VISUAL used to suppress the cursor entirely: the selection tint
    // covered the whole range and nothing said which end was moving.
    // `styled_runs` lets the last mark win, so the cursor is pushed
    // last and reads as inverse video inside the tint.
    let spans = vec![(closure_shell_gpui::BodySpan::Plain, "abcdef".to_owned())];
    let marks = vec![
        (0..4, Emphasis::Selection),
        cursor_mark("abcdef", 3, false).expect("a cursor"),
    ];
    let runs = styled_runs(&spans, &marks);
    let cursor: Vec<_> = runs
        .iter()
        .filter(|(_, _, mark)| *mark == Some(Emphasis::Cursor))
        .map(|(range, _, _)| range.clone())
        .collect();
    assert_eq!(cursor, vec![3..4], "the head of the selection is visible");
    let selected: Vec<_> = runs
        .iter()
        .filter(|(_, _, mark)| *mark == Some(Emphasis::Selection))
        .map(|(range, _, _)| range.clone())
        .collect();
    assert_eq!(selected, vec![0..3], "the rest of it still reads selected");
}

#[test]
fn the_cursor_wins_over_a_search_hit_too() {
    let spans = vec![(closure_shell_gpui::BodySpan::Plain, "needle".to_owned())];
    let marks = vec![
        (0..6, Emphasis::Search),
        cursor_mark("needle", 0, false).expect("a cursor"),
    ];
    let runs = styled_runs(&spans, &marks);
    assert_eq!(runs[0].2, Some(Emphasis::Cursor), "runs: {runs:?}");
}
