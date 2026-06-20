//! V1b: the web shell renders the shared `closure_shell_core::ViewTree`
//! (same tree the TUI renders) to HTML — one declarative description,
//! many embedders. Actionable nodes surface their chord (`<kbd>`).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_shell_core::{App, Node, Shell};
use closure_shell_web::render_view;
use closure_store::Vault;

fn browse_tree() -> (tempfile::TempDir, Node) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* TODO Ship parser\n* Personal wiki\n",
    )
    .expect("write");
    let sh = Shell::new(Vault::open(dir.path()).expect("open"));
    let tree = App::new().view(&sh);
    (dir, tree)
}

#[test]
fn renders_rows_detail_and_hints_to_html() {
    let (_d, tree) = browse_tree();
    let html = render_view(&tree);
    assert!(html.contains("Ship parser"), "headline row: {html}");
    assert!(html.contains("Personal wiki"));
    assert!(html.contains("<footer"), "which-key hints line present");
}

#[test]
fn actionable_field_shows_its_chord_in_a_kbd() {
    let (_d, tree) = browse_tree();
    let html = render_view(&tree);
    assert!(
        html.contains("<kbd"),
        "an actionable detail field surfaces its keybinding: {html}"
    );
}

#[test]
fn escapes_html_in_text() {
    let n = Node::Text("<script>".to_owned());
    let html = render_view(&n);
    assert!(!html.contains("<script>"), "raw tag must not survive");
    assert!(html.contains("&lt;script&gt;"));
}

#[test]
fn widget_node_renders_name_and_content() {
    let node = closure_shell_core::widget_node("banner", "== closure ==");
    let html = render_view(&node);
    assert!(html.contains("data-widget=\"banner\""), "{html}");
    assert!(html.contains("== closure =="), "{html}");
}
