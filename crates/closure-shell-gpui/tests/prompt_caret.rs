//! The caret in a one-line prompt sits where the cursor is.
//!
//! The capture bar, the rename/tag/property fields and the palette's
//! filter were all painted as `format!("{buffer}▏")` — the caret glued
//! to the end of the text. The core had been moving the cursor the
//! whole time (Left, `C-a`, `C-b`, Alt+Backspace all reach a
//! `LineInput`), so the prompts read as if none of those keys were
//! bound: you pressed Left, nothing appeared to happen, and the next
//! character still went where you did not expect.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::field_with_caret;

#[test]
fn the_caret_splits_the_text_at_the_cursor() {
    assert_eq!(field_with_caret("abc", 1), "a\u{258f}bc");
}

#[test]
fn a_cursor_at_the_end_reads_the_way_it_always_did() {
    assert_eq!(field_with_caret("abc", 3), "abc\u{258f}");
    assert_eq!(field_with_caret("", 0), "\u{258f}");
}

#[test]
fn a_cursor_at_the_start_puts_the_caret_first() {
    assert_eq!(field_with_caret("abc", 0), "\u{258f}abc");
}

#[test]
fn a_multibyte_glyph_is_never_split_down_the_middle() {
    // `é` is two bytes. A cursor between them is not a place a string
    // can be cut, and cutting there would panic in the middle of a
    // repaint — so the caret snaps to the boundary below it.
    assert_eq!(field_with_caret("café", 5), "caf\u{e9}\u{258f}");
    assert_eq!(field_with_caret("café", 4), "caf\u{258f}\u{e9}");
}

#[test]
fn a_cursor_past_the_end_lands_at_the_end() {
    // Nothing should be able to make a repaint panic, least of all a
    // stale cursor from the frame before.
    assert_eq!(field_with_caret("ab", 99), "ab\u{258f}");
}
