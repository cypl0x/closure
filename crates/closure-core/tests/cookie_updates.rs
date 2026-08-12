//! A cookie stays true when a child's keyword changes.
//!
//! Counting is `closure-org`'s. This is the half that matters: a
//! `[1/3]` nobody updates is worse than no cookie at all, because an
//! absent count says nothing and a stale one says something false while
//! looking maintained.
//!
//! It rides on `set-todo` rather than being a command of its own. The
//! parent's cookie changing *is* part of finishing the child — one
//! edit, one undo — and a separate command would let the two drift by
//! exactly the amount somebody forgot to run it.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_core::{BlockId, Command as _, Document, SetTodo};

const DOC: &str = "\
* Project [0/2]
:PROPERTIES:
:ID: 01COOKUP000000000000001
:END:
** TODO First
:PROPERTIES:
:ID: 01COOKUP000000000000002
:END:
** TODO Second
:PROPERTIES:
:ID: 01COOKUP000000000000003
:END:
";

fn finish(src: &str, id: &str) -> Document {
    let mut doc = Document::load_str(src).expect("parse");
    let cmd = SetTodo::new(BlockId::from_existing(id), Some("DONE".to_owned()));
    cmd.apply(&mut doc).expect("apply");
    doc
}

#[test]
fn finishing_a_child_moves_the_parents_cookie() {
    let doc = finish(DOC, "01COOKUP000000000000002");
    assert!(doc.source().contains("* Project [1/2]"), "{}", doc.source());
}

#[test]
fn a_percent_cookie_moves_too() {
    let src = DOC.replace("[0/2]", "[0%]");
    let doc = finish(&src, "01COOKUP000000000000002");
    assert!(doc.source().contains("* Project [50%]"), "{}", doc.source());
}

#[test]
fn undo_puts_the_cookie_back_with_the_keyword() {
    // One edit, one undo. A cookie left at [1/2] over a TODO child is a
    // document that never existed.
    let mut doc = Document::load_str(DOC).expect("parse");
    let before = doc.source();
    let cmd = SetTodo::new(
        BlockId::from_existing("01COOKUP000000000000002"),
        Some("DONE".to_owned()),
    );
    cmd.apply(&mut doc).expect("apply");
    doc.undo().expect("undo");
    assert_eq!(doc.source(), before);
}

#[test]
fn a_parent_without_a_cookie_is_left_alone() {
    // Adding one nobody asked for would edit a title the author wrote.
    let src = DOC.replace(" [0/2]", "");
    let doc = finish(&src, "01COOKUP000000000000002");
    assert!(doc.source().contains("* Project\n"), "{}", doc.source());
}

#[test]
fn a_headline_with_no_parent_changes_nothing() {
    let src = "* Alone\n:PROPERTIES:\n:ID: 01COOKUP000000000000004\n:END:\n";
    let doc = finish(src, "01COOKUP000000000000004");
    assert!(doc.source().contains("* DONE Alone"), "{}", doc.source());
}
