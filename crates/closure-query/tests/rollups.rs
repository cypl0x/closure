//! Rollups: a column that aggregates over what points at this row.
//!
//! A relation reads forwards — a task naming its project. A rollup
//! reads the same edge backwards and does arithmetic on it: the
//! project's row showing how much effort its tasks add up to, or how
//! many there are. Without one, a relation gives a board and no
//! totals, which is the half of a Notion database people actually
//! stare at.
//!
//! `rollup:PROJECT.EFFORT:sum` — the relation to follow back, the
//! property to read on the rows that come back, and what to do with
//! them. Four aggregates, and `count` ignores the property because
//! counting rows does not need one.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_query::ViewSpec;
use closure_store::Vault;
use tempfile::TempDir;

const VAULT: &str = "\
* Rewrite the parser :project:
:PROPERTIES:
:ID: 01ROLLPROJ0000000000001
:END:
* Something nobody works on :project:
:PROPERTIES:
:ID: 01ROLLPROJ0000000000002
:END:
* TODO Read the spec :task:
:PROPERTIES:
:ID: 01ROLLTASK0000000000001
:PROJECT: 01ROLLPROJ0000000000001
:EFFORT: 3
:END:
* TODO Write the tests :task:
:PROPERTIES:
:ID: 01ROLLTASK0000000000002
:PROJECT: 01ROLLPROJ0000000000001
:EFFORT: 5
:END:
";

fn vault() -> (TempDir, Vault) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), VAULT).unwrap();
    let v = Vault::open(dir.path()).unwrap();
    (dir, v)
}

/// The cell in the rollup column for the row titled `title`.
fn cell(v: &Vault, spec: &str, title: &str) -> String {
    let spec = ViewSpec::parse(spec).expect("parse");
    let cells = spec.cells(v);
    cells
        .into_iter()
        .find(|r| r[0] == title)
        .map(|r| r[1].clone())
        .expect("a row with that title")
}

#[test]
fn sum_adds_the_property_across_what_points_here() {
    let (_d, v) = vault();
    let got = cell(
        &v,
        ":from tag:project :columns title,rollup:PROJECT.EFFORT:sum",
        "Rewrite the parser",
    );
    assert_eq!(got, "8");
}

#[test]
fn count_counts_the_rows_and_ignores_the_property() {
    let (_d, v) = vault();
    let got = cell(
        &v,
        ":from tag:project :columns title,rollup:PROJECT.EFFORT:count",
        "Rewrite the parser",
    );
    assert_eq!(got, "2");
}

#[test]
fn min_and_max_read_the_ends() {
    let (_d, v) = vault();
    assert_eq!(
        cell(
            &v,
            ":from tag:project :columns title,rollup:PROJECT.EFFORT:min",
            "Rewrite the parser"
        ),
        "3"
    );
    assert_eq!(
        cell(
            &v,
            ":from tag:project :columns title,rollup:PROJECT.EFFORT:max",
            "Rewrite the parser"
        ),
        "5"
    );
}

#[test]
fn a_row_nothing_points_at_sums_to_zero_and_counts_zero() {
    // Not blank: a project with no tasks has an effort of zero, and
    // that is a different statement from "no answer".
    let (_d, v) = vault();
    assert_eq!(
        cell(
            &v,
            ":from tag:project :columns title,rollup:PROJECT.EFFORT:sum",
            "Something nobody works on"
        ),
        "0"
    );
    assert_eq!(
        cell(
            &v,
            ":from tag:project :columns title,rollup:PROJECT.EFFORT:count",
            "Something nobody works on"
        ),
        "0"
    );
}

#[test]
fn min_of_nothing_is_blank_rather_than_zero() {
    // The smallest of no numbers is not zero, and saying zero would be
    // a wrong answer rather than a missing one.
    let (_d, v) = vault();
    assert_eq!(
        cell(
            &v,
            ":from tag:project :columns title,rollup:PROJECT.EFFORT:min",
            "Something nobody works on"
        ),
        ""
    );
}

#[test]
fn a_non_numeric_value_is_skipped_rather_than_poisoning_the_total() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("n.org"),
        "* P :project:\n:PROPERTIES:\n:ID: 01ROLLPROJ0000000000003\n:END:\n\
         * A :task:\n:PROPERTIES:\n:ID: 01ROLLTASK0000000000003\n\
         :PROJECT: 01ROLLPROJ0000000000003\n:EFFORT: 2\n:END:\n\
         * B :task:\n:PROPERTIES:\n:ID: 01ROLLTASK0000000000004\n\
         :PROJECT: 01ROLLPROJ0000000000003\n:EFFORT: soon\n:END:\n",
    )
    .unwrap();
    let v = Vault::open(dir.path()).unwrap();
    assert_eq!(
        cell(
            &v,
            ":from tag:project :columns title,rollup:PROJECT.EFFORT:sum",
            "P"
        ),
        "2"
    );
}

#[test]
fn the_header_says_what_it_rolls_up() {
    let spec = ViewSpec::parse(":columns title,rollup:PROJECT.EFFORT:sum").expect("parse");
    assert_eq!(spec.header()[1], "sum(EFFORT)");
}

#[test]
fn an_unknown_aggregate_falls_back_to_count() {
    // Same shape as an unknown column being a property: a name nobody
    // implemented should not make a document unreadable.
    let spec = ViewSpec::parse(":columns title,rollup:PROJECT.EFFORT:median").expect("parse");
    assert_eq!(spec.header()[1], "count(EFFORT)");
}
