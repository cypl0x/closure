//! A table in a body honours its own `|<r>|` row.
//!
//! Seventh construct through the preview seam (I12). The alignment row
//! is a directive shaped like a table row: the reader wants the column
//! aligned and does not want to see the instruction, and the file keeps
//! both (I1).

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const VAULT: &str = "\
* Hours
:PROPERTIES:
:ID: 01TABLEALIGN000000001
:END:
| Task | Time |
|<l>|<r>|
| Writing | 1:30 |
| A much longer task | 12:05 |
* Plain
:PROPERTIES:
:ID: 01TABLEALIGN000000002
:END:
| Task | Time |
| Writing | 1:30 |
";

fn app(src: &str) -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), src).unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

fn body_of(app: &mut ModalApp, shell: &Shell, title: &str) -> String {
    let rows = app.rows(shell);
    let i = rows.iter().position(|r| r.title == title).expect("a row");
    app.select(i, shell);
    app.selected_detail(shell).expect("a detail").body.clone()
}

#[test]
fn the_column_is_aligned_as_the_row_asked() {
    let (_d, shell, mut app) = app(VAULT);
    let body = body_of(&mut app, &shell, "Hours");
    assert!(body.contains("|  1:30 |"), "{body}");
    assert!(body.contains("| 12:05 |"), "{body}");
}

#[test]
fn the_directive_itself_is_not_shown() {
    // It is an instruction, not data. Printing it back is what closure
    // did, and it made the reader do the parsing.
    let (_d, shell, mut app) = app(VAULT);
    let body = body_of(&mut app, &shell, "Hours");
    assert!(!body.contains("<r>"), "{body}");
}

#[test]
fn a_table_that_asks_for_nothing_is_left_as_it_was() {
    let (_d, shell, mut app) = app(VAULT);
    let body = body_of(&mut app, &shell, "Plain");
    assert!(body.contains("| Task | Time |"), "{body}");
    assert!(body.contains("| Writing | 1:30 |"), "{body}");
}

#[test]
fn the_file_keeps_both() {
    let (dir, shell, mut app) = app(VAULT);
    let _ = body_of(&mut app, &shell, "Hours");
    let on_disk = std::fs::read_to_string(dir.path().join("notes.org")).unwrap();
    assert!(on_disk.contains("|<l>|<r>|"), "{on_disk}");
}
