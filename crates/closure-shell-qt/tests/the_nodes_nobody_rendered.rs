//! The `Node` kinds the QML mapping had never been asked to draw.
//!
//! Same gap as the GTK shell and the same reason it matters: `Palette`,
//! `Widget` and the column axis of a `Split` were never rendered by a
//! test, and a renderer that drops a node kind produces a window with
//! something missing rather than an error.
//!
//! QML has one hazard GTK does not: the output is a string the Qt
//! runtime parses, so anything that reaches it unescaped is a syntax
//! error in a file nobody sees. `qml_escape` exists for that, and these
//! check it is actually applied on the paths that had no tests.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{Action, Node, PaletteItemView, SplitDir, split_node};
use closure_shell_qt::{qml_view, rows};

fn action(command: &str) -> Action {
    Action::new(InputMode::Doom, command).unwrap_or_else(|| panic!("no chord bound to `{command}`"))
}

#[test]
fn the_palette_renders_a_row_per_command_with_its_chord() {
    let node = Node::Palette {
        items: vec![
            PaletteItemView {
                label: "Capture".to_owned(),
                action: action("capture"),
            },
            PaletteItemView {
                label: "Search".to_owned(),
                action: action("search"),
            },
        ],
        cursor: 0,
    };
    let out = qml_view(&node);

    assert!(out.contains("palette"), "{out}");
    assert!(out.contains("Capture") && out.contains("Search"), "{out}");
    assert!(out.contains(action("capture").chord()), "{out}");
    // The cursor row is bolded; without it the palette cannot be
    // navigated by eye.
    assert!(out.contains("font.bold"), "{out}");
}

#[test]
fn an_empty_palette_still_renders_its_container() {
    let out = qml_view(&Node::Palette {
        items: Vec::new(),
        cursor: 0,
    });
    assert!(out.contains("palette"), "{out}");
}

#[test]
fn a_widget_renders_with_its_name_and_content_escaped() {
    // The escaping matters more here than the content: a quote in a
    // widget name would end the QML string and break the whole
    // document, not just that widget.
    let out = qml_view(&Node::Widget {
        name: "a \"quoted\" name".to_owned(),
        content: "line with \"quotes\"".to_owned(),
    });
    assert!(out.contains("quoted"), "{out}");
    assert!(
        out.contains("\\\""),
        "the quotes reached the QML unescaped: {out}"
    );
}

#[test]
fn a_split_says_which_layout_it_is() {
    let leaf = |s: &str| Node::Text(s.to_owned());
    let row = qml_view(&split_node(
        SplitDir::Row,
        vec![leaf("left"), leaf("right")],
    ));
    let column = qml_view(&split_node(
        SplitDir::Column,
        vec![leaf("top"), leaf("bottom")],
    ));

    // Not by comparing first lines — `qml_view` emits an
    // `import QtQuick` preamble, so both documents start identically
    // and that assertion passes for the wrong reason.
    assert!(row.contains("RowLayout"), "{row}");
    assert!(column.contains("ColumnLayout"), "{column}");
    assert!(
        !row.contains("ColumnLayout"),
        "a row split also emitted a column layout: {row}"
    );
    assert!(row.contains("left") && row.contains("right"), "{row}");
    assert!(
        column.contains("top") && column.contains("bottom"),
        "{column}"
    );
}

#[test]
fn rows_of_a_vault_that_is_not_there_is_an_error() {
    assert!(rows(std::path::Path::new("/nonexistent/vault/for/qt")).is_err());
}
