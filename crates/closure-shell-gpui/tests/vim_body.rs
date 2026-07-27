//! The vim grammar as the *window* delivers it.
//!
//! gpui names letter keysyms in lower case and reports the printable
//! character separately, so a chord only reaches the editor through
//! [`editor_key`]. These drive [`ModalApp`] with exactly the strings
//! that seam produces — the difference between "the editor supports
//! `diw`" and "the reference shell supports `diw`".

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{EditorMode, ModalApp, ModalSurface, Shell};
use closure_shell_gpui::editor_key;
use closure_store::Vault;
use tempfile::TempDir;

fn shell() -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* Foo\n** Bar\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

/// Feed a chord the way a keystroke arrives from gpui.
fn feed(app: &mut ModalApp, sh: &mut Shell, keys: &str) {
    for c in keys.chars() {
        let shift = c.is_ascii_uppercase();
        let ch = c.to_string();
        let low = c.to_ascii_lowercase().to_string();
        let key = editor_key(&low, shift, Some(&ch));
        app.on_key(sh, &key, false, false, Some(c));
    }
}

/// The window parked in the body editor over `text`, NORMAL at byte 0.
fn editing(text: &str) -> (TempDir, Shell, ModalApp) {
    let (d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "i", false, false, Some('i'));
    assert_eq!(app.surface(), ModalSurface::EditBody);
    // A modal mode opens the buffer in NORMAL; `i` starts typing.
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
fn diw_deletes_the_inner_word() {
    let (_d, mut sh, mut app) = editing("hello world");
    feed(&mut app, &mut sh, "diw");
    assert_eq!(app.body_buffer(), " world");
}

#[test]
fn viw_selects_it_first() {
    let (_d, mut sh, mut app) = editing("hello world");
    feed(&mut app, &mut sh, "viw");
    assert_eq!(app.body_mode(), EditorMode::Visual);
    assert_eq!(app.body_selection(), Some((0, 5)));
    feed(&mut app, &mut sh, "d");
    assert_eq!(app.body_buffer(), " world");
}

#[test]
fn caw_and_the_uppercase_chords_survive_the_keysym_seam() {
    let (_d, mut sh, mut app) = editing("hello world");
    feed(&mut app, &mut sh, "caw");
    assert_eq!(app.body_mode(), EditorMode::Insert);
    assert_eq!(app.body_buffer(), "world");
    app.on_key(&mut sh, "escape", false, false, None);
    // `A` is append-at-line-end, not `a` — the shift must survive.
    feed(&mut app, &mut sh, "A!");
    assert_eq!(app.body_buffer(), "world!");
}

#[test]
fn the_new_chords_reach_the_window_too() {
    let (_d, mut sh, mut app) = editing("a(bc)d");
    feed(&mut app, &mut sh, "%");
    assert_eq!(app.body_cursor(), (0, 4), "`%` from col 0 finds the pair");
    feed(&mut app, &mut sh, "Y");
    feed(&mut app, &mut sh, "p");
    assert_eq!(app.body_buffer(), "a(bc)d\na(bc)d", "Y is yy");
}

#[test]
fn a_ctrl_chord_reaches_the_window_editor() {
    let (_d, mut sh, mut app) = editing("count 41");
    // gpui reports the modifier separately; the seam must forward it.
    app.on_key(&mut sh, "a", true, false, None);
    assert_eq!(app.body_buffer(), "count 42");
}

// === `e`, as the window delivers it ===

#[test]
fn e_moves_to_the_end_of_the_word() {
    let (_d, mut sh, mut app) = editing("hello world");
    feed(&mut app, &mut sh, "e");
    assert_eq!(app.body_cursor(), (0, 4), "on the last letter of `hello`");
}

#[test]
fn e_extends_a_visual_selection() {
    let (_d, mut sh, mut app) = editing("hello world");
    feed(&mut app, &mut sh, "ve");
    assert_eq!(app.body_mode(), EditorMode::Visual);
    assert_eq!(
        app.body_selection(),
        Some((0, 5)),
        "the selection grew to the word end"
    );
}

#[test]
fn de_deletes_to_the_end_of_the_word() {
    let (_d, mut sh, mut app) = editing("hello world");
    feed(&mut app, &mut sh, "de");
    assert_eq!(app.body_buffer(), " world");
}
