//! "show cursor position in editor view (like line and row)"
//!
//! Every editor has this and closure had it nowhere: the buffer knew
//! where the caret was — the gutter is built from it — but nothing
//! said so. "Which line am I on?" was a question you answered by
//! counting.
//!
//! One-based, because that is what a line number means everywhere
//! else, including closure's own gutter two columns to the left.
//! Reporting a zero-based column beside a one-based gutter would be a
//! small lie told constantly.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const NOTES: &str = "\
* Alpha
:PROPERTIES:
:ID: 01HQPOS000000000000001
:END:
first line
second line here
third
";

fn editing() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let mut shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, "01HQPOS000000000000001"));
    app.run(&mut shell, "edit-body");
    (dir, shell, app)
}

#[test]
fn the_editor_says_where_the_caret_is() {
    let (_d, _sh, mut app) = editing();
    app.body_click(0, 0);
    assert_eq!(app.cursor_position(), Some((1, 1)));
}

#[test]
fn it_counts_from_one_like_the_gutter_beside_it() {
    // The gutter prints `1` for the first line. A position that said
    // `0` next to it would be a small lie told constantly.
    let (_d, _sh, mut app) = editing();
    app.body_click(2, 4);
    assert_eq!(app.cursor_position(), Some((3, 5)));
}

#[test]
fn there_is_nothing_to_say_outside_a_buffer() {
    // The outline has a selection, not a caret; a line/column there
    // would be describing something that is not on screen.
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let app = ModalApp::new(InputMode::Doom);
    let _ = Shell::new(vault);
    assert_eq!(app.cursor_position(), None);
    let _ = dir;
}

#[test]
fn the_column_is_in_characters_not_bytes() {
    // A note with an umlaut in it would otherwise report a column that
    // jumps by two, which is the sort of thing that makes a reader
    // distrust the whole status bar.
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Alpha\n:PROPERTIES:\n:ID: 01HQPOS000000000000001\n:END:\nübermäßig lang\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let mut shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, "01HQPOS000000000000001"));
    app.run(&mut shell, "edit-body");
    app.body_click(0, 3);
    assert_eq!(app.cursor_position(), Some((1, 4)));
    let _ = dir;
}

#[test]
fn it_follows_the_caret() {
    let (_d, mut shell, mut app) = editing();
    app.body_click(0, 0);
    app.on_key(&mut shell, "down", false, false, None);
    assert_eq!(app.cursor_position().map(|(l, _)| l), Some(2));
}

#[test]
fn the_label_is_short_and_says_which_is_which() {
    // A bare `3:5` in a status bar full of counts is ambiguous; the
    // shells paint this string, so it carries its own meaning.
    let (_d, _sh, mut app) = editing();
    app.body_click(2, 4);
    let label = app.cursor_position_label().expect("a label");
    assert!(label.contains('3') && label.contains('5'), "{label}");
    assert!(label.chars().count() <= 12, "{label}");
}
