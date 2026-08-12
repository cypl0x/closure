//! A file's own `#+COLUMNS:` becomes a view.
//!
//! `document_columns` parses the format, `ViewSpec` renders columns,
//! and nothing joined them — so a document that says how it wants to be
//! tabulated was tabulated some other way, and org's column view and
//! closure's database view stayed two ideas that never met.
//!
//! They are the same idea from opposite ends. A `closure-view` block
//! says "here is a table of the vault"; `#+COLUMNS:` says "here is how
//! *this file* should be read". Turning the second into the first is
//! most of what makes an org file closure opens feel like the org file
//! somebody wrote.
//!
//! Org's specials are mapped where closure has an equivalent — `ITEM`
//! is the title — and everything else is a property, which is what
//! `:columns` already means.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_query::{Column, ViewSpec};
use closure_store::Vault;
use tempfile::TempDir;

const VAULT: &str = "\
#+COLUMNS: %40ITEM %TODO %EFFORT
* TODO Read the spec
:PROPERTIES:
:ID: 01COLVIEW00000000000001
:EFFORT: 3
:END:
* DONE Write the tests
:PROPERTIES:
:ID: 01COLVIEW00000000000002
:EFFORT: 5
:END:
";

fn vault(src: &str) -> (TempDir, Vault) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), src).unwrap();
    let v = Vault::open(dir.path()).unwrap();
    (dir, v)
}

#[test]
fn a_declared_format_becomes_a_view() {
    let (_d, v) = vault(VAULT);
    let spec = ViewSpec::from_document_columns(&v, std::path::Path::new("notes.org"))
        .expect("the file declares one");
    assert_eq!(
        spec.columns,
        vec![
            Column::Title,
            Column::Todo,
            Column::Property("EFFORT".to_owned()),
        ]
    );
}

#[test]
fn item_is_the_title_and_the_rest_are_properties() {
    // Org's specials where closure has an equivalent; everything else
    // is a property, which is what `:columns` already means.
    let (_d, v) = vault(
        "#+COLUMNS: %ITEM %CATEGORY\n* A\n:PROPERTIES:\n:ID: 01COLVIEW00000000000003\n:END:\n",
    );
    let spec = ViewSpec::from_document_columns(&v, std::path::Path::new("notes.org")).expect("one");
    assert_eq!(spec.columns[0], Column::Title);
    assert_eq!(spec.columns[1], Column::Property("CATEGORY".to_owned()));
}

#[test]
fn it_only_sees_that_file() {
    // `#+COLUMNS:` is a statement about the document it is in, so the
    // view it produces is scoped to that file rather than the vault.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), VAULT).unwrap();
    std::fs::write(
        dir.path().join("other.org"),
        "* Elsewhere\n:PROPERTIES:\n:ID: 01COLVIEW00000000000004\n:END:\n",
    )
    .unwrap();
    let v = Vault::open(dir.path()).unwrap();
    let spec = ViewSpec::from_document_columns(&v, std::path::Path::new("notes.org")).expect("one");
    let titles: Vec<String> = spec.cells(&v).into_iter().map(|r| r[0].clone()).collect();
    assert!(!titles.iter().any(|t| t == "Elsewhere"), "{titles:?}");
    assert_eq!(titles.len(), 2, "{titles:?}");
}

#[test]
fn the_cells_are_the_files_own_values() {
    let (_d, v) = vault(VAULT);
    let spec = ViewSpec::from_document_columns(&v, std::path::Path::new("notes.org")).expect("one");
    let cells = spec.cells(&v);
    assert_eq!(cells[0], vec!["Read the spec", "TODO", "3"]);
    assert_eq!(cells[1], vec!["Write the tests", "DONE", "5"]);
}

#[test]
fn a_file_that_declares_nothing_gets_no_view() {
    // Distinguishable from an empty one: a caller wants to know whether
    // to show a table at all.
    let (_d, v) = vault("* A\n:PROPERTIES:\n:ID: 01COLVIEW00000000000005\n:END:\n");
    assert!(ViewSpec::from_document_columns(&v, std::path::Path::new("notes.org")).is_none());
}

#[test]
fn a_file_that_is_not_in_the_vault_gets_no_view() {
    let (_d, v) = vault(VAULT);
    assert!(ViewSpec::from_document_columns(&v, std::path::Path::new("nope.org")).is_none());
}
