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

// === U1: path_between — the step plan for jumping to any node. ===

use closure_undo::Step;

#[test]
fn path_between_same_position_is_empty() {
    let mut t: UndoTree<i32> = UndoTree::new();
    let a = t.apply(1);
    assert_eq!(t.path_between(Some(a), a).expect("path"), vec![]);
}

#[test]
fn path_between_ancestor_to_descendant_is_redos_only() {
    let mut t: UndoTree<i32> = UndoTree::new();
    let a = t.apply(1);
    let b = t.apply(2);
    let c = t.apply(3);
    assert_eq!(
        t.path_between(Some(a), c).expect("path"),
        vec![Step::Redo(b), Step::Redo(c)]
    );
}

#[test]
fn path_between_descendant_to_ancestor_is_undos_only() {
    let mut t: UndoTree<i32> = UndoTree::new();
    let a = t.apply(1);
    let b = t.apply(2);
    let c = t.apply(3);
    assert_eq!(
        t.path_between(Some(c), a).expect("path"),
        vec![Step::Undo(c), Step::Undo(b)]
    );
}

#[test]
fn path_between_branches_undoes_to_the_fork_then_redoes() {
    let mut tree: UndoTree<&'static str> = UndoTree::new();
    let _na = tree.apply("a");
    let nb = tree.apply("b");
    tree.undo().unwrap();
    let nc = tree.apply("c");
    let nd = tree.apply("d");
    // From d (a → c → d) to b (a → b): undo d, undo c, redo b.
    assert_eq!(
        tree.path_between(Some(nd), nb).expect("path"),
        vec![Step::Undo(nd), Step::Undo(nc), Step::Redo(nb)]
    );
}

#[test]
fn path_between_from_the_root_position_is_redos() {
    let mut t: UndoTree<i32> = UndoTree::new();
    let a = t.apply(1);
    let b = t.apply(2);
    assert_eq!(
        t.path_between(None, b).expect("path"),
        vec![Step::Redo(a), Step::Redo(b)]
    );
}

#[test]
fn path_between_unknown_target_errors() {
    let mut t: UndoTree<i32> = UndoTree::new();
    let a = t.apply(1);
    let mut other: UndoTree<i32> = UndoTree::new();
    let foreign = other.apply(9);
    assert!(t.path_between(Some(a), foreign).is_err());
}

#[test]
fn walking_the_steps_lands_the_cursor_on_the_target() {
    // The composition contract: applying the plan via undo()/redo(branch)
    // moves current() exactly to the target, on every node pair.
    let mut tree: UndoTree<i32> = UndoTree::new();
    let na = tree.apply(1);
    let nb = tree.apply(2);
    tree.undo().unwrap();
    let nc = tree.apply(3);
    tree.undo().unwrap();
    tree.undo().unwrap();
    let nd = tree.apply(4);
    let all = [na, nb, nc, nd];
    for &from in &all {
        for &to in &all {
            // position the cursor at `from` first (walk from wherever we are)
            let pre = tree.path_between(tree.current(), from).expect("pre");
            for step in pre {
                match step {
                    Step::Undo(_) => {
                        tree.undo().expect("undo");
                    }
                    Step::Redo(id) => {
                        tree.redo(Some(id)).expect("redo");
                    }
                }
            }
            assert_eq!(tree.current(), Some(from), "cursor parked at from");
            let plan = tree.path_between(Some(from), to).expect("plan");
            for step in plan {
                match step {
                    Step::Undo(_) => {
                        tree.undo().expect("undo");
                    }
                    Step::Redo(id) => {
                        tree.redo(Some(id)).expect("redo");
                    }
                }
            }
            assert_eq!(tree.current(), Some(to), "cursor landed on to");
        }
    }
}
