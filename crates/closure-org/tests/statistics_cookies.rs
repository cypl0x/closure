//! `[1/3]` and `[33%]` — a headline counting its children.
//!
//! A cookie is text in a title, so today the number is whatever
//! somebody last typed. That is worse than not having one: an absent
//! count says nothing and a stale count says something false, and it
//! looks maintained either way.
//!
//! Counting is this file. Keeping it true when a child's keyword
//! changes is a mutation, so it is a command and its own item — the
//! split every construct here has landed on.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::{Cookie, cookie_in, parse, statistics_for};

const DOC: &str = "\
* Project [1/3]
:PROPERTIES:
:ID: 01COOKIE000000000000001
:END:
** DONE First
** TODO Second
** TODO Third
* Percent [0%]
:PROPERTIES:
:ID: 01COOKIE000000000000002
:END:
** TODO Only
* Plain title
:PROPERTIES:
:ID: 01COOKIE000000000000003
:END:
";

#[test]
fn a_fraction_cookie_is_recognised() {
    assert_eq!(cookie_in("Project [1/3]"), Some(Cookie::Fraction));
}

#[test]
fn a_percent_cookie_is_recognised() {
    assert_eq!(cookie_in("Percent [0%]"), Some(Cookie::Percent));
}

#[test]
fn a_title_without_one_has_none() {
    assert_eq!(cookie_in("Plain title"), None);
    // A priority is not a cookie, and both live in square brackets.
    assert_eq!(cookie_in("[#A] Important"), None);
}

#[test]
fn the_count_is_of_the_children() {
    let doc = parse(DOC).expect("parse");
    let (done, total) = statistics_for(&doc, "01COOKIE000000000000001").expect("counts");
    assert_eq!((done, total), (1, 3));
}

#[test]
fn a_headline_with_no_children_counts_nothing() {
    let doc = parse(DOC).expect("parse");
    assert_eq!(
        statistics_for(&doc, "01COOKIE000000000000003"),
        Some((0, 0))
    );
}

#[test]
fn only_children_with_a_keyword_are_counted() {
    // A sub-heading that is not a task is structure, not a task that
    // has not been done — counting it would make every project look
    // behind.
    let src = "* P [0/1]\n:PROPERTIES:\n:ID: 01COOKIE000000000000004\n:END:\n\
               ** TODO A task\n** Just a note\n";
    let doc = parse(src).expect("parse");
    assert_eq!(
        statistics_for(&doc, "01COOKIE000000000000004"),
        Some((0, 1))
    );
}

#[test]
fn the_rendered_cookie_says_what_the_children_say() {
    assert_eq!(closure_org::render_cookie(Cookie::Fraction, 1, 3), "[1/3]");
    assert_eq!(closure_org::render_cookie(Cookie::Percent, 1, 3), "[33%]");
    // No children is not a division; org writes it as zero.
    assert_eq!(closure_org::render_cookie(Cookie::Percent, 0, 0), "[0%]");
    assert_eq!(closure_org::render_cookie(Cookie::Fraction, 0, 0), "[0/0]");
}
