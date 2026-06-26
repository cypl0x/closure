//! D9: org tables are queryable structure (rows + cells) *anywhere* — in
//! headline bodies, not only the preamble. Recognition is over the existing
//! `TableRow` nodes, so the byte-exact roundtrip (I1) is untouched. Includes
//! a proptest that `tables()` never panics on random input (I5).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_org::{parse, print};
use proptest::prelude::*;

const SRC: &str = "* Roster\n\
                   | name | role |\n\
                   |------+------|\n\
                   | ada  | dev  |\n\
                   | bob  | ops  |\n\
                   \n\
                   after\n";

#[test]
fn tables_in_a_headline_body_are_recognised() {
    let doc = parse(SRC).expect("parse");
    let tables = doc.tables();
    assert_eq!(
        tables.len(),
        1,
        "the body table is found (not preamble-only)"
    );
    let t = &tables[0];
    assert_eq!(t.rows.len(), 4, "header + separator + 2 data rows");
    assert!(t.rows[1].is_separator, "the |---+---| row is a separator");

    // data_rows skips separators and yields the trimmed cells.
    let data: Vec<Vec<&str>> = t.data_rows().map(<[&str]>::to_vec).collect();
    assert_eq!(
        data,
        vec![vec!["name", "role"], vec!["ada", "dev"], vec!["bob", "ops"],]
    );
}

#[test]
fn table_recognition_preserves_byte_exact_roundtrip() {
    let doc = parse(SRC).expect("parse");
    assert_eq!(print(&doc), SRC, "I1 holds — recognition does not rewrite");
}

#[test]
fn preamble_and_body_tables_are_both_found() {
    let src = "| pre | amble |\n\n* H\n| in | body |\n";
    let doc = parse(src).expect("parse");
    assert_eq!(doc.tables().len(), 2, "one in preamble, one in a body");
}

#[test]
fn no_table_without_pipe_rows() {
    let doc = parse("* H\nplain body\n").expect("parse");
    assert!(doc.tables().is_empty());
}

proptest! {
    #[test]
    fn tables_never_panics_on_random_input(s in ".{0,200}") {
        let doc = parse(&s).expect("parser is total");
        for t in doc.tables() {
            let _ = t.data_rows().count(); // must not panic (I5)
        }
    }
}
