//! Where the cursor is in a one-line prompt, and who can ask.
//!
//! The capture and rename prompts move their cursor correctly — they
//! are [`LineInput`]s, and the arrows and readline chords have always
//! reached them — but nothing outside the core could ask *where* the
//! cursor ended up. So every shell painted the caret after the last
//! character, and pressing Left, `C-a` or Alt+Backspace looked like it
//! had done nothing at all.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;

const SRC: &str = "* Alpha\n\
                   :PROPERTIES:\n\
                   :ID: 01HQCAR0000000000000000001\n\
                   :END:\n";

fn fixture() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

fn type_into(app: &mut ModalApp, sh: &mut Shell, text: &str) {
    for c in text.chars() {
        app.on_key(sh, &c.to_string(), false, false, Some(c));
    }
}

// === the rename / add / tags / property field ===

#[test]
fn the_rename_prompt_says_where_its_cursor_is() {
    let (_d, mut sh, mut app) = fixture();
    app.select(0, &sh);
    app.run(&mut sh, "rename");
    assert_eq!(app.surface(), ModalSurface::Rename);
    app.on_key(&mut sh, "u", true, false, None); // C-u, clear the line
    type_into(&mut app, &mut sh, "abc");
    assert_eq!(app.field_cursor(), 3, "typing leaves it at the end");
    app.on_key(&mut sh, "left", false, false, None);
    app.on_key(&mut sh, "left", false, false, None);
    assert_eq!(app.field_cursor(), 1, "and Left walks it back");
    app.on_key(&mut sh, "a", true, false, None); // C-a
    assert_eq!(app.field_cursor(), 0, "readline reaches it too");
}

#[test]
fn typing_lands_at_the_cursor_not_at_the_end() {
    let (_d, mut sh, mut app) = fixture();
    app.select(0, &sh);
    app.run(&mut sh, "rename");
    app.on_key(&mut sh, "u", true, false, None);
    type_into(&mut app, &mut sh, "abc");
    app.on_key(&mut sh, "left", false, false, None);
    type_into(&mut app, &mut sh, "X");
    assert_eq!(app.field_buffer(), "abXc");
    assert_eq!(app.field_cursor(), 3);
}

#[test]
fn alt_backspace_kills_a_word_in_the_rename_prompt() {
    let (_d, mut sh, mut app) = fixture();
    app.select(0, &sh);
    app.run(&mut sh, "rename");
    app.on_key(&mut sh, "u", true, false, None);
    type_into(&mut app, &mut sh, "foo bar");
    app.on_key(&mut sh, "backspace", false, true, None);
    assert_eq!(app.field_buffer(), "foo ");
    assert_eq!(app.field_cursor(), 4, "and the cursor came with it");
}

// === the capture prompt ===

#[test]
fn the_capture_prompt_says_where_its_cursor_is() {
    let (_d, mut sh, mut app) = fixture();
    app.run(&mut sh, "capture-start");
    assert_eq!(app.surface(), ModalSurface::Capture);
    type_into(&mut app, &mut sh, "hello");
    assert_eq!(app.capture_cursor(), 5);
    app.on_key(&mut sh, "left", false, false, None);
    assert_eq!(app.capture_cursor(), 4);
    app.on_key(&mut sh, "e", true, false, None); // C-e
    assert_eq!(app.capture_cursor(), 5);
}

#[test]
fn the_cursor_is_a_byte_offset_on_a_character_boundary() {
    // The caret splits the rendered string at this offset, so it has to
    // be somewhere `str` can be cut — a two-byte `é` is one Left, not
    // two.
    let (_d, mut sh, mut app) = fixture();
    app.run(&mut sh, "capture-start");
    type_into(&mut app, &mut sh, "café");
    assert_eq!(app.capture_cursor(), 5, "four chars, five bytes");
    app.on_key(&mut sh, "left", false, false, None);
    assert_eq!(app.capture_cursor(), 3, "one glyph back, two bytes");
    assert!(app.capture_buffer().is_char_boundary(app.capture_cursor()));
}
