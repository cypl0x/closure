//! The editor header says which block you are editing.
//!
//! Reported 2026-08-02: "editor should show the ID of the headline that
//! is currently edited in the header. The body preview shows it."
//!
//! The header named the title and the file, which is what a person
//! reads — and the id is what everything *else* addresses the block by:
//! a link, a sync round, an undo entry, a bug report. The preview pane
//! beside it had shown it all along, so the two panes disagreed about
//! whether it was worth knowing.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const ID: &str = "01HQEID0000000000000001";

fn fixture() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        format!("* Alpha\n:PROPERTIES:\n:ID: {ID}\n:END:\nbody\n"),
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

#[test]
fn the_header_carries_the_id_of_the_open_headline() {
    let (_d, mut sh, mut app) = fixture();
    app.select(0, &sh);
    app.run(&mut sh, "edit-body");
    let name = app.buffer_name(&sh).expect("a name");
    assert!(name.contains(ID), "{name}");
}

#[test]
fn it_still_says_the_title_and_the_file() {
    // The id is an addition, not a replacement: the title is what a
    // person recognises the buffer by.
    let (_d, mut sh, mut app) = fixture();
    app.select(0, &sh);
    app.run(&mut sh, "edit-body");
    let name = app.buffer_name(&sh).expect("a name");
    assert!(name.contains("Alpha"), "{name}");
    assert!(name.contains("notes.org"), "{name}");
}

#[test]
fn a_file_buffer_has_no_headline_id_to_show() {
    // The whole file is open, not one block in it; inventing an id here
    // would name a headline the buffer is not about.
    let (_d, mut sh, mut app) = fixture();
    app.run(&mut sh, "toggle-view");
    let name = app.buffer_name(&sh).expect("a name");
    assert!(!name.contains(ID), "{name}");
    assert!(name.contains("notes.org"), "{name}");
}
