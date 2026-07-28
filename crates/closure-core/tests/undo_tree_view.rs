//! The undo history as a *tree*.
//!
//! `history_view` listed the nodes in insertion order with a depth
//! number, which reads as a flat log with odd indentation: the one
//! thing an undo tree is for — seeing that undoing and typing again
//! left the old line intact on another branch — was the one thing it
//! did not show.
//!
//! Rows now come out in walk order with the drawing precomputed, so
//! every shell paints the same tree (the kernel decides, I7), and each
//! row carries the insertion index `jump_in_history` addresses so
//! reordering the display does not move the target.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_core::{Command as _, Document, RenameHeadline};

/// A document with a forked history:
///
/// ```text
/// A ── B
///  └── C ── D
/// ```
fn forked() -> Document {
    let mut doc = Document::load_str("* Old\n").expect("load");
    let id = doc.roots()[0].id().clone();
    let rename = |doc: &mut Document, title: &str| {
        RenameHeadline::new(id.clone(), title.to_owned())
            .apply(doc)
            .expect("apply");
    };
    rename(&mut doc, "A");
    rename(&mut doc, "B");
    doc.undo().expect("undo B");
    rename(&mut doc, "C");
    rename(&mut doc, "D");
    doc
}

#[test]
fn a_fork_is_visible_as_a_fork() {
    let rows = forked().history_view();
    assert_eq!(rows.len(), 4, "A, B, C, D");
    let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
    assert!(labels[0].contains('A'), "the root first: {labels:?}");
    // B and C are siblings under A; D is C's child. Walk order puts a
    // whole branch together rather than interleaving by insertion.
    assert_eq!(rows[0].depth, 0);
    assert_eq!(rows[1].depth, 1);
    assert_eq!(rows[2].depth, 1);
    assert_eq!(rows[3].depth, 2);
}

#[test]
fn every_row_carries_the_index_that_jumps_to_it() {
    // The display order is the tree's; `jump_in_history` addresses
    // insertion order. A row that did not carry its own index would
    // send a click to whatever edit happened to be there.
    let mut doc = forked();
    let rows = doc.history_view();
    let b = rows
        .iter()
        .find(|r| r.label.contains("→ B"))
        .expect("B is in the tree");
    doc.jump_in_history(b.index).expect("jump");
    assert_eq!(
        doc.roots()[0].title(),
        "B",
        "the index took us to the row we clicked"
    );
}

#[test]
fn the_drawing_says_which_rows_are_siblings() {
    let rows = forked().history_view();
    // A is a root: nothing to its left. B is the first of two children,
    // D the last child of C — a tee and a corner.
    assert!(rows[0].graph.trim().is_empty(), "root: {:?}", rows[0].graph);
    assert!(
        rows[1].graph.contains('├'),
        "B has a sibling below it: {:?}",
        rows[1].graph
    );
    assert!(
        rows[2].graph.contains('└'),
        "C is the last child: {:?}",
        rows[2].graph
    );
    assert!(
        rows[3].graph.contains('└'),
        "and D is C's only child: {:?}",
        rows[3].graph
    );
}

#[test]
fn a_row_knows_its_parent_row() {
    let rows = forked().history_view();
    assert_eq!(rows[0].parent, None, "the root has none");
    assert_eq!(rows[1].parent, Some(0), "B under A");
    assert_eq!(rows[2].parent, Some(0), "C under A too — that is the fork");
    assert_eq!(rows[3].parent, Some(2), "D under C");
}

#[test]
fn the_cursor_is_marked_wherever_it_is() {
    let mut doc = forked();
    assert!(
        doc.history_view().iter().filter(|r| r.is_current).count() == 1,
        "exactly one current row"
    );
    let rows = doc.history_view();
    let b = rows.iter().find(|r| r.label.contains("→ B")).expect("B");
    let want = b.index;
    doc.jump_in_history(want).expect("jump");
    let after = doc.history_view();
    let current = after.iter().find(|r| r.is_current).expect("still one");
    assert_eq!(current.index, want, "and it moved with the jump");
}

#[test]
fn a_straight_line_history_is_a_straight_line() {
    let mut doc = Document::load_str("* Old\n").expect("load");
    let id = doc.roots()[0].id().clone();
    for title in ["A", "B", "C"] {
        RenameHeadline::new(id.clone(), title.to_owned())
            .apply(&mut doc)
            .expect("apply");
    }
    let rows = doc.history_view();
    assert_eq!(rows.len(), 3);
    for (i, row) in rows.iter().enumerate() {
        assert_eq!(row.depth, i, "one after another");
        assert_eq!(row.index, i, "and in insertion order too");
    }
}

#[test]
fn walk_order_and_insertion_order_come_apart() {
    // The case the `index` field exists for: jump back onto an older
    // branch and edit there, and the newest node is no longer last on
    // screen. A row that carried only its position would then send a
    // jump to the wrong edit.
    //
    // ```text
    // A ── B ── D      (D applied after C, but drawn above it)
    //  └── C
    // ```
    let mut doc = Document::load_str("* Old\n").expect("load");
    let id = doc.roots()[0].id().clone();
    let rename = |doc: &mut Document, title: &str| {
        RenameHeadline::new(id.clone(), title.to_owned())
            .apply(doc)
            .expect("apply");
    };
    rename(&mut doc, "A");
    rename(&mut doc, "B");
    doc.undo().expect("back to A");
    rename(&mut doc, "C");
    doc.jump_in_history(1).expect("back onto B");
    rename(&mut doc, "D");

    let rows = doc.history_view();
    let labels: Vec<String> = rows.iter().map(|r| r.label.clone()).collect();
    let indices: Vec<usize> = rows.iter().map(|r| r.index).collect();
    assert_ne!(
        indices,
        vec![0, 1, 2, 3],
        "the tree is not in insertion order any more: {labels:?}"
    );
    // …and every row still jumps to itself.
    for row in &rows {
        let mut probe = doc.clone();
        probe.jump_in_history(row.index).expect("jump");
        assert!(
            row.label.contains(probe.roots()[0].title()),
            "{:?} took us to {:?}",
            row.label,
            probe.roots()[0].title()
        );
    }
}

#[test]
fn an_empty_history_draws_nothing() {
    let doc = Document::load_str("* Old\n").expect("load");
    assert!(doc.history_view().is_empty());
}
