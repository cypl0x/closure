//! Relations: a column whose value is another headline.
//!
//! "The thing that makes a Notion database more than a saved search."
//!
//! A property can already hold anything, so a task could always carry
//! `:PROJECT: 01ABC…`. What it could not do is *mean* anything: the
//! column showed a ULID, which is the one string in the vault a reader
//! can do nothing with. A relation column shows the title of the
//! headline it points at, and — because it is a link and not a copy —
//! it keeps pointing at the right thing when that title changes.
//!
//! `rel:PROJECT` rather than a new `:relation` directive, because
//! `:columns` already answers "what does this name mean" and adding a
//! second place to say it is how the two get out of step.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_query::{Column, ViewSpec};
use closure_store::Vault;
use tempfile::TempDir;

const VAULT: &str = "\
* Rewrite the parser
:PROPERTIES:
:ID: 01RELPROJECT00000000001
:END:
* TODO Read the spec :task:
:PROPERTIES:
:ID: 01RELTASK000000000000001
:PROJECT: 01RELPROJECT00000000001
:EFFORT: 3
:END:
* TODO Write the tests :task:
:PROPERTIES:
:ID: 01RELTASK000000000000002
:PROJECT: 01RELPROJECT00000000001
:EFFORT: 5
:END:
* TODO Something else :task:
:PROPERTIES:
:ID: 01RELTASK000000000000003
:END:
";

fn vault() -> (TempDir, Vault) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), VAULT).unwrap();
    let v = Vault::open(dir.path()).unwrap();
    (dir, v)
}

#[test]
fn a_relation_column_is_parsed() {
    let spec = ViewSpec::parse(":columns title,rel:PROJECT").expect("parse");
    assert_eq!(spec.columns[1], Column::Relation("PROJECT".to_owned()));
}

#[test]
fn it_shows_the_title_of_what_it_points_at() {
    let (_d, v) = vault();
    let spec = ViewSpec::parse(":from tag:task :columns title,rel:PROJECT").expect("parse");
    let cells = spec.cells(&v);
    let project: Vec<&str> = cells.iter().map(|r| r[1].as_str()).collect();
    assert!(
        project.contains(&"Rewrite the parser"),
        "showed the id instead of the headline: {project:?}"
    );
    assert!(
        !project.iter().any(|c| c.contains("01RELPROJECT")),
        "a ULID reached the table: {project:?}"
    );
}

#[test]
fn a_row_with_no_relation_shows_nothing_rather_than_a_dash() {
    let (_d, v) = vault();
    let spec = ViewSpec::parse(":from tag:task :columns title,rel:PROJECT").expect("parse");
    let cells = spec.cells(&v);
    let empty = cells
        .iter()
        .find(|r| r[0] == "Something else")
        .expect("the unrelated task");
    assert_eq!(empty[1], "");
}

#[test]
fn a_relation_that_points_nowhere_says_so() {
    // A dangling id is a mistake in the vault, and rendering it as
    // blank would hide it. The reader needs to know the difference
    // between "no project" and "a project that is gone".
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("n.org"),
        "* TODO Orphan :task:\n:PROPERTIES:\n:ID: 01RELTASK000000000000004\n\
         :PROJECT: 01RELGONE0000000000000A\n:END:\n",
    )
    .unwrap();
    let v = Vault::open(dir.path()).unwrap();
    let spec = ViewSpec::parse(":from tag:task :columns title,rel:PROJECT").expect("parse");
    let cells = spec.cells(&v);
    assert!(
        cells[0][1].contains('?'),
        "a dangling relation looked like no relation: {:?}",
        cells[0][1]
    );
}

#[test]
fn the_header_names_the_relation_not_the_syntax() {
    let spec = ViewSpec::parse(":columns title,rel:PROJECT").expect("parse");
    assert_eq!(
        spec.header(),
        vec!["title".to_owned(), "PROJECT".to_owned()]
    );
}

#[test]
fn a_relation_can_be_grouped_by() {
    // Which is the point of having one: a board of tasks by project.
    let (_d, v) = vault();
    let spec = ViewSpec::parse(":from tag:task :columns title :group rel:PROJECT").expect("parse");
    let groups = spec.groups(&v);
    let names: Vec<&str> = groups.iter().map(|(k, _)| k.as_str()).collect();
    assert!(
        names.contains(&"Rewrite the parser"),
        "grouped by the raw id rather than the headline: {names:?}"
    );
}

#[test]
fn the_link_survives_a_rename() {
    // A relation is a link, not a copy. This is the property that
    // makes it worth more than writing the project's name in a
    // property by hand.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("n.org"), VAULT).unwrap();
    let renamed = VAULT.replace("* Rewrite the parser", "* Rewrite the parser (v2)");
    std::fs::write(dir.path().join("n.org"), renamed).unwrap();
    let v = Vault::open(dir.path()).unwrap();
    let spec = ViewSpec::parse(":from tag:task :columns title,rel:PROJECT").expect("parse");
    let cells = spec.cells(&v);
    assert!(
        cells.iter().any(|r| r[1] == "Rewrite the parser (v2)"),
        "the relation did not follow the rename: {cells:?}"
    );
}
