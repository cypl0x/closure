//! "Reload GPUI feature (clean fresh start without quitting and
//! restarting)."
//!
//! Everything closing the window does and everything opening it does,
//! back to back, without the process dying in between: the open edit is
//! saved, where you were is written down, the vault is re-read from
//! disk, the session's own state (buffers, stashes, jumps, the surface
//! you were on) is dropped, and `config.org` is read again — so a
//! keymap or a theme edited in another window takes effect here.
//!
//! The point is the *clean* in "clean fresh start": the re-read is a
//! full walk, not the incremental poll, because this is the command you
//! press when what is on screen looks wrong.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;
use tempfile::TempDir;

const NOTES: &str = "\
* Alpha
:PROPERTIES:
:ID: 01HQRLD000000000000000001
:END:
alpha body
* Beta
:PROPERTIES:
:ID: 01HQRLD000000000000000002
:END:
beta body
";

fn fixture() -> (TempDir, Shell, ModalApp) {
    with_mode(InputMode::Doom)
}

fn with_mode(mode: InputMode) -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(mode))
}

fn open(app: &mut ModalApp, shell: &mut Shell, id: &str) {
    assert!(app.select_by_id(shell, id), "select {id}");
    app.run(shell, "edit-body");
}

fn selected_id(app: &ModalApp, shell: &Shell) -> Option<String> {
    app.rows(shell).get(app.selected()).map(|r| r.id.clone())
}

#[test]
fn the_command_is_bound_in_every_mode() {
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        assert!(
            closure_input::chord_for_command(mode, "reload-shell").is_some(),
            "{mode:?} has no way to reload"
        );
    }
}

#[test]
fn a_reload_reads_the_vault_off_the_disk_again() {
    let (dir, mut shell, mut app) = fixture();
    let before = app.rows(&shell).len();

    // Another program adds a note while the window is open.
    fs::write(dir.path().join("later.org"), "* Gamma\n").expect("write");
    app.run(&mut shell, "reload-shell");

    assert_eq!(
        app.rows(&shell).len(),
        before + 1,
        "the new headline is in the outline"
    );
}

#[test]
fn a_reload_lands_you_back_in_the_outline() {
    let (_d, mut shell, mut app) = fixture();
    open(&mut app, &mut shell, "01HQRLD000000000000000001");
    assert!(app.surface().is_editor(), "in a buffer to begin with");

    app.run(&mut shell, "reload-shell");
    assert_eq!(
        app.surface(),
        ModalSurface::Browse,
        "a fresh start is the outline, the way a launch is"
    );
}

#[test]
fn a_reload_does_not_lose_what_you_typed() {
    // Closing the window saves the open edit rather than dropping it
    // (`save_pending_edit`), and a reload is a close and a launch. Text
    // in a buffer is not repeatable; the gesture that reloaded is.
    let (dir, mut shell, mut app) = fixture();
    open(&mut app, &mut shell, "01HQRLD000000000000000001");
    app.on_key(&mut shell, "i", false, false, Some('i'));
    app.on_key(&mut shell, "z", false, false, Some('z'));

    app.run(&mut shell, "reload-shell");

    let on_disk = fs::read_to_string(dir.path().join("notes.org")).expect("read");
    assert!(
        on_disk.contains("zalpha body") || on_disk.contains("alpha bodyz"),
        "the edit went to disk before the re-read: {on_disk}"
    );
}

#[test]
fn a_reload_forgets_the_buffers_this_session_opened() {
    let (_d, mut shell, mut app) = fixture();
    open(&mut app, &mut shell, "01HQRLD000000000000000001");
    open(&mut app, &mut shell, "01HQRLD000000000000000002");
    assert_eq!(app.buffer_rows(&shell).len(), 2);

    app.run(&mut shell, "reload-shell");
    assert!(
        app.buffer_rows(&shell).is_empty(),
        "a fresh start has nothing open: {:?}",
        app.buffer_rows(&shell)
    );
}

#[test]
fn a_reload_puts_you_back_on_the_note_you_were_in() {
    // A launch restores `last_place` from config.org; so does this, or
    // the reload would throw away the one piece of the session that
    // quitting and restarting actually keeps.
    let (_d, mut shell, mut app) = fixture();
    open(&mut app, &mut shell, "01HQRLD000000000000000002");
    app.run(&mut shell, "reload-shell");

    assert_eq!(
        selected_id(&app, &shell).as_deref(),
        Some("01HQRLD000000000000000002"),
        "the cursor came back to Beta"
    );
}

#[test]
fn a_reload_says_it_reloaded() {
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "reload-shell");
    assert!(
        app.status().to_lowercase().contains("reload"),
        "status: {}",
        app.status()
    );
}

#[test]
fn a_reload_keeps_the_editing_mode_when_the_config_names_none() {
    let (_d, mut shell, mut app) = with_mode(InputMode::Helix);
    app.run(&mut shell, "reload-shell");
    assert_eq!(
        app.input_mode(),
        InputMode::Helix,
        "nothing in config.org asked for a different one"
    );
}

#[test]
fn a_reload_takes_the_editing_mode_the_config_now_asks_for() {
    // The payoff for having a reload at all: edit config.org in another
    // window, press one chord, and this window is running the new one.
    let (dir, mut shell, mut app) = fixture();
    fs::write(
        dir.path().join("config.org"),
        "#+BEGIN_SRC closure-config\ninput_mode = vim\n#+END_SRC\n",
    )
    .expect("write");

    app.run(&mut shell, "reload-shell");
    assert_eq!(app.input_mode(), InputMode::Vim);
}
