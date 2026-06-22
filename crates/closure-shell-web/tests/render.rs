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

#[test]
fn pane_emits_aria_role_and_label() {
    let (_d, tree) = browse_tree();
    let html = render_view(&tree);
    assert!(html.contains("role=\"region\""), "pane role: {html}");
    assert!(
        html.contains("aria-label=\"closure\""),
        "pane label: {html}"
    );
}

#[test]
fn input_emits_aria_label() {
    let node = Node::Input {
        label: "capture".to_owned(),
        buffer: String::new(),
    };
    let html = render_view(&node);
    assert!(
        html.contains("aria-label=\"capture\""),
        "input label: {html}"
    );
}

#[test]
fn vault_page_is_responsive_for_mobile() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* TODO Ship\n").expect("write");
    let mut v = Vault::open(dir.path()).expect("open");
    let resp = closure_shell_web::respond(&mut v, "GET", "/", "");
    assert_eq!(resp.status, 200);
    assert!(
        resp.body.contains("name=\"viewport\""),
        "viewport meta for mobile: {}",
        resp.body
    );
    assert!(
        resp.body.contains("@media"),
        "responsive media query: {}",
        resp.body
    );
}

#[test]
fn export_view_html_is_self_contained_and_declarative() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* TODO Ship\n* Wiki\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    let html = closure_shell_web::export_view_html(&v);
    // Self-contained single file.
    assert!(html.starts_with("<!doctype html>"));
    assert!(
        !html.contains("http://") && !html.contains("https://"),
        "no external refs"
    );
    assert!(html.contains("name=\"viewport\""), "responsive");
    // The declarative ViewTree is embedded as JSON + rendered client-side.
    assert!(
        html.contains("\"k\":\"pane\""),
        "embedded ViewTree json: {html}"
    );
    assert!(
        html.contains("Ship") && html.contains("Wiki"),
        "vault data present"
    );
    assert!(
        html.contains("function render"),
        "inline vanilla-js renderer"
    );
    assert!(
        html.contains("setAttribute('role'"),
        "emits ARIA roles client-side"
    );
}
