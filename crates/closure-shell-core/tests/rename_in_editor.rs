//! "editor -> rename -> back to outline view — Instead I'd like to
//! execute the rename prompt in the editor view"
//!
//! `Esc` out of a prompt opened over a buffer comes back to the buffer
//! since the pane-return rule; *accepting* one did not. Every prompt
//! ends by going home, and home was the outline — so renaming the note
//! you were writing threw you out of it at the moment the rename
//! succeeded, which is the worst moment to be thrown anywhere.
//!
//! What a prompt does when it finishes is the same question as what it
//! does when it is abandoned, and it has the same answer.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

fn editing() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Alpha\n:PROPERTIES:\n:ID: 01HQREN000000000000001\n:END:\nthe body\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let mut shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, "01HQREN000000000000001"));
    app.run(&mut shell, "edit-body");
    assert_eq!(app.surface(), ModalSurface::EditBody);
    (dir, shell, app)
}

fn type_in(app: &mut ModalApp, shell: &mut Shell, s: &str) {
    for c in s.chars() {
        app.on_key(shell, &c.to_string(), false, false, Some(c));
    }
}

#[test]
fn renaming_from_a_buffer_leaves_you_in_the_buffer() {
    let (_d, mut shell, mut app) = editing();
    app.run(&mut shell, "rename");
    for _ in 0..20 {
        app.on_key(&mut shell, "backspace", false, false, None);
    }
    type_in(&mut app, &mut shell, "Renamed");
    app.on_key(&mut shell, "enter", false, false, None);

    assert_eq!(app.surface(), ModalSurface::EditBody, "{}", app.status());
    assert!(
        app.body_buffer().contains("the body"),
        "and it is the same buffer"
    );
}

#[test]
fn the_rename_actually_happened() {
    let (dir, mut shell, mut app) = editing();
    app.run(&mut shell, "rename");
    for _ in 0..20 {
        app.on_key(&mut shell, "backspace", false, false, None);
    }
    type_in(&mut app, &mut shell, "Renamed");
    app.on_key(&mut shell, "enter", false, false, None);
    let on_disk = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(on_disk.contains("* Renamed"), "{on_disk}");
}

#[test]
fn every_prompt_that_finishes_over_a_buffer_comes_back_to_it() {
    // The same question as `Esc`, and the same answer — for all of
    // them, not just the one that was reported.
    for command in ["rename", "edit-tags", "edit-property"] {
        let (_d, mut shell, mut app) = editing();
        app.run(&mut shell, command);
        if !matches!(
            app.surface(),
            ModalSurface::Rename | ModalSurface::TagsEdit | ModalSurface::PropertyEdit
        ) {
            continue;
        }
        app.on_key(&mut shell, "enter", false, false, None);
        assert_eq!(
            app.surface(),
            ModalSurface::EditBody,
            "`{command}` finished and left the buffer"
        );
    }
}

#[test]
fn renaming_from_the_outline_still_ends_at_the_outline() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Alpha\n:PROPERTIES:\n:ID: 01HQREN000000000000002\n:END:\nbody\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let mut shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, "01HQREN000000000000002"));
    app.run(&mut shell, "rename");
    app.on_key(&mut shell, "enter", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Browse);
}
