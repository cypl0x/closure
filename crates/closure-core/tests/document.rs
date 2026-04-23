//! Document model tests.
//!
//! Kernel invariants:
//! * I2 — every headline has a stable `BlockId` derived from its `:ID:`
//!   property when present, otherwise a freshly allocated ULID that
//!   lives in memory only.
//! * The id → headline lookup reaches every nested headline.
//! * Byte-exact roundtrip through `Document::load_str` +
//!   `Document::source` holds (I1 carried from `closure-org`).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_core::Document;

#[test]
fn document_from_str_exposes_roots() {
    let src = "* First\n* Second\n";
    let doc = Document::load_str(src).expect("load");
    assert_eq!(doc.roots().len(), 2);
}

#[test]
fn document_source_is_byte_exact_roundtrip() {
    let src = "#+TITLE: x\n\n* H\nbody\n";
    let doc = Document::load_str(src).expect("load");
    assert_eq!(doc.source(), src);
}

#[test]
fn every_headline_has_a_block_id() {
    let src = "* Parent\n** Child\n*** Grand\n* Other\n";
    let doc = Document::load_str(src).expect("load");
    let ids = doc.all_block_ids();
    assert_eq!(ids.len(), 4);
}

#[test]
fn id_from_property_drawer_is_parsed_verbatim() {
    let src = "\
* H
:PROPERTIES:
:ID: 01HXQZ7F0000000000000000AA
:END:
";
    let doc = Document::load_str(src).expect("load");
    let id = doc.roots()[0].id();
    assert_eq!(id.as_str(), "01HXQZ7F0000000000000000AA");
}

#[test]
fn lookup_by_id_returns_headline() {
    let src = "\
* Target
:PROPERTIES:
:ID: 01HXQZ7F0000000000000000AA
:END:
";
    let doc = Document::load_str(src).expect("load");
    let id = doc.roots()[0].id().clone();
    let h = doc.headline_by_id(&id).expect("lookup");
    assert_eq!(h.title(), "Target");
}

#[test]
fn id_without_property_is_allocated_fresh() {
    let src = "* no-id\n";
    let doc = Document::load_str(src).expect("load");
    let id = doc.roots()[0].id();
    // A fresh ULID has length 26.
    assert_eq!(id.as_str().len(), 26);
}
