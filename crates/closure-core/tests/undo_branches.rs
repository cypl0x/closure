//! Undo is a tree, and the branches have to survive being walked.
//!
//! The vision asks for undo-tree, and the difference between a tree and
//! a stack is exactly this: undo, then do something *else*, and the
//! thing you undid is still reachable. A stack throws it away. That is
//! the property the feature exists for, and it is the one that gets
//! quietly broken by a refactor because ordinary editing never visits
//! it.
//!
//! Written against `Document`'s public undo/redo rather than the tree
//! underneath, because that is the surface a shell has.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_core::{BlockId, Command as _, Document, RenameHeadline};

const ID: &str = "01UNDOBRANCH00000001";

fn doc() -> Document {
    Document::load_str(&format!("* Original\n:PROPERTIES:\n:ID: {ID}\n:END:\n")).expect("loads")
}

fn rename(d: &mut Document, to: &str) {
    let cmd = RenameHeadline::new(BlockId::from_existing(ID), to.to_owned());
    cmd.apply(d).expect("applies");
}

#[test]
fn undo_then_redo_returns_to_where_it_was() {
    let mut d = doc();
    rename(&mut d, "First");
    let after = d.source();
    d.undo().expect("undo");
    assert!(d.source().contains("Original"), "{}", d.source());
    d.redo(None).expect("redo");
    assert_eq!(d.source(), after, "redo did not return to the edit");
}

#[test]
fn undoing_at_the_root_is_refused_rather_than_wrapping() {
    let mut d = doc();
    let before = d.source();
    assert!(d.undo().is_err(), "undo at the root reported success");
    assert_eq!(d.source(), before, "a refused undo changed the file");
}

#[test]
fn redoing_with_nothing_ahead_is_refused() {
    let mut d = doc();
    rename(&mut d, "First");
    let before = d.source();
    assert!(d.redo(None).is_err(), "redo at the tip reported success");
    assert_eq!(d.source(), before);
}

#[test]
fn a_second_edit_after_an_undo_does_not_destroy_the_first() {
    // The whole reason for a tree. In a stack, doing something after an
    // undo throws the undone edit away and it is gone forever.
    let mut d = doc();
    rename(&mut d, "First");
    d.undo().expect("undo");
    rename(&mut d, "Second");
    assert!(d.source().contains("Second"), "{}", d.source());

    // Both edits now hang off the same parent. Walking back and taking
    // the other branch has to reach "First" again.
    d.undo().expect("undo the second");
    assert!(d.source().contains("Original"), "{}", d.source());

    // Both edits are in the history, hanging off the same parent. A
    // stack would have thrown the first away when the second was made.
    let rows = d.history_view();
    let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
    assert!(
        labels.iter().filter(|l| l.contains("rename")).count() >= 2,
        "the undone edit was thrown away — this is a stack, not a tree: {labels:?}"
    );
}

#[test]
fn each_branch_can_be_redone_by_name() {
    let mut d = doc();
    rename(&mut d, "First");
    d.undo().expect("undo");
    rename(&mut d, "Second");
    d.undo().expect("undo");

    // Every row that is a child of the fork is a branch to try. Jumping
    // to each has to land on a different document — that is what makes
    // them branches rather than one edit listed twice.
    let indices: Vec<usize> = d.history_view().iter().map(|r| r.index).collect();
    let mut seen: Vec<String> = Vec::new();
    for i in indices {
        if d.jump_in_history(i).is_ok() {
            seen.push(d.source());
        }
    }
    assert!(
        seen.iter().any(|s| s.contains("First")),
        "the first branch is unreachable: {seen:?}"
    );
    assert!(
        seen.iter().any(|s| s.contains("Second")),
        "the second branch is unreachable: {seen:?}"
    );
}

#[test]
fn a_long_chain_undoes_all_the_way_back() {
    // Nothing exotic, but it walks the reverse path repeatedly, which
    // is where an off-by-one in the cursor shows up.
    let mut d = doc();
    let start = d.source();
    for i in 0..20 {
        rename(&mut d, &format!("Step {i}"));
    }
    for _ in 0..20 {
        d.undo().expect("undo");
    }
    assert_eq!(d.source(), start, "twenty edits did not undo to the start");
}
