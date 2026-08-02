//! Which-key opens from the keyboard, not only from the mouse.
//!
//! The panel had exactly one way in: a `▸ keys` button in the bottom
//! right corner of the gpui window. The state behind it lived in the
//! window too, so there was no command to bind, nothing for the palette
//! to list, and — for a panel whose entire job is telling you what the
//! keys are — no key.
//!
//! `?` is free in all five keymaps and is what a TUI has meant by "show
//! me the bindings" for decades. Emacs gets `C-h` as well, which is
//! where its hands already go for help.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

fn fixture(mode: InputMode) -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* Alpha\n").expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(mode))
}

#[test]
fn the_panel_starts_closed() {
    let (_d, _sh, app) = fixture(InputMode::Doom);
    assert!(!app.which_key_open());
}

#[test]
fn the_command_toggles_it() {
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    app.run(&mut sh, "toggle-which-key");
    assert!(app.which_key_open(), "open");
    app.run(&mut sh, "toggle-which-key");
    assert!(!app.which_key_open(), "and closed again");
}

#[test]
fn question_mark_is_bound_in_every_mode() {
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        assert_eq!(
            closure_input::command_for(mode, "?"),
            Some("toggle-which-key"),
            "{mode:?}"
        );
    }
}

#[test]
fn emacs_also_answers_to_its_own_help_key() {
    assert_eq!(
        closure_input::command_for(InputMode::Emacs, "C-h"),
        Some("toggle-which-key")
    );
}

#[test]
fn pressing_it_opens_the_panel_in_every_mode() {
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        let (_d, mut sh, mut app) = fixture(mode);
        app.on_key(&mut sh, "?", false, false, Some('?'));
        assert!(app.which_key_open(), "{mode:?}");
    }
}

#[test]
fn the_panel_has_something_to_show_when_it_opens() {
    // A toggle that reveals an empty box is a toggle that looks broken.
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    app.run(&mut sh, "toggle-which-key");
    let groups = app.which_key_groups();
    assert!(!groups.is_empty(), "the keymap, grouped");
    assert!(groups.iter().any(|(_, rows)| !rows.is_empty()));
}

// === and from inside a buffer ===
//
// Reported 2026-08-02 with a screenshot: "the ? key will just print the
// character at this header position. Why does it do that?"
//
// Because a buffer resolves bare keys as text and only consults the
// keymap for modified chords — which is right for `?` in INSERT, where
// it is a question mark, and wrong in NORMAL, where it is not text at
// all. `C-h` covers the modes that have no NORMAL to press it in.

#[test]
fn question_mark_opens_it_from_a_buffer_in_normal() {
    for mode in [InputMode::Doom, InputMode::Vim, InputMode::Helix] {
        let (_d, mut sh, mut app) = fixture(mode);
        app.select(0, &sh);
        app.run(&mut sh, "edit-body"); // modal modes open in NORMAL
        app.on_key(&mut sh, "?", false, false, Some('?'));
        assert!(app.which_key_open(), "{mode:?}");
        assert!(
            !app.body_buffer().contains('?'),
            "{mode:?} typed it instead: {:?}",
            app.body_buffer()
        );
    }
}

#[test]
fn question_mark_is_still_a_question_mark_in_insert() {
    // Prose has questions in it. This is the whole reason the buffer
    // does not consult the keymap for bare keys.
    let (_d, mut sh, mut app) = fixture(InputMode::Doom);
    app.select(0, &sh);
    app.run(&mut sh, "edit-body");
    app.on_key(&mut sh, "i", false, false, Some('i'));
    app.on_key(&mut sh, "?", false, false, Some('?'));
    assert!(
        app.body_buffer().contains('?'),
        "typed: {:?}",
        app.body_buffer()
    );
    assert!(!app.which_key_open(), "and the panel stayed shut");
}

#[test]
fn the_friendly_modes_keep_their_question_mark_everywhere() {
    // Notion and Emacs have no NORMAL, so their buffer is always a text
    // field and `?` is always a character in it.
    for mode in [InputMode::Notion, InputMode::Emacs] {
        let (_d, mut sh, mut app) = fixture(mode);
        app.select(0, &sh);
        app.run(&mut sh, "edit-body");
        app.on_key(&mut sh, "?", false, false, Some('?'));
        assert!(app.body_buffer().contains('?'), "{mode:?}");
    }
}

#[test]
fn ctrl_h_opens_it_from_any_buffer_in_any_mode() {
    // The way in for a buffer with no NORMAL — and the way in from
    // INSERT for the ones that have one.
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        let (_d, mut sh, mut app) = fixture(mode);
        app.select(0, &sh);
        app.run(&mut sh, "edit-body");
        app.on_key(&mut sh, "i", false, false, Some('i')); // INSERT where there is one
        app.on_key(&mut sh, "h", true, false, None);
        assert!(app.which_key_open(), "{mode:?}");
        assert!(!app.body_buffer().contains('h'), "{mode:?} typed an h");
    }
}

#[test]
fn ctrl_h_is_bound_in_every_mode() {
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        assert_eq!(
            closure_input::command_for(mode, "C-h"),
            Some("toggle-which-key"),
            "{mode:?}"
        );
    }
}
