//! Switching how you type does not change what you are looking at.
//!
//! Reported 2026-08-02: "switching the mode in the top left corner will
//! show the full view body editor. Disable this behavior, because the
//! full view editor should be toggled separately by its own command."
//!
//! `cycle-mode` used to set the view from the new mode's default and
//! open or close the file buffer to match, on the theory that a mode
//! with a NORMAL wants the file and one without wants the rows. In
//! practice it means clicking the mode chip to try Vim throws away the
//! pane you were reading, which is a large surprise for a small chord.
//!
//! What it still does — and must — is fix the *buffer's* mode: Notion
//! and Emacs have no NORMAL, so a buffer left in one after the switch
//! is a text field that will not take text.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{EditorMode, ModalApp, ModalSurface, Shell, ViewMode};
use closure_store::Vault;

fn fixture(mode: InputMode) -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Alpha\n:PROPERTIES:\n:ID: 01HQMSV0000000000000001\n:END:\nbody\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(mode))
}

#[test]
fn cycling_the_mode_leaves_the_outline_alone() {
    let (_d, mut sh, mut app) = fixture(InputMode::Notion);
    assert_eq!(app.surface(), ModalSurface::Browse);
    app.run(&mut sh, "cycle-mode");
    assert_eq!(app.surface(), ModalSurface::Browse, "still the outline");
    assert_eq!(app.view_mode(), ViewMode::Clickable);
}

#[test]
fn cycling_all_the_way_round_never_opens_the_editor() {
    let (_d, mut sh, mut app) = fixture(InputMode::Notion);
    for _ in 0..5 {
        app.run(&mut sh, "cycle-mode");
        assert_eq!(
            app.surface(),
            ModalSurface::Browse,
            "{:?} opened a buffer",
            app.input_mode()
        );
    }
    assert_eq!(app.input_mode(), InputMode::Notion, "back where it started");
}

#[test]
fn it_does_change_the_input_mode() {
    let (_d, mut sh, mut app) = fixture(InputMode::Vim);
    app.run(&mut sh, "cycle-mode");
    assert_eq!(app.input_mode(), InputMode::Doom);
}

#[test]
fn the_editor_view_survives_a_mode_switch_too() {
    // The rule cuts both ways: someone in the full-window editor who
    // switches to Notion should still be in it.
    let (_d, mut sh, mut app) = fixture(InputMode::Vim);
    app.run(&mut sh, "toggle-view");
    assert!(app.surface().is_editor(), "in the file buffer");
    app.run(&mut sh, "cycle-mode");
    assert!(app.surface().is_editor(), "still in it");
}

#[test]
fn a_buffer_is_still_left_in_a_mode_that_can_type() {
    // Notion and Emacs have no NORMAL, so a buffer left in one would be
    // a text field that will not take text.
    let (_d, mut sh, mut app) = fixture(InputMode::Vim);
    app.select(0, &sh);
    app.run(&mut sh, "edit-body");
    assert_eq!(app.body_mode(), EditorMode::Normal);
    app.run(&mut sh, "cycle-mode"); // Vim -> Doom, still modal
    assert_eq!(app.body_mode(), EditorMode::Normal);
    for _ in 0..2 {
        app.run(&mut sh, "cycle-mode"); // Doom -> Helix -> Notion
    }
    assert_eq!(app.input_mode(), InputMode::Notion);
    assert_eq!(app.body_mode(), EditorMode::Insert, "typing works");
}

#[test]
fn toggle_view_is_still_how_the_editor_opens() {
    let (_d, mut sh, mut app) = fixture(InputMode::Notion);
    app.run(&mut sh, "toggle-view");
    assert!(app.surface().is_editor(), "its own command still does it");
}
