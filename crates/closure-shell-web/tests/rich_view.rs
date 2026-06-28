//! G1a: the web shell renders `Node::Split` as a flex container — the
//! same declarative split the TUI renders, here as HTML with its axis
//! class + grouping role.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_shell_core::{Node, SplitDir, split_node};
use closure_shell_web::render_view;

#[test]
fn split_renders_as_a_grouping_flex_container() {
    let tree = split_node(
        SplitDir::Row,
        vec![Node::Text("left".into()), Node::Text("right".into())],
    );
    let html = render_view(&tree);
    assert!(html.contains("class=\"split split-row\""), "axis class: {html}");
    assert!(html.contains("role=\"group\""), "grouping role: {html}");
    let left = html.find("left").unwrap();
    let right = html.find("right").unwrap();
    assert!(left < right, "panes in render order: {html}");
}
