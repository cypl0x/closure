//! The stateful half of the vim vocabulary: named registers, marks,
//! recorded macros, the last-visual/last-insert jumps, the number
//! increments, REPLACE mode, the sentence objects, and buffer search.
//!
//! As in `vim.rs`, these drive [`BodyEditor`] directly, so the same
//! grammar reaches every shell (I4).

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{BodyEditor, EditorMode};

fn ed(text: &str) -> BodyEditor {
    let mut e = BodyEditor::new();
    e.load(text.to_owned());
    e.to_normal();
    e.set_cursor_byte(0);
    e
}

fn feed(e: &mut BodyEditor, keys: &str) {
    for c in keys.chars() {
        e.modal_key(&c.to_string());
    }
}

/// Type `text` as an INSERT-mode burst.
fn typ(e: &mut BodyEditor, text: &str) {
    for c in text.chars() {
        e.insert_char(c);
    }
}

const fn at(e: &BodyEditor) -> usize {
    e.cursor_byte()
}

// === Named registers. ===

#[test]
fn a_named_register_holds_its_own_yank() {
    let mut e = ed("one\ntwo\n");
    feed(&mut e, "\"ayy"); // yank line 1 into register a
    feed(&mut e, "jyy"); // yank line 2 into the unnamed register
    feed(&mut e, "\"ap");
    assert_eq!(e.text(), "one\ntwo\none\n", "the a register survived");
}

#[test]
fn a_named_delete_does_not_clobber_another_register() {
    let mut e = ed("one\ntwo\nthree\n");
    feed(&mut e, "\"ayy"); // a = "one\n"
    feed(&mut e, "j\"bdd"); // b = "two\n"
    feed(&mut e, "\"aP");
    assert_eq!(e.text(), "one\none\nthree\n");
}

#[test]
fn an_uppercase_register_appends() {
    let mut e = ed("one\ntwo\nthree\n");
    feed(&mut e, "\"ayy");
    feed(&mut e, "j\"Ayy");
    feed(&mut e, "j\"ap");
    assert_eq!(e.text(), "one\ntwo\nthree\none\ntwo\n");
}

#[test]
fn a_named_yank_also_fills_the_unnamed_register() {
    // Vim's rule: `"ayy` sets both, so a bare `p` still pastes it.
    let mut e = ed("one\ntwo\n");
    feed(&mut e, "\"ayyjp");
    assert_eq!(e.text(), "one\ntwo\none\n");
}

// === Marks. ===

#[test]
fn a_mark_is_jumped_back_to_exactly() {
    let mut e = ed("hello world\n");
    e.set_cursor_byte(6);
    feed(&mut e, "ma");
    feed(&mut e, "0");
    assert_eq!(at(&e), 0);
    feed(&mut e, "`a");
    assert_eq!(at(&e), 6);
}

#[test]
fn the_quoted_mark_is_the_lines_first_non_blank() {
    let mut e = ed("one\n   two\n");
    e.set_cursor_byte(8);
    feed(&mut e, "ma");
    feed(&mut e, "gg");
    feed(&mut e, "'a");
    assert_eq!(at(&e), 7, "past the indent");
}

#[test]
fn an_operator_takes_a_mark_as_its_target() {
    let mut e = ed("abcdef\n");
    e.set_cursor_byte(4);
    feed(&mut e, "ma0");
    feed(&mut e, "d`a");
    assert_eq!(e.text(), "ef\n", "charwise, exclusive of the mark");
}

#[test]
fn an_operator_takes_a_quoted_mark_linewise() {
    let mut e = ed("one\ntwo\nthree\n");
    feed(&mut e, "jma");
    feed(&mut e, "gg");
    feed(&mut e, "d'a");
    assert_eq!(e.text(), "three\n");
}

#[test]
fn an_unset_mark_is_a_no_op() {
    let mut e = ed("abc\n");
    feed(&mut e, "`z");
    assert_eq!(at(&e), 0);
    feed(&mut e, "d`z");
    assert_eq!(e.text(), "abc\n");
}

// === Recorded macros. ===

#[test]
fn a_recorded_macro_replays_its_strokes() {
    let mut e = ed("a\nb\nc\n");
    feed(&mut e, "qq"); // record into q
    feed(&mut e, "x"); // delete a char
    feed(&mut e, "j"); // step down
    feed(&mut e, "q"); // stop
    assert_eq!(e.text(), "\nb\nc\n");
    feed(&mut e, "@q");
    assert_eq!(e.text(), "\n\nc\n");
}

#[test]
fn a_macro_replays_the_text_typed_inside_it() {
    let mut e = ed("one\ntwo\n");
    feed(&mut e, "qa");
    feed(&mut e, "I");
    typ(&mut e, "- ");
    e.to_normal();
    feed(&mut e, "j");
    feed(&mut e, "q");
    assert_eq!(e.text(), "- one\ntwo\n");
    feed(&mut e, "@a");
    assert_eq!(e.text(), "- one\n- two\n");
}

#[test]
fn at_at_repeats_the_last_macro() {
    let mut e = ed("a\nb\nc\nd\n");
    feed(&mut e, "qqxjq");
    feed(&mut e, "@q");
    feed(&mut e, "@@");
    assert_eq!(e.text(), "\n\n\nd\n");
}

#[test]
fn a_macro_takes_a_count() {
    let mut e = ed("a\nb\nc\nd\n");
    feed(&mut e, "qqxjq");
    feed(&mut e, "2@q");
    assert_eq!(e.text(), "\n\n\nd\n");
}

#[test]
fn recording_is_visible_while_it_runs() {
    let mut e = ed("abc\n");
    assert_eq!(e.recording_register(), None);
    feed(&mut e, "qa");
    assert_eq!(e.recording_register(), Some('a'));
    feed(&mut e, "q");
    assert_eq!(e.recording_register(), None);
}

// === `gv` and `gi`. ===

#[test]
fn gv_reselects_the_last_visual_range() {
    let mut e = ed("hello world\n");
    feed(&mut e, "viw"); // select "hello"
    e.modal_key("escape");
    feed(&mut e, "$");
    feed(&mut e, "gv");
    assert_eq!(e.mode(), EditorMode::Visual);
    assert_eq!(e.visual_selection(), Some((0, 5)));
}

#[test]
fn gv_after_an_operator_reselects_what_was_operated_on() {
    let mut e = ed("hello world\n");
    feed(&mut e, "viwU"); // upper-case the word
    assert_eq!(e.text(), "HELLO world\n");
    feed(&mut e, "gvu");
    assert_eq!(e.text(), "hello world\n");
}

#[test]
fn gi_resumes_insert_where_it_last_ended() {
    let mut e = ed("hello\n");
    e.set_cursor_byte(5);
    e.to_insert();
    typ(&mut e, "!");
    e.to_normal();
    feed(&mut e, "gg0");
    feed(&mut e, "gi");
    assert_eq!(e.mode(), EditorMode::Insert);
    typ(&mut e, "?");
    assert_eq!(e.text(), "hello!?\n");
}

// === `C-a` / `C-x`. ===

#[test]
fn ctrl_a_increments_the_number_under_the_cursor() {
    let mut e = ed("item 41 here\n");
    e.modal_key("C-a");
    assert_eq!(e.text(), "item 42 here\n");
    assert_eq!(at(&e), 6, "cursor on the last digit");
}

#[test]
fn ctrl_x_decrements_and_takes_a_count() {
    let mut e = ed("x 10\n");
    feed(&mut e, "3");
    e.modal_key("C-x");
    assert_eq!(e.text(), "x 7\n");
}

#[test]
fn an_increment_keeps_a_negative_sign() {
    let mut e = ed("t -1\n");
    e.modal_key("C-x");
    assert_eq!(e.text(), "t -2\n");
}

#[test]
fn an_increment_with_no_number_on_the_line_changes_nothing() {
    let mut e = ed("no digits\n");
    e.modal_key("C-a");
    assert_eq!(e.text(), "no digits\n");
}

// === REPLACE mode. ===

#[test]
fn capital_r_overwrites_instead_of_inserting() {
    let mut e = ed("abcdef\n");
    feed(&mut e, "R");
    assert!(e.replacing(), "the shells show REPLACE, not INSERT");
    typ(&mut e, "XY");
    assert_eq!(e.text(), "XYcdef\n");
}

#[test]
fn replace_stops_at_the_line_end_and_then_appends() {
    let mut e = ed("ab\ncd\n");
    feed(&mut e, "R");
    typ(&mut e, "XYZ");
    assert_eq!(e.text(), "XYZ\ncd\n", "no newline was eaten");
}

#[test]
fn escape_leaves_replace_mode() {
    let mut e = ed("abc\n");
    feed(&mut e, "R");
    e.to_normal();
    assert!(!e.replacing());
    typ(&mut e, "Z");
    assert_eq!(e.text(), "Zabc\n", "back to inserting");
}

// === Sentence objects. ===

#[test]
fn dis_deletes_the_sentence_under_the_cursor() {
    let mut e = ed("One two. Three four. Five.\n");
    e.set_cursor_byte(10);
    feed(&mut e, "dis");
    assert_eq!(e.text(), "One two.  Five.\n");
}

#[test]
fn das_takes_the_trailing_space_too() {
    let mut e = ed("One two. Three four. Five.\n");
    e.set_cursor_byte(10);
    feed(&mut e, "das");
    assert_eq!(e.text(), "One two. Five.\n");
}

#[test]
fn vis_selects_the_sentence() {
    let mut e = ed("Hi there. Bye.\n");
    feed(&mut e, "vis");
    assert_eq!(e.visual_selection(), Some((0, 9)));
}

// === Buffer search. ===

#[test]
fn slash_search_jumps_to_the_next_match() {
    let mut e = ed("alpha beta alpha\n");
    feed(&mut e, "/beta");
    assert_eq!(e.search_prompt(), Some("/beta".to_owned()));
    e.modal_key("enter");
    assert_eq!(at(&e), 6);
    assert_eq!(e.search_prompt(), None);
}

#[test]
fn n_and_capital_n_walk_the_matches() {
    let mut e = ed("x ab y ab z ab\n");
    feed(&mut e, "/ab");
    e.modal_key("enter");
    assert_eq!(at(&e), 2);
    feed(&mut e, "n");
    assert_eq!(at(&e), 7);
    feed(&mut e, "N");
    assert_eq!(at(&e), 2);
}

#[test]
fn search_wraps_around_the_buffer() {
    let mut e = ed("ab cd\n");
    feed(&mut e, "/ab");
    e.modal_key("enter");
    assert_eq!(at(&e), 0, "wrapped past the end back to the start");
}

#[test]
fn question_mark_searches_backwards() {
    let mut e = ed("ab cd ab\n");
    e.set_cursor_byte(5);
    feed(&mut e, "?ab");
    e.modal_key("enter");
    assert_eq!(at(&e), 0);
}

#[test]
fn escape_abandons_a_search() {
    let mut e = ed("alpha\n");
    feed(&mut e, "/alp");
    e.modal_key("escape");
    assert_eq!(e.search_prompt(), None);
    assert_eq!(at(&e), 0);
}

#[test]
fn backspace_edits_the_search_and_closing_it_empty_cancels() {
    let mut e = ed("alpha\n");
    feed(&mut e, "/xy");
    e.modal_key("backspace");
    assert_eq!(e.search_prompt(), Some("/x".to_owned()));
    e.modal_key("backspace");
    e.modal_key("backspace");
    assert_eq!(e.search_prompt(), None, "backspacing past the / closes it");
}

#[test]
fn star_searches_for_the_word_under_the_cursor() {
    let mut e = ed("foo bar foo\n");
    feed(&mut e, "*");
    assert_eq!(at(&e), 8);
    feed(&mut e, "#");
    assert_eq!(at(&e), 0);
}

#[test]
fn a_search_is_an_operator_target() {
    let mut e = ed("alpha beta\n");
    feed(&mut e, "d/beta");
    e.modal_key("enter");
    assert_eq!(e.text(), "beta\n");
}

#[test]
fn a_search_that_matches_nothing_leaves_the_cursor_alone() {
    let mut e = ed("alpha\n");
    feed(&mut e, "/zzz");
    e.modal_key("enter");
    assert_eq!(at(&e), 0);
}
