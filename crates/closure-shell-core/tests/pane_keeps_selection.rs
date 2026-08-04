//! "selection at top of outline headings list when switching from any
//! element to the Jobs panel and back."
//!
//! The read-only panes — Jobs, Journal, Agenda and the rest — use
//! `self.selected` as their own cursor. That is the *outline's*
//! selection field. So walking a pane with `j`/`k` moved the outline's
//! cursor underneath it, and leaving set it to 0 outright — which is
//! why the outline was at the top when you came back, whatever you had
//! been reading.
//!
//! Where you were is not the pane's to spend.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;

const ORG: &str = "* One\n:PROPERTIES:\n:ID: 01PANESEL000000000000001\n:END:\nbody\n\
                   * Two\n:PROPERTIES:\n:ID: 01PANESEL000000000000002\n:END:\nbody\n\
                   * Three\n:PROPERTIES:\n:ID: 01PANESEL000000000000003\n:END:\nbody\n\
                   * Four\n:PROPERTIES:\n:ID: 01PANESEL000000000000004\n:END:\nbody\n";

fn app() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    (dir, shell, ModalApp::new(closure_config::InputMode::Doom))
}

/// Put the outline cursor on the third row.
fn on_third(app: &mut ModalApp, shell: &mut Shell) {
    app.run(shell, "next-file");
    app.run(shell, "next-file");
    assert_eq!(app.selected(), 2, "the fixture did not move");
}

#[test]
fn visiting_the_jobs_panel_leaves_the_outline_where_it_was() {
    let (_d, mut shell, mut app) = app();
    on_third(&mut app, &mut shell);
    app.run(&mut shell, "cron");
    assert_eq!(app.surface(), ModalSurface::Cron);
    app.on_key(&mut shell, "escape", false, false, None);
    assert_eq!(
        app.selected(),
        2,
        "the outline went back to the top after a visit to Jobs"
    );
}

#[test]
fn walking_the_panel_does_not_drag_the_outline_with_it() {
    let (_d, mut shell, mut app) = app();
    on_third(&mut app, &mut shell);
    app.run(&mut shell, "cron");
    for _ in 0..3 {
        app.on_key(&mut shell, "j", false, false, None);
    }
    app.on_key(&mut shell, "escape", false, false, None);
    assert_eq!(app.selected(), 2, "walking the pane moved the outline");
}

#[test]
fn the_same_holds_for_the_journal() {
    // Every pane that shares the cursor has the same bug; fixing one
    // by hand would leave the others.
    let (_d, mut shell, mut app) = app();
    on_third(&mut app, &mut shell);
    app.run(&mut shell, "journal");
    app.on_key(&mut shell, "j", false, false, None);
    app.on_key(&mut shell, "escape", false, false, None);
    assert_eq!(app.selected(), 2);
}

#[test]
fn the_panel_still_has_a_cursor_of_its_own() {
    // The fix must not make the panes unwalkable: they still move,
    // they just do not move the outline.
    let (_d, mut shell, mut app) = app();
    on_third(&mut app, &mut shell);
    app.run(&mut shell, "journal");
    let before = app.pane_cursor();
    app.on_key(&mut shell, "j", false, false, None);
    assert!(
        app.pane_cursor() >= before,
        "the pane's own cursor did not move"
    );
}
