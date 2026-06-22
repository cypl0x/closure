//! V13: serialise a `ViewTree` to compact JSON so a self-contained HTML
//! export can render the declarative tree client-side (no server, no
//! toolchain). Dep-free encoder (no serde).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_shell_core::{Node, RowView, view_to_json};

#[test]
fn encodes_kind_role_and_data() {
    let tree = Node::Pane {
        title: "closure".to_owned(),
        children: vec![
            Node::Rows {
                rows: vec![RowView {
                    id: "1".to_owned(),
                    title: "Ship".to_owned(),
                    level: 1,
                    todo: Some("TODO".to_owned()),
                }],
                selected: 0,
            },
            Node::Hints {
                line: "hint".to_owned(),
            },
        ],
    };
    let json = view_to_json(&tree);
    assert!(json.contains("\"k\":\"pane\""), "kind tag: {json}");
    assert!(
        json.contains("\"role\":\"region\""),
        "aria role embedded: {json}"
    );
    assert!(json.contains("\"title\":\"closure\""));
    assert!(json.contains("\"k\":\"rows\"") && json.contains("\"Ship\""));
    assert!(json.contains("\"k\":\"hints\"") && json.contains("\"hint\""));
}

#[test]
fn escapes_json_special_characters() {
    let json = view_to_json(&Node::Text("a \"quote\" and \\ slash\nnl".to_owned()));
    assert!(json.contains("\\\"quote\\\""), "quotes escaped: {json}");
    assert!(json.contains("\\\\"), "backslash escaped: {json}");
    assert!(json.contains("\\n"), "newline escaped: {json}");
}

#[test]
fn is_deterministic() {
    let n = Node::Hints {
        line: "x".to_owned(),
    };
    assert_eq!(view_to_json(&n), view_to_json(&n));
}
