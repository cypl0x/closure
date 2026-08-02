//! "Doom Emacs dired keybindings, like m to mark an element and after
//! that with something like D to delete the marked elements. Please
//! verify the doom emacs evil keymap for dired."
//!
//! evil-collection's dired binds `m` to mark, `u` to unmark, `U` to
//! unmark everything and `D` to delete what is marked. Two of those
//! are already spoken for in closure's outline — `u` is undo and `d`
//! is cut — and stealing undo to gain an unmark would be a bad trade.
//! So `m` toggles instead of only marking, which is one key doing the
//! job of two, and `U` and `D` keep dired's meaning exactly.
//!
//! dired's own rule about which rows an action applies to matters more
//! than the letters: the marks win when there are any, and the row
//! under the cursor is used when there are none. That is what makes
//! `D` safe to press without looking.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const NOTES: &str = "\
* One
:PROPERTIES:
:ID: 01HQMARK00000000000001
:END:
* Two
:PROPERTIES:
:ID: 01HQMARK00000000000002
:END:
* Three
:PROPERTIES:
:ID: 01HQMARK00000000000003
:END:
";

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

/// Press `m` on the row holding `id`.
fn mark(app: &mut ModalApp, shell: &mut Shell, id: &str) {
    assert!(app.select_by_id(shell, id));
    app.on_key(shell, "m", false, false, Some('m'));
}

fn titles(shell: &Shell) -> String {
    fs::read_to_string(shell.vault.root().join("notes.org")).expect("read")
}

#[test]
fn m_marks_the_row_under_the_cursor() {
    let (_d, mut shell, mut app) = fixture();
    mark(&mut app, &mut shell, "01HQMARK00000000000001");
    assert!(app.is_marked("01HQMARK00000000000001"));
    assert_eq!(app.marked_count(), 1);
}

#[test]
fn m_again_takes_the_mark_off() {
    // One key doing the job of two, because `u` is undo here.
    let (_d, mut shell, mut app) = fixture();
    mark(&mut app, &mut shell, "01HQMARK00000000000001");
    // Marking steps on, so taking the mark off means coming back to
    // the row first — which is dired's behaviour, not a toggle in
    // place.
    mark(&mut app, &mut shell, "01HQMARK00000000000001");
    assert!(!app.is_marked("01HQMARK00000000000001"));
    assert_eq!(app.marked_count(), 0);
}

#[test]
fn marking_moves_down_the_way_dired_does() {
    // You mark a run of rows by holding one key, not by alternating
    // with the arrow.
    let (_d, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQMARK00000000000001"));
    let before = app.selected();
    app.on_key(&mut shell, "m", false, false, Some('m'));
    assert_eq!(app.selected(), before + 1, "the cursor stepped on");
}

#[test]
fn capital_u_clears_every_mark() {
    let (_d, mut shell, mut app) = fixture();
    mark(&mut app, &mut shell, "01HQMARK00000000000001");
    mark(&mut app, &mut shell, "01HQMARK00000000000002");
    assert_eq!(app.marked_count(), 2);
    app.on_key(&mut shell, "U", false, false, Some('U'));
    assert_eq!(app.marked_count(), 0);
}

#[test]
fn capital_d_deletes_what_is_marked() {
    let (_d, mut shell, mut app) = fixture();
    mark(&mut app, &mut shell, "01HQMARK00000000000001");
    mark(&mut app, &mut shell, "01HQMARK00000000000003");
    app.on_key(&mut shell, "D", false, false, Some('D'));
    let text = titles(&shell);
    assert!(!text.contains("* One"), "{text}");
    assert!(!text.contains("* Three"), "{text}");
    assert!(text.contains("* Two"), "the unmarked one survived: {text}");
}

#[test]
fn the_marks_are_cleared_once_they_are_used() {
    // A mark that outlives the thing it pointed at is a mark that
    // deletes something else next time.
    let (_d, mut shell, mut app) = fixture();
    mark(&mut app, &mut shell, "01HQMARK00000000000001");
    app.on_key(&mut shell, "D", false, false, Some('D'));
    assert_eq!(app.marked_count(), 0);
}

#[test]
fn with_nothing_marked_it_acts_on_the_row_under_the_cursor() {
    // dired's own rule, and what makes `D` safe to press without
    // looking at what is marked.
    let (_d, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQMARK00000000000002"));
    app.on_key(&mut shell, "D", false, false, Some('D'));
    let text = titles(&shell);
    assert!(!text.contains("* Two"), "{text}");
    assert!(text.contains("* One") && text.contains("* Three"), "{text}");
}

#[test]
fn a_mark_survives_the_cursor_moving_away() {
    let (_d, mut shell, mut app) = fixture();
    mark(&mut app, &mut shell, "01HQMARK00000000000001");
    assert!(app.select_by_id(&shell, "01HQMARK00000000000003"));
    assert!(app.is_marked("01HQMARK00000000000001"));
}

#[test]
fn every_mode_can_mark_and_act() {
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        for cmd in ["toggle-mark", "unmark-all", "delete-marked"] {
            assert!(
                closure_input::chord_for_command(mode, cmd).is_some(),
                "{mode:?} cannot reach {cmd}"
            );
        }
    }
}
