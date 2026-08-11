//! `#+COLUMNS: %25ITEM %TODO %3PRIORITY %EFFORT{+}`
//!
//! Org's column view is a table over the outline, declared in the file
//! it applies to. closure preserved the line and had no column format,
//! so a document that says how it wants to be tabulated was tabulated
//! some other way — and the vault already grew a database view with
//! columns, widths and aggregates, which is the same idea arrived at
//! from the other end.
//!
//! Reading the format is this item. What a shell does with it is the
//! shell's business, and `closure-query` already knows how to render
//! columns; this is the part that lets a *file* ask.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::{ColumnSpec, Summary, columns_format};

#[test]
fn a_bare_property_is_a_column() {
    let spec = columns_format("%TODO").expect("a format");
    assert_eq!(
        spec,
        vec![ColumnSpec {
            width: None,
            property: "TODO".to_owned(),
            title: None,
            summary: None,
        }]
    );
}

#[test]
fn a_width_is_read() {
    let spec = columns_format("%25ITEM").expect("a format");
    assert_eq!(spec[0].width, Some(25));
    assert_eq!(spec[0].property, "ITEM");
}

#[test]
fn a_title_in_parentheses_is_read() {
    let spec = columns_format("%25ITEM(Task)").expect("a format");
    assert_eq!(spec[0].title.as_deref(), Some("Task"));
    assert_eq!(spec[0].property, "ITEM");
}

#[test]
fn a_summary_in_braces_is_read() {
    let spec = columns_format("%EFFORT{+}").expect("a format");
    assert_eq!(spec[0].summary, Some(Summary::Sum));
    assert_eq!(
        columns_format("%X{min}").unwrap()[0].summary,
        Some(Summary::Min)
    );
    assert_eq!(
        columns_format("%X{max}").unwrap()[0].summary,
        Some(Summary::Max)
    );
    assert_eq!(
        columns_format("%X{X}").unwrap()[0].summary,
        Some(Summary::CheckboxCount)
    );
}

#[test]
fn several_columns_come_back_in_order() {
    let spec = columns_format("%25ITEM %TODO %3PRIORITY %TAGS").expect("a format");
    let names: Vec<&str> = spec.iter().map(|c| c.property.as_str()).collect();
    assert_eq!(names, vec!["ITEM", "TODO", "PRIORITY", "TAGS"]);
    assert_eq!(spec[0].width, Some(25));
    assert_eq!(spec[2].width, Some(3));
}

#[test]
fn everything_at_once() {
    let spec = columns_format("%20ITEM(Task) %EFFORT{+}").expect("a format");
    assert_eq!(spec[0].width, Some(20));
    assert_eq!(spec[0].title.as_deref(), Some("Task"));
    assert_eq!(spec[1].property, "EFFORT");
    assert_eq!(spec[1].summary, Some(Summary::Sum));
}

#[test]
fn a_line_with_no_columns_is_not_a_format() {
    // Distinguishable from an empty one: nothing to show and nothing
    // declared are different states.
    assert_eq!(columns_format(""), None);
    assert_eq!(columns_format("no percent signs here"), None);
}

#[test]
fn an_unknown_summary_is_kept_as_no_summary_rather_than_refused() {
    // Org has a dozen and closure knows four. A format nobody here
    // implemented should not make a document unreadable — the same
    // rule an unknown widget type and an unknown rollup already follow.
    let spec = columns_format("%X{@median}").expect("a format");
    assert_eq!(spec[0].property, "X");
    assert_eq!(spec[0].summary, None);
}

#[test]
fn the_documents_own_declaration_is_found() {
    let src = "#+COLUMNS: %25ITEM %TODO\n* A\n:PROPERTIES:\n:ID: 01COLUMN000000000000AA\n:END:\n";
    let doc = closure_org::parse(src).expect("parse");
    let spec = closure_org::document_columns(&doc).expect("declared");
    assert_eq!(spec.len(), 2);
    assert_eq!(closure_org::print(&doc), src, "I1");
}
