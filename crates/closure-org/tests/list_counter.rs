//! `1. [@3] third` — a list that says where its numbering starts.
//!
//! Org's counter override, for the case a numbered list is interrupted:
//! prose, a code block, a table, and then the list resumes at four
//! rather than starting again at one. Preserved and never read, so a
//! list that said it starts at three was numbered from one and the
//! author's third step was labelled first.
//!
//! It is only a *starting* number. Org does not renumber the rest — the
//! markers in the file are what they are — so this reports where a list
//! claims to begin and nothing rewrites anything (I1).

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::list_counter;

#[test]
fn a_counter_is_read() {
    assert_eq!(list_counter("1. [@3] third step"), Some(3));
}

#[test]
fn the_paren_marker_too() {
    assert_eq!(list_counter("1) [@7] seventh"), Some(7));
}

#[test]
fn an_indented_item_counts() {
    assert_eq!(list_counter("   4. [@10] ten"), Some(10));
}

#[test]
fn an_ordinary_item_has_none() {
    assert_eq!(list_counter("1. just an item"), None);
    assert_eq!(list_counter("- a bullet"), None);
}

#[test]
fn a_bullet_cannot_carry_a_counter() {
    // `[@3]` on an unordered item is meaningless — there is no number
    // to override — and reading it would invent an ordering the author
    // did not ask for.
    assert_eq!(list_counter("- [@3] not a number"), None);
}

#[test]
fn a_checkbox_is_not_a_counter() {
    // The case that decides whether this is safe: `[ ]`, `[X]` and `[-]`
    // sit in exactly the same position, and a vault is full of them.
    assert_eq!(list_counter("1. [ ] unchecked"), None);
    assert_eq!(list_counter("1. [X] done"), None);
    assert_eq!(list_counter("1. [-] partial"), None);
}

#[test]
fn a_counter_may_sit_before_a_checkbox() {
    // Org allows both, in this order, and a numbered checklist that
    // resumes is exactly where somebody would write it.
    assert_eq!(list_counter("1. [@3] [ ] third task"), Some(3));
}

#[test]
fn a_malformed_counter_is_not_one() {
    // I5: no panic, no guess.
    assert_eq!(list_counter("1. [@] nothing"), None);
    assert_eq!(list_counter("1. [@abc] letters"), None);
    assert_eq!(list_counter("1. [@3 unclosed"), None);
}

#[test]
fn prose_that_is_not_a_list_item_is_not_one() {
    assert_eq!(list_counter("see [@3] in the spec"), None);
}
