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

// === D1 typed + directional sort. ===

fn level_vault() -> (TempDir, Vault) {
    // Levels 1, 2, 10 (deep) so lexical vs numeric ordering differ.
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("n.org"),
        "* L1\n** L2\n*** L3\n**** L4\n***** L5\n****** L6\n******* L7\n\
         ******** L8\n********* L9\n********** L10\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, v)
}

#[test]
fn level_sort_is_numeric_not_lexical() {
    let (_d, v) = level_vault();
    let spec = ViewSpec::parse(":columns level,title :sort level").expect("parse");
    let cells = spec.cells(&v);
    let levels: Vec<&str> = cells.iter().map(|r| r[0].as_str()).collect();
    // Numeric: 1,2,...,10. Lexical would put "10" right after "1".
    assert_eq!(levels.first().copied(), Some("1"));
    assert_eq!(levels.last().copied(), Some("10"));
    let pos1 = levels.iter().position(|x| *x == "1").unwrap();
    let pos2 = levels.iter().position(|x| *x == "2").unwrap();
    let pos10 = levels.iter().position(|x| *x == "10").unwrap();
    assert!(pos1 < pos2 && pos2 < pos10, "numeric order: {levels:?}");
}

#[test]
fn descending_sort_reverses() {
    let (_d, v) = vault();
    let asc = ViewSpec::parse(":columns title :sort title").expect("parse");
    let desc = ViewSpec::parse(":columns title :sort -title").expect("parse");
    let mut a: Vec<String> = asc.cells(&v).into_iter().map(|r| r[0].clone()).collect();
    let d: Vec<String> = desc.cells(&v).into_iter().map(|r| r[0].clone()).collect();
    a.reverse();
    assert_eq!(a, d, ":sort -title is the reverse of :sort title");
}

#[test]
fn default_sort_is_ascending() {
    let (_d, v) = vault();
    let spec = ViewSpec::parse(":columns title :sort title").expect("parse");
    let titles: Vec<String> = spec.cells(&v).into_iter().map(|r| r[0].clone()).collect();
    let mut sorted = titles.clone();
    sorted.sort();
    assert_eq!(titles, sorted, "ascending by default");
}

// === D2 filter operators + multiple filters (AND). ===

#[test]
fn contains_filter_matches_substring_case_insensitive() {
    let (_d, v) = vault();
    let spec = ViewSpec::parse(":columns title :filter title~SPEC").expect("parse");
    let titles: Vec<String> = spec.cells(&v).into_iter().map(|r| r[0].clone()).collect();
    assert_eq!(
        titles,
        vec!["Write spec".to_owned()],
        "~ is case-insensitive substring"
    );
}

#[test]
fn not_equal_filter_excludes() {
    let (_d, v) = vault();
    let spec = ViewSpec::parse(":columns title,todo :filter todo!=DONE").expect("parse");
    let todos: Vec<String> = spec.cells(&v).into_iter().map(|r| r[1].clone()).collect();
    assert!(
        !todos.iter().any(|t| t == "DONE"),
        "DONE excluded: {todos:?}"
    );
    assert!(todos.iter().any(|t| t == "TODO"), "TODO kept");
}

#[test]
fn numeric_greater_than_on_level() {
    let (_d, v) = level_vault();
    let spec = ViewSpec::parse(":columns level :filter level>5 :sort level").expect("parse");
    let levels: Vec<i64> = spec
        .cells(&v)
        .into_iter()
        .map(|r| r[0].parse().unwrap())
        .collect();
    assert_eq!(levels, vec![6, 7, 8, 9, 10], "only levels > 5: {levels:?}");
}

#[test]
fn multiple_filters_are_anded() {
    let (_d, v) = vault();
    // todo=TODO AND title contains "parser" -> only "Ship parser".
    let spec =
        ViewSpec::parse(":columns title :filter todo=TODO :filter title~parser").expect("parse");
    let titles: Vec<String> = spec.cells(&v).into_iter().map(|r| r[0].clone()).collect();
    assert_eq!(titles, vec!["Ship parser".to_owned()]);
}

#[test]
fn filter_without_operator_errors() {
    assert!(
        ViewSpec::parse(":filter titlevalue").is_err(),
        "no operator -> error"
    );
}

// === D3 multi-key sort. ===

fn todo_title_vault() -> (TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("t.org"),
        "* TODO Zebra\n* TODO Apple\n* DONE Mango\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, v)
}

#[test]
fn multi_key_sort_ties_break_on_second_key() {
    let (_d, v) = todo_title_vault();
    // todo asc (DONE < TODO), then title asc within each group.
    let spec = ViewSpec::parse(":columns todo,title :sort todo,title").expect("parse");
    let rows: Vec<(String, String)> = spec
        .cells(&v)
        .into_iter()
        .map(|r| (r[0].clone(), r[1].clone()))
        .collect();
    assert_eq!(
        rows,
        vec![
            ("DONE".to_owned(), "Mango".to_owned()),
            ("TODO".to_owned(), "Apple".to_owned()),
            ("TODO".to_owned(), "Zebra".to_owned()),
        ]
    );
}

#[test]
fn multi_key_sort_mixed_directions() {
    let (_d, v) = todo_title_vault();
    // todo asc, then title DESC within the TODO group.
    let spec = ViewSpec::parse(":columns todo,title :sort todo,-title").expect("parse");
    let titles: Vec<String> = spec.cells(&v).into_iter().map(|r| r[1].clone()).collect();
    assert_eq!(
        titles,
        vec!["Mango".to_owned(), "Zebra".to_owned(), "Apple".to_owned()]
    );
}

#[test]
fn single_key_sort_still_works() {
    let (_d, v) = todo_title_vault();
    let spec = ViewSpec::parse(":columns title :sort title").expect("parse");
    let titles: Vec<String> = spec.cells(&v).into_iter().map(|r| r[0].clone()).collect();
    assert_eq!(
        titles,
        vec!["Apple".to_owned(), "Mango".to_owned(), "Zebra".to_owned()]
    );
}

// === D4 multiple named views enumerated from the vault. ===

fn views_vault() -> (TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("db.org"),
        "* Notes\n\
         #+BEGIN: closure-view :name Work :from tag:work :columns title\n#+END:\n\
         #+BEGIN: closure-view :from all :columns title,todo\n#+END:\n\
         * TODO Ship parser :work:\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, v)
}

#[test]
fn enumerates_views_in_order_with_names() {
    let (_d, v) = views_vault();
    let views = closure_query::views(&v).expect("enumerate");
    assert_eq!(views.len(), 2, "two closure-view blocks");
    assert_eq!(views[0].0, "Work", "name from :name param");
    assert!(!views[1].0.is_empty(), "second gets a default name");
    // The named spec carries its parsed source.
    assert!(matches!(views[0].1.from, closure_query::Source::Tag(ref t) if t == "work"));
}

#[test]
fn no_view_blocks_yields_empty() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("a.org"), "* Just a heading\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    assert!(closure_query::views(&v).expect("ok").is_empty());
}

#[test]
fn malformed_view_block_fails_the_batch() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("a.org"),
        "#+BEGIN: closure-view :bogus x\n#+END:\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    assert!(
        closure_query::views(&v).is_err(),
        "malformed block errors the batch"
    );
}
