//! The one-line text fields, and the chords every text field has.
//!
//! Capture, search, the ticket box and the tag/property fields were
//! plain `String`s that only knew `push` and `pop`: typing went to the
//! end, backspace took from the end, and there was no cursor at all.
//! So `C-a` and `C-e` had nothing to move, a typo in the middle of a
//! captured line meant retyping the tail, and `C-w` — the chord your
//! hands already know — did nothing.
//!
//! `LineInput` is the field itself: text plus a cursor, with the
//! readline set the rest of the desktop answers to.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::LineInput;

fn typed(s: &str) -> LineInput {
    let mut line = LineInput::default();
    for c in s.chars() {
        line.insert_char(c);
    }
    line
}

#[test]
fn typing_appends_and_carries_the_cursor() {
    let line = typed("hello");
    assert_eq!(line.text(), "hello");
    assert_eq!(line.cursor(), 5, "the cursor is after what you typed");
}

#[test]
fn the_cursor_moves_by_character_and_to_both_ends() {
    let mut line = typed("hello");
    line.left();
    assert_eq!(line.cursor(), 4);
    line.home();
    assert_eq!(line.cursor(), 0, "C-a");
    line.right();
    assert_eq!(line.cursor(), 1);
    line.end();
    assert_eq!(line.cursor(), 5, "C-e");
}

#[test]
fn typing_in_the_middle_inserts_there() {
    let mut line = typed("helo");
    line.left();
    line.left();
    line.insert_char('l');
    assert_eq!(line.text(), "hello", "not appended to the end");
    assert_eq!(line.cursor(), 3);
}

#[test]
fn backspace_takes_the_character_before_the_cursor() {
    let mut line = typed("hello");
    line.home();
    line.right();
    line.backspace();
    assert_eq!(line.text(), "ello");
    assert_eq!(line.cursor(), 0);
}

#[test]
fn backspace_at_the_start_is_a_no_op() {
    let mut line = typed("hi");
    line.home();
    line.backspace();
    assert_eq!(line.text(), "hi");
}

#[test]
fn delete_takes_the_character_under_the_cursor() {
    let mut line = typed("hello");
    line.home();
    line.delete();
    assert_eq!(line.text(), "ello");
    assert_eq!(line.cursor(), 0);
}

#[test]
fn kill_word_back_takes_one_word() {
    // C-w, and the desktop's ctrl+backspace.
    let mut line = typed("some words here");
    line.delete_word_back();
    assert_eq!(line.text(), "some words ");
    line.delete_word_back();
    assert_eq!(line.text(), "some ");
}

#[test]
fn kill_to_start_and_end_cut_the_rest() {
    let mut line = typed("some words here");
    line.home();
    line.right();
    line.kill_to_start();
    assert_eq!(line.text(), "ome words here", "C-u");

    let mut line = typed("some words here");
    line.home();
    line.kill_to_end();
    assert_eq!(line.text(), "", "C-k");
}

#[test]
fn a_paste_lands_at_the_cursor() {
    let mut line = typed("ab");
    line.left();
    line.insert_str("XY");
    assert_eq!(line.text(), "aXYb");
    assert_eq!(line.cursor(), 3);
}

#[test]
fn multibyte_text_moves_by_character_not_byte() {
    // A German capture line is the normal case here, not an edge one.
    let mut line = typed("Grüße");
    line.left();
    line.backspace();
    assert_eq!(line.text(), "Grüe", "the ß went, not half of it");
    line.home();
    line.right();
    line.right();
    line.delete();
    assert_eq!(line.text(), "Gre");
}

#[test]
fn a_full_stop_is_just_a_character() {
    // Reported as "weird behaviour in capture when inserting a `.`
    // after a sentence": nothing about `.` may be special in a field.
    let mut line = typed("Call Leon");
    line.insert_char('.');
    line.insert_char(' ');
    line.insert_char('T');
    assert_eq!(line.text(), "Call Leon. T");
    assert_eq!(line.cursor(), 12);
}

#[test]
fn the_readline_chords_are_answered_by_the_field_itself() {
    // The surfaces route keys here rather than each reimplementing a
    // text field badly.
    let mut line = typed("some words here");
    assert!(line.key("a", true, false, None), "C-a is consumed");
    assert_eq!(line.cursor(), 0);
    assert!(line.key("e", true, false, None), "C-e");
    assert_eq!(line.cursor(), 15);
    assert!(line.key("w", true, false, None), "C-w");
    assert_eq!(line.text(), "some words ");
    assert!(line.key("backspace", true, false, None), "ctrl+backspace");
    assert_eq!(line.text(), "some ");
    assert!(line.key("left", false, false, None));
    assert_eq!(line.cursor(), 4);
    assert!(
        !line.key("enter", false, false, None),
        "not the field's key"
    );
    assert!(!line.key("escape", false, false, None), "nor this one");
}

#[test]
fn typing_a_character_goes_through_the_same_door() {
    let mut line = LineInput::default();
    assert!(line.key("x", false, false, Some('x')));
    assert!(line.key("y", false, false, Some('y')));
    assert_eq!(line.text(), "xy");
    // A control chord that is not bound must not type its letter.
    assert!(!line.key("z", true, false, Some('z')));
    assert_eq!(line.text(), "xy", "C-z is not the letter z");
}

#[test]
fn setting_the_text_puts_the_cursor_at_the_end() {
    let mut line = LineInput::default();
    line.set_text("restored");
    assert_eq!(line.cursor(), 8, "ready to keep typing");
    line.clear();
    assert_eq!(line.text(), "");
    assert_eq!(line.cursor(), 0);
}
