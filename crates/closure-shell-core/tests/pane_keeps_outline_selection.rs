//! "selection at top of outline headings list when switching from any
//! element to the Jobs panel and back."
//!
//! Put the cursor on a headline, look at Jobs, come back — and you are
//! at the top of the vault again, with no way back but scrolling to
//! where you were. In a 307-headline outline that is the difference
//! between glancing at something and losing your place.
//!
//! The outline's cursor and a pane's cursor are two facts (that is
//! what `pane_cursor` is for). A pane that walks the outline's is a
//! pane that moves the thing you were reading.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const ORG: &str = "\
* One
:PROPERTIES:
:ID: 01PANEAAAAAAAAAAAAAAAAAAAA
:END:
* Two
:PROPERTIES:
:ID: 01PANEBBBBBBBBBBBBBBBBBBBB
:END:
* Three
:PROPERTIES:
:ID: 01PANECCCCCCCCCCCCCCCCCCCC
:END:
* Four
:PROPERTIES:
:ID: 01PANEDDDDDDDDDDDDDDDDDDDD
:END:
* Five
:PROPERTIES:
:ID: 01PANEEEEEEEEEEEEEEEEEEEEE
:END:
";

fn app() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    (dir, shell, ModalApp::new(InputMode::Doom))
}

/// Every pane reachable from the rail, by the command that opens it.
const PANES: &[&str] = &[
    "cron",
    "journal",
    "agenda",
    "sniffer",
    "conflicts",
    "graph",
    "list-blocks",
    "sync",
    "backlinks",
    "db-view",
];

#[test]
fn every_pane_gives_the_outline_cursor_back_where_it_was() {
    // "from any element" — so all of them, not the one that was
    // reported. A property over the rail rather than a fix for Jobs.
    let mut lost = Vec::new();
    for pane in PANES {
        let (_d, mut shell, mut app) = app();
        app.select(3, &shell);
        assert_eq!(app.selected(), 3, "{pane}: setup");
        app.run(&mut shell, pane);
        app.run(&mut shell, "browse");
        if app.selected() != 3 {
            lost.push(format!("{pane} -> row {}", app.selected()));
        }
    }
    assert!(lost.is_empty(), "panes that moved the outline: {lost:?}");
}

#[test]
fn walking_a_pane_does_not_walk_the_outline() {
    // The other half, and the cause: moving inside a pane must move
    // the *pane's* cursor. Otherwise coming back to a cursor that did
    // not move is luck.
    let mut lost = Vec::new();
    for pane in PANES {
        let (_d, mut shell, mut app) = app();
        app.select(3, &shell);
        app.run(&mut shell, pane);
        for _ in 0..3 {
            app.on_key(&mut shell, "j", false, false, Some('j'));
        }
        app.run(&mut shell, "browse");
        if app.selected() != 3 {
            lost.push(format!("{pane} -> row {}", app.selected()));
        }
    }
    assert!(
        lost.is_empty(),
        "panes whose cursor is the outline's: {lost:?}"
    );
}

#[test]
fn escape_out_of_a_pane_keeps_it_too() {
    // The rail is clicked; `Esc` is how a keyboard leaves. Both doors,
    // because a fix in one is not a fix.
    let mut lost = Vec::new();
    for pane in PANES {
        let (_d, mut shell, mut app) = app();
        app.select(2, &shell);
        app.run(&mut shell, pane);
        app.on_key(&mut shell, "escape", false, false, None);
        if app.selected() != 2 {
            lost.push(format!("{pane} -> row {}", app.selected()));
        }
    }
    assert!(lost.is_empty(), "Esc lost the cursor out of: {lost:?}");
}
