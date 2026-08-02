//! "Pressing on the previously (and of course the should be a function
//! and keybinding) should open the system file picker which lets the
//! user select a different vault location"
//!
//! The vault was chosen once, on the command line, and that was that:
//! the path in the header was a label. Switching meant quitting and
//! relaunching with a different argument.
//!
//! Opening the picker is the window's job — a native dialog is not
//! something a dep-free core can raise. What the core owns is what
//! happens *after* a path comes back: everything the app holds that
//! belongs to the old vault has to go, or the new one opens showing
//! the last vault's selection, marks and buffers.
//!
//! And the guard. An unwritten buffer belongs to the vault being left,
//! so switching away from one is throwing it away, and that is the one
//! thing this must not do quietly.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

fn vault_with(name: &str, id: &str) -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        format!("* {name}\n:PROPERTIES:\n:ID: {id}\n:END:\nbody of {name}\n"),
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault))
}

#[test]
fn switching_forgets_the_old_vaults_selection() {
    let (_a, shell) = vault_with("Alpha", "01HQVAULT0000000000001");
    let mut app = ModalApp::new(InputMode::Doom);
    assert!(app.select_by_id(&shell, "01HQVAULT0000000000001"));
    app.on_key(&mut { shell }, "m", false, false, Some('m'));
    assert_eq!(app.marked_count(), 1);

    app.reset_for_vault();
    assert_eq!(app.marked_count(), 0, "marks pointed at the old vault");
    assert_eq!(app.selected(), 0);
}

#[test]
fn switching_closes_whatever_was_open() {
    // A buffer holds a headline from the vault being left; keeping it
    // would show text the new vault does not contain.
    let (_a, mut shell) = vault_with("Alpha", "01HQVAULT0000000000001");
    let mut app = ModalApp::new(InputMode::Doom);
    assert!(app.select_by_id(&shell, "01HQVAULT0000000000001"));
    app.run(&mut shell, "edit-body");
    assert_eq!(app.surface(), ModalSurface::EditBody);

    app.reset_for_vault();
    assert_ne!(app.surface(), ModalSurface::EditBody);
    assert!(app.body_buffer().is_empty(), "{:?}", app.body_buffer());
}

#[test]
fn an_unwritten_buffer_stops_the_switch() {
    // The guard: switching away from an edit is throwing it away.
    let (_a, mut shell) = vault_with("Alpha", "01HQVAULT0000000000001");
    let mut app = ModalApp::new(InputMode::Doom);
    assert!(app.select_by_id(&shell, "01HQVAULT0000000000001"));
    app.run(&mut shell, "edit-body");
    app.on_key(&mut shell, "i", false, false, Some('i'));
    app.on_key(&mut shell, "x", false, false, Some('x'));
    assert!(app.body_dirty(), "the premise");

    assert!(!app.can_switch_vault(), "would have lost the edit");
}

#[test]
fn a_clean_shell_may_switch() {
    let (_a, shell) = vault_with("Alpha", "01HQVAULT0000000000001");
    let app = ModalApp::new(InputMode::Doom);
    let _ = &shell;
    assert!(app.can_switch_vault());
}

#[test]
fn the_command_exists_and_every_mode_can_reach_it() {
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        assert!(
            closure_input::chord_for_command(mode, "open-vault").is_some(),
            "{mode:?} cannot switch vaults"
        );
    }
}

#[test]
fn a_directory_that_is_not_a_vault_is_refused() {
    // A vault is a directory of org files. Pointing closure at an
    // empty one is a mistake worth naming rather than an empty window.
    let dir = tempfile::tempdir().expect("tmp");
    assert!(!closure_shell_core::looks_like_vault(dir.path()));
    fs::write(dir.path().join("notes.org"), "* A\n").expect("write");
    assert!(closure_shell_core::looks_like_vault(dir.path()));
}

#[test]
fn a_directory_with_org_files_below_it_counts() {
    // Vaults are usually a tree, not a flat directory.
    let dir = tempfile::tempdir().expect("tmp");
    fs::create_dir_all(dir.path().join("projects")).expect("mkdir");
    fs::write(dir.path().join("projects/a.org"), "* A\n").expect("write");
    assert!(closure_shell_core::looks_like_vault(dir.path()));
}
