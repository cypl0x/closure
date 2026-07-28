//! Opening a second note while the first one is still being edited.
//!
//! The buffer was a single slot: `edit-body` on another row overwrote
//! it, and the paragraph in the old one was gone — no prompt, no undo,
//! nothing on disk. Clicking a row in the outline beside the buffer is
//! the most ordinary thing there is to do while editing, so it must not
//! be the gesture that loses text.
//!
//! The rule: leaving a modified buffer *stashes* it against its
//! headline. Coming back restores it, still modified, still unsaved.
//! The vault is untouched until something says to write — `:w`,
//! `C-Enter`, or the window closing. `:q!` is how you throw one away.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const SRC: &str = "\
* First
:PROPERTIES:
:ID: 01HQSTASH00000000000000001
:END:
First body.
* Second
:PROPERTIES:
:ID: 01HQSTASH00000000000000002
:END:
Second body.
";

fn shell() -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

/// Open the body editor on the headline with `id`.
fn open(app: &mut ModalApp, sh: &mut Shell, id: &str) {
    app.select_by_id(sh, id);
    app.run(sh, "edit-body");
    assert_eq!(app.surface(), ModalSurface::EditBody);
}

/// Append `text` at the end of the buffer, leaving NORMAL behind.
fn append(app: &mut ModalApp, sh: &mut Shell, text: &str) {
    app.on_key(sh, "A", false, false, Some('A'));
    for c in text.chars() {
        app.on_key(sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(sh, "escape", false, false, None);
}

const FIRST: &str = "01HQSTASH00000000000000001";
const SECOND: &str = "01HQSTASH00000000000000002";

#[test]
fn opening_another_note_keeps_the_edit_in_the_first() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    open(&mut app, &mut sh, FIRST);
    append(&mut app, &mut sh, " and more");
    open(&mut app, &mut sh, SECOND);
    assert!(
        app.body_buffer().contains("Second body"),
        "the second note opened: {:?}",
        app.body_buffer()
    );
    open(&mut app, &mut sh, FIRST);
    assert!(
        app.body_buffer().contains("and more"),
        "and the first came back as it was left: {:?}",
        app.body_buffer()
    );
    assert!(app.body_dirty(), "still unsaved");
}

#[test]
fn leaving_a_buffer_writes_nothing() {
    let (dir, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    open(&mut app, &mut sh, FIRST);
    append(&mut app, &mut sh, " and more");
    open(&mut app, &mut sh, SECOND);
    let disk = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(
        !disk.contains("and more"),
        "the edit is held, not filed: {disk}"
    );
}

#[test]
fn saving_a_restored_buffer_writes_it() {
    let (dir, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    open(&mut app, &mut sh, FIRST);
    append(&mut app, &mut sh, " and more");
    open(&mut app, &mut sh, SECOND);
    open(&mut app, &mut sh, FIRST);
    app.run_ex_line(&mut sh, "w");
    let disk = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(disk.contains("and more"), "{disk}");
    assert!(!app.body_dirty(), "and the buffer is clean again");
}

#[test]
fn a_saved_buffer_leaves_nothing_stashed() {
    // Otherwise the next visit restores an edit that is already in the
    // vault, and the note reads as permanently unsaved.
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    open(&mut app, &mut sh, FIRST);
    append(&mut app, &mut sh, " and more");
    app.run_ex_line(&mut sh, "w");
    open(&mut app, &mut sh, SECOND);
    open(&mut app, &mut sh, FIRST);
    assert!(!app.body_dirty(), "nothing outstanding: {}", app.status());
}

#[test]
fn quit_bang_throws_the_stash_away_too() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    open(&mut app, &mut sh, FIRST);
    append(&mut app, &mut sh, " and more");
    app.run_ex_line(&mut sh, "q!");
    open(&mut app, &mut sh, FIRST);
    assert!(
        !app.body_buffer().contains("and more"),
        "discarded on purpose stays discarded: {:?}",
        app.body_buffer()
    );
    assert!(!app.body_dirty());
}

#[test]
fn a_closing_window_saves_every_edit_it_is_holding() {
    // The gesture that closed the window is recoverable; the
    // paragraphs in the buffers are not. All of them, not just the
    // one that happened to be on screen.
    let (dir, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    open(&mut app, &mut sh, FIRST);
    append(&mut app, &mut sh, " one");
    open(&mut app, &mut sh, SECOND);
    append(&mut app, &mut sh, " two");
    assert!(
        app.save_pending_edit(&mut sh),
        "there was something to save"
    );
    let disk = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(disk.contains("First body. one"), "{disk}");
    assert!(disk.contains("Second body. two"), "{disk}");
}

#[test]
fn a_note_with_nothing_stashed_opens_from_the_vault() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    open(&mut app, &mut sh, FIRST);
    open(&mut app, &mut sh, SECOND);
    open(&mut app, &mut sh, FIRST);
    assert!(app.body_buffer().contains("First body"));
    assert!(!app.body_dirty(), "an untouched buffer is clean");
}

#[test]
fn the_shell_can_say_how_much_it_is_holding() {
    // A window with unsaved work in notes you are not looking at has
    // to be able to say so.
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    assert_eq!(app.unsaved_bodies(), 0);
    open(&mut app, &mut sh, FIRST);
    append(&mut app, &mut sh, " and more");
    assert_eq!(app.unsaved_bodies(), 1, "the one on screen counts");
    open(&mut app, &mut sh, SECOND);
    assert_eq!(app.unsaved_bodies(), 1, "and keeps counting once stashed");
    append(&mut app, &mut sh, " too");
    assert_eq!(app.unsaved_bodies(), 2);
}
