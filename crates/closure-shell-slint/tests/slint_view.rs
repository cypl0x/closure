//! Q13: Slint editor parity — the shared `ViewTree` renders to a
//! `.slint` document (exhaustive over every `Node` kind), and edits
//! through the shared `Shell` (I8) change the rendered document.
//! Hermetic; the windowed embedder is a display-bound follow-up.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_shell_core::{App, Shell};
use closure_shell_slint::slint_view;
use closure_store::Vault;

#[test]
fn browse_view_maps_to_a_slint_document() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("notes.org"), "* TODO Ship parser\n* Wiki\n").unwrap();
    let sh = Shell::new(Vault::open(dir.path()).unwrap());
    let doc = slint_view(&App::new().view(&sh));
    assert!(doc.starts_with("import {"), "a real .slint document: {doc}");
    assert!(doc.contains("export component Main inherits Window"));
    assert!(doc.contains("ListView"), "rows are a ListView: {doc}");
    assert!(
        doc.contains("Ship parser") && doc.contains("Wiki"),
        "headlines rendered: {doc}"
    );
}

#[test]
fn editing_through_the_shared_shell_changes_the_document() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("inbox.org"), "* Existing\n").unwrap();
    let mut sh = Shell::new(Vault::open(dir.path()).unwrap());
    sh.capture("Captured via slint").expect("capture");
    let doc = slint_view(&App::new().view(&sh));
    assert!(doc.contains("Captured via slint"), "edit rendered: {doc}");
    let on_disk = fs::read_to_string(dir.path().join("inbox.org")).unwrap();
    assert!(on_disk.contains("Captured via slint"), "persisted (I8)");
}

#[test]
fn slint_view_renders_every_node_kind_and_escapes() {
    use closure_shell_core::{
        Node, RowView, SplitDir, ToastLevel, modal_node, split_node, toast_node,
    };
    let rich = Node::Pane {
        title: "he said \"hi\"".into(),
        children: vec![
            split_node(
                SplitDir::Row,
                vec![
                    Node::Rows {
                        rows: vec![RowView::new("id1", "Row", 1, Some("TODO".into()))],
                        selected: 0,
                    },
                    Node::Text("plain".into()),
                ],
            ),
            modal_node("confirm", Node::Text("body".into())),
            toast_node(ToastLevel::Error, "boom"),
            Node::Hints {
                line: "q quit".into(),
            },
            Node::Widget {
                name: "banner".into(),
                content: "line1\nline2".into(),
            },
            Node::Input {
                label: "capture".into(),
                buffer: String::new(),
            },
        ],
    };
    let doc = slint_view(&rich);
    for needle in [
        "GroupBox",
        "HorizontalBox",
        "PopupWindow",
        "toast-error",
        "he said \\\"hi\\\"",
        "LineEdit",
        "banner",
    ] {
        assert!(doc.contains(needle), "{needle} in: {doc}");
    }
}
