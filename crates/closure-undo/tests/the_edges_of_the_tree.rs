//! The corners of `UndoTree` nothing had reached.
//!
//! Six lines, and five of them are ordinary answers to questions
//! nobody had asked the tree: what `default()` gives you, what `undo()`
//! says at the root, and what happens when an id from somewhere else
//! is handed in.
//!
//! That last one is the interesting case. `NodeId` is a ulid in a
//! newtype with a private field, so it cannot be forged — but it can be
//! borrowed from *another tree*, which is exactly what a caller holding
//! two documents open will eventually do. Every lookup here has to say
//! "not mine" rather than answering about whichever node happens to sit
//! at that position.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_undo::{UndoError, UndoTree};

#[test]
fn a_default_tree_is_an_empty_one() {
    // `Default` existed and nothing had ever called it, so nothing
    // said it agrees with `new()`.
    let a: UndoTree<&str> = UndoTree::default();
    let b: UndoTree<&str> = UndoTree::new();
    assert!(a.nodes().is_empty());
    assert_eq!(a.nodes().len(), b.nodes().len());
    assert!(a.leaves().is_empty());
}

#[test]
fn undo_at_the_root_says_so_rather_than_pretending() {
    let mut t: UndoTree<&str> = UndoTree::new();
    assert!(matches!(t.undo(), Err(UndoError::AtRoot)));
}

#[test]
fn undo_reaches_the_root_and_then_refuses() {
    // The boundary rather than the empty case: one edit, one undo back
    // to the root, and the *second* undo is the one that has to refuse.
    let mut t: UndoTree<&str> = UndoTree::new();
    t.apply("first");
    assert!(t.undo().expect("the first undo is fine").is_none());
    assert!(
        matches!(t.undo(), Err(UndoError::AtRoot)),
        "undo past the root did not refuse"
    );
}

#[test]
fn the_depth_of_a_node_from_another_tree_is_unknown() {
    // Two trees, two ulids. Asking one about the other's node must be
    // None, not a depth computed from a coincidence.
    let mut mine: UndoTree<&str> = UndoTree::new();
    mine.apply("mine");
    let mut theirs: UndoTree<&str> = UndoTree::new();
    let foreign = theirs.apply("theirs");

    assert_eq!(mine.depth(foreign), None);
}

#[test]
fn depth_counts_the_steps_to_the_root() {
    // The answer the restructured walk has to keep giving.
    let mut t: UndoTree<&str> = UndoTree::new();
    let a = t.apply("a");
    let b = t.apply("b");
    let c = t.apply("c");
    assert_eq!(t.depth(a), Some(0));
    assert_eq!(t.depth(b), Some(1));
    assert_eq!(t.depth(c), Some(2));
}

#[test]
fn a_path_from_a_node_this_tree_does_not_have_is_not_found() {
    let mut mine: UndoTree<&str> = UndoTree::new();
    let to = mine.apply("mine");
    let mut theirs: UndoTree<&str> = UndoTree::new();
    let foreign = theirs.apply("theirs");

    assert!(matches!(
        mine.path_between(Some(foreign), to),
        Err(UndoError::NotFound)
    ));
}

#[test]
fn a_path_to_a_node_this_tree_does_not_have_is_not_found() {
    // The other end of the same question, so neither argument can be
    // the only one checked.
    let mut mine: UndoTree<&str> = UndoTree::new();
    let from = mine.apply("mine");
    let mut theirs: UndoTree<&str> = UndoTree::new();
    let foreign = theirs.apply("theirs");

    assert!(matches!(
        mine.path_between(Some(from), foreign),
        Err(UndoError::NotFound)
    ));
}

#[test]
fn a_path_from_the_root_is_allowed_and_walks_down() {
    let mut t: UndoTree<&str> = UndoTree::new();
    let a = t.apply("a");
    let b = t.apply("b");
    let steps = t.path_between(None, b).expect("root to b");
    assert!(!steps.is_empty(), "no steps from the root to a grandchild");
    assert!(t.depth(a) < t.depth(b));
}

#[test]
fn redo_at_a_leaf_has_nowhere_to_go() {
    // The tip of the tree: an edit has been made and not undone, so
    // there is no child to move forward into. This is the state the
    // user is in almost all the time, and pressing redo there must be
    // a refusal rather than a move to somewhere arbitrary.
    let mut t: UndoTree<&str> = UndoTree::new();
    t.apply("only");
    assert!(matches!(t.redo(None), Err(UndoError::NotFound)));
}

#[test]
fn redo_on_an_empty_tree_has_nowhere_to_go_either() {
    let mut t: UndoTree<&str> = UndoTree::new();
    assert!(matches!(t.redo(None), Err(UndoError::NotFound)));
}

#[test]
fn redo_without_a_branch_takes_the_newest_one() {
    // The documented "canonical redo": undo, make a second edit, and
    // the tree now forks. Redo with no branch named follows the most
    // recently created child, which is the one just made.
    let mut t: UndoTree<&str> = UndoTree::new();
    let first = t.apply("first");
    t.apply("second");
    t.undo().expect("back to first");
    let third = t.apply("third");

    // Back to the fork, then forward without naming a branch.
    t.undo().expect("back to first again");
    assert_eq!(t.redo(None).expect("redo"), third);
    assert_eq!(t.depth(third), t.depth(first).map(|d| d + 1));
}

#[test]
fn redo_can_be_told_which_branch_to_take() {
    // The reason this is a tree rather than a stack: the older branch
    // is still reachable by name, which a linear undo would have
    // thrown away.
    let mut t: UndoTree<&str> = UndoTree::new();
    t.apply("first");
    let second = t.apply("second");
    t.undo().expect("back");
    t.apply("third");
    t.undo().expect("back again");

    assert_eq!(
        t.redo(Some(second)).expect("redo into the old branch"),
        second
    );
}

#[test]
fn redo_into_a_branch_that_is_not_a_child_is_refused() {
    // A node that exists but is not reachable from here in one step.
    // Jumping to it would skip whatever lies between and leave the
    // document in a state no sequence of edits produced.
    let mut t: UndoTree<&str> = UndoTree::new();
    let first = t.apply("first");
    let second = t.apply("second");
    let third = t.apply("third");
    // Currently at `third`; `first` is an ancestor, not a child.
    t.undo().expect("to second");
    assert!(matches!(t.redo(Some(first)), Err(UndoError::NotFound)));
    // And the cursor did not move.
    assert_eq!(t.redo(Some(third)).expect("the real child"), third);
    let _ = second;
}
