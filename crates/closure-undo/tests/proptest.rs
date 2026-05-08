#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_undo::UndoTree;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        .. ProptestConfig::default()
    })]

    /// I3-style invariant on the tree itself: apply n payloads, then
    /// undo n times. The current cursor must be None (root).
    #[test]
    fn apply_then_undo_returns_to_root(payloads in proptest::collection::vec(any::<i32>(), 0..32)) {
        let mut t: UndoTree<i32> = UndoTree::new();
        for p in &payloads {
            t.apply(*p);
        }
        for _ in 0..payloads.len() {
            t.undo().unwrap();
        }
        prop_assert!(t.current().is_none());
    }

    /// After undoing m of n applies, len stays at n (nodes never
    /// disappear; only the cursor moves).
    #[test]
    fn undo_does_not_delete_nodes(
        payloads in proptest::collection::vec(any::<i32>(), 1..16),
        undo_count in 0usize..16,
    ) {
        let mut t: UndoTree<i32> = UndoTree::new();
        for p in &payloads {
            t.apply(*p);
        }
        let len_before = t.len();
        let n = undo_count.min(payloads.len());
        for _ in 0..n {
            t.undo().unwrap();
        }
        prop_assert_eq!(t.len(), len_before);
    }

    /// Apply A, undo, apply B → A and B are siblings (both children
    /// of the root state).
    #[test]
    fn undo_then_apply_creates_sibling(a in any::<i32>(), b in any::<i32>()) {
        let mut t: UndoTree<i32> = UndoTree::new();
        let a_id = t.apply(a);
        t.undo().unwrap();
        let b_id = t.apply(b);
        prop_assert_ne!(a_id, b_id);
        prop_assert_eq!(t.len(), 2);
    }
}
