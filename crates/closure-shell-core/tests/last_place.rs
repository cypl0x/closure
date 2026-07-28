//! Opening the app where you left it.
//!
//! Every session started on row zero, so the note you were in the
//! middle of was something you had to find again — and the cursor
//! inside a body was already remembered ([`navigation.rs`]), which made
//! the outline forgetting the *note* the odd one out.
//!
//! It lives in `config.org` like the other durable half of the view
//! state (input mode, view, wrap, theme, peers): the vault is plain
//! files, and a hidden state directory would be the one thing in it you
//! could not read. Written when the window closes, not on every arrow —
//! a config file rewritten at cursor speed is a config file at war with
//! the editor you have it open in.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const SRC: &str = "\
* First
:PROPERTIES:
:ID: 01HQPLACE00000000000000001
:END:
* Second
:PROPERTIES:
:ID: 01HQPLACE00000000000000002
:END:
* Third
:PROPERTIES:
:ID: 01HQPLACE00000000000000003
:END:
";

fn shell() -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

fn config(dir: &TempDir) -> String {
    fs::read_to_string(dir.path().join("config.org")).unwrap_or_default()
}

#[test]
fn the_selected_note_is_written_when_the_session_ends() {
    let (dir, sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.select_by_id(&sh, "01HQPLACE00000000000000002");
    app.save_last_place(&sh);
    assert!(
        config(&dir).contains("01HQPLACE00000000000000002"),
        "the id is in the file: {}",
        config(&dir)
    );
}

#[test]
fn the_next_session_opens_on_it() {
    let (dir, sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.select_by_id(&sh, "01HQPLACE00000000000000003");
    app.save_last_place(&sh);

    // A fresh app over the same vault, as a restart is.
    let v = Vault::open(dir.path()).expect("reopen");
    let sh2 = Shell::new(v);
    let mut next = ModalApp::new(InputMode::Doom);
    next.restore_last_place(&sh2);
    assert_eq!(
        next.rows(&sh2)[next.selected()].title,
        "Third",
        "back where the last session was"
    );
    assert!(next.selection_active(), "and actually selected");
}

#[test]
fn editing_a_body_is_what_makes_a_note_the_last_place() {
    // "Remember last edited and/or selected element": editing one is
    // the strongest possible statement that it is where you were.
    let (dir, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.select_by_id(&sh, "01HQPLACE00000000000000001");
    app.run(&mut sh, "edit-body");
    app.run_ex_line(&mut sh, "q");
    // …and then the cursor wanders somewhere else without opening it.
    app.select_by_id(&sh, "01HQPLACE00000000000000003");
    app.save_last_place(&sh);
    assert!(
        config(&dir).contains("01HQPLACE00000000000000001"),
        "the note that was edited wins: {}",
        config(&dir)
    );
}

#[test]
fn a_remembered_note_that_is_gone_is_not_an_error() {
    let (dir, sh) = shell();
    fs::write(
        dir.path().join("config.org"),
        "#+BEGIN_SRC closure-config\nlast_place = 01HQDELETED0000000000000000\n#+END_SRC\n",
    )
    .expect("write");
    let mut app = ModalApp::new(InputMode::Doom);
    app.restore_last_place(&sh);
    assert_eq!(app.selected(), 0, "a vault edited elsewhere still opens");
}

#[test]
fn a_vault_that_was_never_left_anywhere_opens_at_the_top() {
    let (_d, sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.restore_last_place(&sh);
    assert_eq!(app.selected(), 0);
}

#[test]
fn saving_nothing_selected_leaves_the_file_alone() {
    // Escape drops the selection; that is not "I was nowhere", it is
    // "do not move me next time".
    let (dir, sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.select_by_id(&sh, "01HQPLACE00000000000000002");
    app.save_last_place(&sh);
    let before = config(&dir);
    app.clear_selection();
    app.save_last_place(&sh);
    assert_eq!(config(&dir), before, "the last real place is kept");
}
