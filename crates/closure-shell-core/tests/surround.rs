//! evil-surround: `ys`, `cs`, `ds`, and `S` in VISUAL.
//!
//! Asked as a question — can it be a wasm plugin? The plugin host is
//! block-transform shaped: a plugin is handed text and returns text. It
//! cannot register an operator-pending chord, which is the whole of
//! what surround *is*. Natively it is one pair table and three arms on
//! the operator machinery that already resolves motions and text
//! objects, so that is where it lives.
//!
//! Org's emphasis markers are pairs too — `*bold*`, `/italic/`,
//! `=verbatim=` — which is the reason to want this in a note-taking app
//! rather than in a code editor.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{BodyEditor, EditorMode};

/// A Normal-mode editor over `text` with the cursor at byte 0.
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

// === ys: surround a motion or a text object ===

#[test]
fn ysiw_wraps_the_word_under_the_cursor() {
    let mut e = ed("hello world");
    feed(&mut e, "ysiw\"");
    assert_eq!(e.text(), "\"hello\" world");
}

#[test]
fn the_closing_bracket_hugs_and_the_opening_one_pads() {
    // vim-surround's oldest convention, and the one people miss: `)`
    // wraps tight, `(` leaves a space inside.
    let mut e = ed("hello world");
    feed(&mut e, "ysiw)");
    assert_eq!(e.text(), "(hello) world");

    let mut e = ed("hello world");
    feed(&mut e, "ysiw(");
    assert_eq!(e.text(), "( hello ) world");
}

#[test]
fn ys_takes_any_motion_the_operators_take() {
    let mut e = ed("one two three");
    feed(&mut e, "ysw]");
    assert_eq!(e.text(), "[one ]two three", "a motion, not only an object");

    let mut e = ed("one two three");
    feed(&mut e, "ys$*");
    assert_eq!(e.text(), "*one two three*");
}

#[test]
fn yss_surrounds_the_line() {
    let mut e = ed("one two\nsecond line\n");
    feed(&mut e, "yss=");
    assert_eq!(
        e.text(),
        "=one two=\nsecond line\n",
        "the line, not the file"
    );
}

#[test]
fn org_emphasis_markers_are_pairs() {
    // The reason to want this here rather than in a code editor.
    for (key, want) in [
        ('*', "*word* rest"),
        ('/', "/word/ rest"),
        ('_', "_word_ rest"),
        ('=', "=word= rest"),
        ('~', "~word~ rest"),
        ('+', "+word+ rest"),
    ] {
        let mut e = ed("word rest");
        feed(&mut e, &format!("ysiw{key}"));
        assert_eq!(e.text(), want, "surrounding with {key}");
    }
}

#[test]
fn a_char_that_names_no_pair_changes_nothing() {
    let mut e = ed("word rest");
    feed(&mut e, "ysiwt");
    assert_eq!(e.text(), "word rest", "html tags are not implemented");
    assert_eq!(e.mode(), EditorMode::Normal, "and it is not left mid-chord");
}

// === ds: delete a surrounding pair ===

#[test]
fn ds_removes_the_pair_around_the_cursor() {
    let mut e = ed("say \"hello there\" now");
    e.set_cursor_byte(7);
    feed(&mut e, "ds\"");
    assert_eq!(e.text(), "say hello there now");
}

#[test]
fn ds_works_on_brackets_and_on_org_emphasis() {
    let mut e = ed("a (bracketed) thing");
    e.set_cursor_byte(5);
    feed(&mut e, "ds)");
    assert_eq!(e.text(), "a bracketed thing");

    let mut e = ed("a *bold* thing");
    e.set_cursor_byte(5);
    feed(&mut e, "ds*");
    assert_eq!(e.text(), "a bold thing");
}

#[test]
fn ds_with_no_such_pair_around_the_cursor_does_nothing() {
    let mut e = ed("plain text");
    e.set_cursor_byte(3);
    feed(&mut e, "ds\"");
    assert_eq!(e.text(), "plain text");
}

// === cs: change one pair into another ===

#[test]
fn cs_swaps_the_delimiters() {
    let mut e = ed("say \"hello\" now");
    e.set_cursor_byte(7);
    feed(&mut e, "cs\"'");
    assert_eq!(e.text(), "say 'hello' now");
}

#[test]
fn cs_from_a_bracket_to_org_emphasis() {
    let mut e = ed("a (word) here");
    e.set_cursor_byte(4);
    feed(&mut e, "cs)/");
    assert_eq!(e.text(), "a /word/ here");
}

#[test]
fn cs_to_a_padding_bracket_pads() {
    let mut e = ed("a 'word' here");
    e.set_cursor_byte(4);
    feed(&mut e, "cs'{");
    assert_eq!(e.text(), "a { word } here");
}

// === VISUAL S ===

#[test]
fn capital_s_surrounds_the_selection() {
    let mut e = ed("one two three");
    feed(&mut e, "vee");
    feed(&mut e, "S*");
    assert_eq!(e.text(), "*one two* three");
    assert_eq!(
        e.mode(),
        EditorMode::Normal,
        "VISUAL ends with the surround"
    );
}

#[test]
fn capital_c_is_still_the_linewise_change() {
    // `S` is the one evil-surround takes; `C` keeps its vim meaning, so
    // the muscle memory for changing a line is untouched.
    let mut e = ed("one two\nsecond\n");
    feed(&mut e, "vC");
    assert_eq!(e.mode(), EditorMode::Insert);
    assert_eq!(e.text(), "\nsecond\n");
}

// === it is an edit like any other ===

#[test]
fn a_surround_is_one_undo_step() {
    let mut e = ed("hello world");
    feed(&mut e, "ysiw\"");
    assert_eq!(e.text(), "\"hello\" world");
    feed(&mut e, "u");
    assert_eq!(e.text(), "hello world", "one `u` takes the whole surround");
}

#[test]
fn a_surround_repeats_with_dot() {
    let mut e = ed("one two");
    feed(&mut e, "ysiw*");
    assert_eq!(e.text(), "*one* two");
    // `w` from the new `*` lands inside `one`; `ft` is the word after.
    feed(&mut e, "ft");
    feed(&mut e, ".");
    assert_eq!(e.text(), "*one* *two*", "`.` repeats the whole chord");
}

#[test]
fn escape_abandons_a_half_typed_surround() {
    let mut e = ed("hello world");
    feed(&mut e, "ysiw");
    e.modal_key("escape");
    assert_eq!(e.text(), "hello world");
    feed(&mut e, "x");
    assert_eq!(e.text(), "ello world", "and the next key is a plain key");
}
