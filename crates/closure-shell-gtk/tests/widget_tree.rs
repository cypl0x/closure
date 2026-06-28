//! G3: GTK4 editor parity. The shell renders the shared
//! `closure_shell_core` `ViewTree` to a GTK4 widget tree (the same `Node`
//! tree tui/web render), and edits through the shared `Shell` (I8) — both
//! proven hermetically, no display. `widget_tree` is the golden mapping
//! the windowed `run` builds for real.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_shell_core::{
    App, Node, RowView, Shell, SplitDir, ToastLevel, modal_node, split_node, toast_node,
};
use closure_shell_gtk::widget_tree;
use closure_store::Vault;

fn browse(dir: &std::path::Path) -> (Shell, Node) {
    let sh = Shell::new(Vault::open(dir).expect("open"));
    let tree = App::new().view(&sh);
    (sh, tree)
}

#[test]
fn browse_view_maps_to_a_gtk_frame_list_and_hints() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("notes.org"), "* TODO Ship parser\n* Wiki\n").unwrap();
    let (_sh, tree) = browse(dir.path());
    let w = widget_tree(&tree);
    assert!(w.contains("GtkFrame"), "pane is a frame: {w}");
    assert!(w.contains("GtkListBox"), "rows are a list box: {w}");
    assert!(w.contains("GtkLabel"), "rows are labels: {w}");
    assert!(w.contains("Ship parser") && w.contains("Wiki"), "headlines: {w}");
}

#[test]
fn editing_through_the_shared_shell_changes_the_rendered_tree() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("inbox.org"), "* Existing\n").unwrap();
    let mut sh = Shell::new(Vault::open(dir.path()).unwrap());
    sh.capture("Captured via gtk").expect("capture");
    // The same Shell drives the view; the new headline appears in the
    // GTK widget tree the window would build (editor parity, not read-only).
    let tree = App::new().view(&sh);
    assert!(
        widget_tree(&tree).contains("Captured via gtk"),
        "captured headline is rendered"
    );
    // Persisted to disk through the registry (I8).
    let on_disk = fs::read_to_string(dir.path().join("inbox.org")).unwrap();
    assert!(on_disk.contains("Captured via gtk"));
}

#[test]
fn widget_tree_renders_every_rich_node_kind_without_panic() {
    // The full G1 vocabulary maps to GTK widgets (exhaustive, V1c).
    let rich = Node::Pane {
        title: "root".into(),
        children: vec![
            split_node(
                SplitDir::Row,
                vec![
                    Node::Rows {
                        rows: vec![RowView::new("x", "row", 1, Some("TODO".into()))],
                        selected: 0,
                    },
                    Node::Text("pane".into()),
                ],
            ),
            modal_node("dialog", Node::Text("body".into())),
            toast_node(ToastLevel::Error, "failed"),
        ],
    };
    let w = widget_tree(&rich);
    assert!(w.contains("GtkBox") && w.contains("orientation=horizontal"), "split: {w}");
    assert!(w.contains("GtkDialog") && w.contains("dialog"), "modal: {w}");
    assert!(w.contains("InfoBar") && w.contains("failed"), "toast: {w}");
}
