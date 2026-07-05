//! G1a: the web shell renders `Node::Split` as a flex container — the
//! same declarative split the TUI renders, here as HTML with its axis
//! class + grouping role.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_shell_core::{Node, SplitDir, ToastLevel, modal_node, split_node, toast_node};
use closure_shell_web::render_view;

#[test]
fn split_renders_as_a_grouping_flex_container() {
    let tree = split_node(
        SplitDir::Row,
        vec![Node::Text("left".into()), Node::Text("right".into())],
    );
    let html = render_view(&tree);
    assert!(
        html.contains("class=\"split split-row\""),
        "axis class: {html}"
    );
    assert!(html.contains("role=\"group\""), "grouping role: {html}");
    let left = html.find("left").unwrap();
    let right = html.find("right").unwrap();
    assert!(left < right, "panes in render order: {html}");
}

#[test]
fn modal_renders_as_an_accessible_dialog_overlay() {
    let tree = modal_node("Confirm", Node::Text("sure?".into()));
    let html = render_view(&tree);
    assert!(html.contains("role=\"dialog\""), "dialog role: {html}");
    assert!(html.contains("class=\"modal\""), "modal class: {html}");
    assert!(html.contains("aria-label=\"Confirm\""), "labelled: {html}");
    assert!(html.contains("sure?"), "body present: {html}");
}

#[test]
fn toast_renders_as_a_severity_classed_live_region() {
    let html = render_view(&toast_node(ToastLevel::Error, "boom"));
    assert!(html.contains("role=\"alert\""), "assertive role: {html}");
    assert!(
        html.contains("class=\"toast toast-error\""),
        "severity class: {html}"
    );
    assert!(html.contains("boom"), "text: {html}");
}
