//! G1a: rich `ViewTree` vocabulary — the `Node::Split` multi-pane layout.
//! A split arranges child panes along an axis (the foundation for a real
//! editor surface: sidebar + main + detail). The exhaustive matches in
//! `kind`/`aria_role`/`view_to_json`/`serialize_view` force every renderer
//! to handle the new kind (V1c), so this kind cannot silently drift.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_shell_core::{
    ALL_NODE_KINDS, Node, NodeKind, RowView, SplitDir, ToastLevel, modal_node, serialize_view,
    split_node, toast_node, view_to_json,
};

fn sample_split() -> Node {
    split_node(
        SplitDir::Row,
        vec![
            Node::Rows {
                rows: vec![RowView::new("a", "Sidebar item", 1, None)],
                selected: 0,
            },
            Node::Text("main pane".into()),
        ],
    )
}

#[test]
fn split_is_a_first_class_node_kind() {
    assert!(
        ALL_NODE_KINDS.contains(&NodeKind::Split),
        "Split listed in the node-kind matrix"
    );
    assert_eq!(sample_split().kind(), NodeKind::Split);
}

#[test]
fn split_is_a_grouping_region_for_accessibility() {
    // A split is a generic grouping container; it has no natural label.
    assert_eq!(sample_split().aria_role(), "group");
    assert_eq!(sample_split().aria_label(), None);
}

#[test]
fn split_direction_is_preserved_distinctly() {
    assert_ne!(SplitDir::Row, SplitDir::Column);
    assert_eq!(SplitDir::Row.as_str(), "row");
    assert_eq!(SplitDir::Column.as_str(), "column");
}

#[test]
fn split_serialises_to_json_with_direction_and_nested_panes() {
    let json = view_to_json(&sample_split());
    assert!(json.contains("\"k\":\"split\""), "tagged split: {json}");
    assert!(
        json.contains("\"dir\":\"row\""),
        "carries direction: {json}"
    );
    // Children are serialised in order, nested under the split.
    assert!(json.contains("Sidebar item"), "first pane present: {json}");
    assert!(json.contains("main pane"), "second pane present: {json}");
    let sidebar = json.find("Sidebar item").unwrap();
    let main = json.find("main pane").unwrap();
    assert!(sidebar < main, "panes serialised in render order");
}

#[test]
fn split_serialises_for_the_llm_snapshot() {
    let snap = serialize_view(&sample_split());
    assert!(
        snap.contains("SPLIT row"),
        "snapshot names the split: {snap}"
    );
    assert!(snap.contains("Sidebar item") && snap.contains("main pane"));
}

fn sample_modal() -> Node {
    modal_node("Confirm delete", Node::Text("Delete this subtree?".into()))
}

#[test]
fn modal_is_a_first_class_node_kind() {
    assert!(ALL_NODE_KINDS.contains(&NodeKind::Modal));
    assert_eq!(sample_modal().kind(), NodeKind::Modal);
}

#[test]
fn modal_is_an_accessible_dialog_with_its_title_as_label() {
    assert_eq!(sample_modal().aria_role(), "dialog");
    assert_eq!(sample_modal().aria_label(), Some("Confirm delete"));
}

#[test]
fn modal_serialises_to_json_with_title_and_nested_body() {
    let json = view_to_json(&sample_modal());
    assert!(json.contains("\"k\":\"modal\""), "tagged modal: {json}");
    assert!(json.contains("Confirm delete"), "title present: {json}");
    assert!(json.contains("Delete this subtree?"), "body nested: {json}");
}

#[test]
fn modal_serialises_for_the_llm_snapshot() {
    let snap = serialize_view(&sample_modal());
    assert!(
        snap.contains("MODAL Confirm delete"),
        "names the modal: {snap}"
    );
    assert!(snap.contains("Delete this subtree?"), "body shown: {snap}");
}

#[test]
fn toast_is_a_first_class_node_kind() {
    assert!(ALL_NODE_KINDS.contains(&NodeKind::Toast));
    let t = toast_node(ToastLevel::Success, "saved");
    assert_eq!(t.kind(), NodeKind::Toast);
}

#[test]
fn toast_level_drives_the_accessibility_severity() {
    // Errors/warnings are assertive `alert`s; info/success are polite
    // `status` updates.
    assert_eq!(toast_node(ToastLevel::Error, "x").aria_role(), "alert");
    assert_eq!(toast_node(ToastLevel::Warning, "x").aria_role(), "alert");
    assert_eq!(toast_node(ToastLevel::Info, "x").aria_role(), "status");
    assert_eq!(toast_node(ToastLevel::Success, "x").aria_role(), "status");
}

#[test]
fn toast_level_has_a_stable_lowercase_tag() {
    assert_eq!(ToastLevel::Info.as_str(), "info");
    assert_eq!(ToastLevel::Success.as_str(), "success");
    assert_eq!(ToastLevel::Warning.as_str(), "warning");
    assert_eq!(ToastLevel::Error.as_str(), "error");
}

#[test]
fn toast_serialises_to_json_with_level_and_text() {
    let json = view_to_json(&toast_node(ToastLevel::Error, "sync failed"));
    assert!(json.contains("\"k\":\"toast\""), "tagged toast: {json}");
    assert!(
        json.contains("\"level\":\"error\""),
        "carries level: {json}"
    );
    assert!(json.contains("sync failed"), "text present: {json}");
}

#[test]
fn toast_serialises_for_the_llm_snapshot() {
    let snap = serialize_view(&toast_node(ToastLevel::Info, "3 jobs ran"));
    assert!(
        snap.contains("TOAST info 3 jobs ran"),
        "names the toast: {snap}"
    );
}

#[test]
fn rows_carry_icons_and_badges_as_data() {
    // G5a: a row carries a leading icon glyph + metadata chips, built with
    // the constructor + builders (empty by default for back-compat).
    let plain = RowView::new("id", "Plain", 1, None);
    assert!(
        plain.icon.is_none() && plain.badges.is_empty(),
        "defaults empty"
    );

    let row = RowView::new("id", "Ship", 1, Some("TODO".into()))
        .with_icon(Some("○".into()))
        .with_badges(vec!["urgent".into(), "#A".into()]);
    let json = view_to_json(&Node::Rows {
        rows: vec![row],
        selected: 0,
    });
    assert!(json.contains("\"icon\":\"○\""), "icon as data: {json}");
    assert!(
        json.contains("urgent") && json.contains("#A"),
        "badges as data: {json}"
    );
}

#[test]
fn browse_view_maps_tags_to_badges_and_todo_to_an_icon() {
    use closure_shell_core::browse_view;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("n.org"),
        "* TODO Ship parser :urgent:soon:\n",
    )
    .unwrap();
    let v = closure_store::Vault::open(dir.path()).unwrap();
    let json = view_to_json(&browse_view(&v));
    assert!(
        json.contains("urgent") && json.contains("soon"),
        "tags → badges: {json}"
    );
    assert!(json.contains("\"icon\":"), "todo → an icon glyph: {json}");
}
