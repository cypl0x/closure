//! G1a: the TUI renders the `Node::Split` multi-pane layout — panes in
//! render order, the split axis labelled. Hermetic, no terminal.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_shell_core::{Node, SplitDir, ToastLevel, modal_node, split_node, toast_node};
use closure_tui::render_snapshot;

#[test]
fn split_renders_panes_in_order_under_a_labelled_axis() {
    let tree = split_node(
        SplitDir::Column,
        vec![Node::Text("top".into()), Node::Text("bottom".into())],
    );
    let snap = render_snapshot(&tree);
    assert!(snap.contains("split:column"), "axis labelled: {snap}");
    let top = snap.find("top").unwrap();
    let bottom = snap.find("bottom").unwrap();
    assert!(top < bottom, "panes in render order: {snap}");
}

#[test]
fn modal_renders_a_titled_overlay_above_its_body() {
    let tree = modal_node("Palette", Node::Text("pick a command".into()));
    let snap = render_snapshot(&tree);
    assert!(snap.contains("modal: Palette"), "titled overlay: {snap}");
    let title = snap.find("Palette").unwrap();
    let body = snap.find("pick a command").unwrap();
    assert!(title < body, "title above body: {snap}");
}

#[test]
fn toast_renders_with_its_level_tag() {
    let snap = render_snapshot(&toast_node(ToastLevel::Warning, "unsaved changes"));
    assert!(snap.contains("[warning]"), "level tag: {snap}");
    assert!(snap.contains("unsaved changes"), "text: {snap}");
}
