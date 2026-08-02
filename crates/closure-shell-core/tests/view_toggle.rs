//! "toggle full view editor should be revamped"
//!
//! Filed with an empty body, so: the toggle, watched. Select the
//! fourth headline in the outline, switch to the whole-file view, and
//! the caret is on line 1 — the top of the file, not the headline you
//! were looking at. Switch back and the outline has forgotten where
//! the caret was.
//!
//! They are two views of one document, so they should agree about
//! where you are in it. That is the whole of the revamp: the toggle
//! carries your position across instead of dropping it, in both
//! directions.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const NOTES: &str = "\
* One
:PROPERTIES:
:ID: 01HQVIEW00000000000001
:END:
first body
* Two
:PROPERTIES:
:ID: 01HQVIEW00000000000002
:END:
second body
* Three
:PROPERTIES:
:ID: 01HQVIEW00000000000003
:END:
third body
";

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

/// The line the caret is on in the file buffer.
fn caret_line(app: &ModalApp) -> usize {
    app.body_cursor().0
}

/// The text of the line the caret is on.
fn caret_text(app: &ModalApp) -> String {
    app.body_buffer()
        .split('\n')
        .nth(caret_line(app))
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn the_file_view_opens_where_the_outline_was() {
    // The report: the fourth headline selected, and the caret on line
    // one.
    let (_d, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQVIEW00000000000003"));
    app.run(&mut shell, "toggle-file-view");
    assert_eq!(app.surface(), ModalSurface::EditFile);
    assert!(
        caret_text(&app).contains("Three"),
        "caret on {:?}",
        caret_text(&app)
    );
}

#[test]
fn the_outline_comes_back_to_where_the_caret_was() {
    // The other direction, which is the same promise.
    let (_d, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQVIEW00000000000001"));
    app.run(&mut shell, "toggle-file-view");
    // Move down to the third headline's line and back out.
    app.body_click(10, 0);
    app.run(&mut shell, "toggle-file-view");
    assert_eq!(app.surface(), ModalSurface::Browse);
    let row = app.rows(&shell)[app.selected()].clone();
    assert_eq!(row.title, "Three", "came back to {:?}", row.title);
}

#[test]
fn a_caret_in_a_body_finds_the_headline_above_it() {
    // Most of a file is body text, so landing on prose has to mean the
    // headline it belongs to rather than nothing.
    let (_d, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQVIEW00000000000001"));
    app.run(&mut shell, "toggle-file-view");
    app.body_click(14, 0); // "third body"
    app.run(&mut shell, "toggle-file-view");
    assert_eq!(app.rows(&shell)[app.selected()].title, "Three");
}

#[test]
fn toggling_twice_leaves_you_where_you_started() {
    // The property that makes it a toggle rather than two commands.
    let (_d, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQVIEW00000000000002"));
    let before = app.selected();
    app.run(&mut shell, "toggle-file-view");
    app.run(&mut shell, "toggle-file-view");
    assert_eq!(app.selected(), before);
    assert_eq!(app.rows(&shell)[app.selected()].title, "Two");
}

#[test]
fn an_empty_outline_does_not_break_the_toggle() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "just prose, no headline\n").expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut shell = Shell::new(vault);
    let mut app = ModalApp::new(InputMode::Doom);
    app.run(&mut shell, "toggle-file-view");
    app.run(&mut shell, "toggle-file-view");
    assert_eq!(app.surface(), ModalSurface::Browse);
}
