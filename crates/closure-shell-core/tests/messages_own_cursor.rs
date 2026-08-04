//! "scrolling messages scrolls the background outline tree view as
//! well … there is a active selection in the background outline
//! headline tree view. Moving up/down will scroll both lists. It looks
//! like that the messages popup doesn't take ownership of the inputs."
//!
//! It takes the input fine — it spends the wrong cursor. The filtered
//! pickers fell through to `self.selected`, which is the *outline's*
//! selection, so walking the message log walked your notes underneath
//! it and leaving set them back to the top.
//!
//! Not every list should have its own: the search overlay filters the
//! outline and picking jumps there, so its cursor *is* the outline's.
//! The distinction is whether the list is showing you the outline or
//! showing you something else.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const ORG: &str = "* One\n:PROPERTIES:\n:ID: 01MSGCUR00000000000000A\n:END:\nb\n\
                   * Two\n:PROPERTIES:\n:ID: 01MSGCUR00000000000000B\n:END:\nb\n\
                   * Three\n:PROPERTIES:\n:ID: 01MSGCUR00000000000000C\n:END:\nb\n\
                   * Four\n:PROPERTIES:\n:ID: 01MSGCUR00000000000000D\n:END:\nb\n";

fn app() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    (dir, shell, ModalApp::new(closure_config::InputMode::Doom))
}

fn on_third(app: &mut ModalApp, shell: &mut Shell) {
    app.run(shell, "next-file");
    app.run(shell, "next-file");
    assert_eq!(app.selected(), 2);
}

#[test]
fn walking_the_message_log_leaves_the_outline_alone() {
    let (_d, mut shell, mut app) = app();
    // Something to scroll through.
    for _ in 0..6 {
        app.run(&mut shell, "build-info");
    }
    on_third(&mut app, &mut shell);
    app.run(&mut shell, "messages");
    for _ in 0..3 {
        app.on_key(&mut shell, "down", false, false, None);
    }
    assert_eq!(
        app.selected(),
        2,
        "the outline scrolled along with the message log"
    );
}

#[test]
fn leaving_the_message_log_leaves_the_outline_alone() {
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "build-info");
    on_third(&mut app, &mut shell);
    app.run(&mut shell, "messages");
    app.on_key(&mut shell, "escape", false, false, None);
    assert_eq!(app.selected(), 2, "leaving put the outline back at the top");
}

#[test]
fn the_message_log_still_has_a_cursor() {
    let (_d, mut shell, mut app) = app();
    // Distinct lines: the log keeps one entry per message and a
    // repeated one would leave a list of length 1, which wraps to
    // itself and would make this test pass or fail for the wrong
    // reason.
    for _ in 0..3 {
        app.run(&mut shell, "toggle-wrap");
        app.run(&mut shell, "build-info");
    }
    assert!(app.messages().len() > 1, "{:?}", app.messages());
    app.run(&mut shell, "messages");
    let before = app.picker_cursor();
    app.on_key(&mut shell, "down", false, false, None);
    assert_ne!(app.picker_cursor(), before, "the log stopped scrolling");
}

#[test]
fn the_search_overlay_still_drives_the_outline() {
    // The other half. Search filters the *outline* and Enter opens the
    // row, so there its cursor is the outline's and must stay that way.
    let (_d, mut shell, mut app) = app();
    app.run(&mut shell, "search");
    app.on_key(&mut shell, "down", false, false, None);
    assert_eq!(
        app.picker_cursor(),
        app.selected(),
        "search and the outline came apart"
    );
}
