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
