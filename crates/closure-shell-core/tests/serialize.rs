//! V3a: serialise the `ViewTree` so an LLM can read *what is on screen*
//! (panes, selection, visible rows, fields) — not just the underlying
//! data. `browse_view` builds the default surface from a borrowed vault.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_shell_core::{Node, browse_view, serialize_view, widget_node};
use closure_store::Vault;

fn vault() -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* TODO Ship parser\n* Personal wiki\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, v)
}

#[test]
fn browse_view_serialises_rows_and_selection() {
    let (_d, v) = vault();
    let s = serialize_view(&browse_view(&v));
    assert!(s.contains("PANE"), "names the pane: {s}");
    assert!(s.contains("ROWS"), "lists rows: {s}");
    assert!(s.contains("selected=0"), "exposes the selection: {s}");
    assert!(
        s.contains("Ship parser") && s.contains("Personal wiki"),
        "{s}"
    );
}

#[test]
fn serialise_shows_widget_name_and_content() {
    let node = widget_node("banner", "== closure ==");
    let s = serialize_view(&node);
    assert!(s.contains("WIDGET banner"), "{s}");
    assert!(s.contains("== closure =="), "{s}");
}

#[test]
fn serialise_is_deterministic() {
    let (_d, v) = vault();
    let node = browse_view(&v);
    assert_eq!(serialize_view(&node), serialize_view(&node));
}

#[test]
fn serialise_text_node_is_plain() {
    assert!(serialize_view(&Node::Text("hi".to_owned())).contains("hi"));
}
