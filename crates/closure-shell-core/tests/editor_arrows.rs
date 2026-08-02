//! The arrow keys move the caret inside a note.
//!
//! Not filed, found while auditing the input surfaces on 2026-08-02: the
//! body editor's INSERT table bound `left`/`right` only *with* ctrl or
//! alt, and never bound `up`/`down` at all, so a bare arrow fell through
//! to the branch that inserts a character and did nothing.
//!
//! In Doom, Vim and Helix you can press Esc and use NORMAL's motions, so
//! the gap was invisible there. Notion and Emacs have no NORMAL to
//! escape to: inside a note the caret could not be moved up or down by
//! any key, and left/right only by `C-b`/`C-f` — while `C-p`/`C-n`, the
//! pair an Emacs hand reaches for, are the completion cycle. The mouse
//! was the only way to reach the line above.
//!
//! `BodyEditor::up`/`down` already existed and were reachable from
//! NORMAL. All that was missing was four rows in the INSERT table.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const NOTES: &str = "\
* Alpha
:PROPERTIES:
:ID: 01HQARROW00000000000001
:END:
first line
second line
third line
";

/// Open Alpha's body in `mode`, in INSERT — which is where a non-modal
/// mode always is.
fn editing(mode: InputMode) -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut shell = Shell::new(vault);
    let mut app = ModalApp::new(mode);
    assert!(app.select_by_id(&shell, "01HQARROW00000000000001"));
    app.run(&mut shell, "edit-body");
    if matches!(mode, InputMode::Doom | InputMode::Vim | InputMode::Helix) {
        // The modal modes open a buffer in NORMAL; the complaint is
        // about INSERT, which is where the friendly modes live.
        app.on_key(&mut shell, "i", false, false, Some('i'));
    }
    // A buffer opens where you left it, which is the end of the text
    // here; every case below is about a known starting point.
    app.body_click(0, 0);
    (dir, shell, app)
}

fn press(app: &mut ModalApp, shell: &mut Shell, key: &str) {
    app.on_key(shell, key, false, false, None);
}

/// `(line, column)` of the caret.
fn at(app: &ModalApp) -> (usize, usize) {
    app.body_cursor()
}

#[test]
fn down_and_up_walk_the_lines() {
    for mode in [InputMode::Notion, InputMode::Emacs] {
        let (_d, mut shell, mut app) = editing(mode);
        assert_eq!(at(&app).0, 0, "{mode:?} starts on the first line");

        press(&mut app, &mut shell, "down");
        assert_eq!(at(&app).0, 1, "{mode:?}: down went nowhere");
        press(&mut app, &mut shell, "down");
        assert_eq!(at(&app).0, 2, "{mode:?}");
        press(&mut app, &mut shell, "up");
        assert_eq!(at(&app).0, 1, "{mode:?}: up went nowhere");
    }
}

#[test]
fn left_and_right_walk_the_characters() {
    for mode in [InputMode::Notion, InputMode::Emacs] {
        let (_d, mut shell, mut app) = editing(mode);
        press(&mut app, &mut shell, "right");
        press(&mut app, &mut shell, "right");
        assert_eq!(at(&app).1, 2, "{mode:?}: right went nowhere");
        press(&mut app, &mut shell, "left");
        assert_eq!(at(&app).1, 1, "{mode:?}: left went nowhere");
    }
}

#[test]
fn an_arrow_does_not_type_anything() {
    // The branch they used to fall into inserts characters.
    let (_d, mut shell, mut app) = editing(InputMode::Notion);
    let before = app.body_buffer().to_owned();
    for key in ["up", "down", "left", "right"] {
        press(&mut app, &mut shell, key);
    }
    assert_eq!(app.body_buffer(), before);
}

#[test]
fn the_column_survives_the_trip_between_lines() {
    // Down and back up returns to the character you left, not to the
    // start of the line — the thing that makes arrows usable at all.
    let (_d, mut shell, mut app) = editing(InputMode::Notion);
    for _ in 0..5 {
        press(&mut app, &mut shell, "right");
    }
    assert_eq!(at(&app), (0, 5));
    press(&mut app, &mut shell, "down");
    assert_eq!(at(&app), (1, 5));
    press(&mut app, &mut shell, "up");
    assert_eq!(at(&app), (0, 5));
}

#[test]
fn a_short_line_clamps_the_column() {
    let (_d, mut shell, mut app) = editing(InputMode::Notion);
    app.on_key(&mut shell, "end", false, false, None);
    let (_, wide) = at(&app);
    press(&mut app, &mut shell, "down");
    let (line, col) = at(&app);
    assert_eq!(line, 1);
    assert!(col <= wide, "clamped to the shorter line: {col} vs {wide}");
}

#[test]
fn the_ends_of_the_buffer_hold() {
    let (_d, mut shell, mut app) = editing(InputMode::Notion);
    press(&mut app, &mut shell, "up");
    assert_eq!(at(&app).0, 0, "up from the first line stays");
    for _ in 0..10 {
        press(&mut app, &mut shell, "down");
    }
    let last = app.body_buffer().lines().count().saturating_sub(1);
    assert!(at(&app).0 <= last, "down past the end stays inside");
}

#[test]
fn the_modal_modes_keep_their_arrows_too() {
    // The gap was invisible in Doom because Esc reaches NORMAL, but
    // INSERT is INSERT: the arrows work there as well.
    let (_d, mut shell, mut app) = editing(InputMode::Doom);
    press(&mut app, &mut shell, "down");
    assert_eq!(at(&app).0, 1);
}

#[test]
fn the_modified_arrows_still_mean_words() {
    // ctrl/alt + arrow is the desktop word jump and was the only thing
    // bound here; adding the bare arrows must not take it away.
    let (_d, mut shell, mut app) = editing(InputMode::Notion);
    app.on_key(&mut shell, "end", false, false, None);
    let (_, end) = at(&app);
    app.on_key(&mut shell, "left", true, false, None);
    let (line, col) = at(&app);
    assert_eq!(line, 0, "still on the first line");
    assert!(col < end, "jumped a word back, not a character: {col}");
}

#[test]
fn home_and_end_still_work() {
    let (_d, mut shell, mut app) = editing(InputMode::Notion);
    app.on_key(&mut shell, "end", false, false, None);
    assert!(at(&app).1 > 0);
    app.on_key(&mut shell, "home", false, false, None);
    assert_eq!(at(&app).1, 0);
}
