//! `e` — reported twice as "not working", in NORMAL and in VISUAL.
//!
//! The motion itself has been tested at the buffer level since it was
//! written, so whatever is wrong is above it: the surface the key
//! arrives on, the mode the shell is in, or the translation the window
//! does on the way. These drive it the way the keyboard does, on every
//! editing surface and in every mode that has a NORMAL.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{EditorMode, ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const SRC: &str = "\
* Note
:PROPERTIES:
:ID: 01HQMOTION0000000000000001
:END:
one two three
four five
";

fn shell() -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

/// The body editor open on the note, in NORMAL, cursor at byte 0.
fn editing(mode: InputMode) -> (TempDir, Shell, ModalApp) {
    let (d, mut sh) = shell();
    let mut app = ModalApp::new(mode);
    app.run(&mut sh, "edit-body");
    assert_eq!(app.surface(), ModalSurface::EditBody);
    assert_eq!(app.body_mode(), EditorMode::Normal, "{mode:?} starts modal");
    (d, sh, app)
}

/// Press a plain letter the way a window does: named key *and* the
/// character it would type.
fn press(app: &mut ModalApp, sh: &mut Shell, c: char) {
    app.on_key(sh, &c.to_string(), false, false, Some(c));
}

#[test]
fn e_moves_to_the_word_end_in_normal() {
    for mode in [InputMode::Vim, InputMode::Doom, InputMode::Helix] {
        let (_d, mut sh, mut app) = editing(mode);
        press(&mut app, &mut sh, 'e');
        assert_eq!(
            app.body_cursor(),
            (0, 2),
            "{mode:?}: e lands on the end of `one`"
        );
        press(&mut app, &mut sh, 'e');
        assert_eq!(app.body_cursor(), (0, 6), "{mode:?}: and then of `two`");
    }
}

#[test]
fn e_crosses_a_line_end() {
    // The case that would look exactly like "e stopped working": at
    // the last word of a line it has to carry on to the next one
    // rather than sit on the newline.
    let (_d, mut sh, mut app) = editing(InputMode::Vim);
    app.body_set_cursor(0);
    for _ in 0..3 {
        press(&mut app, &mut sh, 'e');
    }
    assert_eq!(app.body_cursor(), (0, 12), "end of `three`");
    press(&mut app, &mut sh, 'e');
    assert_eq!(
        app.body_cursor().0,
        1,
        "and on to the next line: {:?}",
        app.body_cursor()
    );
}

#[test]
fn e_extends_a_visual_selection() {
    for mode in [InputMode::Vim, InputMode::Doom] {
        let (_d, mut sh, mut app) = editing(mode);
        press(&mut app, &mut sh, 'v');
        assert_eq!(app.body_mode(), EditorMode::Visual, "{mode:?}: v selects");
        press(&mut app, &mut sh, 'e');
        assert_eq!(
            app.body_cursor(),
            (0, 2),
            "{mode:?}: e moves inside VISUAL too"
        );
    }
}

#[test]
fn d_e_deletes_to_the_word_end() {
    // The operator-pending half: `e` has to be a motion an operator
    // can take, not only a cursor move.
    let (_d, mut sh, mut app) = editing(InputMode::Vim);
    press(&mut app, &mut sh, 'd');
    press(&mut app, &mut sh, 'e');
    assert!(
        app.body_buffer().starts_with(" two"),
        "`de` took the word: {:?}",
        app.body_buffer()
    );
}

#[test]
fn e_works_in_the_whole_file_buffer_too() {
    // The other editing surface: the file view opens the org file
    // itself, and it is the same editor underneath.
    let (_d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Doom);
    app.set_view(closure_shell_core::ViewMode::Editor, &sh);
    assert_eq!(app.surface(), ModalSurface::EditFile);
    press(&mut app, &mut sh, 'e');
    assert_ne!(
        app.body_cursor(),
        (0, 0),
        "e moved something: {:?}",
        app.body_cursor()
    );
}

#[test]
fn e_is_not_eaten_by_the_outline_binding() {
    // Every keymap binds `e` to `block-list` in the outline. That is
    // the outline's `e`, and it must not follow the cursor into a
    // buffer — the surface decides, not the letter.
    let (_d, mut sh, mut app) = editing(InputMode::Doom);
    press(&mut app, &mut sh, 'e');
    assert_eq!(
        app.surface(),
        ModalSurface::EditBody,
        "still in the buffer, not in the block list"
    );
}

// === "in Visual mode for example e doesn't work" ===
//
// "For example" is the word that matters: the report is about VISUAL,
// with `e` as the case that was noticed. This is the vocabulary sweep
// — every motion evil binds in VISUAL, driven through the shell's key
// path, checked for actually moving the cursor.

/// The buffer in VISUAL with the cursor at the start.
fn visual() -> (TempDir, Shell, ModalApp) {
    let (d, mut sh, mut app) = editing(InputMode::Doom);
    press(&mut app, &mut sh, 'v');
    assert_eq!(app.body_mode(), EditorMode::Visual);
    (d, sh, app)
}

#[test]
fn every_visual_motion_moves_the_cursor() {
    for keys in ["l", "w", "e", "E", "W", "$", "G", "f o", "t o", "}"] {
        let (_d, mut sh, mut app) = visual();
        for c in keys.chars().filter(|c| !c.is_whitespace()) {
            press(&mut app, &mut sh, c);
        }
        assert_ne!(
            app.body_cursor(),
            (0, 0),
            "VISUAL {keys:?} left the cursor at the start"
        );
        assert_eq!(
            app.body_mode(),
            EditorMode::Visual,
            "VISUAL {keys:?} left the mode"
        );
        assert!(
            app.body_selection().is_some(),
            "VISUAL {keys:?} lost the selection"
        );
    }
}

#[test]
fn every_visual_text_object_selects_something() {
    for keys in ["iw", "aw", "ip"] {
        let (_d, mut sh, mut app) = visual();
        for c in keys.chars() {
            press(&mut app, &mut sh, c);
        }
        let (lo, hi) = app
            .body_selection()
            .unwrap_or_else(|| panic!("VISUAL {keys:?} has no selection"));
        assert!(hi > lo, "VISUAL {keys:?} selected nothing: {lo}..{hi}");
    }
}

#[test]
fn the_visual_operators_act_on_the_selection() {
    let (_d, mut sh, mut app) = visual();
    press(&mut app, &mut sh, 'e');
    press(&mut app, &mut sh, 'd');
    assert!(
        app.body_buffer().starts_with(" two"),
        "`v e d` took the first word: {:?}",
        app.body_buffer()
    );
    assert_eq!(app.body_mode(), EditorMode::Normal, "and VISUAL ended");
}

#[test]
fn o_swaps_the_ends_of_the_selection() {
    let (_d, mut sh, mut app) = visual();
    press(&mut app, &mut sh, 'e');
    assert_eq!(app.body_cursor(), (0, 2));
    press(&mut app, &mut sh, 'o');
    assert_eq!(app.body_cursor(), (0, 0), "the cursor went to the anchor");
    assert!(app.body_selection().is_some(), "and it is still a selection");
}
