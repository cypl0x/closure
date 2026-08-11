//! The database pane shows the vault's own views.
//!
//! `closure-query` grew filters, multi-key sorts, grouping, relations
//! and rollups, and a vault can save any of it as a
//! `#+BEGIN: closure-view` block. None of it reached the window: the
//! database surface built four fixed columns over every headline in the
//! vault and never looked at what the vault had defined.
//!
//! So a saved view that nobody can see is a query nobody runs. The pane
//! renders the views the vault defines, and falls back to the
//! everything-table only when there are none — which is what a vault
//! that has never defined one should still show.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const WITH_VIEW: &str = "\
* Rewrite the parser :project:
:PROPERTIES:
:ID: 01DBVIEW0000000000000001
:END:
* TODO Read the spec :task:
:PROPERTIES:
:ID: 01DBVIEW0000000000000002
:PROJECT: 01DBVIEW0000000000000001
:EFFORT: 3
:END:
* DONE Old thing :task:
:PROPERTIES:
:ID: 01DBVIEW0000000000000003
:EFFORT: 9
:END:
* Views
:PROPERTIES:
:ID: 01DBVIEW0000000000000004
:END:
#+BEGIN: closure-view :name open-tasks :from tag:task :columns title,rel:PROJECT :filter todo=TODO
#+END:
";

const WITHOUT_VIEW: &str = "\
* Alpha
:PROPERTIES:
:ID: 01DBVIEW0000000000000005
:END:
";

fn app(src: &str) -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), src).unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

#[test]
fn the_pane_uses_the_saved_views_columns() {
    let (_d, shell, app) = app(WITH_VIEW);
    let (header, _rows) = app.db_rows(&shell);
    assert_eq!(
        header,
        vec!["title".to_owned(), "PROJECT".to_owned()],
        "the pane is still showing its own four columns"
    );
}

#[test]
fn the_pane_applies_the_saved_views_filter() {
    let (_d, shell, app) = app(WITH_VIEW);
    let (_header, rows) = app.db_rows(&shell);
    let titles: Vec<&str> = rows.iter().map(|r| r[0].as_str()).collect();
    assert_eq!(
        titles,
        vec!["Read the spec"],
        "the filter and the source were ignored: {titles:?}"
    );
}

#[test]
fn a_relation_column_resolves_in_the_pane_too() {
    let (_d, shell, app) = app(WITH_VIEW);
    let (_header, rows) = app.db_rows(&shell);
    assert_eq!(rows[0][1], "Rewrite the parser");
}

#[test]
fn a_vault_with_no_saved_view_still_gets_a_table() {
    // The everything-table is a reasonable default and stays one.
    let (_d, shell, app) = app(WITHOUT_VIEW);
    let (header, rows) = app.db_rows(&shell);
    assert_eq!(header.len(), 4, "{header:?}");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][0], "Alpha");
}

#[test]
fn the_pane_says_which_view_it_is_showing() {
    // A table with no name is one nobody can tell from the default.
    let (_d, shell, a) = app(WITH_VIEW);
    assert_eq!(a.db_view_name(&shell).as_deref(), Some("open-tasks"));
    let (_d2, shell2, app2) = app(WITHOUT_VIEW);
    assert_eq!(app2.db_view_name(&shell2), None);
}
