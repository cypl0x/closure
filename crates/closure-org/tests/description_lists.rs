//! `- term :: definition` — org's description list.
//!
//! Read as an ordinary list item, so the term was not a term: nothing
//! could render it as one, sort by it, or find a definition by its
//! word. In a personal wiki that is the shape a glossary has, which
//! makes it a worse omission here than the syntax suggests.
//!
//! The separator is ` :: ` with spaces around it, which is what keeps
//! this from firing on prose. `C++ :: a language` is a description;
//! `see foo::bar` is a Rust path in a sentence, and a rule that read
//! the second as a term would put half of every code note in a glossary.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::description_items;

#[test]
fn a_term_and_its_definition_are_separated() {
    let got = description_items("- closure :: the program\n");
    assert_eq!(got, vec![("closure".to_owned(), "the program".to_owned())]);
}

#[test]
fn several_in_one_list() {
    let src = "- alpha :: the first\n- beta :: the second\n";
    assert_eq!(description_items(src).len(), 2);
}

#[test]
fn an_ordinary_item_is_not_a_description() {
    assert!(description_items("- just an item\n").is_empty());
    assert!(description_items("- 1. not this either\n").is_empty());
}

#[test]
fn a_double_colon_inside_a_word_is_not_a_separator() {
    // The case the feature lives or dies on, and a vault is full of
    // them: `std::collections`, `foo::bar`, a C++ note. Without the
    // spaces rule this puts half of every code note in a glossary.
    assert!(description_items("- see std::collections for the rest\n").is_empty());
    assert!(description_items("- foo::bar and foo::baz\n").is_empty());
}

#[test]
fn a_term_may_contain_a_double_colon_itself() {
    // `C++ :: a language` — the separator is the one with spaces, and
    // the first such separator wins.
    let got = description_items("- std::vec :: the growable array\n");
    assert_eq!(
        got,
        vec![("std::vec".to_owned(), "the growable array".to_owned())]
    );
}

#[test]
fn the_definition_may_contain_one_too() {
    let got = description_items("- path :: written std::path::Path\n");
    assert_eq!(got[0].1, "written std::path::Path");
}

#[test]
fn an_indented_item_counts() {
    let got = description_items("  - nested :: still a description\n");
    assert_eq!(got.len(), 1, "{got:?}");
}

#[test]
fn every_bullet_org_allows() {
    for bullet in ["-", "+", "*"] {
        let src = format!("{bullet} term :: definition\n");
        assert_eq!(description_items(&src).len(), 1, "{bullet}");
    }
}

#[test]
fn a_term_with_no_definition_is_not_one() {
    // `- term ::` with nothing after it is a half-written line, and
    // guessing an empty definition would put it in a glossary as a
    // word that means nothing.
    assert!(description_items("- term ::\n").is_empty());
}

#[test]
fn prose_that_is_not_a_list_is_not_a_list() {
    assert!(description_items("closure :: the program\n").is_empty());
}
