//! org-defined database views: a params string (as found on a
//! `#+BEGIN: closure-view` dynamic block) names a row source and
//! property columns; cells materialise from the vault.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_query::{ViewError, ViewSpec};
use closure_store::Vault;
use tempfile::TempDir;

fn vault() -> (TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("work.org"),
        "* TODO Ship parser :work:\n:PROPERTIES:\n:EFFORT: 3d\n:END:\n\
         * DONE Write spec :work:\n:PROPERTIES:\n:EFFORT: 1d\n:END:\n\
         * Groceries :home:\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, v)
}

#[test]
fn parse_tag_source_and_columns() {
    let spec = ViewSpec::parse(":from tag:work :columns title,todo,EFFORT").expect("parse");
    assert_eq!(spec.header(), vec!["title", "todo", "EFFORT"]);
}

#[test]
fn parse_empty_params_uses_defaults() {
    let spec = ViewSpec::parse("").expect("parse");
    assert_eq!(spec.header(), vec!["title", "todo"]);
}

#[test]
fn parse_unknown_directive_errors() {
    assert!(matches!(
        ViewSpec::parse(":bogus x"),
        Err(ViewError::UnknownDirective(_))
    ));
}

#[test]
fn parse_bad_source_errors() {
    assert!(matches!(
        ViewSpec::parse(":from nonsense"),
        Err(ViewError::BadSource(_))
    ));
}

#[test]
fn cells_from_tag_source_with_property_column() {
    let (_td, v) = vault();
    let spec = ViewSpec::parse(":from tag:work :columns title,todo,EFFORT").expect("parse");
    let cells = spec.cells(&v);
    assert_eq!(cells.len(), 2, "only :work: rows");
    assert!(cells.contains(&vec![
        "Ship parser".to_owned(),
        "TODO".to_owned(),
        "3d".to_owned()
    ]));
    assert!(cells.contains(&vec![
        "Write spec".to_owned(),
        "DONE".to_owned(),
        "1d".to_owned()
    ]));
}

#[test]
fn missing_property_renders_empty_cell() {
    let (_td, v) = vault();
    let spec = ViewSpec::parse(":from tag:home :columns title,EFFORT").expect("parse");
    let cells = spec.cells(&v);
    assert_eq!(cells, vec![vec!["Groceries".to_owned(), String::new()]]);
}

#[test]
fn todo_source_selects_by_keyword() {
    let (_td, v) = vault();
    let spec = ViewSpec::parse(":from todo:DONE :columns title").expect("parse");
    let cells = spec.cells(&v);
    assert_eq!(cells, vec![vec!["Write spec".to_owned()]]);
}

#[test]
fn all_source_lists_everything() {
    let (_td, v) = vault();
    let spec = ViewSpec::parse(":from all :columns title").expect("parse");
    assert_eq!(spec.cells(&v).len(), 3);
}

#[test]
fn level_and_priority_builtin_columns() {
    let (_td, v) = vault();
    let spec = ViewSpec::parse(":from tag:home :columns level,priority,title").expect("parse");
    let cells = spec.cells(&v);
    assert_eq!(
        cells,
        vec![vec!["1".to_owned(), String::new(), "Groceries".to_owned()]]
    );
}

#[test]
fn filter_directive_narrows_rows() {
    let (_td, v) = vault();
    let spec = ViewSpec::parse(":from tag:work :filter todo=TODO :columns title").expect("parse");
    let cells = spec.cells(&v);
    assert_eq!(cells, vec![vec!["Ship parser".to_owned()]]);
}

#[test]
fn filter_on_property_column() {
    let (_td, v) = vault();
    let spec = ViewSpec::parse(":from all :filter EFFORT=1d :columns title").expect("parse");
    assert_eq!(spec.cells(&v), vec![vec!["Write spec".to_owned()]]);
}

#[test]
fn sort_directive_orders_rows() {
    let (_td, v) = vault();
    let spec = ViewSpec::parse(":from tag:work :columns title :sort title").expect("parse");
    let cells = spec.cells(&v);
    assert_eq!(
        cells,
        vec![
            vec!["Ship parser".to_owned()],
            vec!["Write spec".to_owned()]
        ]
    );
}

#[test]
fn render_produces_aligned_org_table() {
    let header = vec!["title".to_owned(), "todo".to_owned()];
    let rows = vec![
        vec!["Ship parser".to_owned(), "TODO".to_owned()],
        vec!["Go".to_owned(), String::new()],
    ];
    let out = closure_query::render_table(&header, &rows);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "| title       | todo |");
    assert_eq!(lines[1], "|-------------+------|");
    assert_eq!(lines[2], "| Ship parser | TODO |");
    assert_eq!(lines[3], "| Go          |      |");
}

#[test]
fn render_empty_rows_is_header_only() {
    let header = vec!["a".to_owned()];
    let out = closure_query::render_table(&header, &[]);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines, vec!["| a |", "|---|"]);
}
