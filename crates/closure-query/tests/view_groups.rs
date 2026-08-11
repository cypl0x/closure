//! Grouping: `:group todo` on a view.
//!
//! A Notion database that cannot group is a saved search. Grouping is
//! what turns "every task with this tag" into a board — the same rows,
//! arranged by the value of one column, so the shape of the work is
//! visible rather than inferable.
//!
//! Rendered as one org table with a rule between groups rather than as
//! several tables, because it is one query and one set of columns; a
//! reader scanning down a column should not have to re-find it after
//! every heading. `|---|` is an ordinary org table separator, so the
//! result is still a table Emacs will align.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_query::ViewSpec;
use closure_store::Vault;
use tempfile::TempDir;

const VAULT: &str = "\
* TODO Alpha :work:
:PROPERTIES:
:ID: 01GROUP0000000000000001
:EFFORT: 3
:END:
* DONE Beta :work:
:PROPERTIES:
:ID: 01GROUP0000000000000002
:EFFORT: 1
:END:
* TODO Gamma :work:
:PROPERTIES:
:ID: 01GROUP0000000000000003
:EFFORT: 2
:END:
";

fn vault() -> (TempDir, Vault) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), VAULT).unwrap();
    let v = Vault::open(dir.path()).unwrap();
    (dir, v)
}

#[test]
fn a_group_directive_is_understood() {
    let spec = ViewSpec::parse(":from tag:work :columns title,todo :group todo").expect("parse");
    assert!(spec.group.is_some());
}

#[test]
fn rows_come_back_grouped_by_the_column() {
    let (_d, v) = vault();
    let spec = ViewSpec::parse(":from tag:work :columns title,todo :group todo").expect("parse");
    let groups = spec.groups(&v);
    let names: Vec<&str> = groups.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(names, vec!["DONE", "TODO"], "groups in order of value");
    assert_eq!(groups[0].1.len(), 1, "one DONE");
    assert_eq!(groups[1].1.len(), 2, "two TODO");
}

#[test]
fn a_view_without_grouping_is_one_group_of_everything() {
    // So a renderer has one shape to draw rather than two.
    let (_d, v) = vault();
    let spec = ViewSpec::parse(":from tag:work :columns title,todo").expect("parse");
    let groups = spec.groups(&v);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].0, "", "the one group has no name");
    assert_eq!(groups[0].1.len(), 3);
}

#[test]
fn grouping_by_a_property_works_too() {
    let (_d, v) = vault();
    let spec =
        ViewSpec::parse(":from tag:work :columns title,EFFORT :group EFFORT").expect("parse");
    let groups = spec.groups(&v);
    assert_eq!(groups.len(), 3, "three distinct efforts: {groups:?}");
}

#[test]
fn a_missing_value_groups_under_its_own_name() {
    // A row that has nothing to group by is still a row, and hiding it
    // would make the table disagree with the query that produced it.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("n.org"),
        "* TODO Has :work:\n:PROPERTIES:\n:ID: 01GROUP0000000000000004\n:EFFORT: 1\n:END:\n\
         * TODO Hasnt :work:\n:PROPERTIES:\n:ID: 01GROUP0000000000000005\n:END:\n",
    )
    .unwrap();
    let v = Vault::open(dir.path()).unwrap();
    let spec =
        ViewSpec::parse(":from tag:work :columns title,EFFORT :group EFFORT").expect("parse");
    let groups = spec.groups(&v);
    let total: usize = groups.iter().map(|(_, rows)| rows.len()).sum();
    assert_eq!(total, 2, "a row was dropped: {groups:?}");
}

#[test]
fn the_rendered_table_separates_the_groups() {
    let (_d, v) = vault();
    let spec = ViewSpec::parse(":from tag:work :columns title,todo :group todo").expect("parse");
    let table = closure_query::render_grouped_table(&spec.header(), &spec.groups(&v));
    // One header, and a rule between the groups as well as under the
    // header — still one org table.
    assert_eq!(
        table.matches("|---").count(),
        2,
        "expected a rule under the header and one between groups:\n{table}"
    );
    let beta = table.find("Beta").expect("Beta");
    let alpha = table.find("Alpha").expect("Alpha");
    assert!(beta < alpha, "DONE group should come first:\n{table}");
}

#[test]
fn a_name_that_is_not_a_builtin_column_groups_by_that_property() {
    // The same rule `:columns` already follows: anything that is not a
    // built-in field is a property key. Any property can be a column,
    // so any property can be a grouping — inventing an error here would
    // make `:group` stricter than `:columns` for no reason.
    let spec = ViewSpec::parse(":group EFFORT").expect("parse");
    assert_eq!(
        spec.group,
        Some(closure_query::Column::Property("EFFORT".to_owned()))
    );
}
