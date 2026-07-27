//! Leaving the body editor without losing what is in it.
//!
//! Two ways out of a buffer both threw it away. `Esc` on a quiet
//! Normal surface cleared the buffer and returned to the outline —
//! the paragraph you had just typed was gone, with no prompt and no
//! undo. And `:w`, which in every vi ever written means "write and
//! carry on", wrote the buffer *and closed the editor*, because the
//! ex line returned to Browse before running the command.
//!
//! The rule here: nothing silently discards a modified buffer. `Esc`
//! on an unchanged one still leaves (peeking at a body must stay
//! cheap); on a modified one it refuses and says what saves and what
//! discards. `:w` writes and stays. `:wq` writes and leaves. `:q!`
//! is how you say you meant it.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{EditorMode, ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const SRC: &str = "* Note\n:PROPERTIES:\n:ID: 01HQEXIT00000000000000001\n:END:\nOriginal body.\n";

fn shell() -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

/// The editor open on the note, in NORMAL.
fn editing() -> (TempDir, Shell, ModalApp) {
    let (d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "i", false, false, Some('i'));
    assert_eq!(app.surface(), ModalSurface::EditBody);
    (d, sh, app)
}

/// Type `text` at the end of the buffer and return to NORMAL.
fn append(app: &mut ModalApp, sh: &mut Shell, text: &str) {
    app.on_key(sh, "A", false, false, Some('A'));
    for c in text.chars() {
        app.on_key(sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(sh, "escape", false, false, None);
    assert_eq!(app.body_mode(), EditorMode::Normal);
}

#[test]
fn escape_still_leaves_an_untouched_buffer() {
    // Opening a body to read it and pressing Esc must stay free.
    let (_d, mut sh, mut app) = editing();
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
}

#[test]
fn escape_refuses_to_throw_away_a_modified_buffer() {
    let (_d, mut sh, mut app) = editing();
    append(&mut app, &mut sh, " and more");
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(
        app.surface(),
        ModalSurface::EditBody,
        "the buffer is still open"
    );
    assert!(
        app.body_buffer().contains("and more"),
        "and still holds the text: {:?}",
        app.body_buffer()
    );
    let status = app.status();
    assert!(
        status.contains("C-Enter") || status.contains(":w"),
        "and says how to save: {status}"
    );
}

#[test]
fn escape_in_insert_only_leaves_insert() {
    let (_d, mut sh, mut app) = editing();
    app.on_key(&mut sh, "i", false, false, Some('i'));
    assert_eq!(app.body_mode(), EditorMode::Insert);
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.body_mode(), EditorMode::Normal);
    assert_eq!(app.surface(), ModalSurface::EditBody, "still editing");
}

#[test]
fn write_saves_and_stays_in_the_buffer() {
    // `:w` is the one command in vi that means "carry on".
    let (_d, mut sh, mut app) = editing();
    append(&mut app, &mut sh, " and more");
    app.run_ex_line(&mut sh, "w");
    assert_eq!(
        app.surface(),
        ModalSurface::EditBody,
        "still editing after a write"
    );
    assert!(
        !app.body_dirty(),
        "and the buffer counts as saved: {}",
        app.status()
    );
    assert!(
        app.body_buffer().contains("and more"),
        "with the text intact"
    );
}

#[test]
fn write_quit_saves_and_leaves() {
    let (_d, mut sh, mut app) = editing();
    append(&mut app, &mut sh, " and more");
    app.run_ex_line(&mut sh, "wq");
    assert_ne!(
        app.surface(),
        ModalSurface::EditBody,
        "`wq` is the one that leaves"
    );
}

#[test]
fn a_written_body_reaches_the_file() {
    let (dir, mut sh, mut app) = editing();
    append(&mut app, &mut sh, " and more");
    app.run_ex_line(&mut sh, "w");
    let on_disk = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(
        on_disk.contains("and more"),
        "the write actually wrote: {on_disk}"
    );
}

#[test]
fn quit_bang_discards_on_purpose() {
    // The escape hatch has to exist, and has to be explicit.
    let (_d, mut sh, mut app) = editing();
    append(&mut app, &mut sh, " and more");
    app.run_ex_line(&mut sh, "q!");
    assert_ne!(app.surface(), ModalSurface::EditBody);
}

#[test]
fn escape_twice_does_not_sneak_the_edit_out() {
    // A second Esc is the reflex when the first one "did nothing";
    // it must not be the thing that loses the paragraph.
    let (_d, mut sh, mut app) = editing();
    append(&mut app, &mut sh, " and more");
    app.on_key(&mut sh, "escape", false, false, None);
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::EditBody);
    assert!(app.body_buffer().contains("and more"));
}

// === overlays opened from the full-window editor ===

fn file_view() -> (TempDir, Shell, ModalApp) {
    let (d, sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.set_view(closure_shell_core::ViewMode::Editor, &sh);
    assert_eq!(app.surface(), ModalSurface::EditFile, "the file is open");
    (d, sh, app)
}

#[test]
fn closing_the_palette_returns_to_the_buffer_it_was_opened_from() {
    // Reported as "opening the palette or capture jumps back to the
    // clickable GUI": every overlay returned to the outline, which in
    // the editor view is a different shape of the whole app.
    let (_d, mut sh, mut app) = file_view();
    app.run(&mut sh, "palette");
    assert_eq!(app.surface(), ModalSurface::Palette);
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::EditFile, "back to the file");
}

#[test]
fn cancelling_a_capture_returns_to_the_buffer() {
    let (_d, mut sh, mut app) = file_view();
    app.run(&mut sh, "capture-start");
    assert_eq!(app.surface(), ModalSurface::Capture);
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::EditFile);
}

#[test]
fn completing_a_capture_returns_to_the_buffer() {
    let (_d, mut sh, mut app) = file_view();
    app.run(&mut sh, "capture-start");
    for c in "From the buffer".chars() {
        app.on_key(&mut sh, "x", false, false, Some(c));
    }
    app.on_key(&mut sh, "enter", false, false, None);
    assert_eq!(app.surface(), ModalSurface::EditFile);
}

#[test]
fn the_clickable_view_still_returns_to_the_outline() {
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.run(&mut sh, "capture-start");
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
}
