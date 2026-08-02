//! "Improve name for toggle-view — Which view? Like we do need to
//! streamline the commands and functions. Because their names are not
//! good. I even prefer longer names if they are more sound.", "cycle-
//! mode is not a sound name … Furthermore feeding arguments to
//! cycle-mode in order to directly set a specific keybind mode", and
//! "M for toggling keybind mode isn't working — It looks like it isn't
//! possible at all to cycle-mode via hotkey in the editor view."
//!
//! Three complaints about the same two commands. `toggle-view` does
//! not say which view (the outline, or the whole file as one buffer);
//! `cycle-mode` does not say which mode (the keymap, not the editor's
//! vim mode); and the only chord for the second was a bare `M`, which
//! a buffer eats, so inside a note there was no way to reach it at
//! all.
//!
//! And the ask underneath: a command that only cycles cannot be told
//! *which* one you want. Commands take an argument now — `set-input-
//! mode vim` — which is the smallest honest version of "feeding
//! arguments", and the `:` line is where you feed them.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell, canonical_command};
use closure_store::Vault;
use tempfile::TempDir;

fn fixture() -> (TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(
        dir.path().join("notes.org"),
        "* Alpha\n:PROPERTIES:\n:ID: 01HQARGS00000000000001\n:END:\nbody\n",
    )
    .expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    let mut app = ModalApp::new(InputMode::Doom);
    let shell = Shell::new(vault);
    assert!(app.select_by_id(&shell, "01HQARGS00000000000001"));
    (dir, shell, app)
}

/// Type `line` into the `:` prompt and run it.
fn ex(app: &mut ModalApp, shell: &mut Shell, line: &str) {
    app.run(shell, "ex-command");
    for c in line.chars() {
        app.on_key(shell, &c.to_string(), false, false, Some(c));
    }
    app.on_key(shell, "enter", false, false, None);
}

#[test]
fn the_names_say_which_thing_they_are_about() {
    assert_eq!(canonical_command("toggle-view"), "toggle-file-view");
    assert_eq!(canonical_command("cycle-mode"), "next-input-mode");
}

#[test]
fn the_old_names_still_run() {
    // A rename that breaks the chord you typed yesterday costs more
    // than the tidiness it buys.
    let (_d, mut shell, mut app) = fixture();
    let before = app.input_mode();
    app.run(&mut shell, "cycle-mode");
    assert_ne!(app.input_mode(), before, "the old name still cycles");
}

#[test]
fn a_mode_can_be_named_outright() {
    // "feeding arguments to cycle-mode in order to directly set a
    // specific keybind mode".
    let (_d, mut shell, mut app) = fixture();
    ex(&mut app, &mut shell, "set-input-mode vim");
    assert_eq!(app.input_mode(), InputMode::Vim, "{}", app.status());
}

#[test]
fn every_mode_can_be_asked_for_by_name() {
    for (name, want) in [
        ("emacs", InputMode::Emacs),
        ("vim", InputMode::Vim),
        ("doom", InputMode::Doom),
        ("helix", InputMode::Helix),
        ("notion", InputMode::Notion),
    ] {
        let (_d, mut shell, mut app) = fixture();
        ex(&mut app, &mut shell, &format!("set-input-mode {name}"));
        assert_eq!(app.input_mode(), want, "{name}: {}", app.status());
    }
}

#[test]
fn a_mode_that_does_not_exist_is_named_rather_than_ignored() {
    let (_d, mut shell, mut app) = fixture();
    let before = app.input_mode();
    ex(&mut app, &mut shell, "set-input-mode klingon");
    assert_eq!(app.input_mode(), before);
    assert!(app.status().contains("klingon"), "{}", app.status());
}

#[test]
fn the_argument_reaches_any_command_that_wants_one() {
    // The general shape, not a special case for one command: the `:`
    // line splits a name from its argument and the command reads it.
    let (_d, mut shell, mut app) = fixture();
    ex(&mut app, &mut shell, "set-input-mode   helix   ");
    assert_eq!(app.input_mode(), InputMode::Helix, "{}", app.status());
}

#[test]
fn the_mode_switch_is_reachable_from_inside_a_buffer() {
    // "It looks like it isn't possible at all to cycle-mode via hotkey
    // in the editor view": the only chord was a bare `M`, which a
    // buffer eats as text or as a motion.
    let (_d, mut shell, mut app) = fixture();
    app.run(&mut shell, "edit-body");
    assert_eq!(app.surface(), ModalSurface::EditBody);
    let before = app.input_mode();
    let chord =
        closure_input::chord_for_command(InputMode::Doom, "next-input-mode").expect("a chord");
    assert!(
        chord.contains('-'),
        "a bare letter cannot reach it from a buffer: {chord}"
    );
    // And pressing it there actually switches.
    app.on_key(&mut shell, "m", false, true, None);
    assert_ne!(app.input_mode(), before, "{}", app.status());
    assert_eq!(
        app.surface(),
        ModalSurface::EditBody,
        "and stays in the buffer"
    );
}

#[test]
fn every_mode_keeps_a_way_to_switch_from_a_buffer() {
    for mode in [
        InputMode::Doom,
        InputMode::Vim,
        InputMode::Emacs,
        InputMode::Helix,
        InputMode::Notion,
    ] {
        let chord = closure_input::chord_for_command(mode, "next-input-mode")
            .unwrap_or_else(|| panic!("{mode:?} cannot switch modes"));
        assert!(
            chord.contains('-'),
            "{mode:?}: {chord} is eaten by a buffer"
        );
    }
}
