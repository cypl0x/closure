//! `#+LINK: gh https://github.com/%s` — a prefix that stands for a URL.
//!
//! Org's way of not typing the same host forty times: declare `gh` once
//! and write `[[gh:cypl0x/closure]]`. Preserved and never expanded, so
//! the abbreviation was a link to a scheme no browser has and the
//! feature cost the author the thing it was meant to save.
//!
//! Two substitution forms, both org's. `%s` puts the tail where it is
//! written; a template with no `%s` gets the tail appended. The second
//! is the common case (`#+LINK: gh https://github.com/`) and the first
//! is what you need the moment the tail is not last —
//! `https://example.com/%s/edit`.
//!
//! Expansion is a reading (I1/I12): the file keeps `[[gh:...]]`, which
//! is the whole point of writing it that way.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::{expand_link, link_abbreviations};

const DOC: &str = "\
#+LINK: gh https://github.com/%s
#+LINK: wiki https://en.wikipedia.org/wiki/
#+LINK: edit https://example.com/%s/edit

* Notes
:PROPERTIES:
:ID: 01LINKABBREV00000001
:END:
";

#[test]
fn the_declarations_are_read() {
    let abbrevs = link_abbreviations(DOC);
    assert_eq!(abbrevs.len(), 3, "{abbrevs:?}");
    assert_eq!(
        abbrevs.get("gh").map(String::as_str),
        Some("https://github.com/%s")
    );
}

#[test]
fn a_placeholder_is_where_the_tail_goes() {
    let abbrevs = link_abbreviations(DOC);
    assert_eq!(
        expand_link("gh:cypl0x/closure", &abbrevs).as_deref(),
        Some("https://github.com/cypl0x/closure")
    );
}

#[test]
fn a_template_with_no_placeholder_appends() {
    // The common shape, and the one somebody writes first.
    let abbrevs = link_abbreviations(DOC);
    assert_eq!(
        expand_link("wiki:Org-mode", &abbrevs).as_deref(),
        Some("https://en.wikipedia.org/wiki/Org-mode")
    );
}

#[test]
fn the_tail_need_not_be_last() {
    let abbrevs = link_abbreviations(DOC);
    assert_eq!(
        expand_link("edit:page-42", &abbrevs).as_deref(),
        Some("https://example.com/page-42/edit")
    );
}

#[test]
fn a_real_scheme_is_not_an_abbreviation() {
    // The case that decides whether this is safe to run on every link.
    // `https:` and `id:` are schemes closure and every browser already
    // know, and rewriting one because a file happened to declare it
    // would break links that work.
    let abbrevs = link_abbreviations(DOC);
    assert_eq!(expand_link("https://example.com", &abbrevs), None);
    assert_eq!(expand_link("id:01ABC", &abbrevs), None);
    assert_eq!(expand_link("file:notes.org", &abbrevs), None);
}

#[test]
fn a_prefix_nobody_declared_is_left_alone() {
    // Same rule as an unknown entity or macro: leaving the source is
    // honest, and a blank is a wrong answer that looks like an empty one.
    let abbrevs = link_abbreviations(DOC);
    assert_eq!(expand_link("nosuch:thing", &abbrevs), None);
}

#[test]
fn a_link_with_no_prefix_at_all_is_not_expanded() {
    let abbrevs = link_abbreviations(DOC);
    assert_eq!(expand_link("just-some-text", &abbrevs), None);
}

#[test]
fn a_declaration_inside_a_block_is_not_one() {
    // The rule `#+STARTUP:` and `#+PROPERTY:` already follow.
    let src = "#+BEGIN_EXAMPLE\n#+LINK: gh https://github.com/%s\n#+END_EXAMPLE\n";
    assert!(link_abbreviations(src).is_empty());
}

#[test]
fn a_document_with_no_declarations_has_none() {
    assert!(link_abbreviations("* Just a headline\n").is_empty());
}
