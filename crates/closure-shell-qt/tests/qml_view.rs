//! G4: Qt6/QML editor parity. The shell renders the shared
//! `closure_shell_core` `ViewTree` to a QML document (the same `Node` tree
//! tui/web/gtk render) and edits through the shared `Shell` (I8) — both
//! proven hermetically, no display. `qml_view` is the golden mapping the
//! windowed `run` loads for real.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_shell_core::{
    App, Node, RowView, Shell, SplitDir, ToastLevel, modal_node, split_node, toast_node,
};
use closure_shell_qt::qml_view;
use closure_store::Vault;

#[test]
fn browse_view_maps_to_a_qml_document() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("notes.org"), "* TODO Ship parser\n* Wiki\n").unwrap();
    let sh = Shell::new(Vault::open(dir.path()).unwrap());
    let qml = qml_view(&App::new().view(&sh));
    assert!(qml.contains("import QtQuick"), "valid QML header: {qml}");
    assert!(qml.contains("Ship parser") && qml.contains("Wiki"), "headlines: {qml}");
}

#[test]
fn editing_through_the_shared_shell_changes_the_qml() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("inbox.org"), "* Existing\n").unwrap();
    let mut sh = Shell::new(Vault::open(dir.path()).unwrap());
    sh.capture("Captured via qt").expect("capture");
    let qml = qml_view(&App::new().view(&sh));
    assert!(qml.contains("Captured via qt"), "captured headline rendered: {qml}");
    let on_disk = fs::read_to_string(dir.path().join("inbox.org")).unwrap();
    assert!(on_disk.contains("Captured via qt"), "persisted via registry (I8)");
}

#[test]
fn qml_view_escapes_quotes_and_renders_every_rich_kind() {
    let rich = Node::Pane {
        title: "ro\"ot".into(),
        children: vec![
            split_node(
                SplitDir::Column,
                vec![
                    Node::Rows {
                        rows: vec![RowView {
                            id: "x".into(),
                            title: "row".into(),
                            level: 1,
                            todo: None,
                        }],
                        selected: 0,
                    },
                    Node::Text("pane".into()),
                ],
            ),
            modal_node("dlg", Node::Text("body".into())),
            toast_node(ToastLevel::Warning, "heads up"),
        ],
    };
    let qml = qml_view(&rich);
    assert!(qml.contains("ro\\\"ot"), "quote escaped in QML literal: {qml}");
    assert!(qml.contains("ColumnLayout") || qml.contains("Column"), "split layout: {qml}");
    assert!(qml.contains("Dialog") && qml.contains("dlg"), "modal: {qml}");
    assert!(qml.contains("heads up"), "toast: {qml}");
}
