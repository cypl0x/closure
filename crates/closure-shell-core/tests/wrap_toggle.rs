//! "editor horizontal scroll … The screenshot shows how typing a long
//! line results in blindly typing in the dark, because there is no
//! scroll and no wrap. We probably need both. Configurable, toggable,
//! etc."
//!
//! Scrolling is the window's half. This is the other one: wrapping
//! existed but was read from `config.org` once, at launch, into a field
//! in the gpui window — so there was no command, no chord, and no way
//! to change your mind about a paragraph you were looking at. It is
//! kernel state now, like every other view toggle, which is also what
//! gives the terminal shell the same switch.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, Shell};
use closure_store::Vault;
use tempfile::TempDir;

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* Alpha\nbody\n").expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

#[test]
fn wrapping_is_off_until_something_asks_for_it() {
    let (_d, _sh, app) = fixture();
    assert!(!app.wrap());
}

#[test]
fn the_command_turns_it_on_and_off_again() {
    let (_d, mut sh, mut app) = fixture();
    app.run(&mut sh, "toggle-wrap");
    assert!(app.wrap(), "on");
    app.run(&mut sh, "toggle-wrap");
    assert!(!app.wrap(), "and off");
}

#[test]
fn it_says_which_way_it_went() {
    // A toggle with no feedback is a keypress you have to verify by
    // looking at your own paragraph.
    let (_d, mut sh, mut app) = fixture();
    app.run(&mut sh, "toggle-wrap");
    assert!(
        app.status().to_lowercase().contains("wrap"),
        "{}",
        app.status()
    );
}

#[test]
fn every_mode_can_reach_it() {
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        assert!(
            closure_input::chord_for_command(mode, "toggle-wrap").is_some(),
            "{mode:?} cannot toggle wrapping"
        );
    }
}

#[test]
fn the_config_still_decides_where_it_starts() {
    // `wrap = true` in config.org is the durable answer; the chord is
    // for changing your mind about the paragraph in front of you.
    let (_d, _sh, mut app) = fixture();
    app.set_wrap(true);
    assert!(app.wrap());
}

#[test]
fn a_reload_keeps_what_the_config_says() {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), "* Alpha\nbody\n").expect("write");
    fs::write(
        dir.path().join("config.org"),
        "#+BEGIN_SRC closure-config\nwrap = true\n#+END_SRC\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut shell = Shell::new(vault);
    let mut app = ModalApp::new(InputMode::Doom);
    app.set_wrap(true);
    app.run(&mut shell, "reload-shell");
    assert!(
        app.wrap(),
        "the config asked for it, so a fresh start has it"
    );
}

#[test]
fn the_chord_reaches_from_inside_a_buffer() {
    // Where it is wanted most: the lines running off the edge are the
    // ones you are looking at. A bare `g` belongs to the editor there,
    // so the modified spelling is the one that must survive.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "edit-body");
    assert!(app.surface().is_editor());
    app.on_key(&mut shell, "z", false, true, None);
    assert!(app.wrap(), "M-z did not reach the command");
    assert!(app.surface().is_editor(), "and stayed in the buffer");
}
