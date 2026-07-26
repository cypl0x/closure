//! G8: the cross-shell visual golden harness. ONE canonical rich
//! `ViewTree` (exercising every `NodeKind`) is rendered through every
//! shell's `ViewTree`→native mapping — tui text, web HTML, gtk widget
//! tree, qt QML — and each output is pinned. Pixels can't be gated
//! hermetically; this pins the *mapping* each shell produces, as far as
//! hermetically possible, and self-guards: the canonical tree must cover
//! `ALL_NODE_KINDS`, so a new node kind that no shell forgot is also a
//! kind the harness cannot ignore.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;

use closure_config::InputMode;
use closure_shell_core::{
    ALL_NODE_KINDS, Action, FieldView, Node, NodeKind, PaletteItemView, RowView, SplitDir,
    ToastLevel, modal_node, serialize_view, split_node, toast_node, view_to_json,
};

/// A kitchen-sink `ViewTree` exercising the whole G1 vocabulary.
fn canonical_tree() -> Node {
    let act = || Action::new(InputMode::Notion, "rename").unwrap();
    Node::Pane {
        title: "closure".into(),
        children: vec![
            split_node(
                SplitDir::Row,
                vec![
                    Node::Rows {
                        rows: vec![
                            RowView::new("1", "Ship parser", 1, Some("TODO".into()))
                                .with_icon(Some("○".into()))
                                .with_badges(vec!["urgent".into()]),
                        ],
                        selected: 0,
                    },
                    Node::Detail {
                        fields: vec![FieldView {
                            label: "todo".into(),
                            value: "TODO".into(),
                            action: Some(act()),
                        }],
                    },
                ],
            ),
            Node::Palette {
                items: vec![PaletteItemView {
                    label: "rename".into(),
                    action: act(),
                }],
                cursor: 0,
            },
            Node::Input {
                label: "capture".into(),
                buffer: "hi".into(),
            },
            Node::Widget {
                name: "clock".into(),
                content: "12:00".into(),
            },
            modal_node("Confirm", Node::Text("sure?".into())),
            toast_node(ToastLevel::Warning, "unsaved"),
            Node::Hints {
                line: "g r toggle".into(),
            },
            Node::Text("footer".into()),
        ],
    }
}

/// Every `NodeKind` present in a tree (recursively).
fn kinds_in(node: &Node, out: &mut BTreeSet<&'static str>) {
    out.insert(node_kind_name(node.kind()));
    match node {
        Node::Pane { children, .. } => children.iter().for_each(|c| kinds_in(c, out)),
        Node::Split { panes, .. } => panes.iter().for_each(|p| kinds_in(p, out)),
        Node::Modal { body, .. } => kinds_in(body, out),
        _ => {}
    }
}

const fn node_kind_name(k: NodeKind) -> &'static str {
    // Stable name via Debug — only used as a set key.
    match k {
        NodeKind::Pane => "Pane",
        NodeKind::Rows => "Rows",
        NodeKind::Detail => "Detail",
        NodeKind::Input => "Input",
        NodeKind::Palette => "Palette",
        NodeKind::Hints => "Hints",
        NodeKind::Widget => "Widget",
        NodeKind::Text => "Text",
        NodeKind::Split => "Split",
        NodeKind::Modal => "Modal",
        NodeKind::Toast => "Toast",
    }
}

#[test]
fn canonical_tree_covers_every_node_kind() {
    let mut present = BTreeSet::new();
    kinds_in(&canonical_tree(), &mut present);
    let all: BTreeSet<&'static str> = ALL_NODE_KINDS.iter().map(|k| node_kind_name(*k)).collect();
    assert_eq!(present, all, "the harness must exercise every NodeKind");
}

#[test]
fn tui_text_mapping_is_golden() {
    let snap = closure_tui::render_snapshot(&canonical_tree());
    let expected = "\
# closure
  == split:row ==
    > ○ TODO Ship parser  :urgent:
    todo: TODO  [r]
  > [r] rename
  capture> hi
  «clock»
    12:00
  ▌ modal: Confirm
    sure?
  ⚑ [warning] unsaved
  g r toggle
  footer";
    assert_eq!(snap, expected, "tui text mapping");
}

#[test]
fn web_html_mapping_pins_its_structure() {
    let html = closure_shell_web::render_view(&canonical_tree());
    for marker in [
        "<section role=\"region\"",
        "class=\"split split-row\"",
        "<span class=\"icon\">○</span>",
        "<span class=\"badge\">urgent</span>",
        "role=\"dialog\"",
        "class=\"toast toast-warning\"",
        "Ship parser",
    ] {
        assert!(html.contains(marker), "web missing {marker}: {html}");
    }
}

#[test]
fn gtk_widget_mapping_pins_its_structure() {
    let w = closure_shell_gtk::widget_tree(&canonical_tree());
    for marker in [
        "GtkFrame",
        "GtkListBox",
        "GtkDialog",
        "GtkInfoBar.warning",
        "○ TODO Ship parser  :urgent:",
    ] {
        assert!(w.contains(marker), "gtk missing {marker}: {w}");
    }
}

#[test]
fn qt_qml_mapping_pins_its_structure() {
    let qml = closure_shell_qt::qml_view(&canonical_tree());
    for marker in [
        "ApplicationWindow",
        "RowLayout",
        "Dialog",
        "toast-warning",
        "Ship parser",
    ] {
        assert!(qml.contains(marker), "qt missing {marker}: {qml}");
    }
}

#[test]
fn slint_mapping_pins_its_structure() {
    // Q13: the fifth G8 column — the same canonical tree renders to a
    // .slint document.
    let s = closure_shell_slint::slint_view(&canonical_tree());
    for marker in [
        "export component Main inherits Window",
        "GroupBox",
        "ListView",
        "PopupWindow",
        "toast-warning",
        "Ship parser",
    ] {
        assert!(s.contains(marker), "slint missing {marker}: {s}");
    }
}

#[test]
fn every_mapping_is_deterministic() {
    let t = canonical_tree();
    assert_eq!(
        closure_tui::render_snapshot(&t),
        closure_tui::render_snapshot(&t)
    );
    assert_eq!(
        closure_shell_web::render_view(&t),
        closure_shell_web::render_view(&t)
    );
    assert_eq!(
        closure_shell_gtk::widget_tree(&t),
        closure_shell_gtk::widget_tree(&t)
    );
    assert_eq!(
        closure_shell_qt::qml_view(&t),
        closure_shell_qt::qml_view(&t)
    );
    assert_eq!(
        closure_shell_slint::slint_view(&t),
        closure_shell_slint::slint_view(&t)
    );
    assert_eq!(view_to_json(&t), view_to_json(&t));
    assert!(!serialize_view(&t).is_empty());
}
