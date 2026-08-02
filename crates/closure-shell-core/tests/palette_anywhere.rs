//! M-x from inside a buffer, and back to the buffer afterwards.
//!
//! The palette was a Browse-surface thing: reachable from the outline,
//! unreachable from the one place a writer actually sits. Every key in
//! a buffer belongs to the buffer, which is right for letters and
//! wrong for the desktop's own chords — `M-x` types nothing.
//!
//! Opening it also has to be *reversible*: Escape puts you back in the
//! buffer you were editing, with the text and the cursor where you left
//! them, and a command run from the palette hands the buffer back too.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;

const SRC: &str = "* Alpha\n\
                   :PROPERTIES:\n\
                   :ID: 01HQPAL0000000000000000001\n\
                   :END:\n\
                   body text\n";

fn fixture(mode: InputMode) -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(mode))
}

fn in_the_buffer(app: &mut ModalApp, shell: &mut Shell) {
    app.select(0, shell);
    app.run(shell, "edit-body");
    assert!(app.surface().is_editor(), "the buffer is open");
}

#[test]
fn m_x_opens_the_palette_from_inside_a_buffer() {
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    in_the_buffer(&mut app, &mut shell);
    app.on_key(&mut shell, "x", false, true, None);
    assert_eq!(app.surface(), ModalSurface::Palette);
}

#[test]
fn every_mode_can_reach_it_from_a_buffer() {
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        let (_d, mut shell, mut app) = fixture(mode);
        in_the_buffer(&mut app, &mut shell);
        app.on_key(&mut shell, "x", false, true, None);
        assert_eq!(app.surface(), ModalSurface::Palette, "{mode:?}");
    }
}

#[test]
fn a_chord_the_buffer_already_answers_stays_the_buffer_s() {
    // `C-p` is bound to the palette as the desktop prefix, but in a
    // buffer it is readline's — it walks the completion. The window
    // does not get to take a chord the editor is already using.
    let (_d, mut shell, mut app) = fixture(InputMode::Vim);
    in_the_buffer(&mut app, &mut shell);
    app.on_key(&mut shell, "i", false, false, Some('i'));
    app.on_key(&mut shell, "p", true, false, None);
    assert_eq!(app.surface(), ModalSurface::EditBody, "still editing");
}

#[test]
fn escape_hands_the_buffer_back_untouched() {
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    in_the_buffer(&mut app, &mut shell);
    let before = app.body_buffer().to_owned();
    let cursor = app.body_cursor();
    app.on_key(&mut shell, "x", false, true, None);
    for c in "zoo".chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut shell, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::EditBody, "back in the buffer");
    assert_eq!(app.body_buffer(), before, "with its text");
    assert_eq!(app.body_cursor(), cursor, "and its cursor");
}

#[test]
fn a_command_run_from_the_buffer_palette_returns_to_the_buffer() {
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    in_the_buffer(&mut app, &mut shell);
    app.on_key(&mut shell, "x", false, true, None);
    for c in "zoom-in".chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut shell, "enter", false, false, None);
    assert!(app.zoom() > 1.0, "the command ran");
    assert_eq!(app.surface(), ModalSurface::EditBody, "and gave it back");
}

#[test]
fn the_outline_s_palette_still_goes_home() {
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    app.run(&mut shell, "palette");
    assert_eq!(app.surface(), ModalSurface::Palette);
    app.on_key(&mut shell, "escape", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
}

#[test]
fn typing_in_the_buffer_is_still_typing() {
    // The chord is `M-x`; a bare `x` is a letter, and in INSERT it has
    // to reach the text like any other.
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    in_the_buffer(&mut app, &mut shell);
    app.on_key(&mut shell, "i", false, false, Some('i'));
    app.on_key(&mut shell, "x", false, false, Some('x'));
    assert_eq!(app.surface(), ModalSurface::EditBody, "still editing");
    assert!(app.body_buffer().contains('x'), "{}", app.body_buffer());
}

#[test]
fn the_shell_knows_what_to_paint_behind_the_palette() {
    // The palette floats over what you were doing (Raycast, Zed, the
    // VS Code command bar) rather than replacing a pane, so a shell has
    // to be able to ask what is underneath it.
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    assert_eq!(app.surface_beneath(), ModalSurface::Browse);
    in_the_buffer(&mut app, &mut shell);
    app.on_key(&mut shell, "x", false, true, None);
    assert_eq!(app.surface(), ModalSurface::Palette);
    assert_eq!(
        app.surface_beneath(),
        ModalSurface::EditBody,
        "the buffer is still what is behind it"
    );
}

// === The palette remembers what you ran ===

#[test]
fn a_command_run_from_the_palette_is_suggested_next_time() {
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    app.run(&mut shell, "palette");
    for c in "zoom-in".chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut shell, "enter", false, false, None);
    app.run(&mut shell, "palette");
    let first = app.palette_entries().first().map(|e| e.label.clone());
    assert_eq!(
        first.as_deref(),
        Some("zoom-in"),
        "the last thing you ran is the first thing offered"
    );
}

#[test]
fn a_chord_is_not_palette_history() {
    // `j` and `k` are pressed hundreds of times a session and are never
    // what you open the palette to find. Only what the palette itself
    // ran counts as its history.
    // `zoom-in` because it is nowhere near the top of the unsuggested
    // palette — finding it first could only mean it was suggested.
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    app.run(&mut shell, "zoom-in");
    app.run(&mut shell, "palette");
    let first = app.palette_entries().first().map(|e| e.label.clone());
    assert_ne!(first.as_deref(), Some("zoom-in"));
}

#[test]
fn running_the_same_command_twice_leaves_one_entry() {
    let (_d, mut shell, mut app) = fixture(InputMode::Doom);
    for _ in 0..2 {
        app.run(&mut shell, "palette");
        for c in "zoom-in".chars() {
            app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
        }
        app.on_key(&mut shell, "enter", false, false, None);
    }
    app.run(&mut shell, "palette");
    let zooms = app
        .palette_entries()
        .iter()
        .filter(|e| e.label == "zoom-in")
        .count();
    // Was 2 — "once in Recent, once in the section it lives in" —
    // until the user filed the second listing as a duplicate on
    // 2026-08-02. Promotion moves a command rather than copying it.
    assert_eq!(zooms, 1, "listed once, at the top");
}
