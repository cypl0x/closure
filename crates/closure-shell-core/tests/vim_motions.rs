//! The vim vocabulary the grammar in `vim.rs` did not yet cover:
//! bracket matching, the first-non-blank line motions, the column
//! motion, the `g`-prefixed display motions, the scroll motions, and
//! the whole-line shorthands (`Y`, `gJ`).
//!
//! Same contract as `vim.rs`: these drive [`BodyEditor`] directly, so
//! one grammar covers every shell (I4).

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::BodyEditor;

/// A Normal-mode editor over `text` with the cursor at byte 0.
fn ed(text: &str) -> BodyEditor {
    let mut e = BodyEditor::new();
    e.load(text.to_owned());
    e.to_normal();
    e.set_cursor_byte(0);
    e
}

/// Feed one stroke per char (the single-char vim vocabulary).
fn feed(e: &mut BodyEditor, keys: &str) {
    for c in keys.chars() {
        e.modal_key(&c.to_string());
    }
}

const fn at(e: &BodyEditor) -> usize {
    e.cursor_byte()
}

/// A buffer of `n` numbered lines — enough to page through.
fn lines(n: usize) -> String {
    use std::fmt::Write as _;
    (0..n).fold(String::new(), |mut acc, i| {
        let _ = writeln!(acc, "line {i}");
        acc
    })
}

// === `%`: the matching bracket. ===

#[test]
fn percent_jumps_to_the_closing_bracket() {
    let mut e = ed("a(bc)d");
    e.set_cursor_byte(1);
    feed(&mut e, "%");
    assert_eq!(at(&e), 4, "on `(` -> its `)`");
}

#[test]
fn percent_jumps_back_from_the_closing_bracket() {
    let mut e = ed("a(bc)d");
    e.set_cursor_byte(4);
    feed(&mut e, "%");
    assert_eq!(at(&e), 1);
}

#[test]
fn percent_finds_the_first_bracket_to_its_right() {
    // Vim scans forward on the line for a bracket before matching.
    let mut e = ed("abc(de)f");
    feed(&mut e, "%");
    assert_eq!(at(&e), 6, "from col 0 -> the `)` matching `(`");
}

#[test]
fn percent_matches_across_lines_and_nests() {
    let mut e = ed("{\n  a{b}c\n}\n");
    feed(&mut e, "%");
    assert_eq!(at(&e), 10, "the outer brace pairs across the nested one");
}

#[test]
fn percent_without_a_bracket_stays_put() {
    let mut e = ed("plain text");
    feed(&mut e, "%");
    assert_eq!(at(&e), 0);
}

#[test]
fn d_percent_deletes_the_bracketed_run_inclusive() {
    let mut e = ed("a(bc)d");
    e.set_cursor_byte(1);
    feed(&mut e, "d%");
    assert_eq!(e.text(), "ad");
}

// === First-non-blank line motions. ===

#[test]
fn plus_and_minus_walk_lines_to_the_first_non_blank() {
    let mut e = ed("one\n   two\nthree\n");
    feed(&mut e, "+");
    assert_eq!(at(&e), 7, "line 2, past the indent");
    feed(&mut e, "-");
    assert_eq!(at(&e), 0);
}

#[test]
fn enter_is_the_same_motion_as_plus() {
    let mut e = ed("one\n  two\n");
    e.modal_key("enter");
    assert_eq!(at(&e), 6);
}

#[test]
fn underscore_is_the_current_line_with_no_count() {
    let mut e = ed("  one\n  two\n");
    e.set_cursor_byte(4);
    feed(&mut e, "_");
    assert_eq!(at(&e), 2, "first non-blank of this line");
    feed(&mut e, "2_");
    assert_eq!(at(&e), 8, "`2_` is one line down");
}

#[test]
fn d_underscore_is_linewise() {
    let mut e = ed("one\ntwo\nthree\n");
    feed(&mut e, "d2_");
    assert_eq!(e.text(), "three\n");
}

// === `|`: the column motion. ===

#[test]
fn bar_goes_to_a_one_based_column() {
    let mut e = ed("abcdef\n");
    feed(&mut e, "4|");
    assert_eq!(at(&e), 3);
    feed(&mut e, "|");
    assert_eq!(at(&e), 0, "no count -> column 1");
}

#[test]
fn bar_past_the_line_end_clamps() {
    let mut e = ed("abc\nlonger\n");
    feed(&mut e, "99|");
    assert_eq!(at(&e), 3, "the line end, not the next line");
}

#[test]
fn d_bar_deletes_up_to_the_column() {
    let mut e = ed("abcdef\n");
    feed(&mut e, "d4|");
    assert_eq!(e.text(), "def\n");
}

// === `g`-prefixed motions. ===

#[test]
fn g0_and_g_dollar_are_the_line_edges() {
    let mut e = ed("  hello\n");
    e.set_cursor_byte(4);
    feed(&mut e, "g0");
    assert_eq!(at(&e), 0);
    feed(&mut e, "g$");
    assert_eq!(at(&e), 6, "on the last char, not past it");
}

#[test]
fn g_caret_is_the_first_non_blank() {
    let mut e = ed("   hi\n");
    feed(&mut e, "g^");
    assert_eq!(at(&e), 3);
}

#[test]
fn gj_and_gk_walk_lines() {
    // Without soft wrap a display line is a real line, so these are
    // `j`/`k` — but they must exist, or `gj` swallowed the `g`.
    let mut e = ed("one\ntwo\nthree\n");
    feed(&mut e, "gj");
    assert_eq!(at(&e), 4);
    feed(&mut e, "gk");
    assert_eq!(at(&e), 0);
}

// === Scroll motions. ===

#[test]
fn ctrl_f_and_ctrl_b_page_through_the_buffer() {
    let text = lines(60);
    let mut e = ed(&text);
    e.modal_key("C-f");
    assert_eq!(e.cursor_line_col().0, 20, "a page down");
    e.modal_key("C-b");
    assert_eq!(e.cursor_line_col().0, 0);
}

#[test]
fn ctrl_d_and_ctrl_u_are_half_pages() {
    let text = lines(60);
    let mut e = ed(&text);
    e.modal_key("C-d");
    assert_eq!(e.cursor_line_col().0, 10);
    e.modal_key("C-u");
    assert_eq!(e.cursor_line_col().0, 0);
}

#[test]
fn a_scroll_motion_clamps_at_the_buffer_edges() {
    let mut e = ed("one\ntwo\n");
    e.modal_key("C-f");
    assert_eq!(e.cursor_line_col().0, 2, "the last (empty) line");
    e.modal_key("C-b");
    assert_eq!(e.cursor_line_col().0, 0);
}

// === Whole-line shorthands. ===

#[test]
fn capital_y_yanks_the_line() {
    let mut e = ed("one\ntwo\n");
    feed(&mut e, "Yjp");
    assert_eq!(e.text(), "one\ntwo\none\n", "Y is yy, not y$");
}

#[test]
fn capital_y_takes_a_count() {
    let mut e = ed("one\ntwo\nthree\n");
    feed(&mut e, "2Y");
    feed(&mut e, "jjp");
    assert_eq!(e.text(), "one\ntwo\nthree\none\ntwo\n");
}

#[test]
fn g_capital_j_joins_without_a_space() {
    let mut e = ed("one\ntwo\n");
    feed(&mut e, "gJ");
    assert_eq!(e.text(), "onetwo\n");
}

#[test]
fn plain_j_still_inserts_the_space() {
    let mut e = ed("one\ntwo\n");
    feed(&mut e, "J");
    assert_eq!(e.text(), "one two\n");
}
