//! The three ways out of a buffer, all reachable.
//!
//! Reported 2026-08-02: "save/close/discard editor (changes)".
//!
//! There are three distinct things a person wants from an open buffer
//! and the editor offered two of them as buttons: commit (write and
//! close) and discard. Writing *and carrying on* — the thing you do
//! twenty times an hour — had a chord and no affordance, so the only
//! visible way to keep your work was one that also took the buffer
//! away.
//!
//! All three are commands with chords already; this pins that each one
//! does its own distinct thing, because two of them differing only in
//! whether the buffer survives is exactly the pair that gets confused.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;

const SRC: &str = "* Alpha\n:PROPERTIES:\n:ID: 01HQEXT0000000000000001\n:END:\noriginal\n";

fn editing() -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), SRC).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let (mut sh, mut app) = (Shell::new(vault), ModalApp::new(InputMode::Doom));
    app.select(0, &sh);
    app.run(&mut sh, "edit-body");
    app.on_key(&mut sh, "i", false, false, Some('i'));
    for c in " edited".chars() {
        app.on_key(&mut sh, &c.to_string(), false, false, Some(c));
    }
    app.on_key(&mut sh, "escape", false, false, None);
    (dir, sh, app)
}

fn on_disk(dir: &tempfile::TempDir) -> String {
    fs::read_to_string(dir.path().join("notes.org")).expect("read")
}

#[test]
fn save_writes_and_keeps_the_buffer() {
    let (dir, mut sh, mut app) = editing();
    app.run(&mut sh, "save-buffer");
    assert!(on_disk(&dir).contains("edited"), "written");
    assert!(app.surface().is_editor(), "and still open");
    assert!(!app.body_dirty(), "with nothing left to save");
}

#[test]
fn save_and_close_writes_and_leaves() {
    let (dir, mut sh, mut app) = editing();
    app.run(&mut sh, "commit-edit"); // C-Enter
    assert!(on_disk(&dir).contains("edited"), "written");
    assert!(!app.surface().is_editor(), "and closed");
}

#[test]
fn discard_leaves_without_writing() {
    let (dir, mut sh, mut app) = editing();
    app.run_ex_line(&mut sh, "q!");
    assert!(!on_disk(&dir).contains("edited"), "{}", on_disk(&dir));
    assert!(!app.surface().is_editor(), "and closed");
}

#[test]
fn closing_a_dirty_buffer_without_saying_which_is_refused() {
    // The fourth case, and the reason the other three have to be
    // distinct: `:q` on unsaved work must not guess.
    let (dir, mut sh, mut app) = editing();
    app.run_ex_line(&mut sh, "q");
    assert!(app.surface().is_editor(), "still open");
    assert!(!on_disk(&dir).contains("edited"), "and not written");
    assert!(
        !app.status().is_empty(),
        "and it says why: {}",
        app.status()
    );
}

#[test]
fn each_of_the_three_has_a_chord_in_every_mode() {
    // A button whose chord is unreachable in the active mode teaches
    // the wrong thing.
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        assert!(
            closure_input::chord_for_command(mode, "save-buffer").is_some(),
            "{mode:?} can save"
        );
        assert_eq!(
            closure_input::command_for(mode, ":"),
            Some("ex-command"),
            "{mode:?} can reach :wq and :q!"
        );
    }
}
