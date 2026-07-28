//! Org's table keys, on the keys the buffer actually receives.
//!
//! The chords are org's own, read out of `org-mode-map`: `M-<left>` /
//! `M-<right>` move the column, `M-<up>` / `M-<down>` the row, and with
//! shift they delete and insert instead. Each is the same key as the
//! outline command it shadows, and the *context* decides — which is
//! `org-metaleft`'s whole design.
//!
//! `C-c -` rules a line in Emacs; `C-c` is the desktop copy chord here,
//! so the rule is `M--`.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const SRC: &str = "\
* Note
:PROPERTIES:
:ID: 01HQTABLE000000000000001
:END:
before
| a | b | c |
| 1 | 2 | 3 |
after
";

fn editing() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let v = Vault::open(dir.path()).expect("open");
    let mut sh = Shell::new(v);
    let mut app = ModalApp::new(InputMode::Doom);
    app.run(&mut sh, "edit-body");
    assert_eq!(app.surface(), ModalSurface::EditBody);
    (dir, sh, app)
}

/// Put the cursor in the first data cell of the table (`1`).
fn in_first_cell(app: &mut ModalApp) {
    let at = app.body_buffer().find("| 1 |").expect("the row") + 2;
    app.body_set_cursor(at);
}

/// The table's rows, cells trimmed.
fn cells(app: &ModalApp) -> Vec<Vec<String>> {
    app.body_buffer()
        .lines()
        .filter(|l| l.trim_start().starts_with('|'))
        .map(|l| {
            l.trim()
                .trim_matches('|')
                .split('|')
                .map(|c| c.trim().to_owned())
                .collect()
        })
        .collect()
}

fn key(app: &mut ModalApp, sh: &mut Shell, name: &str, ctrl: bool, alt: bool) {
    app.on_key(sh, name, ctrl, alt, None);
}

#[test]
fn alt_right_moves_the_column() {
    let (_d, mut sh, mut app) = editing();
    in_first_cell(&mut app);
    key(&mut app, &mut sh, "right", false, true);
    assert_eq!(cells(&app)[0], vec!["b", "a", "c"]);
    assert_eq!(cells(&app)[1], vec!["2", "1", "3"]);
}

#[test]
fn alt_left_moves_it_back() {
    let (_d, mut sh, mut app) = editing();
    in_first_cell(&mut app);
    key(&mut app, &mut sh, "right", false, true);
    key(&mut app, &mut sh, "left", false, true);
    assert_eq!(cells(&app)[0], vec!["a", "b", "c"], "back where it was");
}

#[test]
fn alt_down_moves_the_row() {
    let (_d, mut sh, mut app) = editing();
    in_first_cell(&mut app);
    key(&mut app, &mut sh, "up", false, true);
    assert_eq!(cells(&app)[0], vec!["1", "2", "3"], "the row rose");
}

#[test]
fn alt_shift_right_inserts_a_column_and_alt_shift_left_deletes_one() {
    let (_d, mut sh, mut app) = editing();
    in_first_cell(&mut app);
    key(&mut app, &mut sh, "shift-right", false, true);
    assert_eq!(cells(&app)[0].len(), 4, "a column went in");
    key(&mut app, &mut sh, "shift-left", false, true);
    assert_eq!(cells(&app)[0].len(), 3, "and came back out");
}

#[test]
fn alt_shift_down_inserts_a_row_and_alt_shift_up_kills_one() {
    let (_d, mut sh, mut app) = editing();
    in_first_cell(&mut app);
    key(&mut app, &mut sh, "shift-down", false, true);
    assert_eq!(cells(&app).len(), 3, "a row went in");
    key(&mut app, &mut sh, "shift-up", false, true);
    assert_eq!(cells(&app).len(), 2, "and was killed again");
}

#[test]
fn alt_minus_rules_a_line() {
    let (_d, mut sh, mut app) = editing();
    let at = app.body_buffer().find("| a |").expect("header") + 2;
    app.body_set_cursor(at);
    key(&mut app, &mut sh, "-", false, true);
    let ruled = app
        .body_buffer()
        .lines()
        .any(|l| l.starts_with('|') && l.contains("---"));
    assert!(ruled, "a rule under the header: {}", app.body_buffer());
}

#[test]
fn shift_tab_steps_back_a_cell() {
    let (_d, mut sh, mut app) = editing();
    let row = app.body_buffer().find("| a |").expect("header");
    app.body_set_cursor(row + 6); // on `b`
    key(&mut app, &mut sh, "shift-tab", false, false);
    let (_, col) = app.body_cursor();
    assert_eq!(col, 2, "back on `a`: {:?}", app.body_cursor());
}

#[test]
fn a_table_edit_is_one_undo_step() {
    let (_d, mut sh, mut app) = editing();
    in_first_cell(&mut app);
    let before = app.body_buffer().to_owned();
    key(&mut app, &mut sh, "right", false, true);
    assert_ne!(app.body_buffer(), before);
    app.on_key(&mut sh, "escape", false, false, None); // back to NORMAL
    app.on_key(&mut sh, "u", false, false, Some('u'));
    assert_eq!(app.body_buffer(), before, "one `u` puts the column back");
}

#[test]
fn outside_a_table_the_arrows_keep_their_own_jobs() {
    // The context rule, which is the whole point of using org's chords:
    // `M-<right>` on prose must not become a table command.
    let (_d, mut sh, mut app) = editing();
    let at = app.body_buffer().find("before").expect("prose");
    app.body_set_cursor(at);
    let before = app.body_buffer().to_owned();
    for (name, alt) in [("right", true), ("left", true), ("shift-down", true)] {
        key(&mut app, &mut sh, name, false, alt);
    }
    assert_eq!(app.body_buffer(), before, "prose is untouched");
}
