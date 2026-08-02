//! "search/filter visualize matches … This image shows Doom Emacs and
//! the vertico autocompletion with orderless. Can we implement in all
//! of the filterable/searchable input fields with list items these kind
//! of highlighting."
//!
//! Vertico paints the characters your query matched, so a list of
//! near-identical candidates tells you *why* each one is in it. The
//! scorers already find those characters — [`orderless_score`] walks
//! them to decide whether a row survives — and then threw the positions
//! away. These hand them back.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_query::{match_spans, orderless_score};

#[test]
fn a_plain_prefix_marks_the_prefix() {
    assert_eq!(match_spans("ref", "reference"), vec![(0, 3)]);
}

#[test]
fn scattered_letters_come_back_as_separate_runs() {
    // `a-s` in `add-sibling`: the `a`, the `-`, the `s`.
    let spans = match_spans("a-s", "add-sibling");
    assert_eq!(spans, vec![(0, 1), (3, 5)], "{spans:?}");
}

#[test]
fn adjacent_hits_are_one_run_not_three() {
    // Three separate one-character spans would paint the same pixels
    // and cost the renderer three elements to do it.
    assert_eq!(match_spans("add", "add-sibling"), vec![(0, 3)]);
}

#[test]
fn every_component_of_an_orderless_query_is_marked() {
    // The point of orderless: `sibling add` matches, and both words
    // light up even though they are the wrong way round.
    let spans = match_spans("sibling add", "add-sibling");
    assert_eq!(spans, vec![(0, 3), (4, 11)], "{spans:?}");
}

#[test]
fn the_match_is_case_insensitive_like_the_score() {
    assert_eq!(match_spans("REF", "reference"), vec![(0, 3)]);
    assert_eq!(match_spans("ref", "REFERENCE"), vec![(0, 3)]);
}

#[test]
fn a_query_that_does_not_match_marks_nothing() {
    assert!(match_spans("zzz", "reference").is_empty());
    assert!(orderless_score("zzz", "reference").is_none());
}

#[test]
fn an_empty_query_marks_nothing() {
    // Everything matches an empty filter, so highlighting all of it
    // would say nothing at all.
    assert!(match_spans("", "reference").is_empty());
    assert!(match_spans("   ", "reference").is_empty());
}

#[test]
fn the_spans_are_byte_ranges_into_the_original_text() {
    // The shell slices the label with these, so they must be byte
    // offsets on char boundaries — an accent is two bytes and a slice
    // through the middle of one panics a repaint.
    let hay = "café refactor";
    let spans = match_spans("refactor", hay);
    for &(start, end) in &spans {
        assert!(hay.is_char_boundary(start), "{start} in {hay:?}");
        assert!(hay.is_char_boundary(end), "{end} in {hay:?}");
    }
    let marked: String = spans.iter().map(|&(s, e)| &hay[s..e]).collect();
    assert_eq!(marked, "refactor");
}

#[test]
fn a_multibyte_haystack_marks_the_right_characters() {
    let hay = "über alles";
    let spans = match_spans("alles", hay);
    let marked: String = spans.iter().map(|&(s, e)| &hay[s..e]).collect();
    assert_eq!(marked, "alles");
}

#[test]
fn spans_never_overlap_and_always_ascend() {
    // Two components can match the same run; the shell paints spans in
    // order and would double-draw or slice backwards otherwise.
    let spans = match_spans("add add", "add-sibling");
    let mut last = 0;
    for &(start, end) in &spans {
        assert!(start >= last, "{spans:?}");
        assert!(end > start, "{spans:?}");
        last = end;
    }
}

#[test]
fn anything_that_scores_is_marked_and_the_reverse() {
    // The list shows what the scorer kept, so the two must agree about
    // what a match is — a row with no highlight would look like a bug.
    for (needle, hay) in [
        ("ref", "reference"),
        ("a-s", "add-sibling"),
        ("sibling add", "add-sibling"),
        ("zzz", "reference"),
        ("xyz", "add-sibling"),
    ] {
        let scored = orderless_score(needle, hay).is_some();
        let marked = !match_spans(needle, hay).is_empty();
        assert_eq!(scored, marked, "{needle:?} vs {hay:?}");
    }
}
