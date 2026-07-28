//! Folding inside the buffer.
//!
//! A note with three source blocks in it is mostly code you are not
//! reading right now, and a file opened in the editor view is mostly
//! headlines you are not editing. Org folds both; the buffer could not
//! fold either, so the only way past a long block was to scroll it.
//!
//! The fold lives on the *line*: a `#+BEGIN_…` line hides through its
//! `#+END_…`, a headline hides through the line before the next
//! headline at its level or shallower. Which lines are hidden is the
//! kernel's answer, so every shell hides the same ones.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const BODY: &str = "\
intro line
#+BEGIN_SRC sh
echo one
echo two
#+END_SRC
after the block
";

fn shell() -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* Note\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

/// The editor over `text`, in NORMAL with the cursor at the top.
fn editing(text: &str) -> (TempDir, Shell, ModalApp) {
    let (d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "i", false, false, Some('i'));
    app.on_key(&mut sh, "i", false, false, Some('i'));
    for c in text.chars() {
        if c == '\n' {
            app.on_key(&mut sh, "enter", false, false, None);
        } else {
            app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
        }
    }
    app.on_key(&mut sh, "escape", false, false, None);
    app.body_set_cursor(0);
    (d, sh, app)
}

#[test]
fn nothing_is_folded_to_begin_with() {
    let (_d, _sh, app) = editing(BODY);
    assert!(app.body_hidden_lines().is_empty());
}

#[test]
fn folding_a_block_hides_its_contents_but_not_its_first_line() {
    let (_d, mut sh, mut app) = editing(BODY);
    app.body_goto_line(1); // the #+BEGIN_SRC line
    app.run(&mut sh, "toggle-fold");
    let hidden = app.body_hidden_lines();
    assert!(!hidden.contains(&1), "the delimiter stays: {hidden:?}");
    assert!(hidden.contains(&2), "its contents go: {hidden:?}");
    assert!(hidden.contains(&3), "{hidden:?}");
    assert!(hidden.contains(&4), "including the #+END_: {hidden:?}");
    assert!(!hidden.contains(&5), "and the line after stays: {hidden:?}");
}

#[test]
fn folding_from_inside_the_block_folds_the_block() {
    // The cursor is usually *in* the thing you want folded.
    let (_d, mut sh, mut app) = editing(BODY);
    app.body_goto_line(2);
    app.run(&mut sh, "toggle-fold");
    assert!(app.body_hidden_lines().contains(&3), "folded from within");
}

#[test]
fn folding_twice_unfolds() {
    let (_d, mut sh, mut app) = editing(BODY);
    app.body_goto_line(1);
    app.run(&mut sh, "toggle-fold");
    assert!(!app.body_hidden_lines().is_empty());
    app.run(&mut sh, "toggle-fold");
    assert!(app.body_hidden_lines().is_empty(), "back to open");
}

#[test]
fn a_headline_folds_its_subtree() {
    // The editor *view* is a whole file, so this is the common case.
    let file = "* One\nbody of one\n** Child\nchild body\n* Two\nbody of two\n";
    let (_d, mut sh, mut app) = editing(file);
    app.body_goto_line(0);
    app.run(&mut sh, "toggle-fold");
    let hidden = app.body_hidden_lines();
    assert!(!hidden.contains(&0), "the headline itself stays");
    for line in 1..=3 {
        assert!(hidden.contains(&line), "line {line} hidden: {hidden:?}");
    }
    assert!(
        !hidden.contains(&4),
        "the next level-1 headline is not part of it: {hidden:?}"
    );
}

#[test]
fn a_child_headline_folds_only_its_own() {
    let file = "* One\nbody of one\n** Child\nchild body\n* Two\n";
    let (_d, mut sh, mut app) = editing(file);
    app.body_goto_line(2);
    app.run(&mut sh, "toggle-fold");
    let hidden = app.body_hidden_lines();
    assert_eq!(hidden, vec![3], "{hidden:?}");
}

#[test]
fn folding_a_plain_line_says_there_is_nothing_to_fold() {
    let (_d, mut sh, mut app) = editing(BODY);
    app.body_goto_line(0); // "intro line"
    app.run(&mut sh, "toggle-fold");
    assert!(app.body_hidden_lines().is_empty());
    assert!(!app.status().is_empty(), "and it says so");
}

#[test]
fn editing_the_buffer_drops_the_folds() {
    // A fold is a range of lines; once the lines move, the range is a
    // guess. Dropping it is honest and cheap.
    let (_d, mut sh, mut app) = editing(BODY);
    app.body_goto_line(1);
    app.run(&mut sh, "toggle-fold");
    assert!(!app.body_hidden_lines().is_empty());
    app.on_key(&mut sh, "o", false, false, Some('o')); // opens a line, INSERT
    assert!(
        app.body_hidden_lines().is_empty(),
        "an edit clears the folds"
    );
}
