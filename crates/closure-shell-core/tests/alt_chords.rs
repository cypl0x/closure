//! The Alt chords every text field on the desktop answers to.
//!
//! Reported 2026-08-01: "alt+backspace (and most likely the other alt+
//! keybindings) doesn't work — usually in all of the input text fields
//! no matter what these are working (terminal, chrome, firefox, Emacs),
//! but not in closure".
//!
//! The one-line fields did answer (`LineInput::key` takes `ctrl || alt`
//! for the word kill). The body editor took `ctrl` alone, so
//! Alt+Backspace fell through to plain backspace and ate exactly one
//! character — which looks less like an unbound chord than a broken
//! one, because something *did* happen.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

fn shell() -> (tempfile::TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Alpha\n:PROPERTIES:\n:ID: 01HQALT0000000000000001\n:END:\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault))
}

/// The body editor, in INSERT, with `text` typed into it.
fn typing(sh: &mut Shell, text: &str) -> ModalApp {
    let mut app = ModalApp::new(InputMode::Doom);
    app.on_key(sh, "i", false, false, Some('i')); // open the body
    app.on_key(sh, "i", false, false, Some('i')); // INSERT
    for c in text.chars() {
        app.on_key(sh, &c.to_string(), false, false, Some(c));
    }
    app
}

#[test]
fn alt_backspace_kills_a_word_in_the_body_editor() {
    let (_d, mut sh) = shell();
    let mut app = typing(&mut sh, "hello brave world");
    app.on_key(&mut sh, "backspace", false, true, None);
    assert_eq!(app.body_buffer(), "hello brave ", "the word, not a letter");
}

#[test]
fn ctrl_backspace_still_kills_a_word_too() {
    // The desktop chord that already worked must keep working.
    let (_d, mut sh) = shell();
    let mut app = typing(&mut sh, "hello brave world");
    app.on_key(&mut sh, "backspace", true, false, None);
    assert_eq!(app.body_buffer(), "hello brave ");
}

#[test]
fn a_plain_backspace_is_still_one_character() {
    let (_d, mut sh) = shell();
    let mut app = typing(&mut sh, "hello");
    app.on_key(&mut sh, "backspace", false, false, None);
    assert_eq!(app.body_buffer(), "hell");
}

#[test]
fn alt_d_kills_the_word_after_the_cursor() {
    // `M-d` is `kill-word` in readline and in Emacs, and the twin of
    // the Alt+Backspace above. Every terminal answers to it.
    let (_d, mut sh) = shell();
    let mut app = typing(&mut sh, "hello brave world");
    app.on_key(&mut sh, "a", true, false, None); // C-a, to the line start
    app.on_key(&mut sh, "d", false, true, None); // M-d
    assert_eq!(app.body_buffer(), " brave world");
}

#[test]
fn alt_d_at_the_end_of_the_line_changes_nothing() {
    let (_d, mut sh) = shell();
    let mut app = typing(&mut sh, "hello");
    app.on_key(&mut sh, "d", false, true, None);
    assert_eq!(app.body_buffer(), "hello");
}

#[test]
fn the_alt_arrows_still_walk_by_word() {
    // Already bound; pinned so widening the word kills cannot take
    // them with it.
    let (_d, mut sh) = shell();
    let mut app = typing(&mut sh, "hello brave world");
    app.on_key(&mut sh, "left", false, true, None);
    app.on_key(&mut sh, "backspace", false, true, None);
    assert_eq!(app.body_buffer(), "hello world", "killed `brave `");
}

#[test]
fn alt_backspace_kills_a_word_in_the_one_line_fields_too() {
    // These already worked. Same assertion, so a change to either path
    // cannot silently split their behaviour.
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.run(&mut sh, "capture");
    for c in "hello brave world".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "backspace", false, true, None);
    assert_eq!(app.capture_buffer(), "hello brave ");
}
