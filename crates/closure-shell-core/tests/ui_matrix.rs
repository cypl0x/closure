//! V1c: the type-level UI capability matrix. Which `ViewTree` node kinds
//! each shell renders, as data — a shell that does not render a kind is a
//! test-time fact, and `render_view`'s exhaustive match makes adding a
//! kind without handling it a *compile* error.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_shell_core::{
    ALL_NODE_KINDS, Action, FieldView, MINIMAL_NODE_KINDS, Node, NodeKind, PaletteItemView,
    RowView, TUI_NODE_KINDS, WEB_NODE_KINDS, missing_node_kinds, ui_matrix_table,
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
            rows: vec![RowView {
                id: String::new(),
                title: String::new(),
                level: 1,
                todo: None,
            }],
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
    ]
}

#[test]
fn all_node_kinds_covers_every_variant() {
    assert_eq!(ALL_NODE_KINDS.len(), 8);
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
