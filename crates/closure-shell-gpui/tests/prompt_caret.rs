//! The caret in a one-line prompt: where the cursor is, shaped like the
//! mode it is in.
//!
//! Three reports, in order. The caret was welded to the end of the text
//! (`format!("{buffer}▏")`), so Left, `C-a` and Alt+Backspace all looked
//! unbound. Splitting the string at the cursor fixed that and shoved the
//! line sideways by a whole character cell whenever the caret moved —
//! "weird shift in capture prompt when ctlr+a is pressed". A block over
//! the cell fixed *that* and was wrong in a third way: "block cursor
//! shown in capture/etc. prompt instead of line cursor (it's INSERT
//! mode)".
//!
//! A prompt is always INSERT — there is no NORMAL to drop into — so its
//! caret is the thin bar between two glyphs that the body editor draws
//! in INSERT, in the same accent colour, laid out the same way: two
//! spans with a 2px bar between them. Moving it costs the line two
//! pixels rather than a character's width, which is what made the shift
//! visible in the first place.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::caret_split;

#[test]
fn the_text_splits_where_the_cursor_is() {
    assert_eq!(caret_split("abc", 1), ("a", "bc"));
}

#[test]
fn a_cursor_at_either_end_leaves_the_other_half_empty() {
    assert_eq!(caret_split("abc", 0), ("", "abc"));
    assert_eq!(caret_split("abc", 3), ("abc", ""));
}

#[test]
fn an_empty_prompt_splits_into_nothing_twice() {
    assert_eq!(caret_split("", 0), ("", ""));
}

#[test]
fn the_two_halves_are_always_the_whole_text() {
    // Nothing is inserted and nothing is dropped: the line reads the
    // same wherever the caret is, which is the whole of the shift
    // report.
    let line = "hello world";
    for cursor in 0..=line.len() {
        let (head, tail) = caret_split(line, cursor);
        assert_eq!(format!("{head}{tail}"), line, "cursor {cursor}");
    }
}

#[test]
fn a_multibyte_glyph_is_never_split_down_the_middle() {
    // `é` is two bytes. Cutting between them would panic a repaint.
    assert_eq!(caret_split("café", 5), ("café", ""));
    assert_eq!(caret_split("café", 4), ("caf", "é"), "snaps to the glyph");
    assert_eq!(caret_split("café", 3), ("caf", "é"));
}

#[test]
fn a_cursor_past_the_end_lands_at_the_end() {
    // Nothing should be able to make a repaint panic, least of all a
    // stale cursor from the frame before.
    assert_eq!(caret_split("ab", 99), ("ab", ""));
}
