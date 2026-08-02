//! "deleting/yanking with d in outline tree view doesn't add to undo
//! stack (kill-ring?)"
//!
//! It does add to one — the vault keeps an undo stack per file. What
//! it does not do is *reach* it, because `undo` asks the row that is
//! selected *now* which file to undo, and deleting a subtree is
//! precisely the operation that moves the selection off it.
//!
//! Three ways that goes wrong, in rising order of how obviously broken
//! it looks:
//!
//! - the selection lands on a row from another file, and `u` undoes an
//!   edit in that file instead;
//! - the deleted headline was the last one in the vault, so there is
//!   no selected row at all and `u` does nothing whatsoever;
//! - and either way, what you actually asked to undo is untouched.
//!
//! Undo follows the *edit*, not the cursor. The last command that
//! changed the vault says which file it changed, and that is the file
//! `u` speaks to.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

/// Two files, so "the selection moved to another file" is reachable.
fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("alpha.org"),
        "* Alpha one\n:PROPERTIES:\n:ID: 01HQUNDO0000000000001\n:END:\nbody a\n\
         * Alpha two\n:PROPERTIES:\n:ID: 01HQUNDO0000000000002\n:END:\nbody b\n",
    )
    .expect("write");
    fs::write(
        dir.path().join("beta.org"),
        "* Beta one\n:PROPERTIES:\n:ID: 01HQUNDO0000000000003\n:END:\nbody c\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let app = ModalApp::new(InputMode::Doom);
    (dir, Shell::new(vault), app)
}

fn titles(shell: &Shell, app: &ModalApp) -> Vec<String> {
    app.rows(shell).into_iter().map(|r| r.title).collect()
}

#[test]
fn deleting_a_headline_can_be_undone() {
    // The report at its simplest.
    let (_d, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQUNDO0000000000001"));
    app.run(&mut shell, "delete");
    assert!(
        !titles(&shell, &app).iter().any(|t| t == "Alpha one"),
        "the delete did not happen"
    );
    app.run(&mut shell, "undo");
    assert!(
        titles(&shell, &app).iter().any(|t| t == "Alpha one"),
        "u did not bring it back: {:?}",
        titles(&shell, &app)
    );
}

#[test]
fn undo_follows_the_edit_even_when_the_selection_left_the_file() {
    // Delete the last headline of a file and the selection lands
    // somewhere else — in another file, whose history `u` then
    // rewrote instead.
    let (_d, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQUNDO0000000000003"));
    app.run(&mut shell, "delete");
    assert!(!titles(&shell, &app).iter().any(|t| t == "Beta one"));
    // Whatever is selected now, it is not in beta.org.
    app.run(&mut shell, "undo");
    assert!(
        titles(&shell, &app).iter().any(|t| t == "Beta one"),
        "undo went to the wrong file: {:?}",
        titles(&shell, &app)
    );
    assert!(
        titles(&shell, &app).iter().any(|t| t == "Alpha one"),
        "and it damaged the other one: {:?}",
        titles(&shell, &app)
    );
}

#[test]
fn undo_works_when_nothing_is_left_to_select() {
    // The worst case: delete everything, and `u` has no row to ask.
    let (_d, mut shell, mut app) = fixture();
    for id in [
        "01HQUNDO0000000000001",
        "01HQUNDO0000000000002",
        "01HQUNDO0000000000003",
    ] {
        assert!(app.select_by_id(&shell, id));
        app.run(&mut shell, "delete");
    }
    assert!(titles(&shell, &app).is_empty(), "everything gone");
    app.run(&mut shell, "undo");
    assert!(
        !titles(&shell, &app).is_empty(),
        "an empty outline has nothing to ask, so undo did nothing at all"
    );
}

#[test]
fn a_deleted_subtree_is_still_on_the_kill_ring() {
    // The parenthesis in the report. `d` cuts rather than destroys, and
    // `p` puts it back somewhere else — that half already worked and
    // must keep working.
    let (_d, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQUNDO0000000000001"));
    app.run(&mut shell, "delete");
    assert!(app.select_by_id(&shell, "01HQUNDO0000000000002"));
    app.run(&mut shell, "paste-subtree");
    assert!(
        titles(&shell, &app).iter().any(|t| t == "Alpha one"),
        "the cut subtree was not on the ring: {:?}",
        titles(&shell, &app)
    );
}

#[test]
fn redo_follows_the_edit_too() {
    // Whatever is true of undo has to be true of its twin, or the pair
    // is worse than either.
    let (_d, mut shell, mut app) = fixture();
    assert!(app.select_by_id(&shell, "01HQUNDO0000000000003"));
    app.run(&mut shell, "delete");
    app.run(&mut shell, "undo");
    assert!(titles(&shell, &app).iter().any(|t| t == "Beta one"));
    app.run(&mut shell, "redo");
    assert!(
        !titles(&shell, &app).iter().any(|t| t == "Beta one"),
        "redo did not reach the same file: {:?}",
        titles(&shell, &app)
    );
}
