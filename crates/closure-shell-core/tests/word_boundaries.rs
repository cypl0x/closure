//! "ctlr+backpsace when trying to delete a word backwards it is
//! impossible to delete/kill a . or /."
//!
//! Every word op treated "not whitespace" as "part of the word", so
//! `~/dev/closure/src` was a single word: one Alt+Backspace took the
//! whole path and there was no way to remove just the last segment. The
//! user asks to inherit the behaviour rather than reinvent it, and the
//! answer is that readline has *two* rules and closure had only one:
//!
//!   * `C-w` is unix-word-rubout — whitespace-delimited, the shell's
//!     rule, which is what closure was doing everywhere;
//!   * `M-DEL` is backward-kill-word — a word is a run of alphanumerics
//!     and punctuation is a boundary, which is also what ctrl+backspace
//!     does in GTK, in a browser and in every editor.
//!
//! Both are kept, on the chords readline puts them on. Nothing can be
//! inherited from the platform here: gpui hands the window raw key
//! events and no OS text field is involved (answered in the org body).

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const NOTES: &str = "\
* Alpha
:PROPERTIES:
:ID: 01HQWORD000000000000001
:END:
body
";

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let shell = Shell::new(vault);
    app.select_by_id(&shell, "01HQWORD000000000000001");
    (dir, shell, app)
}

/// A rename prompt holding `text`, caret at the end.
fn prompt(text: &str) -> (TempDir, Shell, ModalApp) {
    let (dir, mut shell, mut app) = fixture();
    app.run(&mut shell, "rename");
    app.on_key(&mut shell, "u", true, false, None);
    for c in text.chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    (dir, shell, app)
}

fn alt_backspace(app: &mut ModalApp, shell: &mut Shell) {
    app.on_key(shell, "backspace", false, true, None);
}

fn ctrl_w(app: &mut ModalApp, shell: &mut Shell) {
    app.on_key(shell, "w", true, false, None);
}

#[test]
fn a_slash_is_a_boundary_for_alt_backspace() {
    let (_d, mut shell, mut app) = prompt("~/dev/closure");
    alt_backspace(&mut app, &mut shell);
    assert_eq!(app.prompt_text(), Some("~/dev/"), "the last segment only");
    alt_backspace(&mut app, &mut shell);
    assert_eq!(app.prompt_text(), Some("~/dev"), "then the slash itself");
    alt_backspace(&mut app, &mut shell);
    assert_eq!(app.prompt_text(), Some("~/"));
}

#[test]
fn a_dot_is_a_boundary_too() {
    let (_d, mut shell, mut app) = prompt("notes.org");
    alt_backspace(&mut app, &mut shell);
    assert_eq!(app.prompt_text(), Some("notes."));
    alt_backspace(&mut app, &mut shell);
    assert_eq!(app.prompt_text(), Some("notes"));
}

#[test]
fn ctrl_w_still_takes_the_whole_path() {
    // unix-word-rubout, unchanged: it is the chord you use precisely
    // when you want the lot.
    let (_d, mut shell, mut app) = prompt("edit ~/dev/closure");
    ctrl_w(&mut app, &mut shell);
    assert_eq!(app.prompt_text(), Some("edit "));
}

#[test]
fn plain_words_behave_the_same_under_both() {
    for chord in ["alt-backspace", "ctrl-w"] {
        let (_d, mut shell, mut app) = prompt("alpha beta");
        if chord == "ctrl-w" {
            ctrl_w(&mut app, &mut shell);
        } else {
            alt_backspace(&mut app, &mut shell);
        }
        assert_eq!(app.prompt_text(), Some("alpha "), "{chord}");
    }
}

#[test]
fn the_word_motions_stop_at_punctuation() {
    // M-b and M-f are readline's backward-word / forward-word, and they
    // use the same definition of a word as M-DEL does.
    let (_d, mut shell, mut app) = prompt("notes.org");
    app.on_key(&mut shell, "b", false, true, None);
    assert_eq!(app.prompt_cursor(), 6, "back to the start of `org`");
    app.on_key(&mut shell, "b", false, true, None);
    assert_eq!(app.prompt_cursor(), 5, "then the dot on its own");
    app.on_key(&mut shell, "b", false, true, None);
    assert_eq!(app.prompt_cursor(), 0, "then `notes`");
}

#[test]
fn alt_d_kills_forward_to_the_boundary() {
    let (_d, mut shell, mut app) = prompt("notes.org");
    app.on_key(&mut shell, "a", true, false, None);
    app.on_key(&mut shell, "d", false, true, None);
    assert_eq!(app.prompt_text(), Some(".org"));
}

#[test]
fn the_editor_agrees_with_the_prompt() {
    // The same complaint applies to a path typed into a note.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "edit-body");
    app.on_key(&mut shell, "i", false, false, Some('i'));
    for c in " ~/dev/closure".chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    // The buffer opens at the start of the existing body, so the path
    // is typed in front of it and the caret sits after `closure`.
    assert!(
        app.body_buffer().starts_with(" ~/dev/closure"),
        "{}",
        app.body_buffer()
    );

    app.on_key(&mut shell, "backspace", false, true, None);
    assert!(
        app.body_buffer().starts_with(" ~/dev/") && !app.body_buffer().contains("closure"),
        "one segment taken: {}",
        app.body_buffer()
    );
    app.on_key(&mut shell, "backspace", false, true, None);
    assert!(
        app.body_buffer().starts_with(" ~/dev"),
        "then the separator: {}",
        app.body_buffer()
    );
}

#[test]
fn a_trailing_space_is_eaten_before_the_word() {
    // Readline skips the whitespace, then kills the word behind it.
    let (_d, mut shell, mut app) = prompt("alpha beta  ");
    alt_backspace(&mut app, &mut shell);
    assert_eq!(app.prompt_text(), Some("alpha "));
}

#[test]
fn killing_at_the_start_is_not_an_error() {
    let (_d, mut shell, mut app) = prompt("");
    alt_backspace(&mut app, &mut shell);
    assert_eq!(app.prompt_text(), Some(""));
}

#[test]
fn what_alt_backspace_took_can_be_yanked_back() {
    let (_d, mut shell, mut app) = prompt("notes.org");
    alt_backspace(&mut app, &mut shell);
    app.on_key(&mut shell, "y", true, false, None);
    assert_eq!(app.prompt_text(), Some("notes.org"));
}
