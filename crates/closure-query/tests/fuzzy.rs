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

// === orderless: whitespace splits the query into components ===
//
// Reported 2026-08-01: "when you try to filter for the add-sibling
// function, you have to type the -, in order to get a match. Just
// typing 'add sibling' won't match." A plain subsequence match needs
// the space to appear in the candidate, and `add-sibling` has a hyphen
// there. Doom's completion style is `orderless`: whitespace splits the
// query into components, each of which must match somewhere in the
// candidate, in any order.

use closure_query::orderless_score;

#[test]
fn a_space_no_longer_has_to_be_in_the_candidate() {
    assert!(
        orderless_score("add sibling", "add-sibling").is_some(),
        "the reported case"
    );
    assert!(orderless_score("add sibling", "add_sibling").is_some());
    assert!(orderless_score("move subtree up", "move-subtree-up").is_some());
}

#[test]
fn components_match_in_any_order() {
    // This is the whole point of the name: `orderless`.
    assert!(orderless_score("sibling add", "add-sibling").is_some());
    assert!(orderless_score("up subtree", "move-subtree-up").is_some());
}

#[test]
fn every_component_still_has_to_match() {
    assert!(orderless_score("add zzz", "add-sibling").is_none());
    assert!(orderless_score("sibling delete", "add-sibling").is_none());
}

#[test]
fn a_single_component_behaves_like_the_plain_matcher() {
    for (needle, hay) in [("fb", "foo_bar"), ("fzy", "fuzzy"), ("notes", "notes")] {
        assert_eq!(
            orderless_score(needle, hay).is_some(),
            fuzzy_score(needle, hay).is_some(),
            "{needle} vs {hay}"
        );
    }
    assert!(orderless_score("zq", "fuzzy").is_none());
}

#[test]
fn an_empty_or_blank_query_matches_everything() {
    assert!(orderless_score("", "anything").is_some());
    assert!(orderless_score("   ", "anything").is_some(), "only spaces");
}

#[test]
fn a_tighter_match_still_outranks_a_scattered_one() {
    // Ranking has to survive the split, or the most-wanted candidate
    // stops being the first one.
    let tight = orderless_score("add sib", "add-sibling");
    let loose = orderless_score("add sib", "a-d-d-s-i-b-x");
    assert!(tight.is_some() && loose.is_some(), "both match at all");
    assert!(tight > loose, "tight {tight:?} should beat loose {loose:?}");
}

#[test]
fn extra_whitespace_between_components_is_not_a_component() {
    assert!(orderless_score("add    sibling", "add-sibling").is_some());
    assert!(orderless_score("  add sibling  ", "add-sibling").is_some());
}
