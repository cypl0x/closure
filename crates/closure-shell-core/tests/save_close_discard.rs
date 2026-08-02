//! "save/close/discard editor (changes)"
//!
//! Three verbs, and a buffer is one of three kinds — a headline's body,
//! a source block, a whole file — so nine answers, of which four were
//! wrong.
//!
//! The one that costs you work: `body_dirty` asked whether
//! `edit_target` was set, and a *file* buffer never sets it — it has a
//! `file_target`. So a file you had typed a page into reported itself
//! clean, and every guard that asks before closing let it go without a
//! word. `:q` closed it, the view toggle closed it, `escape` closed it.
//!
//! The one that surprises you: `C-s` in a source block *closed* the
//! block, while the same key in the two buffers beside it wrote and
//! stayed. Save is not close; that is what close is for.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const NOTES: &str = "\
* Alpha
:PROPERTIES:
:ID: 01HQSAVE00000000000001
:END:
original body

#+begin_src sh
echo original
#+end_src
";

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, "01HQSAVE00000000000001"));
    (dir, shell, app)
}

/// Type `s` into whatever buffer is open, in INSERT.
fn type_in(app: &mut ModalApp, shell: &mut Shell, s: &str) {
    for c in s.chars() {
        app.on_key(shell, &c.to_string(), false, false, Some(c));
    }
}

/// A file buffer with something typed into it.
fn modified_file(app: &mut ModalApp, shell: &mut Shell) {
    app.run(shell, "toggle-view");
    assert_eq!(app.surface(), ModalSurface::EditFile, "the file opened");
    app.on_key(shell, "i", false, false, Some('i'));
    type_in(app, shell, "TYPED");
}

#[test]
fn a_modified_file_buffer_says_it_is_modified() {
    // Everything below follows from this one: the guards all ask.
    let (_d, mut shell, mut app) = fixture();
    modified_file(&mut app, &mut shell);
    assert!(app.body_dirty(), "a file buffer with typing in it is dirty");
}

#[test]
fn a_saved_file_buffer_is_clean_again() {
    let (_d, mut shell, mut app) = fixture();
    modified_file(&mut app, &mut shell);
    app.run(&mut shell, "save-buffer");
    assert!(!app.body_dirty(), "the write is the new baseline");
}

#[test]
fn closing_a_modified_file_buffer_is_refused() {
    let (_d, mut shell, mut app) = fixture();
    modified_file(&mut app, &mut shell);
    app.run(&mut shell, "command");
    type_in(&mut app, &mut shell, "q");
    app.on_key(&mut shell, "enter", false, false, None);
    assert_eq!(
        app.surface(),
        ModalSurface::EditFile,
        "still in the buffer: {}",
        app.status()
    );
    assert!(app.body_buffer().contains("TYPED"), "and the text is there");
}

#[test]
fn toggling_the_view_does_not_drop_a_modified_file() {
    // The view toggle closes the file buffer by another door, and it
    // is the door most easily pressed by accident.
    let (_d, mut shell, mut app) = fixture();
    modified_file(&mut app, &mut shell);
    app.run(&mut shell, "toggle-view");
    assert_eq!(
        app.surface(),
        ModalSurface::EditFile,
        "the toggle kept the unsaved file: {}",
        app.status()
    );
}

#[test]
fn discarding_a_file_buffer_still_discards_it() {
    // The refusal must not become a trap: the explicit discard is the
    // way out and it stays a way out.
    let (dir, mut shell, mut app) = fixture();
    modified_file(&mut app, &mut shell);
    app.run(&mut shell, "discard-edit");
    assert_ne!(app.surface(), ModalSurface::EditFile, "the buffer closed");
    let on_disk = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(!on_disk.contains("TYPED"), "nothing written: {on_disk}");
}

#[test]
fn saving_a_source_block_keeps_it_open() {
    // `C-s` is "write it down", not "I am finished". The body buffer
    // and the file buffer beside it both already knew that.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "list-blocks");
    app.run(&mut shell, "edit-special");
    assert_eq!(app.surface(), ModalSurface::EditBlock, "a block opened");
    app.on_key(&mut shell, "i", false, false, Some('i'));
    type_in(&mut app, &mut shell, "X");
    app.run(&mut shell, "save-buffer");
    assert_eq!(
        app.surface(),
        ModalSurface::EditBlock,
        "save is not close: {}",
        app.status()
    );
    assert!(!app.body_dirty(), "but it did write");
}

#[test]
fn saving_a_body_keeps_it_open() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "edit-body");
    app.on_key(&mut shell, "i", false, false, Some('i'));
    type_in(&mut app, &mut shell, "X");
    app.run(&mut shell, "save-buffer");
    assert_eq!(app.surface(), ModalSurface::EditBody);
    assert!(!app.body_dirty());
}

#[test]
fn saving_a_file_keeps_it_open() {
    let (dir, mut shell, mut app) = fixture();
    modified_file(&mut app, &mut shell);
    app.run(&mut shell, "save-buffer");
    assert_eq!(app.surface(), ModalSurface::EditFile);
    let on_disk = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(on_disk.contains("TYPED"), "{on_disk}");
}

#[test]
fn the_refusal_names_keys_that_still_do_it() {
    // `C-Enter` stopped saving when org's `C-c C-c` took the job, and a
    // prompt that names a key which no longer works is worse than no
    // prompt: you press it, nothing happens, and you try harder.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "edit-body");
    app.on_key(&mut shell, "i", false, false, Some('i'));
    type_in(&mut app, &mut shell, "X");
    app.on_key(&mut shell, "escape", false, false, None);
    app.on_key(&mut shell, "escape", false, false, None);
    let msg = app.status().to_owned();
    assert!(
        !msg.contains("C-Enter"),
        "C-Enter does not save any more: {msg}"
    );
    assert!(msg.contains("C-c C-c"), "name the key that does: {msg}");
}

#[test]
fn every_mode_can_save_close_and_discard() {
    // `save-buffer` is a keymap chord like any other. The other two are
    // resolved by the editor rather than the keymap, deliberately —
    // `C-c C-c` in the outline is org's "do the thing at point" — so
    // what has to be true for them is that the buffer says which keys
    // they are, in every mode.
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        assert!(
            closure_input::chord_for_command(mode, "save-buffer").is_some(),
            "{mode:?} cannot reach save-buffer"
        );
        let (_d, mut shell, mut app) = fixture();
        app.run(&mut shell, "edit-body");
        let actions = app.buffer_actions();
        for cmd in ["save-buffer", "commit-edit", "discard-edit"] {
            let action = actions
                .iter()
                .find(|(_, c, _)| *c == cmd)
                .unwrap_or_else(|| panic!("{mode:?} has no {cmd} action"));
            assert!(action.2.is_some(), "{mode:?} does not name a key for {cmd}");
        }
    }
}
