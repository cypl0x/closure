//! Unit tests for the fuzzy matcher.
//!
//! Invariants pinned here are *relative* (ordering between candidates),
//! not absolute score values, so the scoring function can be tuned
//! without rewriting the suite.

use closure_query::{fuzzy_filter, fuzzy_score};

#[test]
fn empty_needle_matches_everything() {
    assert!(fuzzy_score("", "anything").is_some());
    assert!(fuzzy_score("", "").is_some());
}

#[test]
fn exact_match_scores() {
    assert!(fuzzy_score("notes", "notes").is_some());
}

#[test]
fn subsequence_matches() {
    assert!(fuzzy_score("fb", "foo_bar").is_some());
    assert!(fuzzy_score("fzy", "fuzzy").is_some());
}

#[test]
fn non_subsequence_is_none() {
    assert!(fuzzy_score("fbx", "foo_bar").is_none());
    assert!(fuzzy_score("ba", "ab").is_none());
}

#[test]
fn needle_longer_than_hay_is_none() {
    assert!(fuzzy_score("abcdef", "abc").is_none());
}

#[test]
fn match_is_case_insensitive() {
    assert!(fuzzy_score("FB", "foo_bar").is_some());
    assert!(fuzzy_score("fb", "FOO_BAR").is_some());
}

#[test]
fn contiguous_beats_scattered() {
    let tight = fuzzy_score("bar", "bar");
    let spread = fuzzy_score("bar", "b_a_r");
    assert!(tight > spread, "tight={tight:?} spread={spread:?}");
}

#[test]
fn earlier_start_beats_later_start() {
    let early = fuzzy_score("foo", "foo_x");
    let late = fuzzy_score("foo", "x_foo");
    assert!(early > late, "early={early:?} late={late:?}");
}

#[test]
fn filter_returns_only_matches_sorted_best_first() {
    let items = vec!["b_a_r", "bar", "zzz", "xbar"];
    let got = fuzzy_filter("bar", &items);
    let names: Vec<&str> = got.iter().map(|(s, _)| *s).collect();
    assert_eq!(names, vec!["bar", "xbar", "b_a_r"]);
}

#[test]
fn filter_empty_needle_keeps_input_order() {
    let items = vec!["c", "a", "b"];
    let got = fuzzy_filter("", &items);
    let names: Vec<&str> = got.iter().map(|(s, _)| *s).collect();
    assert_eq!(names, vec!["c", "a", "b"]);
}

#[test]
fn filter_no_matches_is_empty() {
    let items = vec!["a", "b"];
    assert!(fuzzy_filter("zzz", &items).is_empty());
}
