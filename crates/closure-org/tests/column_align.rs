//! `|<r>|<l>|<c>|` — org's column alignment row.
//!
//! Kept and never applied, so every rendered table was left-aligned
//! whatever it asked for. In a table of durations or amounts that is
//! the difference between a column you can scan and a ragged wall of
//! digits, which is the whole reason org has the row.
//!
//! The row is *not* data. It is a directive that happens to be shaped
//! like a table row, so anything that renders a table has to read it
//! and then not print it — printing it back is what closure did, which
//! at least meant the file roundtripped (I1).

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::{Align, column_alignments};

#[test]
fn the_three_alignments_are_read() {
    let got = column_alignments("|<r>|<l>|<c>|").expect("an alignment row");
    assert_eq!(got, vec![Align::Right, Align::Left, Align::Centre]);
}

#[test]
fn a_width_cookie_carries_an_alignment_too() {
    // `<r10>` is org's "right-aligned, 10 wide". The width is a
    // display hint closure has no use for; the alignment is not.
    let got = column_alignments("|<r10>|<l5>|").expect("an alignment row");
    assert_eq!(got, vec![Align::Right, Align::Left]);
}

#[test]
fn a_column_that_says_nothing_is_left() {
    let got = column_alignments("|<r>||<c>|").expect("an alignment row");
    assert_eq!(got, vec![Align::Right, Align::Left, Align::Centre]);
}

#[test]
fn an_ordinary_row_is_not_an_alignment_row() {
    // The one that matters: a data row read as a directive would make
    // the table's first line vanish.
    assert!(column_alignments("| Name | Time |").is_none());
    assert!(column_alignments("|------+------|").is_none());
    assert!(column_alignments("not a row at all").is_none());
}

#[test]
fn a_row_of_something_that_merely_looks_like_a_cookie_is_not_one() {
    assert!(column_alignments("| <not> | <a cookie> |").is_none());
    assert!(column_alignments("|<x>|").is_none());
}

#[test]
fn whitespace_around_the_cookies_is_allowed() {
    let got = column_alignments("|  <r>  |  <c>  |").expect("an alignment row");
    assert_eq!(got, vec![Align::Right, Align::Centre]);
}
