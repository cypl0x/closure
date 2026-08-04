//! "header in editor doesn't show if current headline a TODO/DONE
//! item. There is some info missing in the header. … Since this
//! heading is a TODO item."
//!
//! The detail pane has shown `TODO git integration` all along. Open the
//! same headline as a buffer and the header read `✎ git integration ·
//! /home/wap/vault/inbox.org · 01KZ…` — the state that makes it a task
//! rather than a note simply absent, so the two panes disagreed about
//! whether it mattered.
//!
//! The keyword belongs to the headline, not to one view of it.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const ORG: &str = "\
* TODO git integration
:PROPERTIES:
:ID: 01BUFNAME0000000000000AA
:END:
body
* DONE something finished
:PROPERTIES:
:ID: 01BUFNAME0000000000000BB
:END:
body
* just a note
:PROPERTIES:
:ID: 01BUFNAME0000000000000CC
:END:
body
";

fn app() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    let shell = Shell::new(Vault::open(dir.path()).unwrap());
    (dir, shell, ModalApp::new(closure_config::InputMode::Doom))
}

/// Open the row at `at` as a body buffer.
fn open(app: &mut ModalApp, shell: &mut Shell, at: usize) {
    for _ in 0..at {
        app.run(shell, "next-file");
    }
    app.run(shell, "edit-body");
}

#[test]
fn the_header_says_a_todo_is_a_todo() {
    let (_d, mut shell, mut app) = app();
    open(&mut app, &mut shell, 0);
    let name = app.buffer_name(&shell).expect("a buffer is open");
    assert!(
        name.contains("TODO"),
        "the header does not say this is a task: {name}"
    );
    assert!(name.contains("git integration"), "{name}");
}

#[test]
fn the_header_says_a_done_is_done() {
    let (_d, mut shell, mut app) = app();
    open(&mut app, &mut shell, 1);
    let name = app.buffer_name(&shell).expect("a buffer is open");
    assert!(name.contains("DONE"), "{name}");
}

#[test]
fn a_plain_headline_gains_nothing() {
    // No keyword is not a keyword. A header that said `— · note` for
    // every ordinary headline would be noise on most of them.
    let (_d, mut shell, mut app) = app();
    open(&mut app, &mut shell, 2);
    let name = app.buffer_name(&shell).expect("a buffer is open");
    assert!(!name.contains("TODO"), "{name}");
    assert!(!name.contains("DONE"), "{name}");
    assert!(name.starts_with("just a note"), "{name}");
}

#[test]
fn the_id_and_the_file_are_still_there() {
    // The keyword is added, not swapped in for what was already
    // useful: the id is what a link, a sync round and a bug report
    // address the block by.
    let (_d, mut shell, mut app) = app();
    open(&mut app, &mut shell, 0);
    let name = app.buffer_name(&shell).expect("a buffer is open");
    assert!(name.contains("01BUFNAME0000000000000AA"), "{name}");
    assert!(name.contains("notes.org"), "{name}");
}
