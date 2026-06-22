//! V12a: accessibility metadata on the `ViewTree`. Each node maps to a
//! semantic ARIA role (and, where natural, a label), so a shell can emit
//! screen-reader-navigable output.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_shell_core::Node;

#[test]
fn each_node_kind_has_a_semantic_role() {
    assert_eq!(
        Node::Pane {
            title: "t".into(),
            children: vec![]
        }
        .aria_role(),
        "region"
    );
    assert_eq!(
        Node::Rows {
            rows: vec![],
            selected: 0
        }
        .aria_role(),
        "list"
    );
    assert_eq!(Node::Detail { fields: vec![] }.aria_role(), "group");
    assert_eq!(
        Node::Input {
            label: "l".into(),
            buffer: String::new()
        }
        .aria_role(),
        "textbox"
    );
    assert_eq!(
        Node::Palette {
            items: vec![],
            cursor: 0
        }
        .aria_role(),
        "listbox"
    );
    assert_eq!(
        Node::Hints {
            line: String::new()
        }
        .aria_role(),
        "status"
    );
    assert_eq!(
        Node::Widget {
            name: "w".into(),
            content: String::new()
        }
        .aria_role(),
        "region"
    );
    assert_eq!(Node::Text(String::new()).aria_role(), "note");
}

#[test]
fn labelled_nodes_expose_their_label() {
    assert_eq!(
        Node::Pane {
            title: "closure".into(),
            children: vec![]
        }
        .aria_label(),
        Some("closure")
    );
    assert_eq!(
        Node::Input {
            label: "capture".into(),
            buffer: String::new()
        }
        .aria_label(),
        Some("capture")
    );
    assert_eq!(
        Node::Widget {
            name: "banner".into(),
            content: String::new()
        }
        .aria_label(),
        Some("banner")
    );
    // Unlabelled kinds.
    assert_eq!(
        Node::Rows {
            rows: vec![],
            selected: 0
        }
        .aria_label(),
        None
    );
}
