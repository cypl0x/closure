#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_undo::UndoTree;

#[test]
fn fresh_tree_is_empty() {
    let t: UndoTree<i32> = UndoTree::new();
    assert!(t.is_empty());
    assert_eq!(t.len(), 0);
    assert!(t.current().is_none());
}

#[test]
fn linear_apply_then_undo_sequence() {
    let mut t: UndoTree<i32> = UndoTree::new();
    let a = t.apply(1);
    let b = t.apply(2);
    let c = t.apply(3);
    assert_eq!(t.current(), Some(c));
    let back = t.undo().expect("undo").expect("some");
    assert_eq!(back, b);
    let back = t.undo().expect("undo").expect("some");
    assert_eq!(back, a);
    let back = t.undo().expect("undo");
    assert!(back.is_none());
}

#[test]
fn undo_then_apply_creates_branch_not_overwrite() {
    let mut t: UndoTree<&'static str> = UndoTree::new();
    let a = t.apply("a");
    let b = t.apply("b");
    // Undo b, apply c → c becomes second child of a, not replacement of b.
    t.undo().unwrap();
    let c = t.apply("c");
    let a_node = t.node(a).expect("a");
    assert_eq!(a_node.children, vec![b, c]);
    assert_eq!(t.current(), Some(c));
}

#[test]
fn redo_default_picks_most_recent_branch() {
    let mut t: UndoTree<i32> = UndoTree::new();
    let a = t.apply(1);
    t.apply(2);
    t.undo().unwrap();
    let c = t.apply(3);
    // Go back to root of branching choice.
    t.undo().unwrap();
    let chosen = t.redo(None).expect("redo");
    assert_eq!(chosen, c, "default redo picks latest sibling");
    assert_eq!(t.current(), Some(c));
    assert_eq!(t.node(a).unwrap().children.len(), 2);
}

#[test]
fn redo_specific_branch() {
    let mut t: UndoTree<i32> = UndoTree::new();
    t.apply(1);
    let b = t.apply(2);
    t.undo().unwrap();
    let c = t.apply(3);
    t.undo().unwrap();
    let chosen = t.redo(Some(b)).expect("redo");
    assert_eq!(chosen, b);
    // The other branch is still addressable.
    t.undo().unwrap();
    let chosen2 = t.redo(Some(c)).expect("redo");
    assert_eq!(chosen2, c);
}

#[test]
fn redo_missing_branch_is_error() {
    let mut t: UndoTree<i32> = UndoTree::new();
    t.apply(1);
    t.undo().unwrap();
    // Forge a random id by applying-then-rolling back an unrelated tree.
    let mut other: UndoTree<i32> = UndoTree::new();
    let random = other.apply(9);
    let err = t.redo(Some(random));
    assert!(err.is_err());
}

#[test]
fn clear_resets_tree() {
    let mut t: UndoTree<i32> = UndoTree::new();
    t.apply(1);
    t.apply(2);
    t.clear();
    assert!(t.is_empty());
    assert!(t.current().is_none());
}

#[test]
fn depth_zero_for_root() {
    let mut t: UndoTree<i32> = UndoTree::new();
    let a = t.apply(1);
    assert_eq!(t.depth(a), Some(0));
    let b = t.apply(2);
    assert_eq!(t.depth(b), Some(1));
}

#[test]
fn leaves_are_siblings_after_branch() {
    let mut t: UndoTree<i32> = UndoTree::new();
    let _a = t.apply(1);
    let _b = t.apply(2);
    t.undo().expect("undo");
    let _c = t.apply(3);
    let leaves = t.leaves();
    assert_eq!(leaves.len(), 2);
}

#[test]
fn path_to_walks_from_root() {
    let mut t: UndoTree<i32> = UndoTree::new();
    let a = t.apply(1);
    let b = t.apply(2);
    let path = t.path_to(b);
    assert_eq!(path, vec![a, b]);
}
