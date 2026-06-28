//! V10a: headless render-snapshot harness. `render_snapshot` turns a
//! `ViewTree` into a deterministic text snapshot, so the render path is
//! golden-tested with NO display — closing the "can't verify pixels"
//! caveat for the shared `Node` renderer.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_shell_core::{Action, FieldView, Node, PaletteItemView, RowView};
use closure_tui::render_snapshot;

/// A fixed tree exercising every node kind.
fn sample() -> Node {
    let action = Action::new(closure_config::InputMode::Notion, "rename").unwrap();
    Node::Pane {
        title: "closure".to_owned(),
        children: vec![
            Node::Rows {
                rows: vec![
                    RowView::new("1", "Ship parser", 1, Some("TODO".to_owned())),
                    RowView::new("2", "Wiki", 1, None),
                ],
                selected: 0,
            },
            Node::Detail {
                fields: vec![FieldView {
                    label: "title".to_owned(),
                    value: "Ship parser".to_owned(),
                    action: Some(action.clone()),
                }],
            },
            Node::Palette {
                items: vec![PaletteItemView {
                    label: "rename".to_owned(),
                    action,
                }],
                cursor: 0,
            },
            Node::Input {
                label: "capture".to_owned(),
                buffer: "draft".to_owned(),
            },
            Node::Widget {
                name: "banner".to_owned(),
                content: "== hi ==".to_owned(),
            },
            Node::Text("footer note".to_owned()),
            Node::Hints {
                line: "[Notion] type: filter".to_owned(),
            },
        ],
    }
}

#[test]
fn render_snapshot_is_a_stable_golden() {
    let chord = Action::new(closure_config::InputMode::Notion, "rename")
        .unwrap()
        .chord()
        .to_owned();
    // Children carry their depth's "  " indent; widget content is nested
    // one level deeper. Pins the shared renderer's exact output.
    let expected = format!(
        "# closure\n\
         \x20 > TODO Ship parser\n\
         \x20   Wiki\n\
         \x20 title: Ship parser  [{chord}]\n\
         \x20 > [{chord}] rename\n\
         \x20 capture> draft\n\
         \x20 «banner»\n\
         \x20   == hi ==\n\
         \x20 footer note\n\
         \x20 [Notion] type: filter"
    );
    assert_eq!(render_snapshot(&sample()), expected);
}

#[test]
fn render_snapshot_is_deterministic() {
    assert_eq!(render_snapshot(&sample()), render_snapshot(&sample()));
}
