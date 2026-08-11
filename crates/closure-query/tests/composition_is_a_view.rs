//! "Decide, and write into docs/spec.md, what a block is."
//!
//! The vision's first item is composability and it is the least built.
//! Nothing can be built on it until the spec says what composes with
//! what — and the load-bearing half of that answer is not a definition
//! but a rule about writes.
//!
//! Org's own `#+BEGIN: ... #+END:` dynamic blocks are *written into the
//! file* by `org-dblock-update`: the expansion becomes source. closure
//! cannot do that and keep I1, because then the file's bytes depend on
//! when it was last refreshed and against which vault. So composition
//! here is the other thing: the region between the delimiters is
//! authored content, the expansion is derived, and the derivation is
//! never written back.
//!
//! That makes composition reduce to `query` — one of the seven kernel
//! primitives — and adds no eighth. Which is the rule the spec's
//! built-to-last section already sets for every new feature.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_query::expand_doc_widgets;
use closure_store::Vault;

/// A vault with a widget definition and a call site that uses it.
fn vault() -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("widgets.org"),
        "#+BEGIN: closure-widget :name banner\n== closure ==\n#+END:\n",
    )
    .expect("write");
    fs::write(
        dir.path().join("notes.org"),
        "* Home\n:PROPERTIES:\n:ID: 01HQBLOCK000000000000000A\n:END:\n\
         #+BEGIN: closure-widget :name page\ntop {{banner}} bottom\n#+END:\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, v)
}

#[test]
fn expanding_a_document_does_not_touch_the_file() {
    // The whole difference between closure's composition and org's
    // dynamic blocks: refreshing a view is not an edit.
    let (dir, v) = vault();
    let path = dir.path().join("notes.org");
    let before = fs::read(&path).expect("read");
    let _ = expand_doc_widgets(&v, std::path::Path::new("notes.org")).unwrap();
    let after = fs::read(&path).expect("read");
    assert_eq!(before, after, "expansion wrote itself into the source");
}

#[test]
fn the_source_still_roundtrips_after_expanding_it() {
    // I1 is what forbids writing the expansion back, so I1 is what has
    // to still hold once it has been expanded a few times.
    let (dir, v) = vault();
    let path = dir.path().join("notes.org");
    for _ in 0..3 {
        let _ = expand_doc_widgets(&v, std::path::Path::new("notes.org")).unwrap();
    }
    let src = fs::read_to_string(&path).expect("read");
    let doc = closure_org::parse(&src).expect("parse");
    assert_eq!(closure_org::print(&doc), src, "I1 broke after expansion");
}

#[test]
fn the_same_vault_expands_to_the_same_thing() {
    // I6. A derived view that is not deterministic is not a view, it is
    // a second source of truth.
    let (_d, v) = vault();
    let a = expand_doc_widgets(&v, std::path::Path::new("notes.org")).unwrap();
    let b = expand_doc_widgets(&v, std::path::Path::new("notes.org")).unwrap();
    assert_eq!(a, b);
}
