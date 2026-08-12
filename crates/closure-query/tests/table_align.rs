//! A rendered table honours `|<r>|`.
//!
//! `render_table` left-aligned everything, so a column of durations or
//! amounts came out as a ragged wall of digits — the exact thing org's
//! alignment row exists to prevent.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::Align;
use closure_query::render_table_aligned;

fn head() -> Vec<String> {
    vec!["Task".to_owned(), "Time".to_owned()]
}

fn rows() -> Vec<Vec<String>> {
    vec![
        vec!["Writing".to_owned(), "1:30".to_owned()],
        vec!["A much longer task".to_owned(), "12:05".to_owned()],
    ]
}

#[test]
fn a_right_aligned_column_is_right_aligned() {
    let out = render_table_aligned(&head(), &rows(), &[Align::Left, Align::Right]);
    // The short value is padded on its left, so the two line up on
    // their right edge.
    assert!(out.contains("|  1:30 |"), "{out}");
    assert!(out.contains("| 12:05 |"), "{out}");
}

#[test]
fn a_centred_column_is_centred() {
    let out = render_table_aligned(&head(), &rows(), &[Align::Left, Align::Centre]);
    assert!(
        out.contains("| 1:30  |") || out.contains("|  1:30 |"),
        "{out}"
    );
}

#[test]
fn no_alignments_is_what_it_always_was() {
    // The old behaviour is the default, so every existing caller and
    // every existing golden file keeps its answer.
    let a = render_table_aligned(&head(), &rows(), &[]);
    let b = closure_query::render_table(&head(), &rows());
    assert_eq!(a, b);
}

#[test]
fn fewer_alignments_than_columns_leaves_the_rest_alone() {
    let out = render_table_aligned(&head(), &rows(), &[Align::Right]);
    assert!(out.contains("| 1:30  |"), "{out}");
}

#[test]
fn the_separator_row_still_spans_every_column() {
    let out = render_table_aligned(&head(), &rows(), &[Align::Left, Align::Right]);
    let sep = out.lines().nth(1).expect("a separator row");
    assert!(sep.starts_with('|') && sep.ends_with('|'), "{sep}");
    assert!(sep.contains('+'), "{sep}");
}
