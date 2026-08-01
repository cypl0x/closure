//! The caret in a one-line prompt sits where the cursor is, and costs
//! the line no width when it moves.
//!
//! Two bugs, one after the other. The capture bar, the rename/tag/
//! property fields and the palette's filter were painted as
//! `format!("{buffer}▏")` — the caret glued to the end of the text. The
//! core had been moving the cursor the whole time (Left, `C-a`, `C-b`,
//! Alt+Backspace all reach a `LineInput`), so the prompts read as if
//! none of those keys were bound.
//!
//! Splitting the string at the cursor fixed that and introduced the
//! next one, reported as "weird shift in capture prompt when ctlr+a is
//! pressed": an inserted glyph *takes width*, so a caret that moves to
//! the front shoves the whole line sideways. A caret is not a
//! character. It is a mark over the cell it is on — which is what the
//! body editor's block cursor has always been ([`cursor_cell`]), and
//! reusing that means the two cursors cannot drift apart.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::field_caret_cell;

#[test]
fn the_caret_covers_the_glyph_the_cursor_is_on() {
    let (text, range) = field_caret_cell("abc", 1);
    assert_eq!(text, "abc", "nothing inserted: the line is the line");
    assert_eq!(range, 1..2, "one cell, the 'b'");
}

#[test]
fn a_cursor_at_the_end_gets_a_cell_to_sit_on() {
    // There is no glyph after the last one, and a caret you cannot see
    // is worse than one drawn over a space.
    let (text, range) = field_caret_cell("abc", 3);
    assert_eq!(text, "abc ");
    assert_eq!(range, 3..4);
}

#[test]
fn an_empty_prompt_still_shows_its_caret() {
    let (text, range) = field_caret_cell("", 0);
    assert_eq!(text, " ");
    assert_eq!(range, 0..1);
}

#[test]
fn moving_the_caret_never_changes_the_text() {
    // This is the whole of the `C-a` report: the painted string has to
    // be identical wherever the cursor is, or the line reflows under
    // the user as they move through it.
    let line = "hello world";
    let at_end = field_caret_cell(line, line.len()).0;
    for cursor in 0..line.len() {
        assert_eq!(
            field_caret_cell(line, cursor).0,
            line,
            "cursor {cursor} repainted the line"
        );
    }
    assert_eq!(at_end, "hello world ", "only the end pads, by one space");
}

#[test]
fn a_multibyte_glyph_is_one_cell_not_one_byte() {
    // `é` is two bytes and one cell. A caret covering one byte would
    // paint half a glyph.
    let (text, range) = field_caret_cell("café", 3);
    assert_eq!(text, "café");
    assert_eq!(range, 3..5, "the whole of the é");
}

#[test]
fn a_cursor_inside_a_glyph_snaps_to_it() {
    // Nothing should be able to make a repaint panic, least of all a
    // byte offset that lands mid-glyph.
    let (text, range) = field_caret_cell("café", 4);
    assert_eq!(text, "café");
    assert_eq!(range, 3..5);
}

#[test]
fn a_cursor_past_the_end_lands_on_the_pad() {
    let (text, range) = field_caret_cell("ab", 99);
    assert_eq!(text, "ab ");
    assert_eq!(range, 2..3);
}
