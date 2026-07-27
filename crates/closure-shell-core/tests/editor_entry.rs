//! Which mode the body editor opens in.
//!
//! `BodyEditor::load` put every buffer into INSERT with the cursor at
//! the end, whatever the user's input mode was. In a modal mode that is
//! wrong in the way that matters: the first thing an evil user types
//! into an open buffer is a normal-mode command, so `diw` deleted
//! nothing and inserted the literal text `diw` into the note instead.
//! It is also not what Doom does — `org-edit-special` lands you in
//! normal state, because a buffer is a buffer and not a text field.
//!
//! So: Vim, Doom and Helix open in NORMAL at the top of the buffer;
//! Notion and Emacs — the two non-modal modes, where there is no
//! NORMAL to be in — keep opening in INSERT.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{EditorMode, ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const SRC: &str = "* Note\n:PROPERTIES:\n:ID: 01HQENTRY000000000000001\n:END:\n\
                   one two three\nsecond line\n";

fn fixture(mode: InputMode) -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(mode))
}

fn feed(app: &mut ModalApp, shell: &mut Shell, keys: &str) {
    for c in keys.chars() {
        app.on_key(shell, &c.to_string(), false, false, Some(c));
    }
}

// === the entry mode follows the input mode ===

#[test]
fn a_modal_mode_opens_the_body_in_normal_at_the_top() {
    for mode in [InputMode::Vim, InputMode::Doom, InputMode::Helix] {
        let (_d, mut shell, mut app) = fixture(mode);
        app.run(&mut shell, "edit-body");
        assert_eq!(app.surface(), ModalSurface::EditBody);
        assert_eq!(
            app.body_mode(),
            EditorMode::Normal,
            "{mode:?} edits modally"
        );
        assert_eq!(
            app.body_cursor(),
            (0, 0),
            "{mode:?} starts at the top, the way opening a buffer does"
        );
    }
}

#[test]
fn a_non_modal_mode_still_opens_the_body_in_insert() {
    for mode in [InputMode::Notion, InputMode::Emacs] {
        let (_d, mut shell, mut app) = fixture(mode);
        app.run(&mut shell, "edit-body");
        assert_eq!(
            app.body_mode(),
            EditorMode::Insert,
            "{mode:?} has no NORMAL to be in"
        );
    }
}

#[test]
fn the_status_line_says_which_mode_you_landed_in() {
    // Landing in NORMAL is only a good idea if the window says so; the
    // whole bug was a user typing into a mode they did not know about.
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    app.run(&mut shell, "edit-body");
    let status = app.status().to_lowercase();
    assert!(status.contains("normal"), "status: {}", app.status());
    assert!(status.contains('i'), "and how to type: {}", app.status());
}

// === the bug the user reported ===

#[test]
fn diw_deletes_the_inner_word_right_after_opening_the_editor() {
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    app.run(&mut shell, "edit-body");
    feed(&mut app, &mut shell, "diw");
    assert_eq!(
        app.body_buffer(),
        " two three\nsecond line\n",
        "`diw` ran as a command, not as three characters of prose"
    );
}

#[test]
fn ciw_changes_the_inner_word_right_after_opening_the_editor() {
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    app.run(&mut shell, "edit-body");
    feed(&mut app, &mut shell, "ciw");
    assert_eq!(app.body_mode(), EditorMode::Insert, "`c` ends in INSERT");
    feed(&mut app, &mut shell, "ONE");
    assert_eq!(app.body_buffer(), "ONE two three\nsecond line\n");
}

#[test]
fn insert_is_one_keystroke_away() {
    // The cost of landing in NORMAL: `i`. It has to work the moment the
    // buffer opens, at the position the cursor is actually at.
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    app.run(&mut shell, "edit-body");
    feed(&mut app, &mut shell, "i");
    assert_eq!(app.body_mode(), EditorMode::Insert);
    feed(&mut app, &mut shell, "X");
    assert_eq!(app.body_buffer(), "Xone two three\nsecond line\n");
}

#[test]
fn a_source_block_also_opens_in_normal() {
    // org-edit-special is the other door into the same editor, and Doom
    // opens *that* buffer in normal state too.
    let src = "* Note\n:PROPERTIES:\n:ID: 01HQENTRY000000000000002\n:END:\n\
               #+BEGIN_SRC sh\necho one two\n#+END_SRC\n";
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), src).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let (mut shell, mut app) = (Shell::new(vault), ModalApp::new(InputMode::Doom));
    app.run(&mut shell, "edit-body");
    app.run(&mut shell, "edit-special");
    assert_eq!(app.surface(), ModalSurface::EditBlock);
    assert_eq!(app.body_mode(), EditorMode::Normal);
    feed(&mut app, &mut shell, "diw");
    assert_eq!(
        app.body_buffer(),
        " one two\n",
        "the block's interior took the command too"
    );
}
