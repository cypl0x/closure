//! The shell's key seam into the body editor.
//!
//! `ModalApp` used to drop every Normal-mode `Ctrl` chord but `C-r`:
//! the modifier was consumed by the surface dispatch and never reached
//! [`closure_shell_core::BodyEditor::modal_key`], so `C-d`, `C-f` and
//! `C-a` were dead in the GUI while working in the editor's own tests.
//! Same seam, same test, for the org-edit-special surface.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{EditorMode, ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

fn shell() -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* Note\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

/// An app with `text` in the body editor, in NORMAL at byte 0.
fn editing(text: &str) -> (TempDir, Shell, ModalApp) {
    let (d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "i", false, false, Some('i'));
    assert_eq!(app.surface(), ModalSurface::EditBody);
    // The buffer opens in NORMAL in a modal mode; `i` starts typing.
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

fn feed(app: &mut ModalApp, sh: &mut Shell, keys: &str) {
    for c in keys.chars() {
        app.on_key(sh, &c.to_string(), false, false, Some(c));
    }
}

#[test]
fn ctrl_f_pages_the_body_editor() {
    use std::fmt::Write as _;
    let text = (0..40).fold(String::new(), |mut acc, i| {
        let _ = writeln!(acc, "line {i}");
        acc
    });
    let (_d, mut sh, mut app) = editing(&text);
    app.on_key(&mut sh, "f", true, false, None);
    assert_eq!(app.body_cursor().0, 20, "C-f reached the editor");
}

#[test]
fn ctrl_a_increments_in_the_body_editor() {
    let (_d, mut sh, mut app) = editing("count 41");
    app.on_key(&mut sh, "a", true, false, None);
    assert_eq!(app.body_buffer(), "count 42");
}

#[test]
fn ctrl_r_still_redoes() {
    let (_d, mut sh, mut app) = editing("hello");
    feed(&mut app, &mut sh, "x");
    assert_eq!(app.body_buffer(), "ello");
    feed(&mut app, &mut sh, "u");
    assert_eq!(app.body_buffer(), "hello");
    app.on_key(&mut sh, "r", true, false, None);
    assert_eq!(app.body_buffer(), "ello", "C-r is redo, not a chord");
}

#[test]
fn an_open_search_line_keeps_escape_to_itself() {
    // Esc on a quiet NORMAL surface cancels the whole edit. With the
    // search line open it must only close the search.
    let (_d, mut sh, mut app) = editing("alpha beta");
    feed(&mut app, &mut sh, "/bet");
    assert_eq!(app.body_search_prompt().as_deref(), Some("/bet"));
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::EditBody, "still editing");
    assert_eq!(app.body_search_prompt(), None);
    // And now a quiet Esc does cancel.
    app.on_key(&mut sh, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
}

#[test]
fn the_search_line_takes_a_typed_pattern_and_enter_runs_it() {
    let (_d, mut sh, mut app) = editing("alpha beta");
    feed(&mut app, &mut sh, "/beta");
    app.on_key(&mut sh, "enter", false, false, None);
    assert_eq!(app.body_cursor(), (0, 6));
}

#[test]
fn replace_mode_is_visible_to_the_shell() {
    let (_d, mut sh, mut app) = editing("abcdef");
    feed(&mut app, &mut sh, "R");
    assert_eq!(app.body_mode(), EditorMode::Insert);
    assert!(app.body_replacing(), "the mode chip must say REPLACE");
    feed(&mut app, &mut sh, "XY");
    assert_eq!(app.body_buffer(), "XYcdef");
}

#[test]
fn a_recording_macro_is_visible_to_the_shell() {
    let (_d, mut sh, mut app) = editing("abc");
    feed(&mut app, &mut sh, "qa");
    assert_eq!(app.body_recording(), Some('a'));
    feed(&mut app, &mut sh, "q");
    assert_eq!(app.body_recording(), None);
}

#[test]
fn a_colon_inside_a_search_pattern_is_text_not_the_ex_line() {
    let (_d, mut sh, mut app) = editing("see id:01 here");
    feed(&mut app, &mut sh, "/id:0");
    assert_eq!(app.surface(), ModalSurface::EditBody, "not the ex line");
    assert_eq!(app.body_search_prompt().as_deref(), Some("/id:0"));
}
