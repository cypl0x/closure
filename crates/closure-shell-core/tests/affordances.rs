//! G5c: Notion affordances modelled as commands/state — the slash command
//! menu, the block "+" insert affordance, and drag-to-reorder. All
//! hermetic; the actual mutation is a registry command (I8), the drag is a
//! pure index computation.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_config::InputMode;
use closure_shell_core::{DragReorder, block_insert_action, reorder_indices, slash_menu};

#[test]
fn slash_menu_lists_commands_with_their_chords() {
    let all = slash_menu("", InputMode::Notion);
    assert!(!all.is_empty(), "empty query lists every command");
    // Every item carries a non-empty chord (V1: actionable ⇒ has a chord).
    assert!(all.iter().all(|it| !it.action.chord().is_empty()));
}

#[test]
fn slash_menu_fuzzy_filters_by_query() {
    let hits = slash_menu("rename", InputMode::Notion);
    assert!(
        hits.iter().any(|it| it.label.contains("rename")),
        "rename matched"
    );
    let none = slash_menu("zzzznope", InputMode::Notion);
    assert!(none.is_empty(), "no fuzzy match ⇒ empty menu");
}

#[test]
fn block_plus_affordance_maps_to_the_add_sibling_command() {
    let act = block_insert_action(InputMode::Notion).expect("add-sibling is bound");
    assert!(
        !act.chord().is_empty(),
        "the + button shows its keybinding too"
    );
}

#[test]
fn drag_reorder_reports_the_from_and_to_on_drop() {
    let mut d = DragReorder::default();
    assert_eq!(d.drop(), None, "drop without a begin does nothing");
    d.begin(0);
    d.over(2);
    assert_eq!(d.drop(), Some((0, 2)));
    assert_eq!(d.drop(), None, "drop is consumed");
}

#[test]
fn drag_reorder_cancel_clears_the_gesture() {
    let mut d = DragReorder::default();
    d.begin(1);
    d.cancel();
    assert_eq!(d.drop(), None);
}

#[test]
fn reorder_indices_moves_an_element_to_a_new_position() {
    // Move element 0 to position 2 in a list of 4.
    assert_eq!(reorder_indices(4, 0, 2), vec![1, 2, 0, 3]);
    // Move the last to the front.
    assert_eq!(reorder_indices(3, 2, 0), vec![2, 0, 1]);
    // No-op when from == to.
    assert_eq!(reorder_indices(3, 1, 1), vec![0, 1, 2]);
    // Out-of-range is the identity order (no panic, I5).
    assert_eq!(reorder_indices(3, 9, 0), vec![0, 1, 2]);
}

// === The live drag needs to be *visible*, not just recorded ===
//
// Reordering by drag is unusable without an insertion indicator, and a
// shell can only paint one if it can ask the gesture where it is
// pointing mid-drag — `drop()` only answers once, at the end.

#[test]
fn an_idle_gesture_points_nowhere() {
    let drag = closure_shell_core::DragReorder::default();
    assert_eq!(drag.source(), None);
    assert_eq!(drag.target(), None);
}

#[test]
fn a_started_drag_exposes_its_source_before_any_move() {
    let mut drag = closure_shell_core::DragReorder::default();
    drag.begin(3);
    assert_eq!(drag.source(), Some(3));
    assert_eq!(
        drag.target(),
        None,
        "no drop line until the pointer actually moves"
    );
}

#[test]
fn dragging_over_a_row_exposes_it_as_the_target() {
    let mut drag = closure_shell_core::DragReorder::default();
    drag.begin(3);
    drag.over(7);
    assert_eq!(drag.source(), Some(3));
    assert_eq!(drag.target(), Some(7));
    drag.over(2);
    assert_eq!(drag.target(), Some(2), "the target follows the pointer");
}

#[test]
fn completing_or_cancelling_clears_both() {
    let mut drag = closure_shell_core::DragReorder::default();
    drag.begin(1);
    drag.over(4);
    assert_eq!(drag.drop(), Some((1, 4)));
    assert_eq!(drag.source(), None, "the indicator must not linger");
    assert_eq!(drag.target(), None);

    drag.begin(1);
    drag.over(4);
    drag.cancel();
    assert_eq!(drag.source(), None);
    assert_eq!(drag.target(), None);
}
