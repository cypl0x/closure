//! V1c: the type-level UI capability matrix. Which `ViewTree` node kinds
//! each shell renders, as data — a shell that does not render a kind is a
//! test-time fact, and `render_view`'s exhaustive match makes adding a
//! kind without handling it a *compile* error.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_shell_core::{
    ALL_NODE_KINDS, Action, FieldView, MINIMAL_NODE_KINDS, Node, NodeKind, PaletteItemView,
    RowView, SplitDir, TUI_NODE_KINDS, ToastLevel, WEB_NODE_KINDS, missing_node_kinds,
    ui_matrix_table,
};

/// One value of every `Node` variant, to check `kind()` + coverage.
fn one_of_each() -> Vec<Node> {
    let action = Action::new(closure_config::InputMode::Notion, "rename").unwrap();
    vec![
        Node::Pane {
            title: String::new(),
            children: vec![],
        },
        Node::Rows {
            rows: vec![RowView::new(String::new(), String::new(), 1, None)],
            selected: 0,
        },
        Node::Detail {
            fields: vec![FieldView {
                label: String::new(),
                value: String::new(),
                action: None,
            }],
        },
        Node::Input {
            label: String::new(),
            buffer: String::new(),
        },
        Node::Palette {
            items: vec![PaletteItemView {
                label: String::new(),
                action,
            }],
            cursor: 0,
        },
        Node::Hints {
            line: String::new(),
        },
        Node::Widget {
            name: String::new(),
            content: String::new(),
        },
        Node::Text(String::new()),
        Node::Split {
            direction: SplitDir::Row,
            panes: vec![],
        },
        Node::Modal {
            title: String::new(),
            body: Box::new(Node::Text(String::new())),
        },
        Node::Toast {
            level: ToastLevel::Info,
            text: String::new(),
        },
    ]
}

#[test]
fn all_node_kinds_covers_every_variant() {
    assert_eq!(ALL_NODE_KINDS.len(), 11);
    for n in one_of_each() {
        assert!(
            ALL_NODE_KINDS.contains(&n.kind()),
            "{:?} kind listed",
            n.kind()
        );
    }
    // Distinct kinds.
    let kinds: Vec<NodeKind> = one_of_each().iter().map(Node::kind).collect();
    for (i, k) in kinds.iter().enumerate() {
        assert!(!kinds[i + 1..].contains(k), "{k:?} duplicated");
    }
}

#[test]
fn tui_and_web_render_every_node_kind() {
    assert!(
        missing_node_kinds(TUI_NODE_KINDS).is_empty(),
        "TUI renders all kinds"
    );
    assert!(
        missing_node_kinds(WEB_NODE_KINDS).is_empty(),
        "web renders all kinds"
    );
}

#[test]
fn gtk_and_qt_are_complete_renderers_too() {
    use closure_shell_core::{GTK_NODE_KINDS, QT_NODE_KINDS};
    // G9: after G3/G4 the native shells render the full ViewTree, not a
    // headline list — proven complete in the matrix.
    assert!(missing_node_kinds(GTK_NODE_KINDS).is_empty(), "gtk renders all kinds");
    assert!(missing_node_kinds(QT_NODE_KINDS).is_empty(), "qt renders all kinds");
}

#[test]
fn matrix_table_includes_the_native_shells() {
    let t = ui_matrix_table();
    assert!(t.contains("GTK") && t.contains("QT"), "matrix lists gtk + qt: {t}");
}

#[test]
fn every_shell_is_a_superset_of_the_minimal_floor() {
    for shell in [TUI_NODE_KINDS, WEB_NODE_KINDS] {
        for kind in MINIMAL_NODE_KINDS {
            assert!(shell.contains(kind), "{kind:?} missing from a shell");
        }
    }
}

#[test]
fn matrix_table_lists_kinds_and_shells() {
    let t = ui_matrix_table();
    assert!(t.contains("Pane") && t.contains("Palette") && t.contains("Text"));
    assert!(t.contains("TUI") && t.contains("WEB"));
}
