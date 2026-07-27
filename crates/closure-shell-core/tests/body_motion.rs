//! Vertical motion keeps the column.
//!
//! `j` and `k` in NORMAL resolved to `line_col_offset(line ± n, 0)` —
//! column zero, every time. Every downward move jumped to the start of
//! the line, which is the one thing vertical motion must not do, and
//! what "moving a line downwards shouldn't always jump to the beginning
//! of line" is about.
//!
//! Vim keeps a *desired* column (`curswant`), not merely the current
//! one: moving down through a short line and on to a long one comes
//! back to where you started, and only a horizontal move or an edit
//! forgets it.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

fn shell() -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* Note\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

/// The editor over `text`, in NORMAL with the cursor at the start.
fn editing(text: &str) -> (TempDir, Shell, ModalApp) {
    let (d, mut sh) = shell();
    let mut app = ModalApp::new(InputMode::Vim);
    app.on_key(&mut sh, "i", false, false, Some('i'));
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

fn press(app: &mut ModalApp, sh: &mut Shell, key: &str) {
    app.on_key(sh, key, false, false, key.chars().next());
}

const LINES: &str = "aaaaaaaaaa\nbbbbbbbbbb\ncc\ndddddddddd";

#[test]
fn j_keeps_the_column() {
    let (_d, mut sh, mut app) = editing(LINES);
    for _ in 0..4 {
        press(&mut app, &mut sh, "l");
    }
    assert_eq!(app.body_cursor(), (0, 4));
    press(&mut app, &mut sh, "j");
    assert_eq!(
        app.body_cursor(),
        (1, 4),
        "down a line, same column — not back to the start of it"
    );
}

#[test]
fn k_keeps_the_column() {
    let (_d, mut sh, mut app) = editing(LINES);
    press(&mut app, &mut sh, "j");
    for _ in 0..3 {
        press(&mut app, &mut sh, "l");
    }
    assert_eq!(app.body_cursor(), (1, 3));
    press(&mut app, &mut sh, "k");
    assert_eq!(app.body_cursor(), (0, 3));
}

#[test]
fn a_short_line_clamps_the_column_without_forgetting_it() {
    // Vim's `curswant`: passing through a two-character line does not
    // cost you the column you were in.
    let (_d, mut sh, mut app) = editing(LINES);
    for _ in 0..8 {
        press(&mut app, &mut sh, "l");
    }
    assert_eq!(app.body_cursor(), (0, 8));
    press(&mut app, &mut sh, "j");
    press(&mut app, &mut sh, "j");
    assert_eq!(
        app.body_cursor().0,
        2,
        "on the short line, clamped: {:?}",
        app.body_cursor()
    );
    assert!(app.body_cursor().1 <= 2);
    press(&mut app, &mut sh, "j");
    assert_eq!(
        app.body_cursor(),
        (3, 8),
        "and back out to the column we started in"
    );
}

#[test]
fn a_horizontal_move_sets_a_new_column() {
    let (_d, mut sh, mut app) = editing(LINES);
    for _ in 0..8 {
        press(&mut app, &mut sh, "l");
    }
    press(&mut app, &mut sh, "j");
    press(&mut app, &mut sh, "h");
    assert_eq!(app.body_cursor(), (1, 7));
    press(&mut app, &mut sh, "j");
    assert_eq!(
        app.body_cursor().1,
        2,
        "the new column is 7, clamped to a two-character line"
    );
    press(&mut app, &mut sh, "j");
    assert_eq!(app.body_cursor(), (3, 7), "and it is 7 that comes back");
}

#[test]
fn the_arrows_keep_the_column_too() {
    let (_d, mut sh, mut app) = editing(LINES);
    for _ in 0..5 {
        press(&mut app, &mut sh, "l");
    }
    app.on_key(&mut sh, "down", false, false, None);
    assert_eq!(app.body_cursor(), (1, 5));
    app.on_key(&mut sh, "up", false, false, None);
    assert_eq!(app.body_cursor(), (0, 5));
}

#[test]
fn a_count_still_lands_on_the_right_line() {
    let (_d, mut sh, mut app) = editing(LINES);
    for _ in 0..4 {
        press(&mut app, &mut sh, "l");
    }
    press(&mut app, &mut sh, "3");
    press(&mut app, &mut sh, "j");
    assert_eq!(app.body_cursor(), (3, 4), "3j: three lines down, column 4");
}

#[test]
fn dj_is_still_linewise() {
    // The column memory must not turn a linewise operator into a
    // character-wise one.
    let (_d, mut sh, mut app) = editing(LINES);
    for _ in 0..4 {
        press(&mut app, &mut sh, "l");
    }
    press(&mut app, &mut sh, "d");
    press(&mut app, &mut sh, "j");
    assert_eq!(
        app.body_buffer(),
        "cc\ndddddddddd",
        "`dj` took both whole lines"
    );
}
