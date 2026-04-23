//! AST shape tests for headline parsing.
//!
//! These assert structural properties of the parse tree that the
//! roundtrip test cannot: that a line starting with `* ` yields a
//! [`Headline`] in `roots`, not a paragraph in `preamble`. Byte-exact
//! roundtrip is checked in `roundtrip.rs`; this file checks semantic
//! classification.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_org::{NodeKind, parse};

#[test]
fn simple_heading_is_a_headline_not_a_paragraph() {
    let doc = parse("* Hello\n").expect("parse");
    assert!(
        doc.preamble().is_empty(),
        "heading must not land in preamble"
    );
    assert_eq!(doc.roots().len(), 1);
    let h = &doc.roots()[0];
    assert_eq!(h.level(), 1);
    assert_eq!(h.title(), "Hello");
}

#[test]
fn sibling_headings_all_become_roots() {
    let doc = parse("* First\n* Second\n* Third\n").expect("parse");
    assert_eq!(doc.roots().len(), 3);
    assert_eq!(doc.roots()[0].title(), "First");
    assert_eq!(doc.roots()[1].title(), "Second");
    assert_eq!(doc.roots()[2].title(), "Third");
}

#[test]
fn empty_title_heading_stars_plus_newline() {
    let doc = parse("*\n").expect("parse");
    assert_eq!(doc.roots().len(), 1);
    assert_eq!(doc.roots()[0].level(), 1);
    assert_eq!(doc.roots()[0].title(), "");
}

#[test]
fn empty_title_level_two() {
    let doc = parse("**\n").expect("parse");
    assert_eq!(doc.roots().len(), 1);
    assert_eq!(doc.roots()[0].level(), 2);
    assert_eq!(doc.roots()[0].title(), "");
}

#[test]
fn stars_followed_by_non_space_non_eol_is_paragraph() {
    let doc = parse("**foo\n").expect("parse");
    assert!(doc.roots().is_empty());
    assert_eq!(doc.preamble().len(), 1);
    assert_eq!(doc.preamble()[0].kind(), NodeKind::Paragraph);
}

#[test]
fn preamble_before_first_heading_preserved() {
    let doc = parse("#+TITLE: x\n\n* Heading\n").expect("parse");
    assert_eq!(doc.preamble().len(), 2);
    assert_eq!(doc.preamble()[0].kind(), NodeKind::Keyword);
    assert_eq!(doc.preamble()[1].kind(), NodeKind::BlankLine);
    assert_eq!(doc.roots().len(), 1);
    assert_eq!(doc.roots()[0].title(), "Heading");
}

#[test]
fn body_lines_after_heading_become_headline_body() {
    let doc = parse("* Hello\nbody line\n").expect("parse");
    assert_eq!(doc.roots().len(), 1);
    let h = &doc.roots()[0];
    assert_eq!(h.title(), "Hello");
    assert_eq!(h.body().len(), 1);
    assert_eq!(h.body()[0].kind(), NodeKind::Paragraph);
}

#[test]
fn headline_title_excludes_stars_and_leading_space() {
    let doc = parse("*** Three stars title\n").expect("parse");
    assert_eq!(doc.roots().len(), 1);
    let h = &doc.roots()[0];
    assert_eq!(h.level(), 3);
    assert_eq!(h.title(), "Three stars title");
}
