//! "db-view doesn't render the results in command palette" and "graph
//! still uses the old view component to display its data instead of
//! the command palette".
//!
//! Both are the last two surfaces still painting a list of their own
//! design. Everything else that answers "which of these do you mean"
//! became a floating picker — one filter, one set of chords, one look
//! — and these two kept a bespoke table and a bespoke three-section
//! pane, each with its own navigation and neither of them filterable.
//!
//! They are lists of headlines like all the others, so they are
//! pickers like all the others. The db-view's columns survive as the
//! picker's own three fields: the title is what you are looking for,
//! the keyword is what marks it, and the tags say the rest.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const NOTES: &str = "\
* Current Issues
:PROPERTIES:
:ID: 01HQDBV000000000000001
:END:
** TODO [#A] Wondering App                                         :app:idea:
:PROPERTIES:
:ID: 01HQDBV000000000000002
:END:
See [[id:01HQDBV000000000000003]].
** DONE The font has not resized
:PROPERTIES:
:ID: 01HQDBV000000000000003
:END:
** TODO An orphan nobody links to
:PROPERTIES:
:ID: 01HQDBV000000000000004
:END:
";

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, "01HQDBV000000000000001"));
    (dir, shell, app)
}

#[test]
fn the_db_view_is_a_picker() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "db-view");
    assert_eq!(app.surface(), ModalSurface::DbView);
    let view = app.picker_view(&shell).expect("a floating picker");
    assert!(!view.rows.is_empty(), "the vault's headlines");
    assert!(!view.title.is_empty());
}

#[test]
fn its_rows_carry_what_the_table_columns_carried() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "db-view");
    let view = app.picker_view(&shell).expect("picker");
    let row = view
        .rows
        .iter()
        .find(|r| r.label.contains("Wondering App"))
        .expect("the headline");
    assert!(row.trailing.contains("TODO"), "{row:?}");
    assert!(row.detail.contains("app"), "the tags: {row:?}");
    assert!(row.detail.contains("#A"), "the priority: {row:?}");
}

#[test]
fn the_db_view_filters_like_every_other_picker() {
    // The point of the item: one filter and one set of chords, not a
    // table you can only stare at.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "db-view");
    let all = app.picker_view(&shell).expect("picker").rows.len();
    for c in "orphan".chars() {
        app.on_key(&mut shell, &c.to_string(), false, false, Some(c));
    }
    let some = app.picker_view(&shell).expect("picker").rows.len();
    assert!(some < all, "{some} of {all} after filtering");
    assert!(some > 0, "the orphan still matches");
}

#[test]
fn the_graph_is_a_picker_too() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "graph");
    assert_eq!(app.surface(), ModalSurface::Graph);
    let view = app.picker_view(&shell).expect("a floating picker");
    assert!(!view.rows.is_empty(), "hubs, orphans and dead links");
}

#[test]
fn the_graph_still_says_which_kind_each_row_is() {
    // The old pane had three labelled sections; a flat list that lost
    // the labels would be a worse pane, not a better one.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "graph");
    let view = app.picker_view(&shell).expect("picker");
    let orphan = view
        .rows
        .iter()
        .find(|r| r.label.contains("orphan nobody links"))
        .expect("the orphan");
    assert!(
        orphan.trailing.to_lowercase().contains("orphan"),
        "{orphan:?}"
    );
}

#[test]
fn neither_paints_itself_behind_itself() {
    // What the floating-picker rule already guarantees for the other
    // seven, now that these two are in the list.
    for command in ["db-view", "graph"] {
        let (_d, mut shell, mut app) = fixture();
        app.run(&mut shell, command);
        let surface = app.surface();
        assert_ne!(app.surface_beneath(), surface, "{command}");
    }
}

#[test]
fn opening_one_from_a_buffer_comes_back_to_the_buffer() {
    for command in ["db-view", "graph"] {
        let (_d, mut shell, mut app) = fixture();
        app.run(&mut shell, "edit-body");
        app.run(&mut shell, command);
        app.on_key(&mut shell, "escape", false, false, None);
        assert_eq!(app.surface(), ModalSurface::EditBody, "{command}");
    }
}
