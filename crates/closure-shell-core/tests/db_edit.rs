//! Editing a cell in a database view.
//!
//! Everything the database layer grew — filters, sorts, grouping,
//! relations, rollups — reads. A Notion database is a place you *work*,
//! and a table you can only look at is a report.
//!
//! A cell is a property on a headline, so writing one is `set-property`
//! through the registry (I8), undoable like every other mutation (I3).
//! What this adds is the addressing: "the EFFORT cell of the row the
//! cursor is on" has to become "this block, this key", and the view
//! that produced the row is the only thing that knows which is which
//! once a filter, a sort and a grouping have been applied.
//!
//! Not every column is editable, and saying which is the point. A
//! relation shows a title and holds an id; a rollup is arithmetic over
//! other rows. Letting either be typed into would write a value that
//! the next render throws away.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const VAULT: &str = "\
* Rewrite the parser :project:
:PROPERTIES:
:ID: 01DBEDIT000000000000001
:END:
* TODO Read the spec :task:
:PROPERTIES:
:ID: 01DBEDIT000000000000002
:PROJECT: 01DBEDIT000000000000001
:EFFORT: 3
:END:
* Views
:PROPERTIES:
:ID: 01DBEDIT000000000000003
:END:
#+BEGIN: closure-view :name work :from tag:task :columns title,EFFORT,rel:PROJECT,rollup:PROJECT.EFFORT:sum
#+END:
";

fn app() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), VAULT).unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

#[test]
fn a_property_cell_can_be_written() {
    let (_d, mut shell, app) = app();
    app.set_db_cell(&mut shell, 0, 1, "8")
        .expect("a writable cell");
    let doc = shell
        .vault
        .document_relative(std::path::Path::new("notes.org"));
    assert!(
        doc.expect("the file").source().contains(":EFFORT: 8"),
        "the property was not written"
    );
}

#[test]
fn it_writes_the_row_the_view_actually_produced() {
    // The addressing is the whole feature: after a filter, a sort and a
    // grouping, row 0 of the table is not row 0 of the vault, and
    // writing by position into the file would edit somebody else.
    let (_d, mut shell, app) = app();
    app.set_db_cell(&mut shell, 0, 1, "8").expect("written");
    let doc = shell
        .vault
        .document_relative(std::path::Path::new("notes.org"))
        .expect("the file");
    // The project, which is *not* in the view, keeps its own drawer.
    assert!(
        !doc.source().contains(":EFFORT: 8\n:END:\n* TODO Read"),
        "the value landed on the wrong headline:\n{}",
        doc.source()
    );
}

#[test]
fn the_title_column_renames_the_headline() {
    // `title` is not a property, and a database that cannot rename a
    // row is missing the edit people make most.
    let (_d, mut shell, app) = app();
    app.set_db_cell(&mut shell, 0, 0, "Read the whole spec")
        .expect("written");
    let doc = shell
        .vault
        .document_relative(std::path::Path::new("notes.org"))
        .expect("the file");
    assert!(doc.source().contains("* TODO Read the whole spec :task:"));
}

#[test]
fn a_relation_column_refuses() {
    // It shows a title and holds an id. Typing a title into it would
    // write something the next render throws away.
    let (_d, mut shell, app) = app();
    assert!(app.set_db_cell(&mut shell, 0, 2, "Something else").is_err());
}

#[test]
fn a_rollup_column_refuses() {
    // It is arithmetic over other rows; there is nothing to write to.
    let (_d, mut shell, app) = app();
    assert!(app.set_db_cell(&mut shell, 0, 3, "99").is_err());
}

#[test]
fn a_row_that_is_not_there_refuses() {
    let (_d, mut shell, app) = app();
    assert!(app.set_db_cell(&mut shell, 99, 1, "8").is_err());
}

#[test]
fn the_edit_is_undoable() {
    // I3. It went through the registry, so the undo tree has it.
    let (_d, mut shell, app) = app();
    let before = shell
        .vault
        .document_relative(std::path::Path::new("notes.org"))
        .expect("the file")
        .source();
    app.set_db_cell(&mut shell, 0, 1, "8").expect("written");
    shell
        .vault
        .undo_in(&shell.vault.root().join("notes.org"))
        .expect("undo");
    let after = shell
        .vault
        .document_relative(std::path::Path::new("notes.org"))
        .expect("the file")
        .source();
    assert_eq!(after, before, "undo did not put the cell back");
}
