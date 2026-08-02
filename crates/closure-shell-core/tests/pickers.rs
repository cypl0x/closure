//! "M-x list commands don't seem to use the 'new' command palette":
//! `buffer-list`, `block-list`, `undo-history`, `headline-list`.
//!
//! The palette floats over the work with a filter at the top and its
//! matches under it. These four opened a *pane* instead — the right
//! column, where the note is — with a bare `j`/`k` list in it and no
//! way to narrow it. Four lists, four presentations, one of which was
//! the good one.
//!
//! So they are all the same thing now: a filter and a list of things to
//! pick. [`ModalApp::picker_view`] is that thing, whatever the surface
//! is over, and every shell paints one picker rather than five panes.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const NOTES: &str = "\
* Alpha heading
:PROPERTIES:
:ID: 01HQPICK00000000000000001
:END:
alpha body

#+BEGIN_SRC sh
echo alpha
#+END_SRC
** Beta child
:PROPERTIES:
:ID: 01HQPICK00000000000000002
:END:
beta body
* Gamma heading
:PROPERTIES:
:ID: 01HQPICK00000000000000003
:END:
gamma body
";

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let shell = Shell::new(vault);
    app.select_by_id(&shell, "01HQPICK00000000000000001");
    (dir, shell, app)
}

fn type_in(app: &mut ModalApp, shell: &mut Shell, s: &str) {
    for c in s.chars() {
        app.on_key(shell, &c.to_string(), false, false, Some(c));
    }
}

/// The four the user named, plus the file picker, which is the same
/// family and would have been the odd one out.
const PICKERS: &[&str] = &[
    "palette",
    "buffer-list",
    "headline-list",
    "block-list",
    "undo-history",
    "recent-files",
];

#[test]
fn every_list_command_opens_a_picker() {
    for cmd in PICKERS {
        let (_d, mut shell, mut app) = fixture();
        app.run(&mut shell, cmd);
        let view = app
            .picker_view(&shell)
            .unwrap_or_else(|| panic!("{cmd} is not a picker"));
        assert!(!view.title.is_empty(), "{cmd} has no title");
        assert!(!view.hint.is_empty(), "{cmd} does not say what RET does");
    }
}

#[test]
fn every_picker_has_a_field_to_filter_with() {
    for cmd in PICKERS {
        let (_d, mut shell, mut app) = fixture();
        app.run(&mut shell, cmd);
        app.on_key(&mut shell, "u", true, false, None);
        type_in(&mut app, &mut shell, "x");
        assert_eq!(app.prompt_text(), Some("x"), "{cmd} swallowed the filter");
    }
}

#[test]
fn the_headline_picker_narrows_as_you_type() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "headline-list");
    let all = app.picker_view(&shell).expect("picker").rows.len();
    assert!(all >= 3, "three headlines to start with: {all}");

    type_in(&mut app, &mut shell, "Beta");
    let narrowed = app.picker_view(&shell).expect("picker").rows;
    assert_eq!(narrowed.len(), 1, "{narrowed:?}");
    assert!(narrowed[0].label.contains("Beta"), "{narrowed:?}");
}

#[test]
fn the_block_picker_narrows_as_you_type() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "block-list");
    assert_eq!(app.picker_view(&shell).expect("picker").rows.len(), 1);

    type_in(&mut app, &mut shell, "zzzz");
    assert!(app.picker_view(&shell).expect("picker").rows.is_empty());
}

#[test]
fn the_undo_picker_narrows_as_you_type() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "undo-history");
    let before = app.picker_view(&shell).expect("picker").rows.len();
    type_in(&mut app, &mut shell, "zzzzznothing");
    let after = app.picker_view(&shell).expect("picker").rows.len();
    assert!(after < before || before == 0, "{before} -> {after}");
}

#[test]
fn the_cursor_walks_the_rows_that_are_left() {
    // The filter and the cursor have to agree: a cursor pointing past
    // the narrowed list is how a picker opens the wrong thing.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "headline-list");
    app.on_key(&mut shell, "down", false, false, None);
    app.on_key(&mut shell, "down", false, false, None);
    type_in(&mut app, &mut shell, "Beta");

    let view = app.picker_view(&shell).expect("picker");
    assert!(
        view.cursor < view.rows.len(),
        "cursor {} into {} row(s)",
        view.cursor,
        view.rows.len()
    );
}

#[test]
fn picking_a_headline_goes_to_it() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "headline-list");
    type_in(&mut app, &mut shell, "Gamma");
    app.on_key(&mut shell, "enter", false, false, None);

    let row = app.rows(&shell)[app.selected()].clone();
    assert_eq!(row.id, "01HQPICK00000000000000003", "landed on Gamma");
}

#[test]
fn a_picker_marks_the_row_you_are_on() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "headline-list");
    let view = app.picker_view(&shell).expect("picker");
    assert!(
        view.rows[view.cursor].current,
        "{:?}",
        view.rows[view.cursor]
    );
}

#[test]
fn a_picker_marks_what_the_filter_matched() {
    // Vertico's highlighting: the row says *why* it survived, which is
    // the whole difference between a list of near-identical candidates
    // and a list you can read.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "headline-list");
    type_in(&mut app, &mut shell, "gam");

    let view = app.picker_view(&shell).expect("picker");
    let row = &view.rows[0];
    let marked: String = row.matches.iter().map(|&(s, e)| &row.label[s..e]).collect();
    assert_eq!(marked, "Gam", "{:?}", row.matches);
}

#[test]
fn an_unfiltered_picker_marks_nothing() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "headline-list");
    let view = app.picker_view(&shell).expect("picker");
    assert!(view.rows.iter().all(|r| r.matches.is_empty()));
}
