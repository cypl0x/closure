//! "promote / demote => unexpected and almost unfixable creation of
//! subtree. If you promote a subheading all of the lower subheadings on
//! the same level will get this newly promoted subheading as
//! subsubheadings. This shouldn't be the default case. The default case
//! is that just this subheading gets promoted."
//!
//! Promoting took the headline's own subtree with it, which is right.
//! What it also did was leave the headline sitting *between* its former
//! siblings — so every sibling below it, which had been a child of the
//! same parent, silently became a child of the promoted headline.
//!
//! Org behaves this way too, and it is defensible on the grounds that
//! nothing moved. It is still surprising, and it is expensive to undo:
//! putting it back means promoting each stranded sibling in turn and
//! hoping you counted right. "Just this subheading gets promoted" means
//! the rest of the tree keeps the shape it had.
//!
//! So a promoted subtree steps out of the parent it is leaving, and
//! lands after it.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_core::{BlockId, Command as _, Demote, Document, Promote};

fn promote(src: &str, id: &str) -> String {
    let mut doc = Document::load_str(src).unwrap();
    Promote::new(BlockId::from_existing(id))
        .apply(&mut doc)
        .expect("promote");
    doc.source()
}

#[test]
fn a_following_sibling_keeps_its_parent() {
    let out = promote("* Parent\n** B\n:PROPERTIES:\n:ID: bb\n:END:\n** C\n", "bb");
    let c = out.find('C').expect("C is still there");
    let b = out.find("* B").expect("B is still there");
    assert!(
        out.contains("** C"),
        "C was re-parented under the promoted headline:\n{out}"
    );
    assert!(
        c < b,
        "C should stay with its parent, above the promoted subtree:\n{out}"
    );
}

#[test]
fn the_promoted_headline_keeps_its_own_children() {
    let out = promote(
        "* Parent\n** B\n:PROPERTIES:\n:ID: bb\n:END:\n*** B1\n** C\n",
        "bb",
    );
    assert!(out.contains("* B"), "{out}");
    assert!(out.contains("** B1"), "B lost its own child:\n{out}");
}

#[test]
fn a_headline_with_no_siblings_below_is_unaffected_in_shape() {
    let out = promote("* Parent\n** A\n** B\n:PROPERTIES:\n:ID: bb\n:END:\n", "bb");
    assert!(out.contains("** A"), "the sibling above moved:\n{out}");
    assert!(out.contains("* B"), "{out}");
}

#[test]
fn promoting_to_top_level_is_still_refused() {
    let mut doc = Document::load_str("* A\n:PROPERTIES:\n:ID: aa\n:END:\nbody\n").unwrap();
    assert!(
        Promote::new(BlockId::from_existing("aa"))
            .apply(&mut doc)
            .is_err(),
        "a level-1 headline has nowhere to be promoted to"
    );
}

#[test]
fn demote_is_unchanged() {
    // Demote never had this problem: the headline becomes a child of
    // the sibling above it and everything below stays where it is.
    // Asserted so the fix to promote cannot quietly alter it.
    let mut doc =
        Document::load_str("* A\n* B\n:PROPERTIES:\n:ID: bb\n:END:\n** B1\n* C\n").unwrap();
    Demote::new(BlockId::from_existing("bb"))
        .apply(&mut doc)
        .expect("demote");
    let out = doc.source();
    assert!(out.contains("** B"), "{out}");
    assert!(out.contains("*** B1"), "{out}");
    assert!(out.contains("\n* C"), "C moved:\n{out}");
}
