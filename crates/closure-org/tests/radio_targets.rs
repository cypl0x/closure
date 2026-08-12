//! Radio targets: `<<<name>>>` makes later mentions of "name" links.
//!
//! The last construct the matrix still only preserved, and the only one
//! whose cost is per *word* rather than per line: every occurrence of
//! every target in every body has to be found. That is why it is worth
//! writing down what it costs beside what it does — I11 is a rule about
//! the kernel, and a feature that is right and quadratic is still
//! wrong.
//!
//! Matching is org's: case-insensitive, on whole words, and never
//! inside the target's own definition. The last of those is not
//! pedantry — a definition that matched itself would link a word to the
//! place it already is.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::{radio_matches, radio_targets};

const DOC: &str = "\
* Glossary
:PROPERTIES:
:ID: 01RADIO0000000000000001
:END:
<<<closure>>> is the program. <<<org mode>>> is the format.
* Notes
:PROPERTIES:
:ID: 01RADIO0000000000000002
:END:
closure reads Org Mode files, and enclosure does not count.
";

#[test]
fn a_definition_is_found() {
    let mut got = radio_targets(DOC);
    got.sort();
    assert_eq!(got, vec!["closure".to_owned(), "org mode".to_owned()]);
}

#[test]
fn a_later_mention_matches() {
    let hits = radio_matches("closure reads files", &["closure".to_owned()]);
    assert_eq!(hits.len(), 1);
    assert_eq!(&"closure reads files"[hits[0].clone()], "closure");
}

#[test]
fn matching_ignores_case_the_way_org_does() {
    let hits = radio_matches("Org Mode is a format", &["org mode".to_owned()]);
    assert_eq!(hits.len(), 1, "{hits:?}");
}

#[test]
fn a_word_that_merely_contains_the_target_does_not_match() {
    // "enclosure" contains "closure" and is a different word. A
    // substring match here would link half the vault to the glossary.
    let hits = radio_matches("enclosure and disclosure", &["closure".to_owned()]);
    assert!(hits.is_empty(), "{hits:?}");
}

#[test]
fn the_definition_itself_is_not_a_match() {
    // Otherwise a target links the word to where it already is.
    let hits = radio_matches("<<<closure>>> is the program", &["closure".to_owned()]);
    assert!(hits.is_empty(), "{hits:?}");
}

#[test]
fn several_targets_in_one_line() {
    let targets = vec!["closure".to_owned(), "org mode".to_owned()];
    let hits = radio_matches("closure reads org mode", &targets);
    assert_eq!(hits.len(), 2, "{hits:?}");
}

#[test]
fn a_document_with_no_targets_costs_nothing() {
    assert!(radio_targets("* Just a headline\nbody\n").is_empty());
    assert!(radio_matches("any text at all", &[]).is_empty());
}

#[allow(clippy::cast_precision_loss)]
#[test]
fn scanning_is_linear_in_the_text() {
    // I11 in the small: this is the one construct whose work is per
    // word, so four times the text must not cost four times as much
    // *per target*. A naive implementation rescans the text once per
    // target and is quadratic the moment a glossary grows.
    use std::time::Instant;
    let targets: Vec<String> = (0..64).map(|i| format!("term{i}")).collect();
    let small = "some ordinary prose about nothing in particular ".repeat(200);
    let large = "some ordinary prose about nothing in particular ".repeat(800);
    let _ = radio_matches(&small, &targets);
    let t = Instant::now();
    let _ = radio_matches(&small, &targets);
    let a = t.elapsed();
    let t = Instant::now();
    let _ = radio_matches(&large, &targets);
    let b = t.elapsed();
    let growth = b.as_nanos() as f64 / a.as_nanos().max(1) as f64;
    assert!(
        growth < 8.0,
        "four times the text took {growth:.1} times as long ({a:?} -> {b:?})"
    );
}
