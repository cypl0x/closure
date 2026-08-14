//! The `Node` kinds the GTK mapping had never been asked to draw.
//!
//! `widget_tree` is the golden mapping the windowed `run` builds for
//! real, and three of its arms — `Palette`, `Widget`, and a `Split`
//! along the column axis — were never reached by a test. A renderer
//! that drops a node kind does not fail: it draws a window with
//! something missing from it, which is the failure a hermetic test
//! exists to catch precisely because no one is looking at the window.
//!
//! `rows` on a vault that is not there is here for the same reason it
//! is everywhere else in this session: it is the ordinary typo, and it
//! must be an error rather than an empty outline that looks like an
//! empty vault.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{Action, Node, PaletteItemView, SplitDir, split_node};
use closure_shell_gtk::{rows, widget_tree};

fn action(command: &str) -> Action {
    Action::new(InputMode::Doom, command).unwrap_or_else(|| panic!("no chord bound to `{command}`"))
}

#[test]
fn the_palette_draws_a_row_per_command_and_marks_the_cursor() {
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
        cursor: 1,
    };
    let out = widget_tree(&node);

    assert!(out.contains("palette"), "{out}");
    assert!(out.contains("Capture"), "{out}");
    assert!(out.contains("Search"), "{out}");
    // The chord is half the point of a palette — a list of names with
    // no keys teaches nobody the keyboard.
    assert!(out.contains(action("capture").chord()), "{out}");
    // And which row is highlighted, since a palette with no cursor is
    // a list you cannot pick from.
    assert!(out.contains("selected"), "{out}");
}

#[test]
fn an_empty_palette_still_draws_its_container() {
    // Otherwise the popup vanishes rather than showing "nothing
    // matches", and a user reads that as the palette being broken.
    let out = widget_tree(&Node::Palette {
        items: Vec::new(),
        cursor: 0,
    });
    assert!(out.contains("palette"), "{out}");
}

#[test]
fn a_widget_is_drawn_with_its_name_and_every_line_of_content() {
    let node = Node::Widget {
        name: "burndown".to_owned(),
        content: "first line\nsecond line\nthird line".to_owned(),
    };
    let out = widget_tree(&node);

    assert!(out.contains("burndown"), "{out}");
    for line in ["first line", "second line", "third line"] {
        assert!(out.contains(line), "`{line}` was dropped: {out}");
    }
}

#[test]
fn a_widget_with_no_content_still_carries_its_name() {
    let out = widget_tree(&Node::Widget {
        name: "empty".to_owned(),
        content: String::new(),
    });
    assert!(out.contains("empty"), "{out}");
}

#[test]
fn a_split_says_which_axis_it_is_on() {
    // Row and column are different windows. Only one had ever been
    // rendered, so a mapping that returned the same orientation for
    // both would have passed.
    let leaf = |s: &str| Node::Text(s.to_owned());
    let row = widget_tree(&split_node(
        SplitDir::Row,
        vec![leaf("left"), leaf("right")],
    ));
    let column = widget_tree(&split_node(
        SplitDir::Column,
        vec![leaf("top"), leaf("bottom")],
    ));

    assert_ne!(
        row.lines().next(),
        column.lines().next(),
        "both axes rendered the same container:\n{row}\n---\n{column}"
    );
    assert!(column.contains("vertical"), "{column}");
    // Whatever the axis, both panes have to be in there.
    assert!(row.contains("left") && row.contains("right"), "{row}");
    assert!(
        column.contains("top") && column.contains("bottom"),
        "{column}"
    );
}

#[test]
fn rows_of_a_vault_that_is_not_there_is_an_error() {
    assert!(rows(std::path::Path::new("/nonexistent/vault/for/gtk")).is_err());
}
